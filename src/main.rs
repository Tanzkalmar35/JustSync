use std::{
    env,
    net::SocketAddr,
    process::exit,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
// some test change

use crate::{
    lsp::TextEdit,
    network::{NetMessage, NetworkManager},
    proxy::start_proxy,
    state::Workspace,
};

pub mod diff;
pub mod fs;
pub mod logger;
pub mod lsp;
pub mod network;
pub mod proxy;
pub mod state;

struct Context {
    target: String,
    target_args: Vec<String>,
    remote_ip: Option<std::net::SocketAddr>,
    network: Option<NetworkManager>,
    is_host: bool,
}

impl Context {
    fn parse_ctx(args: Vec<String>, mut target_start_idx: usize) -> anyhow::Result<Context> {
        {
            let mut network: Option<NetworkManager> = None;
            let mut is_host = false;
            let mut remote_ip: Option<std::net::SocketAddr> = None;

            if args.len() > 1 {
                if args[1] == "--host" {
                    // ./lsp-proxy --host rust-analyzer
                    logger::log(">> Starting as HOST");
                    is_host = true;
                    network = Some(NetworkManager::init_host(4444).expect("Failed to start host"));
                    target_start_idx = 2;
                } else if args[1] == "--join" {
                    // ./lsp-proxy --join 127.0.0.1 rust-analyzer
                    // (We will handle the connection logic later, just init endpoint for now)
                    logger::log(">> Starting as PEER");

                    let ip_parse_res = args[2].parse::<SocketAddr>();
                    if let Err(e) = ip_parse_res {
                        logger::log(">> ERROR: Invalid ip provided");
                        return Err(e.into());
                    }
                    remote_ip = Some(ip_parse_res.unwrap());

                    network = Some(NetworkManager::init_client(0).expect("Failed to start client"));
                    target_start_idx = 3; // --join + IP + cmd
                }
            }

            if args.len() < target_start_idx + 1 {
                // Print usage...
                exit(1);
            }

            let target = args[target_start_idx].clone();
            let target_args = args[target_start_idx + 1..].to_vec();

            Ok(Self {
                network,
                is_host,
                remote_ip,
                target,
                target_args,
            })
        }
    }
}

#[tokio::main]
pub async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Parse application context from command line params
    let args: Vec<String> = env::args().collect();
    let ctx_res = Context::parse_ctx(args, 1);
    if let Err(e) = ctx_res {
        panic!("{}", e.to_string());
    }
    let ctx = ctx_res.unwrap();

    let workspace: Arc<Mutex<Workspace>> = Arc::new(Mutex::new(Workspace::new()));
    let (editor_tx, editor_rx) = mpsc::channel::<(String, Vec<TextEdit>)>(100);
    let (tx, rx) = mpsc::channel(100);

    if let Some(net) = ctx.network {
        let workspace_ref = workspace.clone();

        // Assign the handle
        let handle = tokio::spawn(async move {
            start_task(
                net,
                ctx.is_host,
                rx,
                ctx.remote_ip,
                workspace_ref,
                editor_tx,
            )
            .await;
        });

        // Spawn a monitor task
        tokio::spawn(async move {
            match handle.await {
                Ok(_) => crate::logger::log(">> [Network] Task finished."),
                Err(e) => {
                    if e.is_panic() {
                        crate::logger::log("!! [Network] TASK PANICKED!");
                    } else {
                        crate::logger::log("!! [Network] Task cancelled.");
                    }
                }
            }
        });
    }
    match start_proxy(
        ctx.target,
        ctx.target_args,
        workspace,
        tx,
        editor_rx,
        env::current_dir().unwrap().to_string_lossy().to_string(),
    )
    .await
    {
        Ok(_) => {
            logger::log("Proxy exited successfully.");
        }
        Err(e) => {
            logger::log(&format!("Proxy failed: {}", e));
            exit(1);
        }
    }
}

async fn start_task(
    net: NetworkManager,
    is_host: bool,
    rx: mpsc::Receiver<(String, Vec<u8>)>,
    remote_ip: Option<SocketAddr>,
    state: Arc<Mutex<Workspace>>,
    editor_tx: mpsc::Sender<(String, Vec<TextEdit>)>,
) {
    logger::log(">> [Network] Task started");

    // establish connection
    let connection = if is_host {
        net.get_next_connection().await
    } else {
        if let Some(ip) = remote_ip {
            net.connect(ip).await.ok()
        } else {
            logger::log("Client started without IP!");
            None
        }
    };

    if let Some(conn) = connection {
        logger::log(">> [Network] Handshake complete.");

        // open stream
        // Host accepts stream, client opens stream.
        let stream_result = if is_host {
            conn.accept_bi().await
        } else {
            conn.open_bi().await
        };

        if let Ok((mut send, recv)) = stream_result {
            if is_host {
                // Transmitting initial file state
                crate::logger::log(">> [Network] Scanning project files...");
                let files = crate::fs::get_project_files();
                logger::log(&format!(">> [Network] Sending {} files...", files.len()));

                let msg = NetMessage::ProjectState { files };
                // Use your existing send helper
                if let Err(e) = crate::network::send_message(&mut send, &msg).await {
                    logger::log(&format!("Failed to send project state: {}", e));
                }
            } else {
                // Send initial msg containing application version - "Hey, I'm there!'"
                let msg = crate::network::NetMessage::Handshake {
                    version: "0.2.0".to_string(),
                };
                if let Err(e) = crate::network::send_message(&mut send, &msg).await {
                    crate::logger::log(&format!("Failed to send handshake: {}", e));
                }
            }

            sync_loop(rx, send, recv, state, &editor_tx).await;
        }
    };
}

