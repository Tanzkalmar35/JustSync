use serde::{Deserialize, Serialize};

pub const CREATE_SESSION_EXPORTER: &[u8] = b"justsync/1 create-session";

#[derive(Serialize, Deserialize)]
pub enum HostToRelay {
    Register { p_version: u16 },
    CreateSession { identity_pk: [u8; 32], sig: [u8; 32] },
}

#[derive(Serialize, Deserialize)]
pub enum RelayToHost {
    Challenge {
        nonce: [u8; 32],
        session_id: [u8; 16],
    },
    SessionCreated {
        capabilities: Capabilities,
    },
}

/// Relay server capabilities
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Capabilities {
    /// The maximum lobby size
    pub max_members: u16,
    // TODO: Add more
}
