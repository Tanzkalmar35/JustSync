use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use tokio::sync::mpsc;
use tracing::{error, info};
use uuid::Uuid;

use just_sync_client::{
    adapters::{fs::FileSystem, handler::StdioAdapter, network::QuicNetworkAdapter},
    context::{ClientContext, ClientMode},
    internal::{
        core::{Core, Event},
        crypto::hash,
        fs::FsOps,
        handler::EditorAdapter,
        network::{NetworkAdapter, NetworkCommand, SessionCfg, SessionRole},
        relay_endpoint::RelayEndpoint,
    },
    logger,
};

#[tokio::main]
pub async fn main() {
    // Setup Environment
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ctx = ClientContext::parse();
    let is_host = matches!(ctx.mode, ClientMode::Host { .. });
    let _log_guard = logger::init(
        &SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string(),
    );

    info!("Starting JustSync client");

    let (core_tx, core_rx) = mpsc::channel::<Event>(100);
    let (net_tx, net_rx) = mpsc::channel::<NetworkCommand>(100);
    let (editor_tx, editor_rx) = mpsc::channel(100);

    let agent_id = Uuid::new_v4().to_string();

    // Connect to relay and run network actor
    let role = if is_host {
        SessionRole::Host {}
    } else {
        SessionRole::Peer {
            session_name: ctx.session_name.unwrap(),
        }
    };
    let session = SessionCfg {
        agent_id: agent_id.clone(),
        key: hash(&ctx.key),
        relay_addr: RelayEndpoint::parse(&ctx.remote_ip, 5000)
            .expect("Invalid remote endpoint provided"),
        role,
    };
    let net_to_core_tx = core_tx.clone();

    tokio::spawn(async move {
        if let Err(e) = QuicNetworkAdapter::connect_and_run(session, net_to_core_tx, net_rx).await {
            error!("Network loop panicked: {}", e);
            std::process::exit(1);
        }
    });

    // Host: Scan files
    let fs = FileSystem {};
    if is_host {
        info!(">> Scanning workspace files...");

        let files = fs.scan_project_directory(".");
        for (uri, content) in files {
            if let Err(e) = core_tx.send(Event::LoadFromDisk { uri, content }).await {
                error!("{}", e)
            }
        }

        info!(">> File scanning complete!")
    }

    // Spawn Core
    let core = Core::new(agent_id, net_tx, editor_tx);
    tokio::spawn(core.run(core_rx, is_host, fs));

    // Run editor adapter on main thread
    let mut adapter = StdioAdapter::new(core_tx);
    adapter.run(editor_rx).await;
}
