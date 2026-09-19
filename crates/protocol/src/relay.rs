//! Control messages exchanged between a client and the relay server.

use serde::{Deserialize, Serialize};

/// Messages exchanged during setup of a new client on a relay server.
/// Controls how and where a new client connects to a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Host -> relay: request the creation of a new session with the given key.
    Register { key: String },

    /// Relay -> host: the session was created under `name` with the given `status`.
    SessionCreated { status: String, name: String },

    /// Peer -> relay: request to join the session `name` using `key`.
    Join { name: String, key: String },

    /// Relay -> peer: the result of a [`ControlMessage::Join`] request.
    SessionJoined { status: String },
}
