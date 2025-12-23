// src/network.rs

use anyhow::Result;
use quinn::{ClientConfig, Endpoint, ServerConfig, TransportConfig, VarInt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

use crate::{core::Event, logger};

/// The packet we serialize and send over the QUIC stream.
#[derive(Serialize, Deserialize, Debug)]
enum WireMessage {
    Patch {
        uri: String,
        data: Vec<u8>,
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
    BroadcastPatch { uri: String, patch: Vec<u8> },
    SendFullSyncResponse { files: Vec<(String, Vec<u8>)> },
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
) {
    // Initialize QUIC Endpoint (Bind socket)
    let endpoint = if mode == "host" {
        init_host(port).expect("Failed to bind host port")
    } else {
        init_client(0).expect("Failed to bind client port")
    };

    // Establish Connection (Handshake)
    let connection = if mode == "host" {
        crate::logger::log(">> [Network] Waiting for peer to connect...");
        match endpoint.accept().await {
            Some(incoming) => match incoming.await {
                Ok(conn) => {
                    crate::logger::log(&format!(
                        ">> [Network] Peer connected: {}",
                        conn.remote_address()
                    ));
                    conn
                }
                Err(e) => {
                    crate::logger::log(&format!("!! [Network] Handshake failed: {}", e));
                    return;
                }
            },
            None => return, // Endpoint closed
        }
    } else {
        let ip_str = remote_ip.expect("Remote IP required for peer mode");
        // Handle IP parsing (append port if missing)
        let addr_str = if ip_str.contains(':') {
            ip_str
        } else {
            format!("{}:4444", ip_str)
        };
        let addr = addr_str.parse().expect("Invalid remote address format");

        crate::logger::log(&format!(">> [Network] Connecting to {}...", addr));
        match endpoint.connect(addr, "localhost").unwrap().await {
            Ok(conn) => {
                crate::logger::log(">> [Network] Connected to Host.");
                conn
            }
            Err(e) => {
                crate::logger::log(&format!("!! [Network] Connection failed: {}", e));
                return;
            }
        }
    };

    if mode == "peer" {
        crate::logger::log(">> [Network] Sending RequestFullSync...");
        let msg = WireMessage::RequestFullSync;
        let bytes = serde_json::to_vec(&msg).unwrap();

        // Open a stream just for this request
        if let Ok(mut stream) = connection.open_uni().await {
            let _ = stream.write_all(&bytes).await;
            let _ = stream.finish();
        }
    }

    // Start IO Loops
    // We clone the connection handle for the sender task.
    let conn_sender = connection.clone();

    // LOOP A: Outbound (Core -> Network -> Wire)
    let send_task = tokio::spawn(async move {
        while let Some(cmd) = net_rx.recv().await {
            let wire_msg = match cmd {
                NetworkCommand::BroadcastPatch { uri, patch } => {
                    WireMessage::Patch { uri, data: patch }
                }
                NetworkCommand::SendFullSyncResponse { files } => {
                    WireMessage::FullSyncResponse { files }
                }
            };

            let bytes = serde_json::to_vec(&wire_msg).unwrap();

            // Send logic (same as before)
            match conn_sender.open_uni().await {
                Ok(mut stream) => {
                    let _ = stream.write_all(&bytes).await;
                    let _ = stream.finish();
                }
                Err(e) => crate::logger::log(&format!("!! Write error: {}", e)),
            }
        }
    });

    // LOOP B: Inbound (Wire -> Network -> Core)
    // We run this on the current task
    loop {
        match connection.accept_uni().await {
            Ok(mut recv) => {
                let tx = core_tx.clone();
                tokio::spawn(async move {
                    match recv.read_to_end(50 * 1024 * 1024).await {
                        // Bump limit for full sync
                        Ok(bytes) => {
                            if let Ok(wire_msg) = serde_json::from_slice::<WireMessage>(&bytes) {
                                match wire_msg {
                                    // Existing
                                    WireMessage::Patch { uri, data } => {
                                        logger::log(&format!(
                                            ">> [Network] Sending patch for {}",
                                            uri
                                        ));
                                        let _ =
                                            tx.send(Event::RemotePatch { uri, patch: data }).await;
                                    }
                                    // NEW: Host received a request
                                    WireMessage::RequestFullSync => {
                                        let _ = tx.send(Event::PeerRequestedSync).await;
                                    }
                                    // NEW: Peer received the huge payload
                                    WireMessage::FullSyncResponse { files } => {
                                        let _ = tx.send(Event::RemoteFullSync { files }).await;
                                    }
                                }
                            }
                        }
                        Err(e) => crate::logger::log(&format!("!! Read error: {}", e)),
                    }
                });
            }
            Err(_) => break,
        }
    }

    // Cleanup
    send_task.abort();
    let _ = core_tx.send(Event::Shutdown).await;
}

// =========================================================================
//  Configuration Boilerplate (TLS & QUIC)
// =========================================================================

fn make_transport_config() -> TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100)); // Allow many concurrent patches
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(VarInt::from_u32(30_000).into()));
    transport_config
}

fn init_host(port: u16) -> Result<Endpoint> {
    let (server_config, _cert) = configure_server()?;
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let endpoint = Endpoint::server(server_config, addr)?;
    crate::logger::log(&format!("Host listening on {}", endpoint.local_addr()?));
    Ok(endpoint)
}

fn init_client(bind_port: u16) -> Result<Endpoint> {
    let client_config = configure_client();
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], bind_port));
    let mut endpoint = Endpoint::client(addr)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

fn configure_server() -> Result<(ServerConfig, Vec<u8>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = cert.cert;
    let key_pair = cert.signing_key;
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

    let mut config = ServerConfig::with_single_cert(vec![cert_der.clone().into()], private_key)?;
    config.transport = Arc::new(make_transport_config());
    Ok((config, cert_der.der().to_vec()))
}

fn configure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();

    let mut crypto = crypto;
    // DANGER: We skip verification for this Alpha P2P tool.
    // In production, use real CA certs or fingerprint pinning.
    crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    let mut config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ));
    config.transport_config(Arc::new(make_transport_config()));
    config
}

// --- TLS Verification Skipper ---

#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
