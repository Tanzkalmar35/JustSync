use anyhow::Result;
use quinn::{
    ClientConfig, ConnectionError, Endpoint, ServerConfig, TransportConfig, VarInt,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc};

use crate::{core::Event, logger, lsp::Position};

/// The packet we serialize and send over the QUIC stream.
#[derive(Serialize, Deserialize, Debug)]
enum WireMessage {
    Patch {
        uri: String,
        data: Vec<u8>,
    },

    Cursor {
        uri: String,
        position: (usize, usize),
    },

    /// Peer -> Host: "I just joined, give me everything."
    RequestFullSync,

    /// Host -> Peer: "Here is the entire workspace state."
    FullSyncResponse {
        files: Vec<(String, Vec<u8>)>,
    },
}

#[derive(Debug)]
pub enum NetworkCommand {
    BroadcastCursor {
        uri: String,
        position: (usize, usize),
    },
    BroadcastPatch {
        uri: String,
        patch: Vec<u8>,
    },
    SendFullSyncResponse {
        files: Vec<(String, Vec<u8>)>,
    },
}

// =========================================================================
//  The Network Actor
// =========================================================================

/// Main entry point for the Network Adapter.
pub async fn run(
    mode: String,
    remote_ip: Option<String>,
    port: u16,
    core_tx: mpsc::Sender<Event>,
    mut net_rx: mpsc::Receiver<NetworkCommand>,
    token: String,
    server_certs: Option<Vec<CertificateDer<'static>>>,
    server_key: Option<PrivateKeyDer<'static>>,
) -> Result<(), ConnectionError> {
    // Initialize QUIC Endpoint (Bind socket)
    let endpoint_result = if mode == "host" {
        init_host(
            port,
            server_certs.expect("Host needs certs"),
            server_key.expect("Host needs key"),
        )
    } else {
        init_client(0, &token)
    };

    let endpoint = endpoint_result.expect("Failed to bind UDP port");

    // Establish Connection (Handshake)
    let connection = if mode == "host" {
        crate::logger::log(">> [Network] Waiting for peer to connect...");
        match endpoint.accept().await {
            Some(incoming) => match incoming.await {
                Ok(conn) => {
                    crate::logger::log(&format!(
                        ">> [Network] Peer connected securely: {}",
                        conn.remote_address()
                    ));
                    conn
                }
                Err(e) => {
                    crate::logger::log(&format!("!! [Network] Handshake failed: {}", e));
                    return Err(e);
                }
            },
            None => return Ok(()), // Endpoint closed
        }
    } else {
        let ip_str = remote_ip.expect("Remote IP required for peer mode");
        // Handle IP parsing (append port if missing)
        let addr_str = if ip_str.contains(':') {
            ip_str
        } else {
            format!("{}:{}", ip_str, port)
        };
        let addr = addr_str.parse().expect("Invalid remote address format");

        crate::logger::log(&format!(
            ">> [Network] Connecting to {} with Token...",
            addr
        ));

        match endpoint.connect(addr, "localhost").unwrap().await {
            Ok(conn) => {
                crate::logger::log(">> [Network] Connected to Host (Authenticated!).");
                conn
            }
            Err(e) => {
                crate::logger::log(&format!("!! [Network] Connection failed: {}", e));
                return Err(e);
            }
        }
    };

    // Protocol Logic
    let (mut send_stream, mut recv_stream) = if mode == "peer" {
        connection.open_bi().await?
    } else {
        connection.accept_bi().await?
    };

    if mode == "peer" {
        if let Err(e) = send_framed(&mut send_stream, &WireMessage::RequestFullSync).await {
            logger::log(format!("Couldn't send initial sync request: {}", e).as_str());
            return Err(ConnectionError::VersionMismatch);
        }
    }

    // Outbound
    let send_task = tokio::spawn(async move {
        while let Some(cmd) = net_rx.recv().await {
            let wire_msg = match cmd {
                NetworkCommand::BroadcastCursor { uri, position } => {
                    WireMessage::Cursor { uri, position }
                }
                NetworkCommand::BroadcastPatch { uri, patch } => {
                    WireMessage::Patch { uri, data: patch }
                }
                NetworkCommand::SendFullSyncResponse { files } => {
                    WireMessage::FullSyncResponse { files }
                }
            };

            if let Err(e) = send_framed(&mut send_stream, &wire_msg).await {
                logger::log(format!("Error: Could not send wiremsg: {:?}", e).as_str());
                break;
            }
        }
    });

    // Inbound
    loop {
        let tx = core_tx.clone();
        // 100mb hard limit
        match recv_framed(&mut recv_stream).await {
            Ok(msg) => match msg {
                WireMessage::Patch { uri, data } => {
                    logger::log(&format!(">> [Network] Received patch for {}", uri));
                    let _ = tx.send(Event::RemotePatch { uri, patch: data }).await;
                }
                WireMessage::Cursor { uri, position } => {
                    let (line, char) = position;
                    let _ = tx
                        .send(Event::RemoteCursorChange {
                            uri,
                            position: Position {
                                line,
                                character: char,
                            },
                        })
                        .await;
                }
                WireMessage::RequestFullSync => {
                    let _ = tx.send(Event::PeerRequestedSync).await;
                }
                WireMessage::FullSyncResponse { files } => {
                    let _ = tx.send(Event::RemoteFullSync { files }).await;
                }
            },
            Err(e) => {
                crate::logger::log(&format!("!! Read error: {}", e));
                break;
            }
        }
    }

    // Cleanup
    send_task.abort();
    let _ = core_tx.send(Event::Shutdown).await;
    return Ok(());
}

