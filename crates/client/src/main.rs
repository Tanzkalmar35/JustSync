use clap::{Arg, Command};
use std::process::exit;
use tokio::sync::mpsc;
use tracing::{error, info};
use uuid::Uuid;

use just_sync_client::{
    adapters::{fs::FileSystem, handler::StdioAdapter, network::QuicNetworkAdapter},
    internal::{
        core::{Core, Event},
        crypto::hash,
        fs::FsOps,
        handler::EditorAdapter,
        network::{NetworkAdapter, NetworkCommand, SessionCfg, SessionRole},
    },
    logger,
};

struct Context {
    mode: String,
    remote_ip: String,
    session_name: Option<String>,
    key: String,
}

#[tokio::main]
pub async fn main() {
    // Setup Environment
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ctx = parse_cmd();
    let is_host = ctx.mode == "host";

    let _log_guard = logger::init(&ctx.mode);
    info!("Starting JustSync in {} mode", ctx.mode);

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
        relay_addr: ctx.remote_ip.parse().unwrap(),
        role,
    };
    tokio::spawn(QuicNetworkAdapter::connect_and_run(
        session,
        core_tx.clone(),
        net_rx,
    ));

    // Host: Scan files
    let fs = FileSystem {};
    if is_host {
        info!(">> Scanning workspace files...");
        let files = fs.scan_project_directory(".");
        for (uri, content) in files {
            let _ = core_tx.send(Event::LoadFromDisk { uri, content }).await;
        }
    }

    // Spawn Core
    let core = Core::new(agent_id, net_tx, editor_tx);
    tokio::spawn(core.run(core_rx, is_host, fs));

    // Run editor adapter on main thread
    let mut adapter = StdioAdapter::new(core_tx);
    adapter.run(editor_rx).await;
}

fn parse_cmd() -> Context {
    let matches = Command::new("just_sync_client")
        .version("1.0")
        .about("A real-time, editor agnostic collaboration engine")
        .arg(
            Arg::new("mode")
                .long("mode")
                .help("The daemon mode (host / peer)")
                .required(true),
        )
        .arg(
            Arg::new("remote-ip")
                .long("remote-ip")
                .help("The remote ip address to connect to (required for peer)")
                .required(true),
        )
        .arg(
            Arg::new("name")
                .long("session-name")
                .help("The name of the session to join (retrieve from host)")
                .required(false),
        )
        .arg(
            Arg::new("key")
                .long("key")
                .help("The security token (required for peer)")
                .required(true),
        )
        .arg(
            Arg::new("stdio")
                .long("stdio")
                .hide(true)
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let mode = matches.get_one::<String>("mode").unwrap().clone();
    let remote_ip = matches
        .get_one::<String>("remote-ip")
        .cloned()
        .expect("Expected remote ip");
    let session_name = matches.get_one::<String>("name").cloned();
    let key = matches
        .get_one::<String>("key")
        .cloned()
        .expect("Expected session key");

    if mode != "host" && mode != "peer" {
        error!("Invalid mode. Use --mode host or --mode peer.");
        exit(1);
    }

    Context {
        mode,
        remote_ip,
        session_name,
        key,
    }
}
