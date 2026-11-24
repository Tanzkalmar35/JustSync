use anyhow::anyhow;
use tokio::sync::mpsc;

use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};

use crate::{
    adapters::network::iroh::manager::IrohPeerManager,
    internal::{
        core::NetworkToCoreMessage,
        network::{CoreToNetworkMessage, NetworkAdapter},
    },
};

mod peer;
mod manager;

const ALPN: &str = "justsync_v1";

pub(crate) struct IrohNetworkAdapter {
    endpoint: Endpoint,
    router: Router,

    core_tx: mpsc::Sender<NetworkToCoreMessage>,
    net_rx: mpsc::Receiver<CoreToNetworkMessage>,
}

#[async_trait::async_trait]
impl NetworkAdapter for IrohNetworkAdapter {
    async fn start(
        agent_id: String,
        core_tx: mpsc::Sender<NetworkToCoreMessage>,
        net_rx: mpsc::Receiver<CoreToNetworkMessage>,
    ) -> anyhow::Result<Self> {
        let secret_key = SecretKey::generate();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .bind()
            .await?;

        let protocol_handler = IrohPeerManager::new(agent_id);
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, protocol_handler)
            .spawn();

        endpoint.online().await;

        Ok(Self {
            endpoint,
            router,
            core_tx,
            net_rx,
        })
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        self.router
            .shutdown()
            .await
            .map_err(|e| anyhow!(e.to_string()))
    }
}

impl IrohNetworkAdapter {}
