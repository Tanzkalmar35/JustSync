use serde::{Deserialize, Serialize};

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
