use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::mpsc,
};

use crate::{
    fs::{to_absolute_uri, to_relative_path},
    logger,
    lsp::{self, DidChangeParams, DidOpenParams, InitializeParams, LspHeader, TextEdit},
    state::Workspace,
};

pub struct Handler {
    state: Arc<Mutex<Workspace>>,
    network_tx: mpsc::Sender<(String, Vec<u8>)>, // (uri, patch)
    root_dir: String,
}

impl Handler {
    pub fn new(
        state: Arc<Mutex<Workspace>>,
        network_tx: mpsc::Sender<(String, Vec<u8>)>,
        root_dir: String,
    ) -> Self {
        Self {
            state,
            network_tx,
            root_dir,
        }
    }

    /// The Main Loop: Bridges Editor (Stdin/Stdout) with Network (Channels)
    pub async fn run_with_streams(
        self,
        mut stdin: BufReader<tokio::io::Stdin>,
        mut stdout: tokio::io::Stdout,
        mut editor_rx: mpsc::Receiver<(String, Vec<TextEdit>)>,
    ) {
        loop {
            tokio::select! {
                // ----------------------------------------------------------------
                // INBOUND: Editor sent a message (User typed something)
                // ----------------------------------------------------------------
                read_res = lsp::read_message(&mut stdin) => {
                    match read_res {
                        Ok(Some(body)) => {
                            self.handle_editor_message(&body).await;
                        }
                        Ok(None) => {
                            logger::log("Editor closed Stdin. Exiting.");
                            break; // EOF (Editor quit)
                        }
                        Err(e) => {
                            logger::log(&format!("!! Stdin Error: {}", e));
                            break;
                        }
                    }
                }

                // ----------------------------------------------------------------
                // OUTBOUND: Network sent a patch (Remote user typed)
                // ----------------------------------------------------------------
                Some((uri, edits)) = editor_rx.recv() => {
                    self.send_apply_edit(&mut stdout, &uri, edits).await;
                }
            }
        }
    }

    /// Decides what to do with a JSON message from the editor
    async fn handle_editor_message(&self, body: &str) {
        // We attempt to parse the header first
        if let Ok(header) = serde_json::from_str::<LspHeader>(body) {
            if let Some(method) = header.method {
                match method.as_str() {
                    "textDocument/didOpen" => {
                        if let Some(params) = header.params {
                            // Extract file content and initialize CRDT
                            if let Ok(p) = serde_json::from_value::<DidOpenParams>(params) {
                                let uri = to_relative_path(&p.text_document.uri, &self.root_dir);
                                let mut guard = self.state.lock().unwrap();
                                guard.get_or_create(uri, p.text_document.text);
                            }
                        }
                    }
                    "textDocument/didChange" => {
                        if let Some(params) = header.params {
                            // The heavy lifting: Syncing the change
                            if let Ok(p) = serde_json::from_value::<DidChangeParams>(params) {
                                self.process_did_change(p).await;
                            }
                        }
                    }
                    _ => {
                        // Ignore any other requests we don't have an action for
                    }
                }
            }
        }
    }

