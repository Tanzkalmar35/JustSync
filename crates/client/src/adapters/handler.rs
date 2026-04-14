use crate::adapters::fs::FsOps;
use crate::internal::core::Event;
use crate::internal::lsp::{
    self, CursorPositionParams, DidChangeParams, DidCloseParams, DidOpenParams, LspHeader,
    Position, TextEdit,
};
use crate::logger;
use serde_json::json;
use std::path::Path;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum EditorCommand {
    ApplyEdits { uri: String, edits: Vec<TextEdit> },
    RemoteCursor { uri: String, position: Position },
}

pub trait EditorAdapter: Send + Sync {
    /// Initialize the connection and return the root directory.
    async fn init(&mut self) -> anyhow::Result<String>;

    /// Read a message from the editor.
    async fn read_msg(&mut self) -> anyhow::Result<Option<LspHeader>>;

    /// Send a command to the editor.
    async fn send_cmd(&mut self, cmd: EditorCommand) -> anyhow::Result<()>;
}

pub struct StdioAdapter {
    reader: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
    root_dir: String,
}

impl StdioAdapter {
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
            root_dir: String::new(),
        }
    }

    async fn write_rpc(&mut self, msg: &str) -> anyhow::Result<()> {
        self.stdout
            .write_all(format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg).as_bytes())
            .await?;
        self.stdout.flush().await?;
        Ok(())
    }
}

impl EditorAdapter for StdioAdapter {
    async fn init(&mut self) -> anyhow::Result<String> {
        let body = lsp::read_message(&mut self.reader)
            .await?
            .ok_or_else(|| anyhow::anyhow!("EOF during init"))?;

        let header: LspHeader = serde_json::from_str(&body)?;
        let params: lsp::InitializeParams = serde_json::from_value(
            header
                .params
                .ok_or_else(|| anyhow::anyhow!("Missing init params"))?,
        )?;

        self.root_dir = params
            .root_uri
            .unwrap_or_else(|| ".".to_string())
            .replace("file://", "");

        let response = json!({
            "jsonrpc": "2.0",
            "id": header.id,
            "result": {
                "capabilities": {
                    "textDocumentSync": 2 // Incremental Sync
                }
            }
        });
        self.write_rpc(&response.to_string()).await?;

        Ok(self.root_dir.clone())
    }

    async fn read_msg(&mut self) -> anyhow::Result<Option<LspHeader>> {
        match lsp::read_message(&mut self.reader).await? {
            Some(body) => Ok(Some(serde_json::from_str(&body)?)),
            None => Ok(None),
        }
    }

    async fn send_cmd(&mut self, cmd: EditorCommand) -> anyhow::Result<()> {
        match cmd {
            EditorCommand::ApplyEdits { uri, edits } => {
                if edits.is_empty() {
                    return Ok(());
                }
                let abs_uri = format!("file://{}", Path::new(&self.root_dir).join(&uri).display());
                let mut changes = serde_json::Map::new();
                changes.insert(abs_uri, serde_json::to_value(edits)?);

                let msg = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "workspace/applyEdit",
                    "params": {
                        "label": "JustSync Remote Update",
                        "edit": { "changes": changes }
                    }
                });
                self.write_rpc(&msg.to_string()).await?;
            }
            EditorCommand::RemoteCursor { uri, position } => {
                let abs_uri = format!("file://{}", Path::new(&self.root_dir).join(&uri).display());
                let msg = json!({
                    "jsonrpc": "2.0",
                    "method": "$/justsync/remoteCursor",
                    "params": {
                        "uri": abs_uri,
                        "position": position
                    }
                });
                self.write_rpc(&msg.to_string()).await?;
            }
        }
        Ok(())
    }
}

/// Orchestrator: The main loop, generic over Ports.
pub async fn run<A: EditorAdapter, F: FsOps>(
    mut adapter: A,
    fs: F,
    core_tx: mpsc::Sender<Event>,
    mut editor_rx: mpsc::Receiver<EditorCommand>,
) {
    let root_dir = adapter.init().await.expect("Failed to init editor");

    loop {
        tokio::select! {
            // INBOUND: Editor -> Core
            read_res = adapter.read_msg() => {
                match read_res {
                    Ok(Some(header)) => {
                        process_editor_message(header, &core_tx, &fs, &root_dir).await;
                    }
                    Ok(None) => {
                        let _ = core_tx.send(Event::Shutdown).await;
                        break;
                    }
                    Err(e) => {
                        logger::log(&format!("!! Adapter Error: {}", e));
                        break;
                    }
                }
            }

            // OUTBOUND: Core -> Editor
            Some(cmd) = editor_rx.recv() => {
                if let Err(e) = adapter.send_cmd(cmd).await {
                    logger::log(&format!("!! Failed to send to editor: {}", e));
                }
            }
        }
    }
}

/// Translation: Business Logic for LSP -> Event mapping.
/// Now generic over FsOps for 100% mockable path testing!
async fn process_editor_message<F: FsOps>(
    header: LspHeader,
    tx: &mpsc::Sender<Event>,
    fs: &F,
    root_dir: &str,
) {
    let method = match header.method {
        Some(m) => m,
        None => return,
    };

    match method.as_str() {
        "textDocument/didOpen" => {
            if let Some(params_val) = header.params {
                if let Ok(params) = serde_json::from_value::<DidOpenParams>(params_val) {
                    let uri = fs.to_relative_path(&params.text_document.uri, root_dir);
                    if is_ignored(&uri) {
                        return;
                    }

                    let _ = tx
                        .send(Event::ClientDidOpen {
                            uri,
                            content: params.text_document.text,
                        })
                        .await;
                }
            }
        }
        "textDocument/didChange" => {
            if let Some(params_val) = header.params {
                if let Ok(params) = serde_json::from_value::<DidChangeParams>(params_val) {
                    let uri = fs.to_relative_path(&params.text_document.uri, root_dir);
                    if is_ignored(&uri) {
                        return;
                    }

                    let _ = tx
                        .send(Event::LocalChange {
                            uri,
                            changes: params.content_changes,
                        })
                        .await;
                }
            }
        }
        "textDocument/didClose" => {
            if let Some(params_val) = header.params {
                if let Ok(params) = serde_json::from_value::<DidCloseParams>(params_val) {
                    let uri = fs.to_relative_path(&params.text_document.uri, root_dir);
                    let _ = tx.send(Event::ClientDidClose { uri }).await;
                }
            }
        }
        "$/justsync/cursor" => {
            if let Some(params_val) = header.params {
                if let Ok(params) = serde_json::from_value::<CursorPositionParams>(params_val) {
                    let uri = fs.to_relative_path(&params.text_document.uri, root_dir);
                    if is_ignored(&uri) {
                        return;
                    }

                    let _ = tx
                        .send(Event::LocalCursorChange {
                            uri,
                            position: params.position,
                        })
                        .await;
                }
            }
        }
        _ => {}
    }
}

fn is_ignored(uri: &str) -> bool {
    uri.is_empty() || uri == "/" || uri.starts_with("oil://")
}
