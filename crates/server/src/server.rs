use crate::session::{Session};


pub struct Server {
    sessions: Vec<Session>
}

impl Server {
    pub fn register_session(&mut self, s: Session) {
        self.sessions.push(s);
    }

    pub fn deregister_session(&mut self, s: Session) -> Result<(), String> {
        if !self.sessions.contains(&s) { 
            return Err(String::from("Error deregistering session - No session to deregister found!"));
        }

        self.sessions.retain(|session| session.name != s.name);
        Ok(())
    }
}
