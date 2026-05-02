use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{ClientConfig, TransportConfig, VarInt, crypto::rustls::QuicClientConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    internal::{core::Event, crypto::TokenVerifier, lsp::Position},
    logger,
};

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

pub trait NetworkAdapter: Send {
    async fn connect_and_run(
        session: SessionCfg,
        core_tx: mpsc::Sender<Event>,
        net_rx: mpsc::Receiver<NetworkCommand>,
    );
}

pub fn into_internal(cmd: WireMessage, is_host: bool) -> Event {
    return match cmd {
        WireMessage::Patch { uri, data } => {
            logger::log(&format!(">> [Network] Received patch for {}", uri));
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
                logger::log(">> [Network] Received sync request from peer.");
                Event::PeerRequestedSync
            } else {
                Event::Ignoring
            }
        }
        WireMessage::FullSyncResponse { files } => {
            logger::log(&format!(
                ">> [Network] Received full sync response with {} files.",
                files.len()
            ));
            Event::RemoteFullSync { files }
        }
    };
}

pub fn into_external(cmd: NetworkCommand) -> WireMessage {
    return match cmd {
        NetworkCommand::BroadcastCursor { uri, position } => WireMessage::Cursor { uri, position },
        NetworkCommand::BroadcastPatch { uri, patch } => WireMessage::Patch { uri, data: patch },
        NetworkCommand::SendFullSyncResponse { files } => WireMessage::FullSyncResponse { files },
    };
}

pub fn make_transport_config() -> TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.max_concurrent_bidi_streams(VarInt::from_u32(100));
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(VarInt::from_u32(30_000).into()));
    transport_config
}

pub fn configure_client(token: &str) -> ClientConfig {
    // Use own verifier
    let verifier = TokenVerifier::new(token);

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
