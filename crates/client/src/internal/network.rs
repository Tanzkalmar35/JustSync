use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{ClientConfig, TransportConfig, VarInt, crypto::rustls::QuicClientConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::debug;

use crate::internal::{core::Event, crypto::NoVerifier, lsp::Position};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WireMessage {
    Patch {
        uri: String,
        data: Vec<u8>,
    },

    Cursor {
        uri: String,
        position: (usize, usize),
    },

    /// Peer -> Host
    RequestFullSync,

    /// Host -> Peer
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlMessage {
    Register { key: String },
    SessionCreated { status: String, name: String },
    Join { name: String, key: String },
    SessionJoined { status: String },
    InitPeer { agent_id: String, is_host: bool },
    Spake2MsgA { data: Vec<u8> },
    Spake2MsgB { data: Vec<u8> },
}

#[derive(Clone)]
pub struct SessionCfg {
    pub agent_id: String,
    pub key: String,
    pub relay_addr: SocketAddr,
    pub role: SessionRole,
}

#[derive(Clone)]
pub enum SessionRole {
    Host {},
    Peer { session_name: String },
}

#[async_trait::async_trait]
pub trait NetworkAdapter: Send {
    async fn connect_and_run(
        session: SessionCfg,
        core_tx: mpsc::Sender<Event>,
        net_rx: mpsc::Receiver<NetworkCommand>,
    );
}

#[must_use]
pub fn into_internal(cmd: WireMessage, is_host: bool) -> Event {
    match cmd {
        WireMessage::Patch { uri, data } => {
            debug!("[Net] Received patch for {}", uri);
            Event::RemotePatch { uri, patch: data }
        }
        WireMessage::Cursor { uri, position } => {
            let (line, char) = position;
            Event::RemoteCursorChange {
                uri,
                position: Position {
                    line,
                    character: char,
                },
            }
        }
        WireMessage::RequestFullSync => {
            if is_host {
                debug!("[Net] Received full sync request");
                Event::PeerRequestedSync
            } else {
                Event::Ignoring
            }
        }
        WireMessage::FullSyncResponse { files } => {
            debug!("[Net] Received full sync containing {} files", files.len());
            Event::RemoteFullSync { files }
        }
    }
}

#[must_use]
pub fn into_external(cmd: NetworkCommand) -> WireMessage {
    match cmd {
        NetworkCommand::BroadcastCursor { uri, position } => WireMessage::Cursor { uri, position },
        NetworkCommand::BroadcastPatch { uri, patch } => WireMessage::Patch { uri, data: patch },
        NetworkCommand::SendFullSyncResponse { files } => WireMessage::FullSyncResponse { files },
    }
}

#[must_use]
pub fn make_transport_config() -> TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.max_concurrent_bidi_streams(VarInt::from_u32(100));
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(VarInt::from_u32(30_000).into()));
    transport_config
}

/// Configures client's network security options
///
/// # Arguments
///
/// * `token` - The TLS token
///
/// # Panics
///
/// * If the crypto config couldn't be applied to the `QuicClientConfig`
#[must_use]
pub fn configure_client() -> ClientConfig {
    // Use own verifier
    let verifier = Arc::new(NoVerifier {});

    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    // ALPN has to match
    crypto.alpn_protocols = vec![b"justsync".to_vec()];

    let mut config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto).unwrap()));
    config.transport_config(Arc::new(make_transport_config()));
    config
}
