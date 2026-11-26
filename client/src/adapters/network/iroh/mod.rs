use std::sync::Arc;

use anyhow::anyhow;
use tokio::sync::{Mutex, mpsc};

use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};

use crate::{
    adapters::network::iroh::manager::IrohPeerManager,
    internal::{
        core::NetworkToCoreMessage,
        network::{CoreToNetworkMessage, NetworkAdapter},
    },
};

mod manager;
mod peer;

const ALPN: &str = "justsync_v1";

pub(crate) struct IrohNetworkAdapter {
    endpoint: Endpoint,
    router: Router,

    core_tx: Arc<Mutex<mpsc::Sender<NetworkToCoreMessage>>>,
    net_rx: Arc<Mutex<mpsc::Receiver<CoreToNetworkMessage>>>,

    is_host: bool,
}

#[async_trait::async_trait]
impl NetworkAdapter for IrohNetworkAdapter {
    async fn start(
        agent_id: String,
        is_host: bool,
        core_tx: mpsc::Sender<NetworkToCoreMessage>,
        net_rx: mpsc::Receiver<CoreToNetworkMessage>,
    ) -> anyhow::Result<Self> {
        let secret_key = SecretKey::generate();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .bind()
            .await?;

        let shared_core_tx = Arc::new(Mutex::new(core_tx));
        let shared_net_rx = Arc::new(Mutex::new(net_rx));

        let protocol_handler = IrohPeerManager::new(endpoint.clone(), agent_id, is_host, shared_core_tx, shared_net_rx);
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, protocol_handler)
            .spawn();

        endpoint.online().await;

        Ok(Self {
            endpoint,
            router,
            core_tx: shared_core_tx,
            net_rx: shared_net_rx,
            is_host,
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
