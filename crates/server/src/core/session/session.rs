use std::sync::Arc;

use just_sync_protocol::client_relay_handshake::Capabilities;
use tokio::sync::Mutex;

use crate::models::Connection;

const DEFAULT_CAPABILITIES: Capabilities = Capabilities { 
    max_members: 10 
};

pub(super) struct Session {
    host: Arc<dyn Connection>,
    peers: Arc<Mutex<Vec<Box<dyn Connection>>>>,
    capabilities: Capabilities,
}

impl Session {
    pub(super) fn new(host: Arc<dyn Connection>) -> Self {
        Self {
            host: host,
            peers: Arc::new(Mutex::new(Vec::new())),
            capabilities: DEFAULT_CAPABILITIES,
        }
    }

    // pub(super) fn register_invite(invite) -> anyhow::Result<()> {}

    // pub(super) fn validate_invite(invite) -> bool {}

    // pub(super) fn redeem_invite(invite) -> anyhow::Result<()> {}
}
