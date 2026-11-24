use std::io::Error;

use dashmap::DashMap;
use iroh::protocol::AcceptError;

use crate::adapters::network::{
    iroh::peer::IrohPeer,
    protocol::{AsyncReceiver, AsyncSender, SetupMessage},
};

#[derive(Debug)]
pub(super) struct IrohPeerManager {
    agent_id: String,
    is_host: bool,
    peers: DashMap<String, IrohPeer>,
}

impl iroh::protocol::ProtocolHandler for IrohPeerManager {
    /// Accepts a new connection.
    ///
    /// # Returns
    ///
    /// Nothing, if successful, an `iroh::protocol::AcceptError` if not.
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let endpoint_id = connection.remote_id();
        println!("incoming connection from {endpoint_id}");

        let (send, recv) = connection.accept_bi().await?;
        let mut peer = IrohPeer {
            sender: send,
            receiver: recv,
        };

        // Expect the new peer to immediatly send their identification including agent_id
        if let Some(SetupMessage::Id { agent_id }) = peer.recv().await.unwrap() {
            if let Err(e) = peer
                .send(&SetupMessage::Id {
                    agent_id: self.agent_id.clone(),
                })
                .await
            {
                eprintln!("Failed to provide own identity to new peer: {e}");
                return Err(AcceptError::from_err(Error::other(
                    "Failed to provide own identity to new peer",
                )));
            }

            if self.is_host {
                peer.send(&SetupMessage::FullSync { files: () })
            }

            self.peers.insert(agent_id, peer);

            return Ok(());
        } else {
            eprintln!("Invalid protocol from new peer -> closing!");
            return Err(AcceptError::from_err(Error::other(
                "New peer follows invalid protocol",
            )));
        }
    }
}

impl IrohPeerManager {
    pub(super) fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            peers: DashMap::new(),
        }
    }
}
