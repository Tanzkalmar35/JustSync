use quinn::{Connection, SendStream};
use rand::RngExt;

use crate::connection::connect_to_host;

#[derive(Clone)]
pub struct Session {
    pub name: String,
    key: String,
    pub host: Connection,
}

impl Session {
    pub fn new(host: Connection, key: String) -> Self {
        let names = petname::petname(2, "-").expect("petname session name generation failed!");
        let mut rng = rand::rng();
        let number: u16 = rng.random_range(100..1000);

        let session_name = format!("{names}-{number}");

        Self {
            name: session_name,
            key,
            host,
        }
    }

    pub async fn join(
        &self,
        peer: Connection,
        key: String,
        send: &mut SendStream,
    ) -> Result<(), String> {
        if !self.key.eq(&key) {
            return Err(String::from("Error joining session - invalid key"));
        }

        // Send an "OK" to the peer
        send.write_all(b"{\"status\":\"ok\"}").await.expect("Couldn't report status");
        send.finish();

        tokio::spawn(connect_to_host(self.host.clone(), peer));

        Ok(())
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
