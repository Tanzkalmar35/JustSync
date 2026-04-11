use std::sync::{Arc, Mutex};

use quinn::{Connection, SendStream};
use rand::RngExt;

use crate::{ControlMessage, connection::hotwire};

#[derive(Clone)]
pub struct Session {
    pub name: String,
    key: String,
    pub host: Connection,
    pub peers: Arc<Mutex<Vec<Connection>>>,
}

impl Session {
    pub fn new(host: Connection, key: String) -> Self {
        Self {
            name: Self::generate_name(),
            key,
            host,
            peers: Arc::new(Mutex::new(Vec::new()))
        }
    }

    pub async fn join(
        &mut self,
        peer: Connection,
        key: String,
        send: &mut SendStream,
    ) -> Result<(), String> {
        if !self.key.eq(&key) {
            return Err(String::from("Error joining session - invalid key"));
        }

        tokio::spawn(hotwire(self.host.clone(), peer.clone()));
        self.peers.lock().unwrap().iter().for_each(|p| {
            tokio::spawn(hotwire(p.clone(), peer.clone()));
        });

        let msg = ControlMessage::SessionJoined { status: String::from("ok") };
        send.write_all(&serde_json::to_vec(&msg).unwrap())
            .await
            .expect("Couldn't report status");

        self.peers.lock().expect("Couldn't lock peers...").push(peer.clone());
        Ok(())
    }

    pub fn regenerate_name(&mut self) {
        self.name = Self::generate_name();
    }

    fn generate_name() -> String {
        let names = petname::petname(2, "-").expect("petname session name generation failed!");
        let mut rng = rand::rng();
        let number: u16 = rng.random_range(100..1000);

        format!("{names}-{number}")
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
