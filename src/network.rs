use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use quinn::{ClientConfig, Endpoint, ServerConfig};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

use crate::{lsp::TextEdit, state::Workspace};

pub struct NetworkManager {
    pub endpoint: Endpoint,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NetMessage {
    Handshake {
        version: String,
    },
    ProjectState {
        files: Vec<(String, String, Vec<u8>)>,
    },
    Sync {
        uri: String,
        data: Vec<u8>,
    },
}

struct OutboundStream {
    send: tokio::sync::Mutex<quinn::SendStream>,
}

impl OutboundStream {
    async fn send_msg(&self, msg: &NetMessage) -> Result<()> {
        let mut stream = self.send.lock().await;
        let bytes = serde_json::to_vec(msg)?;
        stream.write_u32_le(bytes.len() as u32).await?;
        stream.write_all(&bytes).await?;
        Ok(())
    }
}

impl NetworkManager {
    pub fn init_host(port: u16) -> Result<Self> {
        let (server_config, _cert) = configure_server()?;
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        let endpoint = Endpoint::server(server_config, addr)?;
        crate::logger::log(&format!("Host listening on {}", endpoint.local_addr()?));
        Ok(Self { endpoint })
    }

    pub fn init_client(bind_port: u16) -> Result<Self> {
        let client_config = configure_client();
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], bind_port));
        let mut endpoint = Endpoint::client(addr)?;
        endpoint.set_default_client_config(client_config);
        Ok(Self { endpoint })
    }

    pub async fn connect(&self, server_addr: std::net::SocketAddr) -> Result<quinn::Connection> {
        let connection = self.endpoint.connect(server_addr, "localhost")?.await?;
        crate::logger::log(&format!("Successfully connected to {}", server_addr));
        Ok(connection)
    }
}

pub async fn run_network_loop(
    net: NetworkManager,
    is_host: bool,
    initial_conn: Option<quinn::Connection>,
    network_rx: &mut mpsc::Receiver<(String, Vec<u8>)>,
    editor_tx: &mpsc::Sender<(String, Vec<TextEdit>)>,
    state: Arc<Mutex<Workspace>>,
) {
    let peers = Arc::new(Mutex::new(Vec::<Arc<OutboundStream>>::new()));

    // Setup a new Peer
    let spawn_peer_handler = |_conn: quinn::Connection,
                              stream: (quinn::SendStream, quinn::RecvStream),
                              peers_list: Arc<Mutex<Vec<Arc<OutboundStream>>>>,
                              state_ref: Arc<Mutex<Workspace>>,
                              tx_ref: mpsc::Sender<(String, Vec<TextEdit>)>,
                              is_host_val: bool| {
        let (send, recv) = stream;

        let outbound = Arc::new(OutboundStream {
            send: tokio::sync::Mutex::new(send),
        });
        {
            peers_list.lock().unwrap().push(outbound.clone());
        }

        tokio::spawn(async move {
            handle_stream_read_loop(recv, state_ref, tx_ref, is_host_val, outbound).await;
        });
    };

    // --- CLIENT MODE ---
    if let Some(conn) = initial_conn {
        match conn.open_bi().await {
            Ok(stream) => {
                let (mut send, recv) = stream;

                let handshake = NetMessage::Handshake {
                    version: "1.0".to_string(),
                };
                let bytes = serde_json::to_vec(&handshake).unwrap();
                let _ = send.write_u32_le(bytes.len() as u32).await;
                let _ = send.write_all(&bytes).await;

                spawn_peer_handler(
                    conn,
                    (send, recv),
                    peers.clone(),
                    state.clone(),
                    editor_tx.clone(),
                    is_host,
                );
            }
            Err(e) => crate::logger::log(&format!("!! Failed to open initial stream: {}", e)),
        }
    }

    crate::logger::log("Event Loop Started.");

    loop {
        tokio::select! {
            // --- OUTBOUND ---
            Some((uri, patch)) = network_rx.recv() => {
                let msg = NetMessage::Sync { uri, data: patch };

                let sessions = {
                    peers.lock().unwrap().clone()
                };

                for peer in sessions {
                    let msg_clone = msg.clone();
                    tokio::spawn(async move {
                        if let Err(e) = peer.send_msg(&msg_clone).await {
                             crate::logger::log(&format!("!! Failed to send to peer: {}", e));
                        }
                    });
                }
            }

            // --- HOST MODE ---
            Some(connecting) = net.endpoint.accept() => {
                if let Ok(conn) = connecting.await {
                    crate::logger::log(&format!(">> [Network] New Peer Connected: {}", conn.remote_address()));

                    let peers_clone = peers.clone();
                    let state_ref = state.clone();
                    let tx_ref = editor_tx.clone();
                    let is_host_val = is_host;

                    tokio::spawn(async move {
                        if let Ok(stream) = conn.accept_bi().await {
                            spawn_peer_handler(conn, stream, peers_clone, state_ref, tx_ref, is_host_val);
                        } else {
                            crate::logger::log("!! Client connected but didn't open stream.");
                        }
                    });
                }
            }
        }
    }
}

async fn handle_stream_read_loop(
    mut recv: quinn::RecvStream,
    state: Arc<Mutex<Workspace>>,
    editor_tx: mpsc::Sender<(String, Vec<TextEdit>)>,
    is_host: bool,
    outbound: Arc<OutboundStream>,
) {
    loop {
        match recv.read_u32_le().await {
            Ok(len) => {
                let mut buf = vec![0u8; len as usize];
                if let Ok(()) = recv.read_exact(&mut buf).await {
                    if let Ok(msg) = serde_json::from_slice::<NetMessage>(&buf) {
                        process_message(msg, &outbound, &state, &editor_tx, is_host).await;
                    }
                } else {
                    break;
                }
            }
            Err(_) => {
                crate::logger::log(">> [Network] Stream closed/disconnected.");
                break;
            }
        }
    }
}

