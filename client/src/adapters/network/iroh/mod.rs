use iroh::{Endpoint, EndpointAddr, endpoint::presets};

use crate::internal::network::NetworkAdapter;

const ALPN: &str = "justsync_v1";

pub(crate) struct IrohNetworkAdapter {}

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
    
    async fn setup() -> anyhow::Result<Self> {
        let endpoint = Endpoint::bind(presets::N0).await?;

        endpoint.online().await;
        endpoint.connect(EndpointAddr::new(id), ALPN.as_bytes()).await;

        Ok(Self{})
    }

    async fn run() {}
}

impl IrohNetworkAdapter {
}
