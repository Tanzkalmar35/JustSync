use std::{io::Error, sync::Arc};

use anyhow::anyhow;
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

#[must_use]
fn err_accept(msg: &str) -> AcceptError {
    AcceptError::from_err(Error::other(msg))
}

#[derive(Debug)]
pub(super) struct IrohPeerManager {
    endpoint: Endpoint,

    agent_id: String,
    is_host: bool,
    peers: DashMap<String, IrohPeer>,

    core_tx: Arc<Mutex<mpsc::Sender<NetworkToCoreMessage>>>,
    net_rx: Arc<Mutex<mpsc::Receiver<CoreToNetworkMessage>>>,
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
        let mut peer = self
            .build_peer(connection)
            .await
            .map_err(|e| err_accept(&e.to_string()))?;

        // TODO: Possibly freeze session for this time?

        // Expect the new peer to immediatly send their identification including agent_id
        match peer.recv().await.map_err(|e| err_accept(&e.to_string()))? {
            Some(SetupMessage::Id { agent_id }) => self
                .init_peer(peer, agent_id)
                .await
                .map_err(|e| err_accept(&e.to_string()))?,
            _ => return Err(err_accept("Failed to follow init protocol for new peer")),
        }

        Ok(())
    }
}

impl IrohPeerManager {
    pub(super) fn new(
        endpoint: Endpoint,
        agent_id: String,
        is_host: bool,
        core_tx: Arc<Mutex<mpsc::Sender<NetworkToCoreMessage>>>,
        net_rx: Arc<Mutex<mpsc::Receiver<CoreToNetworkMessage>>>,
    ) -> Self {
        Self {
            endpoint,
            agent_id,
            is_host,
            peers: DashMap::new(),
            core_tx,
            net_rx,
        }
    }

    async fn build_peer(&self, conn: iroh::endpoint::Connection) -> anyhow::Result<IrohPeer> {
        let endpoint_id = conn.remote_id();
        println!("Incoming connection from {endpoint_id}");

        let (send, recv) = conn.accept_bi().await?;

        let remote_id = conn.remote_id();
        if let Some(info) = self.endpoint.remote_info(remote_id).await {
            let addr =
                EndpointAddr::from_parts(info.id(), info.into_addrs().map(|addr| addr.into_addr()));

            let peer = IrohPeer {
                addr,
                sender: send,
                receiver: recv,
            };
            Ok(peer)
        } else {
            Err(anyhow!("Failed to obtain remote information of new peer"))
        }
    }

    async fn init_peer(&self, mut peer: IrohPeer, agent_id: String) -> anyhow::Result<()> {
        // Provide all necessary session information to new peer
        peer.send(&SetupMessage::SessionInformation {
            host_agent_id: self.agent_id.clone(),
            peers: self
                .peers
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().addr.clone()))
                .collect(),
        })
        .await
        .map_err(|e| err_accept(&e.to_string()))?;

        if self.is_host {
            // Send full application state
            self.core_tx
                .send(NetworkToCoreMessage::PeerRequestedSync {})
                .await?;

            if let Some(CoreToNetworkMessage::SendFullSyncResponse { files }) =
                self.net_rx.recv().await
            {
                peer.send(&SetupMessage::FullSync { files }).await?;
            }
        }

        self.peers.insert(agent_id, peer);
        Ok(())
    }
}
