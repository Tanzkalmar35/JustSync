use tokio::sync::mpsc;
use tracing::{error, info};

use crate::{internal::{
    core::Event,
    fs::to_relative_path,
    lsp::{
        CursorPositionParams, DidChangeParams, DidCloseParams, DidOpenParams, LspHeader, Position,
        TextEdit,
    },
}, logger};

#[derive(Debug, Clone)]
pub enum EditorCommand {
    ApplyEdits {
        uri: String,
        edits: Vec<TextEdit>,
    },
    RemoteCursor {
        agent_id: String,
        uri: String,
        position: Position,
    },
    SessionCreated {
        name: String
    }
}

#[async_trait::async_trait]
pub trait EditorAdapter {
    /// Orchestrator: The main loop, orchestrating handler behavior
    async fn run(&mut self, editor_rx: mpsc::Receiver<EditorCommand>);
}

#[must_use]
pub fn is_ignored(uri: &str) -> bool {
    uri.is_empty() || uri == "/" || uri.starts_with("oil://")
}

pub async fn handle_open_cmd(header: LspHeader, to_core: &mpsc::Sender<Event>, root: &str) {
    if let Some(params_val) = header.params {
        match serde_json::from_value::<DidOpenParams>(params_val) {
            Ok(params) => {
                let uri = to_relative_path(&params.text_document.uri, root);
                if is_ignored(&uri) {
                    return;
                }

                let _ = to_core
                    .send(Event::ClientDidOpen {
                        uri,
                        content: params.text_document.text,
                    })
                    .await;
            }
            Err(e) => error!("[Handler] An error occured reading didOpen params: {}", e),
        }
    }
}

pub async fn handle_change_cmd(header: LspHeader, to_core: &mpsc::Sender<Event>, root: &str) {
    if let Some(params_val) = header.params {
        match serde_json::from_value::<DidChangeParams>(params_val) {
            Ok(params) => {
                info!("[Handler] Received local didchange");
                let uri = to_relative_path(&params.text_document.uri, root);
                if is_ignored(&uri) {
                    return;
                }

                let _ = to_core
                    .send(Event::LocalChange {
                        uri,
                        changes: params.content_changes,
                    })
                    .await;
            }
            Err(e) => error!(
                "[Handler] Error occured while receiving didchange params: {}",
                e
            ),
        }
    } else {
        error!("[Handler] headers for didchange not existing");
    }
}

pub async fn handle_close_cmd(header: LspHeader, to_core: &mpsc::Sender<Event>, root: &str) {
    if let Some(params_val) = header.params
        && let Ok(params) = serde_json::from_value::<DidCloseParams>(params_val)
    {
        let uri = to_relative_path(&params.text_document.uri, root);
        let _ = to_core.send(Event::ClientDidClose { uri }).await;
    }
}

pub async fn handle_cursor_cmd(header: LspHeader, to_core: &mpsc::Sender<Event>, root: &str) {
    if let Some(params_val) = header.params
        && let Ok(params) = serde_json::from_value::<CursorPositionParams>(params_val)
    {
        let uri = to_relative_path(&params.text_document.uri, root);
        if is_ignored(&uri) {
            return;
        }

        let _ = to_core
            .send(Event::LocalCursorChange {
                uri,
                position: params.position,
            })
            .await;
    }
}
