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
use tracing::{debug, error};

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

    /// Sends an RPC (Remote Procedure Call) message to the standard output stream.
    ///
    /// This asynchronous function formats the message according to the Content-Length
    /// protocol, ensuring the message is properly structured before being written
    /// to the output. It appends the message length in the header, followed by the
    /// actual content, and ensures all data is flushed to the stream.
    ///
    /// # Arguments
    ///
    /// * `msg` - A string slice containing the RPC message to be sent.
    ///
    /// # Returns
    ///
    /// * `anyhow::Result<()>` - Returns `Ok(())` if the message is successfully written
    ///   and flushed to the output stream, or an `Err` if any IO error occurs during the process.
    ///
    /// # Errors
    ///
    /// This function will return an error if writing to or flushing the output stream fails.
    async fn write_rpc(&mut self, msg: &str) -> anyhow::Result<()> {
        self.stdout
            .write_all(format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg).as_bytes())
            .await?;
        self.stdout.flush().await?;
        Ok(())
    }

    /// Initializes the Language Server Protocol (LSP) session by reading the initialization parameters
    /// from the client and sending back the server's capabilities.
    ///
    /// # Errors
    /// Returns an `anyhow::Error` in the following scenarios:
    /// - Unable to read or decode the message from the client.
    /// - The initialization message does not contain required parameters.
    /// - Writing the response message to the client fails.
    ///
    /// # Returns
    /// `Ok(())` if the initialization is successful.
    ///
    /// # Notes
    /// - The `root_uri` parameter is used to set the `root_dir` for the server. If absent, it defaults
    ///   to the current directory (`"."`).
    /// - The `file://` prefix in the `root_uri` is stripped during processing to normalize the path.
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

    /// Asynchronously reads and processes an LSP (Language Server Protocol) message from the input stream.
    ///
    /// # Returns
    /// - `Ok(Some(LspHeader))` if a valid LSP header is successfully read and parsed.
    /// - `Ok(None)` in the following cases:
    ///    - There is no message to read (end of input).
    ///    - The message fails to parse as an `LspHeader`, but the loop should continue.
    /// - `Err(anyhow::Error)` if an I/O error or other unexpected error occurs while reading the message.
    ///
    /// # Errors
    /// Returns an `Err(anyhow::Error)` if an asynchronous I/O operation or unexpected issue occurs during reading.
    async fn read_msg(&mut self) -> anyhow::Result<Option<LspHeader>> {
        match lsp::read_message(&mut self.reader).await? {
            Some(body) => {
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

    /// Sends an editor command to the connected server or processes it locally.
    ///
    /// # Arguments
    ///
    /// * `cmd` - An instance of `EditorCommand` representing the command to execute.
    ///
    /// # Errors
    ///
    /// This function can fail for the following reasons:
    /// * JSON serialization errors while constructing the JSON-RPC messages.
    /// * Errors while writing the RPC message to the underlying communication channel.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let command = EditorCommand::ApplyEdits {
    ///     uri: "file.txt".into(),
    ///     edits: vec![/* ... edits ... */],
    /// };
    /// editor.send_cmd(command).await?;
    /// ```
    ///
    /// ```ignore
    /// let command = EditorCommand::RemoteCursor {
    ///     agent_id: "agent123".into(),
    ///     uri: "file.txt".into(),
    ///     position: (10, 5),
    /// };
    /// editor.send_cmd(command).await?;
    /// ```
    ///
    /// ```ignore
    /// let command = EditorCommand::SessionCreated {
    ///     name: "new_session".into(),
    /// };
    /// editor.send_cmd(command).await?;
    /// ```
    ///
    /// # JSON-RPC Specification
    ///
    /// The method constructs JSON-RPC messages as follows:
    ///
    /// - `workspace/applyEdit`:
    ///   ```json
    ///   {
    ///       "jsonrpc": "2.0",
    ///       "id": 1,
    ///       "method": "workspace/applyEdit",
    ///       "params": {
    ///           "label": "JustSync Remote Update",
    ///           "edit": {
    ///               "changes": {
    ///                   "<file_uri>": [<edits_json_array>]
    ///               }
    ///           }
    ///       }
    ///   }
    ///   ```
    ///
    /// - `$/justsync/remoteCursor`:
    ///   ```json
    ///   {
    ///       "jsonrpc": "2.0",
    ///       "method": "$/justsync/remoteCursor",
    ///       "params": {
    ///           "agent_id": "<agent_id>",
    ///           "uri": "<absolute_file_uri>",
    ///           "position": "<cursor_position>"
    ///       }
    ///   }
    ///   ```
    ///
    /// - `$/justsync/sessionCreated`:
    ///   ```json
    ///   {
    ///       "jsonrpc": "2.0",
    ///       "method": "$/justsync/sessionCreated",
    ///       "params": {
    ///           "name": "<session_name>"
    ///       }
    ///   }
    ///   ```
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

    /// Processes incoming messages from an editor in the Language Server Protocol (LSP) context.
    ///
    /// This asynchronous function handles messages received from the editor by matching them against
    /// known LSP method strings. Based on the method, it delegates the processing to respective handler
    /// functions. It effectively enables the language server to respond to editor events such as opening,
    /// modifying, closing files, or handling other custom commands.
    ///
    /// # Parameters
    /// * `self`: Immutable reference to the instance of the struct implementing the method.
    /// * `header`: The LSP header containing metadata about the received message, including the method name.
    ///
    /// # Handled Methods
    /// * `"textDocument/didOpen"`
    /// * `"textDocument/didChange"`
    /// * `"textDocument/didClose"`
    /// * `$/justsync/cursor`
    /// * `"initialized"`
    /// * Any other method: Logs an error indicating the receipt of an unknown or unimplemented command.
    async fn process_editor_message(&self, header: LspHeader) {
        let Some(ref method) = header.method else {
            debug!("Received message with no method (likely a response)");
            return;
        };

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
    /// Asynchronously runs the main loop for handling communication between the editor and core components.
    ///
    /// # Parameters
    /// - `editor_rx`: An asynchronous receiver channel (`mpsc::Receiver<EditorCommand>`) that receives commands
    ///   from the core to be sent to the editor.
    ///
    /// # Errors
    /// - Panics if `self.init()` fails during initialization of the editor adapter.
    /// - Logs errors encountered during reading messages from the editor or sending commands to the editor,
    ///   but does not propagate them further (terminates the loop instead).
    ///
    /// # Usage Example
    /// ```ignore
    /// use crate::just_sync_client::adapters::handler::StdioAdapter;
    /// use tokio::sync::mpsc;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let (core_tx, _) = mpsc::channel(100);
    ///     let (_, editor_rx) = mpsc::channel(100);
    ///
    ///     let mut handler = StdioAdapter::new(core_tx);
    ///     handler.run(editor_rx).await;
    /// }
    /// ```
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
