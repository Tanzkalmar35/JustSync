use rand::RngExt;

use crate::{connection::Connection, session, user::{Host, Peer}};


pub struct Session {
    // List of room names
    pub name: String,
    key: String,
    host: Host,
    connections: Vec<Connection>
}

impl Session {
    pub fn new(host: Host, key: String) -> Self {
        let names = petname::petname(2, "-").expect("petname session name generation failed!");
        let mut rng = rand::rng();
        let number: u16 = rng.random_range(100..1000);

        let session_name = format!("{names}-{number}");

        Self {
            name: session_name,
            key,
            host,
            connections: vec![]
        }
    }

    pub fn join(&mut self, peer: Peer, key: String) -> Result<(), String> {
        if !self.key.eq(&key) {
            return Err(String::from("Error joining session - invalid key"));
        }

        // Patch host and peer together ("extension cord")

        // Add patch to self.connections

        Ok(())
    }

    // pub fn leave(&mut self, peer: Peer) -> Result<(), String> {
    // }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
