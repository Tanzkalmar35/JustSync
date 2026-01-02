use clap::{Arg, Command};
use std::process::exit;
use tokio::sync::mpsc;
use uuid::Uuid;

pub mod core;
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
}

#[tokio::main]
pub async fn main() {
    // Setup Environment
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ctx = parse_cmd();
    let is_host = ctx.mode == "host";
    crate::logger::init(is_host);

    // Create Channels (The "Nervous System")

    // Core Inbox: Where everyone sends events (User typed, Network packet, etc.)
    let (core_tx, core_rx) = mpsc::channel::<Event>(100);
    // Network Outbox: Core tells Network to "Send this bytes to peer"
    let (net_out_tx, net_out_rx) = mpsc::channel::<NetworkCommand>(100);
    // Editor Outbox: Core tells Editor to "Apply these text edits"
    let (editor_out_tx, editor_out_rx) = mpsc::channel(100);

    // Initialize & Spawn the CORE Actor (The Brain)
    let agent_id = Uuid::new_v4().to_string();
    let core = Core::new(agent_id, net_out_tx, editor_out_tx);

    // If we are the Host, we scan the files and load them into Core immediately.
    if is_host {
        logger::log(">> [Host] Scanning workspace files...");
        let files = crate::fs::scan_project_directory("."); // Scan cwd
        for (uri, content) in files {
            // We inject these as events, just as if the user opened them
            // This populates the Core's state without needing backdoor access.
            let _ = core_tx.send(Event::LoadFromDisk { uri, content }).await;
        }
    }

    // Spawn Core on its own lightweight thread
    tokio::spawn(async move {
        core.run(core_rx).await;
    });

    // Spawn the NETWORK Actor (The Mouth)

    // It consumes `net_out_rx` (messages from Core)
    // It produces events into `core_tx` (messages to Core)
    let net_core_tx = core_tx.clone();
    tokio::spawn(async move {
        crate::network::run(
            ctx.mode,
            ctx.remote_ip,
            ctx.port,    // port
            net_core_tx, // Send to Core
            net_out_rx,  // Receive from Core
        )
        .await;
    });

    // Run the EDITOR Adapter - Main Thread

    // It reads Stdin and sends `LocalChange` events to `core_tx`.
    // It reads `editor_out_rx` and writes to Stdout.
    crate::handler::run(core_tx, editor_out_rx).await;
}

// Argument Parsing
fn parse_cmd() -> Context {
    let matches = Command::new("JustSync")
        .version("1.0")
        .about("A real-time, editor agnostic collaboration engine")
        .arg(
            Arg::new("mode")
                .long("mode") // Allows --mode
                .help("The daemon mode (host / peer)")
                .required(true),
        )
        .arg(
            Arg::new("remote-ip")
                .long("remote-ip") // Allows --remote-ip
                .help("The remote ip address to connect to (required for peer)")
                .required(false),
        )
        .arg(
            Arg::new("port")
                .long("port") // Allows --port
                .help("The port to listen on or connect to")
                .default_value("4444")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("stdio")
                .long("stdio")
                .hide(true) // Hidden from help
                .action(clap::ArgAction::SetTrue)
                .help("VS Code compatibility flag"),
        )
        .get_matches();

    let mode = matches.get_one::<String>("mode").unwrap().clone();
    let remote_ip = matches.get_one::<String>("remote-ip").cloned();
    let port = *matches.get_one::<u16>("port").unwrap();

    if mode != "host" && mode != "peer" {
        eprintln!("Invalid mode. Use --mode host or --mode peer.");
        exit(1);
    }

    Context {
        mode,
        remote_ip,
        port,
    }
}
