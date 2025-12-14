use clap::Command;
use std::{
    net::SocketAddr,
    process::exit,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    handler::{Handler, perform_editor_handshake},
    network::NetworkManager,
    state::Workspace,
};

pub mod diff;
pub mod fs;
pub mod handler;
pub mod logger;
pub mod lsp;
pub mod network;
pub mod state;

pub struct Context {
    pub mode: String,
    pub remote_ip: Option<String>,
}

#[tokio::main]
pub async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ctx = parse_cmd();

    match ctx.mode.as_str() {
        "host" => start_host().await,
        "peer" => start_peer(ctx.remote_ip).await,
        _ => {
            logger::log(
                "[Daemon] Exiting due to invalid mode provided, expected was 'join' | 'peer'",
            );
            exit(1);
        }
    }
}

fn parse_cmd() -> Context {
    let matches = Command::new("JustSync")
        .version("1.0")
        .about("A real-time, editor agnostic collaboration engine written in Rust")
        .arg(
            clap::Arg::new("mode")
                .help("The daemon mode (join / host)")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::new("remote-ip")
                .help("The remote ip address to connect to")
                .required(false)
                .index(2),
        )
        .get_matches();

    let mode = matches.get_one::<String>("mode").unwrap().clone();
    let remote_ip = matches.get_one::<String>("remote-ip").cloned();

    Context { mode, remote_ip }
}

async fn start_host() {
    crate::logger::init(true);

    // Block for editor handshake
    let (root_dir, stdin, stdout) = perform_editor_handshake().await;

    let agent_id = Uuid::new_v4().to_string();
    let workspace = Arc::new(Mutex::new(Workspace::new(agent_id)));

    // network_tx: Local patches -> Network
    // editor_tx: Network patches -> Local Editor
    let (network_tx, mut network_rx) = mpsc::channel::<(String, Vec<u8>)>(4096);
    let (editor_tx, editor_rx) = mpsc::channel(4096);

    // Start the network process
    let net_workspace = workspace.clone();
    let net_editor_tx = editor_tx.clone();

    tokio::spawn(async move {
        let net = NetworkManager::init_host(4444).expect("Could not bind port 4444");

        crate::network::run_network_loop(
            net,
            true,
            None,
            &mut network_rx,
            &net_editor_tx,
            net_workspace,
        )
        .await;
    });

    // Start editor handler
    let handler = Handler::new(workspace, network_tx, root_dir);
    handler.run_with_streams(stdin, stdout, editor_rx).await;
}

async fn start_peer(remote_ip: Option<String>) {
    crate::logger::init(false);

    // Editor handshake
    let (root_dir, stdin, stdout) = perform_editor_handshake().await;

    // Parse IP
    let raw_ip = remote_ip.expect("Peer mode requires a remote IP!");

    // Auto-add port 4444 if missing
    let addr_str = if raw_ip.contains(':') {
        raw_ip
    } else {
        format!("{}:4444", raw_ip)
    };

    let ip: SocketAddr = addr_str
        .parse()
        .expect("Invalid IP Address format. Use IP:PORT");

    // State
    let agent_id = Uuid::new_v4().to_string();
    let workspace = Arc::new(Mutex::new(Workspace::new(agent_id)));

    // network_tx: Local patches -> Network
    // editor_tx: Network patches -> Local Editor
    let (network_tx, mut network_rx) = mpsc::channel::<(String, Vec<u8>)>(4096);
    let (editor_tx, editor_rx) = mpsc::channel(4096);

    let net_workspace = workspace.clone();
    let net_editor_tx = editor_tx.clone();

    tokio::spawn(async move {
        let net = NetworkManager::init_client(0).expect("Could not bind client port");

        let initial_conn = match net.connect(ip).await {
            Ok(c) => Some(c),
            Err(e) => {
                crate::logger::log(&format!("!! Failed to connect to host: {}", e));
                None
            }
        };

        crate::network::run_network_loop(
            net,
            false,
            initial_conn,
            &mut network_rx,
            &net_editor_tx,
            net_workspace,
        )
        .await;
    });

    let handler = Handler::new(workspace, network_tx, root_dir);
    handler.run_with_streams(stdin, stdout, editor_rx).await;
}
