//! Messages exchanged between two peers after [`crate::handshake`] completed.
//!
//! These are the payloads of the end-to-end encrypted data channel. They travel
//! through the relay server, which hotwires the two peers together and treats
//! every byte as opaque.

use serde::{Deserialize, Serialize};

/// A single unit of project state broadcast between peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    /// The full contents of the file at `uri`.
    Patch { uri: String, data: Vec<u8> },

    /// A cursor movement of the sending peer inside the file at `uri`.
    /// The position value represents the cursor's new position.
    Cursor {
        uri: String,
        position: (usize, usize),
    },

    /// A new peer requests the full project state from the session host.
    RequestFullSync,

    /// The session host's answer `RequestFullSync` with the full project state, (uri -> content).
    FullSyncResponse { files: Vec<(String, Vec<u8>)> },
}
