use dashmap::DashMap;
use iroh::{
    endpoint::{RecvStream, SendStream},
    protocol::AcceptError,
};
use quinn::udp::RecvMeta;

use crate::adapters::network::protocol::{SetupMessage, receive_message};

#[derive(Debug)]
struct IrohPeer {
    sender: SendStream,
    receiver: RecvStream,
}

#[derive(Debug)]
pub(super) struct IrohPeerManager {
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

        let (send, mut recv) = connection.accept_bi().await?;

        // Expect the new peer to immediatly send their identification including agent_id
        if let Some(SetupMessage::Id { agent_id }) = receive_message(&mut recv).await.unwrap() {
            // Insert new peer
            let peer = IrohPeer {
                sender: send,
                receiver: recv,
            };
            self.peers.insert(agent_id, peer);

            return Ok(());
        } else {
            eprintln!("Invalid protocol from new peer -> closing!");
            return Err(AcceptError::NotAllowed { meta:  })
        }
    }
}

impl IrohPeerManager {
    pub(super) fn new() -> Self {
        Self {
            peers: DashMap::new(),
        }
    }
}
