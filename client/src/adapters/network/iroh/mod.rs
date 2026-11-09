use tokio::sync::mpsc;

use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};

use crate::{
    adapters::network::iroh::protocol::IrohProtocol,
    internal::{
        core::NetworkToCoreMessage,
        network::{CoreToNetworkMessage, NetworkAdapter},
    },
};

mod protocol;

const ALPN: &str = "justsync_v1";

pub(crate) struct IrohNetworkAdapter {
    endpoint: Endpoint,
    router: Router,

    core_tx: mpsc::Sender<NetworkToCoreMessage>,
    net_rx: mpsc::Receiver<CoreToNetworkMessage>,
}

#[async_trait::async_trait]
impl NetworkAdapter for IrohNetworkAdapter {
    // async fn connect_and_run(
    //     session: SessionCfg,
    //     core_tx: mpsc::Sender<NetworkToCoreMessage>,
    //     net_rx: mpsc::Receiver<CoreToNetworkEvent>,
    // ) -> anyhow::Result<()> {
    //
    //     Ok(())
    // }

    async fn setup(
        core_tx: mpsc::Sender<NetworkToCoreMessage>,
        net_rx: mpsc::Receiver<CoreToNetworkMessage>,
    ) -> anyhow::Result<Self> {
        let secret_key = SecretKey::generate();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .bind()
            .await?;

        let protocol_handler = IrohProtocol {};
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, protocol_handler)
            .spawn();

        // endpoint.online().await;
        // endpoint.connect(endpoint.addr(), ALPN.as_bytes()).await;

        Ok(Self {
            endpoint,
            router,
            core_tx,
            net_rx,
        })
    }

    async fn start() {}

    async fn shutdown() {}
}

impl IrohNetworkAdapter {}
