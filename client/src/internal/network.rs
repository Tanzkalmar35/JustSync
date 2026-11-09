use std::{sync::Arc, time::Duration};

use quinn::{ClientConfig, TransportConfig, VarInt, crypto::rustls::QuicClientConfig};
use ring::signature::Ed25519KeyPair;
use tokio::sync::mpsc;

use crate::internal::{core::NetworkToCoreMessage, crypto::NoVerifier};

#[derive(Debug)]
pub enum CoreToNetworkMessage {
    BroadcastCursor {
        uri: String,
        position: (usize, usize),
    },
    BroadcastPatch {
        uri: String,
        patch: Vec<u8>,
    },
    SendFullSyncResponse {
        files: Vec<(String, Vec<u8>)>,
    },
}

pub struct SessionCfg {
    pub agent_id: String,

    /// Holds an invariant: None means the session is host - Some(..) means joining peer.
    pub invitation: Option<String>,

    /// The Ed25519 private/public keypair used for authentication.
    pub keypair: Ed25519KeyPair,
}

#[derive(Clone)]
pub enum SessionRole {
    Host {},
    Peer { session_name: String },
}

#[async_trait::async_trait]
pub trait NetworkAdapter: Send {
    async fn setup(
        core_tx: mpsc::Sender<NetworkToCoreMessage>,
        net_rx: mpsc::Receiver<CoreToNetworkMessage>,
    ) -> anyhow::Result<impl NetworkAdapter>;

    async fn start();

    async fn shutdown();

    // async fn connect_and_run(
    //     session: SessionCfg,
    //     core_tx: mpsc::Sender<NetworkToCoreMessage>,
    //     net_rx: mpsc::Receiver<CoreToNetworkEvent>,
    // ) -> anyhow::Result<()>;
}

/// Builds the transport configuration for quinn.
#[must_use]
pub fn make_transport_config() -> TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.max_concurrent_bidi_streams(VarInt::from_u32(100));
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(VarInt::from_u32(30_000).into()));
    transport_config
}

/// Configures client's network security options
///
/// # Arguments
///
/// * `token` - The TLS token
///
/// # Panics
///
/// * If the crypto config couldn't be applied to the `QuicClientConfig`
///
/// # Returns
///
/// The quinn ClientConfig ready for use.
#[must_use]
pub fn configure_client() -> ClientConfig {
    // Use own verifier
    let verifier = Arc::new(NoVerifier {});

    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    // ALPN has to match
    // crypto.alpn_protocols = alpn(); TODO

    let mut config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto).unwrap()));
    config.transport_config(Arc::new(make_transport_config()));
    config
}