async fn process_message(
    msg: NetMessage,
    outbound: &Arc<OutboundStream>,
    state: &Arc<Mutex<Workspace>>,
    editor_tx: &mpsc::Sender<(String, Vec<TextEdit>)>,
    is_host: bool,
) {
    match msg {
        NetMessage::Handshake { .. } => {
            if is_host {
                crate::logger::log("Sending Project State...");
                let resp = {
                    let guard = state.lock().unwrap();
                    let mut files = Vec::new();
                    for (uri, doc) in &guard.state {
                        let content = doc.content.to_string();
                        let history = doc
                            .crdt
                            .oplog
                            .encode(diamond_types::list::encoding::EncodeOptions::default());
                        files.push((uri.clone(), content, history));
                    }
                    NetMessage::ProjectState { files }
                };
                let _ = outbound.send_msg(&resp).await;
            }
        }

        NetMessage::ProjectState { files } => {
            crate::logger::log(&format!("Received State ({} files)", files.len()));
            let mut updates = Vec::new();
            {
                let mut guard = state.lock().unwrap();
                for (uri, content, crdt_data) in files {
                    let mut new_crdt = diamond_types::list::ListCRDT::load_from(&crdt_data)
                        .unwrap_or_else(|_| diamond_types::list::ListCRDT::new());

                    // Force branch to match history
                    new_crdt
                        .branch
                        .merge(&new_crdt.oplog, &new_crdt.oplog.local_version());

                    // Initialize Document with the Expectation set
                    let new_doc = crate::state::Document {
                        uri: uri.clone(),
                        content: ropey::Rope::from_str(&content),
                        crdt: new_crdt,
                        pending_remote_updates: std::sync::atomic::AtomicUsize::new(1),

                        // We set the expectation to the content we just received.
                        last_synced: Some((content.clone(), std::time::Instant::now())),
                    };

                    guard.state.insert(uri.clone(), new_doc);

                    let full_range = crate::lsp::Range {
                        start: crate::lsp::Position {
                            line: 0,
                            character: 0,
                        },
                        end: crate::lsp::Position {
                            line: 999999,
                            character: 0,
                        },
                    };
                    let edit = TextEdit {
                        range: full_range,
                        new_text: content,
                    };
                    updates.push((uri, vec![edit]));
                }
            }

            for (uri, edits) in updates {
                let _ = editor_tx.send((uri, edits)).await;
            }
        }

        NetMessage::Sync { uri, data } => {
            crate::logger::log(&format!("Received Sync for {} ({} bytes)", uri, data.len()));

            let edits_to_send = {
                let mut guard = state.lock().unwrap();
                let doc = guard.get_or_create(uri.clone(), "".to_string());

                let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    doc.crdt.merge_data_and_ff(&data).or_else(|_| {
                        doc.crdt
                            .oplog
                            .decode_and_add(&data)
                            .map(|_| doc.crdt.oplog.local_version())
                    })
                }));

                match decode_result {
                    Ok(Ok(_)) => {
                        let new_text_str = doc.crdt.branch.content().to_string();
                        let new_rope = ropey::Rope::from_str(&new_text_str);

                        doc.content = new_rope.clone();
                        doc.last_synced = Some((new_text_str.clone(), std::time::Instant::now()));

                        // Increment Guard
                        doc.pending_remote_updates
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                        // TODO: Hacky approach, replace with proper diff calculation
                        // We replace the entire file content on every sync.
                        // This guarantees Neovim matches our internal state.
                        let full_range = crate::lsp::Range {
                            start: crate::lsp::Position {
                                line: 0,
                                character: 0,
                            },
                            end: crate::lsp::Position {
                                line: 999999,
                                character: 0,
                            },
                        };

                        let edits = vec![crate::lsp::TextEdit {
                            range: full_range,
                            new_text: new_text_str,
                        }];

                        if !edits.is_empty() { Some(edits) } else { None }
                    }
                    Ok(Err(_)) => {
                        crate::logger::log("Failed to merge patch");
                        None
                    }
                    Err(_) => {
                        crate::logger::log("CRITICAL: Decode panicked.");
                        None
                    }
                }
            };

            if let Some(edits) = edits_to_send {
                let _ = editor_tx.send((uri, edits)).await;
            }
        }
    }
}

fn make_transport_config() -> quinn::TransportConfig {
    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_concurrent_uni_streams(0_u8.into());
    transport_config.max_concurrent_bidi_streams(10_u32.into());
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(quinn::VarInt::from_u32(30_000).into()));
    transport_config
}

pub fn configure_server() -> Result<(ServerConfig, Vec<u8>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = cert.cert;
    let key_pair = cert.signing_key;
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    let mut config = ServerConfig::with_single_cert(vec![cert_der.clone().into()], private_key)?;
    config.transport = Arc::new(make_transport_config());
    Ok((config, cert_der.der().to_vec()))
}

pub fn configure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    let mut crypto = crypto;
    crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));
    let mut config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ));
    config.transport_config(Arc::new(make_transport_config()));
    config
}

#[derive(Debug)]
struct SkipServerVerification;
impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
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
