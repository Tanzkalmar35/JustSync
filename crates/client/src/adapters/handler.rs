use crate::internal::core::Event;
use crate::internal::handler::{
    EditorAdapter, EditorCommand, handle_change_cmd, handle_close_cmd, handle_cursor_cmd,
    handle_open_cmd,
};
use crate::internal::lsp::{self, LspHeader};
use serde_json::json;
use std::path::Path;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[must_use]
pub struct StdioAdapter {
    reader: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
    core_tx: mpsc::Sender<Event>,
    root_dir: String,
}

impl StdioAdapter {
    pub fn new(core_tx: mpsc::Sender<Event>) -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
            core_tx,
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

    async fn init(&mut self) -> anyhow::Result<()> {
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
        Ok(())
    }

    async fn read_msg(&mut self) -> anyhow::Result<Option<LspHeader>> {
        match lsp::read_message(&mut self.reader).await? {
            Some(body) => {
                tracing::debug!("RAW LSP: {}", body);
                match serde_json::from_str::<LspHeader>(&body) {
                    Ok(header) => Ok(Some(header)),
                    Err(e) => {
                        tracing::error!("Failed to parse LspHeader: {} | Body: {}", e, body);
                        // Don't crash the loop, just skip this message
                        Ok(None)
                    }
                }
            }
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
            EditorCommand::RemoteCursor {
                agent_id,
                uri,
                position,
            } => {
                let abs_uri = format!("file://{}", Path::new(&self.root_dir).join(&uri).display());
                let msg = json!({
                    "jsonrpc": "2.0",
                    "method": "$/justsync/remoteCursor",
                    "params": {
                        "agent_id": agent_id,
                        "uri": abs_uri,
                        "position": position
                    }
                });
                self.write_rpc(&msg.to_string()).await?;
            }
            EditorCommand::SessionCreated { name } => {
                let msg = json!({
                    "jsonrpc": "2.0",
                    "method": "$/justsync/sessionCreated",
                    "params": {
                        "name": name
                    }
                });
                self.write_rpc(&msg.to_string()).await?;
            }
        }
        Ok(())
    }

    async fn process_editor_message(&self, header: LspHeader) {
        let Some(ref method) = header.method else {
            debug!("Received message with no method (likely a response)");
            return;
        };

        info!("[AAAAA] Editor sent {}", method);

        match method.as_str() {
            "textDocument/didOpen" => handle_open_cmd(header, &self.core_tx, &self.root_dir).await,
            "textDocument/didChange" => {
                handle_change_cmd(header, &self.core_tx, &self.root_dir).await;
            }
            "textDocument/didClose" => {
                handle_close_cmd(header, &self.core_tx, &self.root_dir).await;
            }
            "$/justsync/cursor" => handle_cursor_cmd(header, &self.core_tx, &self.root_dir).await,
            "initialized" => debug!("Initialization with editor as lsp complete!"),
            _ => {
                error!(
                    "Editor handler received a command that's not implemented!: {}",
                    method.as_str()
                );
            }
        }
    }
}

#[async_trait::async_trait]
impl EditorAdapter for StdioAdapter {
    async fn run(&mut self, mut editor_rx: mpsc::Receiver<EditorCommand>) {
        self.init().await.expect("Editor adapter init failed!");
        loop {
            tokio::select! {
                // INBOUND: Editor -> Handler -> Core
                read_res = self.read_msg() => {
                    match read_res {
                        Ok(Some(header)) => {
                            self.process_editor_message(header).await;
                        }
                        Ok(None) => {
                            let _ = self.core_tx.send(Event::Shutdown).await;
                            break;
                        }
                        Err(e) => {
                            error!("[Handler] An error occured while reading message from editor: {}", e);
                            break;
                        }
                    }
                }

                // OUTBOUND: Core -> Handler -> Editor
                Some(cmd) = editor_rx.recv() => {
                    if let Err(e) = self.send_cmd(cmd).await {
                        error!("[Handler] Failed to send message to editor: {}", e);
                    }
                }
            }
        }
    }
}
