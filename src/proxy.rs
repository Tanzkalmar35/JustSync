use anyhow::Result;
use diamond_types::list::encoding::EncodeOptions;
use serde_json::{Value, json};
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader, Stdin, Stdout},
    process::{ChildStdin, ChildStdout, Command},
    sync::mpsc,
};

use crate::{
    logger,
    lsp::{self, DidChangeParams, DidOpenParams, LspHeader, TextEdit},
    state::Document,
};

pub async fn start_proxy(
    target_cmd: String,
    target_args: Vec<String>,
    state: Arc<Mutex<Option<Document>>>,
    patch_tx: mpsc::Sender<(String, Vec<u8>)>,
    editor_rx: mpsc::Receiver<(String, Vec<TextEdit>)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new(target_cmd)
        .args(target_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn child process");

    let child_stdin = child.stdin.take().expect("Failed to open stdin");
    let child_stdout = child.stdout.take().expect("Failed to open stdout");
    let mut child_stderr = child.stderr.take().expect("Failed to open stderr");

    let parent_stdin = tokio::io::stdin();
    let parent_stdout = tokio::io::stdout();
    let mut parent_stderr = tokio::io::stderr();

    let state_ref = state.clone();

    // Task A: Parent Stdin -> Child Stdin (With Interception)
    let stdin_task = tokio::spawn(async move {
        let reader = BufReader::new(parent_stdin);
        if let Err(e) = process_stdin(child_stdin, reader, &state_ref, patch_tx).await {
            eprintln!("Error processing stdin: {}", e);
            logger::log(&format!("!! [Proxy] Stdin task failed: {}", e));
        }
    });

    // Task B: Child Stdout + Injections -> Parent Stdout
    let stdout_task = tokio::spawn(run_stdout_loop(child_stdout, parent_stdout, editor_rx));

    // Task C: Child Stderr -> Parent Stderr
    let stderr_task = tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut child_stderr, &mut parent_stderr).await {
            eprintln!("Error copying stderr: {}", e);
        }
    });

    let status = child.wait().await?;
    let _ = tokio::join!(stdin_task, stdout_task, stderr_task);

    eprintln!("Proxy: Child process exited with {}", status);
    Ok(())
}

