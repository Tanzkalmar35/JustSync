// src/handler.rs

use crate::core::Event;
use crate::logger;
use crate::lsp::{self, DidChangeParams, DidOpenParams, LspHeader, TextEdit};
use serde_json::json;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// The main IO loop for the Editor.
/// It bridges the gap between "JSON on Stdin" and "Events in Rust Channels".
pub async fn run(
    core_tx: mpsc::Sender<Event>,
    mut editor_rx: mpsc::Receiver<(String, Vec<TextEdit>)>,
) {
    // Setup Stdin/Stdout
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();

    // Initial Handshake (blocking/sequential part)
    // We need to establish the "root" and tell the editor we are ready.
    let (root_dir, _) = perform_initialization_handshake(&mut reader, &mut stdout).await;

    // The Main Event Loop
    loop {
        tokio::select! {
            // --- INBOUND: From Editor (User Typed) ---
            read_res = lsp::read_message(&mut reader) => {
                match read_res {
                    Ok(Some(body)) => {
                        // Parse JSON and convert to Event
                        process_editor_message(&body, &core_tx, &root_dir).await;
                    }
                    Ok(None) => {
                        // EOF: Editor closed the pipe. We shut down.
                        let _ = core_tx.send(Event::Shutdown).await;
                        break;
                    }
                    Err(e) => {
                        eprintln!("!! Stdin Error: {}", e);
                        break;
                    }
                }
            }

            // --- OUTBOUND: From Core (Remote Edits) ---
            Some((uri, edits)) = editor_rx.recv() => {
                send_edits_to_editor(&mut stdout, &uri, edits, &root_dir).await;
            }
        }
    }
}

async fn process_editor_message(body: &str, tx: &mpsc::Sender<Event>, root_dir: &str) {
    if let Ok(header) = serde_json::from_str::<LspHeader>(body) {
        if let Some(method) = header.method {
            logger::log(&format!(">> [Handler] Method: {}", method));
            match method.as_str() {
                "textDocument/didOpen" => {
                    if let Some(params_val) = header.params {
                        if let Ok(params) = serde_json::from_value::<DidOpenParams>(params_val) {
                            let uri =
                                crate::fs::to_relative_path(&params.text_document.uri, root_dir);

                            logger::log(&format!(">> [Handler] didOpen URI: '{}'", uri));

                            if uri.is_empty() || uri == "/" {
                                return;
                            }

                            // Convert to Event
                            let event = Event::ClientDidOpen {
                                uri,
                                content: params.text_document.text,
                            };
                            let _ = tx.send(event).await;
                        }
                    }
                }
                "textDocument/didChange" => {
                    if let Some(params_val) = header.params {
                        if let Ok(params) = serde_json::from_value::<DidChangeParams>(params_val) {
                            let uri =
                                crate::fs::to_relative_path(&params.text_document.uri, root_dir);

                            logger::log(&format!(">> [Handler] didChange URI: '{}'", uri));

                            if uri.is_empty() || uri == "/" {
                                return;
                            }

                            // Convert to Event
                            let event = Event::LocalChange {
                                uri,
                                changes: params.content_changes,
                            };
                            let _ = tx.send(event).await;
                        }
                    }
                }
                _ => { /* Ignore other LSP messages */ }
            }
        }
    }
}

async fn send_edits_to_editor(
    stdout: &mut tokio::io::Stdout,
    uri: &str,
    edits: Vec<TextEdit>,
    root_dir: &str,
) {
    if edits.is_empty() {
        return;
    }

    let abs_uri = crate::fs::to_absolute_uri(uri, root_dir);
    let mut changes = serde_json::Map::new();
    changes.insert(abs_uri, serde_json::to_value(edits).unwrap());

    // Construct the workspace/applyEdit JSON
    let msg = json!({
        "jsonrpc": "2.0",
        "method": "workspace/applyEdit",
        "params": {
            "label": "JustSync Remote Update",
            "edit": { "changes": changes }
        }
    });

    write_rpc(stdout, &msg.to_string()).await;
}

// Simple helper to write Content-Length headers
async fn write_rpc(stdout: &mut tokio::io::Stdout, msg: &str) {
    let _ = stdout
        .write_all(format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg).as_bytes())
        .await;
    let _ = stdout.flush().await;
}

// Handshake logic separated out for cleanliness
async fn perform_initialization_handshake(
    reader: &mut BufReader<tokio::io::Stdin>,
    stdout: &mut tokio::io::Stdout,
) -> (String, ()) {
    // Wait for "initialize" request
    let body = lsp::read_message(reader)
        .await
        .expect("Failed to read init")
        .unwrap();
    let header: LspHeader = serde_json::from_str(&body).unwrap();

    // Extract Root URI
    let params: crate::lsp::InitializeParams =
        serde_json::from_value(header.params.unwrap()).unwrap();
    let raw_root = params.root_uri.unwrap_or_else(|| ".".to_string());
    let root_dir = raw_root.replace("file://", "");

    // Send "initialize" response
    let response = json!({
        "jsonrpc": "2.0",
        "id": header.id,
        "result": {
            "capabilities": {
                "textDocumentSync": 2 // Incremental Sync
            }
        }
    });
    write_rpc(stdout, &response.to_string()).await;

    (root_dir, ())
}
