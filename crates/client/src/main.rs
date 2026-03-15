use clap::{Arg, Command};
use std::process::exit;
use tokio::sync::mpsc;
use uuid::Uuid;

// Module definitions
pub mod core;
pub mod crypto;
pub mod diff;
pub mod fs;
pub mod handler;
pub mod logger;
pub mod lsp;
pub mod network;
pub mod state;

use crate::{
    core::{Core, Event},
    network::NetworkCommand,
};

struct Context {
    mode: String,
    remote_ip: Option<String>,
    port: u16,
    token: Option<String>,
}

#[tokio::main]
pub async fn main() {
    // Setup Environment
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ctx = parse_cmd();
    let is_host = ctx.mode == "host";

    // Logging init
    crate::logger::init(is_host);

    // Prepare crypto
    let (server_cert, server_key, active_token) = if is_host {
        // Host - generate everything from scratch
        let (cert, key, token_str) = crypto::generate_cert_and_token();

        // Note: It's eprintln!() so it's automatically picked up by editors (as an lsp error)
        eprintln!("---------------------------------------------------");
        eprintln!("🔑 SECRET TOKEN: {}", token_str);
        eprintln!("---------------------------------------------------");

        (Some(cert), Some(key), token_str)
    } else {
        // peer - just take token from args
        if ctx.token.is_none() {
            eprintln!("Fehler: Als Peer musst du --token <TOKEN> angeben!");
            exit(1);
        }
        (None, None, ctx.token.unwrap())
    };

    // --- CHANNEL SETUP ---

    // Core Inbox
    let (core_tx, core_rx) = mpsc::channel::<Event>(100);
    // Network Outbox
    let (net_out_tx, net_out_rx) = mpsc::channel::<NetworkCommand>(100);
    // Editor Outbox
    let (editor_out_tx, editor_out_rx) = mpsc::channel(100);

    // --- CORE ACTOR ---
    let agent_id = Uuid::new_v4().to_string();
    let core = Core::new(agent_id, net_out_tx, editor_out_tx);

    // Host: Scan files
    if is_host {
        logger::log(">> [Host] Scanning workspace files...");
        let files = crate::fs::scan_project_directory(".");
        for (uri, content) in files {
            let _ = core_tx.send(Event::LoadFromDisk { uri, content }).await;
        }
    }

    // Spawn Core
    tokio::spawn(async move {
        core.run(core_rx).await;
    });

    // --- NETWORK ACTOR ---

    let net_core_tx = core_tx.clone();

    let net_mode = ctx.mode.clone();
    let net_ip = ctx.remote_ip.clone();
    let net_port = ctx.port;

    tokio::spawn(async move {
        crate::network::run(
            net_mode,
            net_ip,
            net_port,
            net_core_tx, // Send to Core
            net_out_rx,  // Receive from Core
            active_token,
            server_cert,
            server_key,
        )
        .await;
    });

    // --- EDITOR ADAPTER (Main Thread) ---
    crate::handler::run(core_tx, editor_out_rx).await;
}

fn parse_cmd() -> Context {
    let matches = Command::new("JustSync")
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
                .required(false),
        )
        .arg(
            Arg::new("token")
                .long("token")
                .help("The security token (required for peer)")
                .required(false),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .help("The port to listen on or connect to")
                .default_value("4444")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("stdio")
                .long("stdio")
                .hide(true)
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let mode = matches.get_one::<String>("mode").unwrap().clone();
    let remote_ip = matches.get_one::<String>("remote-ip").cloned();
    let token = matches.get_one::<String>("token").cloned();
    let port = *matches.get_one::<u16>("port").unwrap();

    if mode != "host" && mode != "peer" {
        eprintln!("Invalid mode. Use --mode host or --mode peer.");
        exit(1);
    }

    Context {
        mode,
        remote_ip,
        port,
        token,
    }
}