async fn process_stdin(
    mut child_stdin: ChildStdin,
    mut reader: BufReader<Stdin>,
    state_ref: &Arc<Mutex<Option<Document>>>,
    tx: mpsc::Sender<(String, Vec<u8>)>,
) -> Result<()> {
    loop {
        match lsp::read_message(&mut reader).await {
            Ok(Some(body)) => {
                // Default: Forward everything unless we explicitly stop it
                let mut should_forward = true;

                if let Ok(header) = serde_json::from_str::<LspHeader>(&body) {
                    // CASE A: It has a Method (Request or Notification)
                    if let Some(method) = header.method {
                        match method.as_str() {
                            "textDocument/didOpen" => {
                                if let Some(params) = header.params {
                                    process_did_open_action(params, state_ref).await;
                                }
                            }
                            "textDocument/didChange" => {
                                if let Some(params) = header.params {
                                    should_forward =
                                        process_did_change_action(params, state_ref, &tx).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    // CASE B: It is a Response (Has ID, No Method)
                    // This is likely the response to our 'workspace/applyEdit'.
                    // We MUST swallow this, or rust-analyzer will crash.
                    else if header.id.is_some() {
                        should_forward = false;
                        logger::log(">> [Proxy] Swallowed ApplyEdit Response from Client");
                    }
                } else {
                    // If we can't parse it, log it.
                    // In a robust app, we might forward it, but for debugging let's know about it.
                    logger::log("!! [Proxy] Failed to parse Header");
                }

                // THE GATEKEEPER
                if should_forward {
                    let header = format!("Content-Length: {}\r\n\r\n", body.len());

                    // Handle Broken Pipes (Child Crash)
                    if let Err(e) = child_stdin.write_all(header.as_bytes()).await {
                        logger::log(&format!(
                            "!! [Proxy] Write Header Error (Child Dead?): {}",
                            e
                        ));
                        break;
                    }
                    if let Err(e) = child_stdin.write_all(body.as_bytes()).await {
                        logger::log(&format!("!! [Proxy] Write Body Error: {}", e));
                        break;
                    }
                    if let Err(e) = child_stdin.flush().await {
                        logger::log(&format!("!! [Proxy] Flush Error: {}", e));
                        break;
                    }
                }
            }
            Ok(None) => {
                logger::log(">> [Proxy] Parent Stdin EOF");
                break;
            }
            Err(e) => {
                logger::log(&format!("!! [Proxy] Read Error: {}", e));
                break;
            }
        }
    }
    Ok(())
}

async fn run_stdout_loop(
    mut child_stdout: ChildStdout,
    mut parent_stdout: Stdout,
    mut editor_rx: mpsc::Receiver<(String, Vec<TextEdit>)>,
) {
    let mut buf = [0u8; 4096];

    loop {
        tokio::select! {
            // Branch A: Read from Child LS
            read_result = child_stdout.read(&mut buf) => {
                match read_result {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if parent_stdout.write_all(&buf[0..n]).await.is_err() { break; }
                        let _ = parent_stdout.flush().await;
                    }
                    Err(e) => {
                        eprintln!("Error reading from child stdout: {}", e);
                        break;
                    }
                }
            }

            // Branch B: Inject Editor Command
            Some((uri, edits)) = editor_rx.recv() => {
                let mut changes = serde_json::Map::new();

                // Serialize the edits vector directly into JSON
                changes.insert(uri, serde_json::to_value(edits).unwrap());

                let msg = json!({
                    "jsonrpc": "2.0",
                    "id": 100,
                    "method": "workspace/applyEdit",
                    "params": {
                        "label": "JustSync",
                        "edit": {
                            "changes": changes
                        }
                    }
                });

                let msg_str = msg.to_string();
                let header = format!("Content-Length: {}\r\n\r\n", msg_str.len());

                if parent_stdout.write_all(header.as_bytes()).await.is_err() { break; }
                if parent_stdout.write_all(msg_str.as_bytes()).await.is_err() { break; }
                if parent_stdout.flush().await.is_err() { break; }

                logger::log(">> [Proxy] Sent ApplyEdit to Editor");
            }
        }
    }
}

async fn process_did_open_action(params_val: Value, state: &Arc<Mutex<Option<Document>>>) {
    if let Ok(params) = serde_json::from_value::<DidOpenParams>(params_val) {
        let doc = Document::new(params.text_document.uri.clone(), params.text_document.text);
        let mut doc_guard = state.lock().unwrap();
        *doc_guard = Some(doc);
        logger::log(&format!(">> Opened Document: {}", params.text_document.uri));
    }
}

async fn process_did_change_action(
    params_val: Value,
    state: &Arc<Mutex<Option<Document>>>,
    tx: &mpsc::Sender<(String, Vec<u8>)>,
) -> bool {
    // <--- Change return type to bool
    if let Ok(params) = serde_json::from_value::<DidChangeParams>(params_val) {
        let mut doc_guard = state.lock().unwrap();
        if doc_guard.is_none() {
            // If no doc, maybe forward? Or drop? Let's forward to be safe.
            return true;
        }
        let doc = doc_guard.as_mut().unwrap();

        // Check if we should ignore this (Echo Suppression)
        let pending = doc
            .pending_remote_updates
            .load(std::sync::atomic::Ordering::SeqCst);
        if pending > 0 {
            doc.pending_remote_updates
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            crate::logger::log(
                ">> [Proxy] Echo suppressed (Counter decrement). NOT Forwarding to Child.",
            );
            return false; // <--- DO NOT FORWARD
        }

        for change in params.content_changes {
            if let Some(range) = change.range {
                let (start, end) = doc.get_offsets(
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                );

                // 1. Update View
                doc.update_rope(start, end, &change.text);

                // 2. Update Truth
                doc.update_crdt(start, end, &change.text);

                // 3. Broadcast
                let patch = doc.crdt.oplog.encode(EncodeOptions::default());
                let uri = doc.uri.clone();
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    let _ = tx_clone.send((uri, patch)).await;
                });

                crate::logger::log(&format!("Wrote bytes | CRDT Len: {}", doc.crdt.len()));
            } else {
                // Full sync logic
                *doc = Document::new(doc.uri.clone(), change.text);
                crate::logger::log("WARNING: Received change without range (Full Sync reset)");
            }
        }
        return true; // Real user change, FORWARD it.
    } else {
        crate::logger::log("Could not parse body params for didChange action");
        return true; // Parse error, forward just in case.
    }
}
