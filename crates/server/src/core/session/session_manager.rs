use std::sync::Arc;

use dashmap::{DashMap, Entry};
use just_sync_protocol::client_relay_handshake::Capabilities;

use crate::core::session::session::Session;

pub(in crate::core) struct SessionManager {
    sessions: Arc<DashMap<[u8; 16], Session>>, // id -> session
}

impl SessionManager {
    /// Initializes a new instance of the session manager with no default sessions.
    ///
    /// # Returns
    ///
    /// The newly created instance of `SessionManager`.
    pub(in crate::core) fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    /// Creates a new session with a unique session id.
    ///
    /// # Arguments
    ///
    /// * `host` - The hosting peer of the new session.
    ///
    /// # Returns
    ///
    /// A tuple consisting of the new session id and the session's capabilities.
    pub(in crate::core) fn create_session(
        &self,
        host: quinn::Connection,
    ) -> ([u8; 16], Capabilities) {
        let session = Session::new(host);
        let capabilities = session.capabilities;

        loop {
            let id: [u8; 16] = rand::random();

            if let Entry::Vacant(entry) = self.sessions.entry(id) {
                entry.insert(session);
                return (id, capabilities);
            }
        }
    }
}
