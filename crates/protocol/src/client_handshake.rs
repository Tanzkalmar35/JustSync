//! Control messages exchanged directly between two clients while they set up a p2p, e2ee session.

use serde::{Deserialize, Serialize};

/// Messages used to identify peers and run the SPAKE2 key exchange between them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Announces a peer on the freshly opened stream, offering the most basic kind of
    /// identification.
    InitPeer {
        agent_id: String,
        is_host: bool,
    },

    /// Spake setup messages including the respective SPAKE2 key.
    /// Spake setup works by 2 peers exchanging their respective public key, in order.
    /// That means, `ControlMessage::Spake2MsgA` will always be sent & read first, then B gets sent.
    /// The determination of which peer sends A and which sends B is left open to the actual
    /// implementation.
    Spake2MsgA {
        data: Vec<u8>,
    },
    Spake2MsgB {
        data: Vec<u8>,
    },
}
