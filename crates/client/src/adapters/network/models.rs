use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// The packet we serialize and send over the QUIC stream.
#[derive(Serialize, Deserialize, Debug, Clone)]
enum WireMessage {
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
enum ControlMessage {
    Register { key: String },
    SessionCreated { status: String, name: String },
    Join { name: String, key: String },
    SessionJoined { status: String },
    InitPeer { agent_id: String, is_host: bool },
}

pub trait NetworkAdapter {
    async fn connect( relay_addr: SocketAddr, token: &str,) -> Result<quinn::Connection, Box<dyn std::error::Error>>; 
}