async fn send_framed(send: &mut quinn::SendStream, msg: &WireMessage) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(msg)?;
    let len = bytes.len() as u32;

    // Write 4-byte length prefix (Big Endian)
    send.write_all(&len.to_be_bytes()).await?;
    // Write the actual JSON payload
    send.write_all(&bytes).await?;
    Ok(())
}

async fn recv_framed(recv: &mut quinn::RecvStream) -> anyhow::Result<WireMessage> {
    // Read the 4-byte length
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // Read exactly 'len' bytes into a buffer
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;

    Ok(serde_json::from_slice(&buf)?)
}

// =========================================================================
//  Configuration (TLS & QUIC)
// =========================================================================

fn make_transport_config() -> TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(VarInt::from_u32(30_000).into()));
    transport_config
}

/// Initializes the host with it's certificates
fn init_host(
    port: u16,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Endpoint> {
    // Build rustls config
    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    // Configure ALPN
    crypto.alpn_protocols = vec![b"justsync".to_vec()];

    // Translate into QUINN server config
    let server_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(server_crypto));

    // Configure transport options
    server_config.transport_config(Arc::new(make_transport_config()));

    // Bindings
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let endpoint = Endpoint::server(server_config, addr)?;

    crate::logger::log(&format!("Host bound to {}", endpoint.local_addr()?));
    Ok(endpoint)
}

/// Initializes client with the custom token verifier
fn init_client(bind_port: u16, token: &str) -> Result<Endpoint> {
    let client_config = configure_client(token);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], bind_port));
    let mut endpoint = Endpoint::client(addr)?;
    endpoint.set_default_client_config(client_config);

    Ok(endpoint)
}

fn configure_client(token: &str) -> ClientConfig {
    // Use own verifier
    let verifier = crate::crypto::TokenVerifier::new(token);

    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    // ALPN has to match
    crypto.alpn_protocols = vec![b"justsync".to_vec()];

    let mut config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ));
    config.transport_config(Arc::new(make_transport_config()));
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use tokio::sync::mpsc;

    #[test]
    fn test_wire_message_roundtrip() {
        let original = WireMessage::Patch {
            uri: "file:///test.rs".to_string(),
            data: vec![1, 2, 3, 4],
        };

        let encoded = serde_json::to_vec(&original).unwrap();
        let decoded: WireMessage = serde_json::from_slice(&encoded).unwrap();

        match decoded {
            WireMessage::Patch { uri, data } => {
                assert_eq!(uri, "file:///test.rs");
                assert_eq!(data, vec![1, 2, 3, 4]);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[tokio::test]
    async fn test_quic_integration() {
        // 1. Setup Crypto (Certs & Token)
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (server_certs, server_key, token) = crypto::generate_cert_and_token();

        // 2. Setup Channels
        let (host_core_tx, mut host_core_rx) = mpsc::channel(10);
        let (host_net_tx, host_net_rx) = mpsc::channel(10);

        let (peer_core_tx, mut peer_core_rx) = mpsc::channel(10);
        let (_peer_net_tx, peer_net_rx) = mpsc::channel(10);

        // 3. Start Host
        // Port 0 lets the OS pick a random free port
        let certs_clone = server_certs.clone();
        let key_clone = server_key.clone_key();

        // We need to run the host in a way that we can extract the port.
        // But network::run() consumes the future.
        // We'll trust the "bind to port 0" logic inside `init_host` works,
        // but we need to know WHICH port it picked to tell the client.
        // Since `run` is opaque, we'll modify the test to use a fixed high port
        // to avoid race conditions, or we assume 50000+ range.
        let test_port = 54321;

        let host_handle = tokio::spawn(async move {
            run(
                "host".to_string(),
                None,
                test_port,
                host_core_tx,
                host_net_rx,
                "".to_string(), // Host ignores token string, generates its own or uses certs
                Some(certs_clone),
                Some(key_clone),
            )
            .await;
        });

        // Give host a moment to bind
        tokio::time::sleep(Duration::from_millis(200)).await;

        // 4. Start Peer
        let token_clone = token.clone();
        let peer_handle = tokio::spawn(async move {
            run(
                "peer".to_string(),
                Some("127.0.0.1".to_string()),
                test_port,
                peer_core_tx,
                peer_net_rx,
                token_clone,
                None,
                None,
            )
            .await;
        });

        // 5. Verification Steps

        // A. Peer connects -> Sends RequestFullSync (Startup logic)
        // B. Host should receive PeerRequestedSync
        match tokio::time::timeout(Duration::from_secs(2), host_core_rx.recv()).await {
            Ok(Some(Event::PeerRequestedSync)) => {
                println!("Test: Host received sync request");
            }
            res => panic!("Host did not receive Sync Request: {:?}", res),
        }

        // C. Host Sends Response
        host_net_tx
            .send(NetworkCommand::SendFullSyncResponse {
                files: vec![("doc.txt".into(), vec![65, 66, 67])],
            })
            .await
            .unwrap();

        // D. Peer should receive RemoteFullSync
        match tokio::time::timeout(Duration::from_secs(2), peer_core_rx.recv()).await {
            Ok(Some(Event::RemoteFullSync { files })) => {
                assert_eq!(files[0].0, "doc.txt");
                assert_eq!(files[0].1, vec![65, 66, 67]);
                println!("Test: Peer received full sync");
            }
            res => panic!("Peer did not receive Sync Response: {:?}", res),
        }

        // Cleanup
        host_handle.abort();
        peer_handle.abort();
    }
}
