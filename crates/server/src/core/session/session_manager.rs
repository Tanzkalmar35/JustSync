use std::sync::Arc;

use dashmap::DashMap;
use just_sync_protocol::client_relay_handshake::Capabilities;

use crate::{core::session::session::Session, models::Connection};

pub(in crate::core) struct SessionManager {
    sessions: Arc<DashMap<String, Session>>,
}

impl SessionManager {
    pub(in crate::core) fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub(in crate::core) fn create_session(&self, host: Arc<dyn Connection>) -> anyhow::Result<([u8; 16], Capabilities)> {
        let _session = Session::new(host);
        todo!()
    }
}
