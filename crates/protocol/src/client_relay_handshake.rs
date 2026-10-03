//! Control messages exchanged between a peer and a relay server.

use serde::Serialize;

/// Traffic between the hosting peer and the relay server.
/// Successful case leads to a valid session being created on the relay server, so the host can
/// invite peers to join that session.
#[derive(Serialize, Deserialize)]
pub enum CreateSessionMsg {
    /// Hosting peer -> relay: Initiate a session creation
    Register {
        /// Protocol version
        p_version: u16,

        // TODO: maybe action???
    },

    /// Relay -> hosting peer: A challenge used for identification
    Challenge {
        /// Challenging nonce
        nonce: [u8; 32],

        /// Time frame in which the peer has to respond
        deadline_ms: u32,
    },

    /// Hosting peer -> relay: The command to actually initialize a new session, including the
    /// response to the challenge
    CreateSession {
        /// Stored on the relay to identify the session owner
        identity_pk: [u8; 32],

        /// Policy defined by relay; TODO: Determine design
        admission_token: Option<u8>,

        /// The sign of the challenge received previously from the host
        /// Sign(identity_sk, "create" | nonce | exporter | session_id | identity_pk | adimission_token)
        sig_id: [u8; 32],
    },

    /// Relay -> hosting peer: Session successfully created
    SessionCreated {
        /// The id of the created session used for the peer's invitation generation
        session_id: [u8; 16],

        /// The relay-captured time of session creation
        server_time: [u8; 64],

        /// The capabilities that were configured on the relay that apply for this session
        capabilities: Capabilities,
    },
}

/// Relay server capabilities
pub struct Capabilities {
    /// The maximum lobby size
    pub max_members: usize,
    
    // TODO: Add more
}