    /// The Core Logic: Syncs local changes to CRDT and Network
    async fn process_did_change(&self, params: DidChangeParams) {
        let uri = to_relative_path(&params.text_document.uri, &self.root_dir);
        let mut guard = self.state.lock().unwrap();
        let agent = guard.local_agent_id.clone();
        let doc = guard.get_or_create(uri.clone(), "".to_string());

        // Apply to Temp Rope
        let mut temp_rope = doc.content.clone();
        for change in &params.content_changes {
            if let Some(range) = &change.range {
                let (s, e) = doc.get_offsets(
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                );
                let len = temp_rope.len_chars();
                let s = s.min(len);
                let e = e.min(len);
                if s <= e {
                    temp_rope.remove(s..e);
                    temp_rope.insert(s, &change.text);
                }
            } else {
                temp_rope = ropey::Rope::from_str(&change.text);
            }
        }
        let new_content = temp_rope.to_string();

        // Echo guard logic
        if let Some((expected, time)) = &doc.last_synced {
            let normalized_expected = expected.trim_end();
            let normalized_actual = new_content.trim_end();

            if normalized_expected == normalized_actual {
                crate::logger::log("Sync Complete.");
                doc.last_synced = None;
                doc.content = temp_rope;
                return;
            }

            // Mismatch. Is it a "Transient Mismatch" during the sync window?
            if time.elapsed() < std::time::Duration::from_millis(200) {
                crate::logger::log("Mismatch during Sync Window (Ignored as noise).");
                // We do NOT clear the flag. We wait for the *real* match or timeout.
                return;
            }

            // Timeout expired. User must be typing.
            crate::logger::log("Sync Expectation Timed Out. Treating as User Input.");
            doc.last_synced = None;
        }

        // User edit
        for change in params.content_changes {
            // update rope, update crdt, send
            if let Some(range) = change.range {
                let (start, end) = doc.get_offsets(
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                );
                doc.update_rope(start, end, &change.text);
                if doc.is_synced() {
                    continue;
                }
                doc.update_crdt(start, end, &change.text, &agent);
                let patch = doc
                    .crdt
                    .oplog
                    .encode(diamond_types::list::encoding::EncodeOptions::default());
                let _ = self.network_tx.send((uri.clone(), patch)).await;
            }
        }
    }

    /// Formats edits into LSP JSON and writes to Stdout
    async fn send_apply_edit(
        &self,
        stdout: &mut tokio::io::Stdout,
        uri: &str,
        edits: Vec<TextEdit>,
    ) {
        if edits.is_empty() {
            return;
        }

        let mut changes = serde_json::Map::new();
        let abs_uri = to_absolute_uri(uri, &self.root_dir);

        crate::logger::log(&format!(
            "Applying {} Remote Edits to {}",
            edits.len(),
            abs_uri
        ));

        changes.insert(abs_uri, serde_json::to_value(edits).unwrap());

        let msg = json!({
            "jsonrpc": "2.0",
            "method": "workspace/applyEdit",
            "params": {
                "label": "JustSync Remote Update",
                "edit": { "changes": changes }
            }
        });

        let str = msg.to_string();
        let _ = stdout
            .write_all(format!("Content-Length: {}\r\n\r\n{}", str.len(), str).as_bytes())
            .await;
        let _ = stdout.flush().await;

        crate::logger::log("Wrote workspace/applyEdit to Stdout");
    }
}

// Returns: (root_dir, stdin_reader, stdout_writer)
pub async fn perform_editor_handshake() -> (String, BufReader<tokio::io::Stdin>, tokio::io::Stdout)
{
    logger::log("Initiating editor handshake.");

    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();

    // Block until we get the 'initialize' message
    let body = match lsp::read_message(&mut stdin).await {
        Ok(Some(b)) => b,
        _ => panic!("Client disconnected before initialization"),
    };

    // Parse it to get the Root URI
    let header: lsp::LspHeader = serde_json::from_str(&body).unwrap();
    if header.method.as_deref() != Some("initialize") {
        panic!("First message was NOT initialize");
    }

    let params: InitializeParams = serde_json::from_value(header.params.unwrap()).unwrap();
    let raw_root = params.root_uri.unwrap_or_else(|| ".".to_string());

    // Clean up "file://" prefix if present
    let root_dir = if raw_root.starts_with("file://") {
        raw_root.replace("file://", "")
    } else {
        raw_root
    };

    // Reply to the Editor: "I am ready, I support Incremental Sync"
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": header.id.unwrap_or(serde_json::Value::Number(1.into())),
        "result": {
            "capabilities": {
                "textDocumentSync": 2 // 2 = Incremental
            }
        }
    });
    let resp_str = response.to_string();
    let _ = stdout
        .write_all(format!("Content-Length: {}\r\n\r\n{}", resp_str.len(), resp_str).as_bytes())
        .await;
    let _ = stdout.flush().await;

    crate::logger::log(&format!("Editor handshake Complete. Root: {}", root_dir));

    (root_dir, stdin, stdout)
}
