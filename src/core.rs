use crate::lsp::{TextDocumentContentChangeEvent, TextEdit};
use crate::network::NetworkCommand;
use crate::state::Workspace;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Event {
    /// The user typed something in the editor (Stdin)
    LocalChange {
        uri: String,
        changes: Vec<TextDocumentContentChangeEvent>,
    },

    /// A peer sent us a CRDT patch (Network)
    RemotePatch {
        uri: String,
        patch: Vec<u8>,
    },

    /// The user opened a file (Stdin)
    ClientDidOpen {
        uri: String,
        content: String,
    },

    /// We should stop the daemon
    Shutdown,

    // Peer requests full state from hosting peer
    PeerRequestedSync,

    // Response to PeerRequestedSync containing the state
    RemoteFullSync {
        files: Vec<(String, Vec<u8>)>,
    },
}

pub struct Core {
    // The State
    workspace: Workspace,

    // The Outputs
    network_tx: mpsc::Sender<NetworkCommand>, // Send patches to peers
    editor_tx: mpsc::Sender<(String, Vec<TextEdit>)>, // Send edits to editor
}

impl Core {
    pub fn new(
        agent_id: String,
        network_tx: mpsc::Sender<NetworkCommand>,
        editor_tx: mpsc::Sender<(String, Vec<TextEdit>)>,
    ) -> Self {
        Self {
            workspace: Workspace::new(agent_id),
            network_tx,
            editor_tx,
        }
    }

    /// The Main Loop: Process one event at a time.
    pub async fn run(mut self, mut rx: mpsc::Receiver<Event>) {
        while let Some(event) = rx.recv().await {
            match event {
                Event::LocalChange { uri, changes } => {
                    self.handle_local_change(uri, changes).await;
                }
                Event::RemotePatch { uri, patch } => {
                    self.handle_remote_patch(uri, patch).await;
                }
                Event::ClientDidOpen { uri, content } => {
                    // Just update state, no network output needed usually
                    self.workspace.get_or_create(uri, content);
                }
                Event::PeerRequestedSync => {
                    crate::logger::log(">> [Core] Peer requested sync. Bundling state...");
                    let snapshot = self
                        .workspace
                        .get_snapshot()
                        .into_iter()
                        .filter(|(uri, _)| !uri.is_empty() && uri != "/")
                        .collect();

                    let _ = self
                        .network_tx
                        .send(NetworkCommand::SendFullSyncResponse { files: snapshot })
                        .await;
                }

                Event::RemoteFullSync { files } => {
                    crate::logger::log(
                        ">> [Core] Received Full Sync. Hydrating & Writing to Disk...",
                    );

                    let mut files_to_write = Vec::new();

                    for (uri, patch) in files {
                        // [FIX 1] Check if we are actually tracking this file (User has it open)
                        let is_open = self.workspace.documents.contains_key(&uri);

                        // Hydrate Memory
                        let doc = self.workspace.get_or_create_empty(uri.clone());
                        let edits_opt = doc.apply_remote_patch(&patch);

                        // Capture for Disk
                        let content = doc.content.to_string();
                        files_to_write.push((uri.clone(), content));

                        // If it's not open, writing to disk (below) is sufficient.
                        // Sending edits to an untracked file causes the "Double Apply" bug.
                        if is_open {
                            if let Some(edits) = edits_opt {
                                let _ = self.editor_tx.send((uri, edits)).await;
                            }
                        }
                    }

                    // Write to Disk
                    // This ensures that when the user does something like ":e src/main.rs" in nvim, the file actually exists.
                    if let Err(e) = crate::fs::write_project_files(files_to_write) {
                        crate::logger::log(&format!(
                            "!! [Disk] Failed to write synced files: {}",
                            e
                        ));
                    } else {
                        crate::logger::log(">> [Disk] Full sync written to storage.");
                    }
                }
                Event::Shutdown => break,
            }
        }
    }

    async fn handle_local_change(
        &mut self,
        uri: String,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) {
        // Get the document
        let doc = self.workspace.get_or_create_empty(uri.clone());

        // Apply logic (The logic inside Document should return the binary patch if effective)
        if let Some(patch) = doc.apply_local_changes(changes) {
            // CHANGE: Wrap in Enum
            crate::logger::log(&format!(
                "-> [Core] Generated Patch for '{}' ({} bytes)",
                uri,
                patch.len()
            ));
            let _ = self
                .network_tx
                .send(NetworkCommand::BroadcastPatch { uri, patch })
                .await;
        }
    }

    async fn handle_remote_patch(&mut self, uri: String, patch: Vec<u8>) {
        crate::logger::log(&format!(
            "<- [Core] Received Patch for '{}' ({} bytes)",
            uri,
            patch.len()
        ));
        let doc = self.workspace.get_or_create_empty(uri.clone());

        // Apply logic
        if let Some(edits) = doc.apply_remote_patch(&patch) {
            // Side Effect: Tell the editor
            if let Err(e) = self.editor_tx.send((uri, edits)).await {
                eprintln!("Failed to send edits to editor actor: {}", e);
            }
        }
    }
}
