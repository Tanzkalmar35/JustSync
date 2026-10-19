// //! Control messages exchanged between a peer and a relay server.
//
// use serde::{Deserialize, Serialize};
//
// /// Traffic between the hosting peer and the relay server.
// /// Peers ping the relay to see if it's up and running, so further traffic can be trusted to
// /// arrive.
// #[derive(Serialize, Deserialize)]
// pub enum PingRelayMsg {
//     /// Peer -> relay: Peer pings the relay to see if the open connection does its job.
//     Ping { intend: PingRelayIntend },
//
//     /// Relay -> peer: Response to a ping, signaling it's up and running (the negative case would
//     /// mean that the ping does not arrive and therefore no answer is sent).
//     /// TODO: Maybe introduce a detailed status, but probably not needed rn.
//     Pong {},
// }
//
// #[derive(Serialize, Deserialize)]
// pub enum PingRelayIntend {
//     CreateSession,
//     JoinSession,
//     // TODO: More?
// }
//
//
// /// Traffic between the hosting peer and the relay server.
// /// Successful case leads to a valid session being created on the relay server, so the host can
// /// invite peers to join that session.
// #[derive(Serialize, Deserialize)]
// pub enum CreateSessionMsg {
//     /// Hosting peer -> relay: Initiate a session creation
//     Register {
//         /// Protocol version
//         p_version: u16,
//         // TODO: maybe action???
//     },
//
//     /// Relay -> hosting peer: A challenge used for identification
//     Challenge {
//         /// Challenging nonce
//         nonce: [u8; 32],
//     },
//
//     /// Hosting peer -> relay: The command to actually initialize a new session, including the
//     /// response to the challenge
//     CreateSession {
//         /// Stored on the relay to identify the session owner
//         identity_pk: [u8; 32],
//
//         /// The sign of the challenge received previously from the host
//         /// Sign(identity_sk, "create" | nonce | exporter | session_id | identity_pk | adimission_token)
//         sig_id: [u8; 32],
//     },
//
//     /// Relay -> hosting peer: Session successfully created
//     SessionCreated {
//         /// The id of the created session used for the peer's invitation generation
//         session_id: [u8; 16],
//
//         /// The capabilities that were configured on the relay that apply for this session
//         capabilities: Capabilities,
//     },
// }
//
// #[derive(Serialize, Deserialize)]
// pub enum JoinSessionMsg {
//
// }
//
// /// Relay server capabilities
// #[derive(Serialize, Deserialize, Clone, Copy)]
// pub struct Capabilities {
//     /// The maximum lobby size
//     pub max_members: usize,
//     // TODO: Add more
// }