// The actual sync loop
// Wait for RX (Local changes) OR RECV (Remote changes)
async fn sync_loop(
    mut rx: mpsc::Receiver<(String, Vec<u8>)>,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    state: Arc<Mutex<Workspace>>,
    editor_tx: &mpsc::Sender<(String, Vec<TextEdit>)>,
) {
    crate::logger::log(">> [Network] Stream open. Entering Sync Loop.");

    loop {
        tokio::select! {
            // BRANCH 1: Local Change (from Proxy) -> Send to Network
            maybe_patch = rx.recv() => {
                crate::logger::log(">> [Network] Processing local patch..."); // <--- ADD THIS
                match maybe_patch {
                    Some((uri, patch)) => {
                        let msg = crate::network::NetMessage::Sync { uri, data: patch };
                        if let Err(e) = crate::network::send_message(&mut send, &msg).await {
                            crate::logger::log(&format!("!! [Network] Send Error: {}", e));
                            break; // Connection likely dead
                        }
                    }
                    None => {
                        // The mpsc channel was closed (Proxy died). We should exit.
                        crate::logger::log(">> [Network] Proxy channel closed. Exiting.");
                        break;
                    }
                }
            }

            // BRANCH 2: Remote Change (from Network) -> Apply to Doc
            result = crate::network::recv_message(&mut recv) => {
                crate::logger::log(">> [Network] Host attempting to receive remote message...");
                match result {
                    Ok(msg) => {
                        crate::logger::log(&format!(">> [Network] Received Message: {:?}", msg));
                        merge_incoming(state.clone(), msg, &editor_tx).await;
                    }
                    Err(e) => {
                        // QUIC stream error or connection closed
                        crate::logger::log(&format!("!! [Network] Recv Error (Peer disconnected?): {}", e));
                        break;
                    }
                }
            }
        }
    }
    crate::logger::log(">> [Network] Sync Loop Exited!");
}

async fn merge_incoming(
    state: Arc<Mutex<Workspace>>,
    msg: NetMessage,
    editor_tx: &mpsc::Sender<(String, Vec<crate::lsp::TextEdit>)>,
) {
    match msg {
        crate::network::NetMessage::Sync { uri, data } => {
            let notification_data = {
                let mut workspace_guard = state.lock().unwrap();

                // 1. GET/CREATE DOC
                let doc = workspace_guard.get_or_create(uri.clone(), "".to_string());

                // 2. SNAPSHOT OLD STATE
                let old_rope = doc.content.clone();

                // 3. MERGE REMOTE CHANGES
                if let Err(e) = doc.crdt.merge_data_and_ff(&data) {
                    crate::logger::log(&format!("!! [Network] Merge Failed: {:?}", e));
                    None
                } else {
                    // 4. UPDATE VIEW (Rope)
                    let new_text_str = doc.crdt.branch.content().to_string();
                    doc.content = ropey::Rope::from_str(&new_text_str);

                    // 5. CALCULATE DIFF (Old Rope vs Current Rope)
                    let edits = crate::diff::calculate_edits(&old_rope, &doc.content);

                    crate::logger::log(&format!(
                        ">> [Network] Merged {}. Generated {} surgical edits.",
                        uri,
                        edits.len()
                    ));

                    if !edits.is_empty() {
                        doc.pending_remote_updates
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Some((uri, edits))
                    } else {
                        None
                    }
                }
            }; // Lock dropped

            // 6. SEND EDITS
            if let Some((uri, edits)) = notification_data {
                if let Err(e) = editor_tx.send((uri, edits)).await {
                    crate::logger::log(&format!("Could not send refresh to editor: {}", e));
                }
            }
        }
        NetMessage::ProjectState { files } => {
            crate::logger::log(&format!(
                ">> [Network] Received Project State: {} files",
                files.len()
            ));

            // Write to disk
            if let Err(e) = crate::fs::write_project_files(files) {
                crate::logger::log(&format!("!! [FS] Failed to write files: {}", e));
            } else {
                crate::logger::log(">> [FS] Project initialization complete.");
            }
        }
        _ => {}
    }
}
d to write files: {}", e));
            } else {
                crate::logger::log(">> [FS] Project initialization complete.");
            }
        }
        _ => {}
    }
}

