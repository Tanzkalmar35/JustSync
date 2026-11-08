use tracing::debug;

use crate::{
    adapters::network::protocol::WireMessage,
    internal::{core::NetworkToCoreMessage, lsp::Position, network::CoreToNetworkMessage},
};

/// Handles translating a message from the network to a message the just_sync_client core can interpret.
///
/// # Arguments
///
/// * `cmd` - The cmd to translate.
/// * `agent_id` - The agent_id of the peer that made the change.
/// * `is_host` - Whether the peer that made the change is the hosting peer.
///
/// # Returns
///
/// The `Event` translation result.
#[must_use]
pub fn into_internal(cmd: WireMessage, agent_id: &str, is_host: bool) -> NetworkToCoreMessage {
    match cmd {
        WireMessage::Patch { uri, data } => {
            debug!("[Net] Received patch for {}", uri);
            NetworkToCoreMessage::RemotePatch { uri, patch: data }
        }
        WireMessage::Cursor { uri, position } => {
            let (line, char) = position;
            NetworkToCoreMessage::RemoteCursorChange {
                agent_id: agent_id.to_string(),
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
                NetworkToCoreMessage::PeerRequestedSync
            } else {
                NetworkToCoreMessage::Ignoring
            }
        }
        WireMessage::FullSyncResponse { files } => {
            debug!("[Net] Received full sync containing {} files", files.len());
            NetworkToCoreMessage::RemoteFullSync { files }
        }
    }
}

/// Translates a message from the insides of just_sync_client into a message that's ready for network sync.
///
/// # Arguments
///
/// * `cmd` - The internal message to translate to a network message.
///
/// # Returns
///
/// The [`WireMessage`] translation result.
#[must_use]
pub fn into_external(cmd: CoreToNetworkMessage) -> WireMessage {
    match cmd {
        CoreToNetworkMessage::BroadcastCursor { uri, position } => {
            WireMessage::Cursor { uri, position }
        }
        CoreToNetworkMessage::BroadcastPatch { uri, patch } => {
            WireMessage::Patch { uri, data: patch }
        }
        CoreToNetworkMessage::SendFullSyncResponse { files } => {
            WireMessage::FullSyncResponse { files }
        }
    }
}
