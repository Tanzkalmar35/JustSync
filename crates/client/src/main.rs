use clap::{Arg, Command};
use std::process::exit;
use tokio::sync::mpsc;
use uuid::Uuid;

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

    crate::logger::init(&ctx.mode);

    crate::logger::log(&format!("Starting JustSync in {} mode", ctx.mode));

    let (core_tx, core_rx) = mpsc::channel::<Event>(100);
    let (net_out_tx, net_out_rx) = mpsc::channel::<NetworkCommand>(100);
    let (editor_out_tx, editor_out_rx) = mpsc::channel(100);

    let agent_id = Uuid::new_v4().to_string();

    // Connect to relay and run network actor
    let conn = match network::connect(ctx.remote_ip.parse().unwrap(), "").await {
        Ok(conn) => conn,
        Err(e) => panic!("{}", e),
    };
    let _ = network::run_peer(
        ctx.key,
        ctx.session_name,
        agent_id.clone(),
        core_tx.clone(),
        net_out_rx,
        conn,
    )
    .await;

    // Run core actor
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

    // Run editor adapter on main thread
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
        eprintln!("Invalid mode. Use --mode host or --mode peer.");
        exit(1);
    }

    Context {
        mode,
        remote_ip,
        session_name,
        key,
    }
}
