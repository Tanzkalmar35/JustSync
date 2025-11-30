use std::{
    env,
    net::SocketAddr,
    process::exit,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

use crate::{
    lsp::TextEdit,
    network::{NetMessage, NetworkManager},
    proxy::start_proxy,
    state::Document,
};

pub mod diff;
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

    let doc: Arc<Mutex<Option<Document>>> = Arc::new(Mutex::new(None));
    let (editor_tx, editor_rx) = mpsc::channel::<(String, Vec<TextEdit>)>(100);
    let (tx, rx) = mpsc::channel(100);

    if let Some(net) = ctx.network {
        let doc_ref = doc.clone();

        tokio::spawn(async move {
            start_task(net, ctx.is_host, rx, ctx.remote_ip, doc_ref, editor_tx).await;
        });
    }

    match start_proxy(ctx.target, ctx.target_args, doc, tx, editor_rx).await {
        Ok(_) => {
            eprintln!("Proxy exited successfully.");
        }
        Err(e) => {
            eprintln!("Proxy failed: {}", e);
            exit(1);
        }
    }
}

async fn start_task(
    net: NetworkManager,
    is_host: bool,
    rx: mpsc::Receiver<(String, Vec<u8>)>,
    remote_ip: Option<SocketAddr>,
    state: Arc<Mutex<Option<Document>>>,
    editor_tx: mpsc::Sender<(String, Vec<TextEdit>)>,
) {
    logger::log(">> [Network] Task started");

    // establish connection
    let connection = if is_host {
        net.get_next_connection().await
    } else {
        // FIX: Actually connect!
        if let Some(ip) = remote_ip {
            net.connect(ip).await.ok() // .ok() converts Result to Option
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

        if let Ok((send, recv)) = stream_result {
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
    state: Arc<Mutex<Option<Document>>>,
    editor_tx: &mpsc::Sender<(String, Vec<TextEdit>)>,
) {
    crate::logger::log(">> [Network] Stream open. Entering Sync Loop.");

    loop {
        // tokio::select! waits for the FIRST one of these to complete.
        // It then drops the other future.
        tokio::select! {
            // BRANCH 1: Local Change (from Proxy) -> Send to Network
            // We await rx.recv(). It returns Option<Vec<u8>>.
            maybe_patch = rx.recv() => {
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
            // We await recv_message. It returns Result<NetMessage>.
            result = crate::network::recv_message(&mut recv) => {
                match result {
                    Ok(msg) => {
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
}

async fn merge_incoming(
    state: Arc<Mutex<Option<Document>>>,
    msg: NetMessage,
    editor_tx: &mpsc::Sender<(String, Vec<crate::lsp::TextEdit>)>,
) {
    match msg {
        crate::network::NetMessage::Sync { uri, data } => {
            let notification_data = {
                let mut doc_guard = state.lock().unwrap();

                // 1. PREPARE OLD STATE
                // If we have a doc, clone the current rope. 
                // If not (hydration), start with an empty rope.
                let old_rope = if let Some(doc) = doc_guard.as_ref() {
                    doc.content.clone()
                } else {
                    ropey::Rope::new()
                };

                // 2. UPDATE STATE (Merge or Load)
                let merge_result = if let Some(doc) = doc_guard.as_mut() {
                    // Case A: Existing Doc -> Merge
                    doc.crdt.merge_data_and_ff(&data).map(|_| doc)
                } else {
                    // Case B: New Doc -> Load from Patch
                    match diamond_types::list::ListCRDT::load_from(&data) {
                        Ok(crdt) => {
                            // Init with 0 pending updates
                            *doc_guard = Some(Document::new(uri.clone(), "".to_string()));
                            // We must overwrite the CRDT we just made with the loaded one
                            let doc = doc_guard.as_mut().unwrap();
                            doc.crdt = crdt;
                            Ok(doc)
                        }
                        Err(e) => Err(e),
                    }
                };

                // 3. UPDATE VIEW & CALCULATE DIFF
                match merge_result {
                    Ok(doc) => {
                        // Sync Rope to match new CRDT state
                        let new_text_str = doc.crdt.branch.content().to_string();
                        doc.content = ropey::Rope::from_str(&new_text_str);

                        // CALCULATE DIFF (Old Rope vs New Rope)
                        let edits = crate::diff::calculate_edits(&old_rope, &doc.content);

                        crate::logger::log(&format!(
                            ">> [Network] Merged. Generated {} surgical edits.",
                            edits.len()
                        ));

                        // Increment the Echo Counter by the number of edits? 
                        // No, Neovim sends one 'didChange' per 'applyEdit' call usually, 
                        // regardless of how many textEdits are inside.
                        // So we increment by 1.
                        if !edits.is_empty() {
                            doc.pending_remote_updates.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            Some((uri.clone(), edits))
                        } else {
                            None // No visual changes, don't bother the editor
                        }
                    }
                    Err(e) => {
                        crate::logger::log(&format!("!! [Network] Merge/Load Failed: {:?}", e));
                        None
                    }
                }
            }; // Lock dropped

            // 4. SEND EDITS
            if let Some((uri, edits)) = notification_data {
                if let Err(e) = editor_tx.send((uri, edits)).await {
                    crate::logger::log(&format!("Could not send refresh to editor: {}", e));
                }
            }
        }
        _ => {}
    }
}
