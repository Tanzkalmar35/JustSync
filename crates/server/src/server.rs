use dashmap::DashMap;
use std::sync::Arc;

use crate::session::Session;

#[derive(Clone)]
pub struct Server {
    // Session name -> Session
    sessions: Arc<DashMap<String, Session>>,
}

impl Server {
    pub fn setup() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn register_session(&self, mut session: Session) {
        while self.sessions.contains_key(&session.name) {
            session.regenerate_name();
        }

        self.sessions.insert(session.name.clone(), session);
    }

    pub fn deregister_session(&self, s: String) -> Result<(), String> {
        if !self.sessions.contains_key(&s) {
            return Err(String::from(
                "Error deregistering session - No session to deregister found!",
            ));
        }

        self.sessions.remove(&s);
        Ok(())
    }

    pub fn find_session(&self, name: &str) -> Option<Session> {
        self.sessions.get(name).map(|s| s.value().clone())
    }
}
