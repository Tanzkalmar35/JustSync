use std::{collections::HashMap, io::Error};

use dashmap::DashMap;
use iroh::{Endpoint, EndpointAddr, protocol::AcceptError};
use tokio::sync::{Mutex, mpsc};

use crate::{
    adapters::network::{
        iroh::peer::IrohPeer,
        protocol::{AsyncReceiver, AsyncSender, SetupMessage},
    },
    internal::{core::NetworkToCoreMessage, network::CoreToNetworkMessage},
};

#[derive(Debug)]
pub(super) struct IrohPeerManager {
    endpoint: Endpoint,

    agent_id: String,
    is_host: bool,
    peers: DashMap<String, IrohPeer>,

    core_tx: mpsc::Sender<NetworkToCoreMessage>,
    net_rx: Mutex<mpsc::Receiver<CoreToNetworkMessage>>,
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
        println!("Incoming connection from {endpoint_id}");

        let (send, recv) = connection.accept_bi().await?;

        let remote_id = connection.remote_id();
        let mut peer: IrohPeer;
        if let Some(info) = self.endpoint.remote_info(remote_id).await {
            let addr = EndpointAddr::from_parts(
                info.id(),
                info.into_addrs().map(|addr| addr.into_addr()),
            );

            peer = IrohPeer {
                addr,
                sender: send,
                receiver: recv,
            };
        }

        // Expect the new peer to immediatly send their identification including agent_id
        if let Some(SetupMessage::Id { agent_id }) = peer.recv().await.unwrap() {
            // Identify self on remote peer
            let peers = 
            if let Err(e) = peer
                .send(&SetupMessage::SessionInformation { 
                    host_agent_id: self.agent_id.clone(),
                    peers: self.peers.iter().collect()
                })
                .await
            {
                eprintln!("Failed to provide own identity to new peer: {e}");
                return Err(AcceptError::from_err(Error::other(
                    "Failed to provide own identity to new peer",
                )));
            };

            // If localhost is the session host, send the full state
            if self.is_host {
                if let Err(e) = self.init_peer(&mut peer).await {
                    eprintln!("Failed to init the full state for the new peer");
                    return Err(AcceptError::from_err(Error::other(e.to_string())));
                }
            }

            // Insert into list of peers, including it in broadcast messages
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
    pub(super) fn new(
        agent_id: String,
        is_host: bool,
        core_tx: mpsc::Sender<NetworkToCoreMessage>,
        net_rx: mpsc::Receiver<CoreToNetworkMessage>,
    ) -> Self {
        Self {
            agent_id,
            is_host,
            peers: DashMap::new(),
            core_tx,
            net_rx: Mutex::new(net_rx),
        }
    }

    async fn init_peer(&self, peer: &mut IrohPeer) -> anyhow::Result<()> {
        // Send full application state
        self.core_tx
            .send(NetworkToCoreMessage::PeerRequestedSync {})
            .await?;

        if let Some(CoreToNetworkMessage::SendFullSyncResponse { files }) =
            self.net_rx.lock().await.recv().await
        {
            peer.send(&SetupMessage::FullSync { files }).await?;
        }

        Ok(())
    }
}
