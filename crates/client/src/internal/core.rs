use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::handler::EditorCommand;
use crate::logger;
use crate::lsp::{Position, TextDocumentContentChangeEvent};
use crate::network::NetworkCommand;
use crate::state::Workspace;
use ropey::Rope;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Event {
    Ignoring,

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

    /// Only for initial scan
    LoadFromDisk {
        uri: String,
        content: String,
    },

    /// The user opened a file
    ClientDidOpen {
        uri: String,
        content: String,
    },

    /// The user closed a file
    ClientDidClose {
        uri: String,
    },

    LocalCursorChange {
        uri: String,
        position: Position,
    },

    RemoteCursorChange {
        uri: String,
        position: Position,
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
    editor_tx: mpsc::Sender<EditorCommand>,   // Send edits to editor
    //
    dirty_files: HashSet<String>,
    last_flushed_ropes: HashMap<String, Rope>,
}

impl Core {
    pub fn new(
        agent_id: String,
        network_tx: mpsc::Sender<NetworkCommand>,
        editor_tx: mpsc::Sender<EditorCommand>,
    ) -> Self {
        Self {
            workspace: Workspace::new(agent_id),
            network_tx,
            editor_tx,
            dirty_files: HashSet::new(),
            last_flushed_ropes: HashMap::new(),
        }
    }

    /// The Main Loop: Process one event at a time.
    pub async fn run(mut self, mut rx: mpsc::Receiver<Event>, is_host: bool) {
        let mut flush_timer = tokio::time::interval(Duration::from_millis(5));

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    match event {
                        Event::Ignoring => {},
                        Event::LocalChange { uri, changes } => {
                            self.handle_local_change(uri, changes, is_host).await;
                        }
                        Event::RemotePatch { uri, patch } => {
                            self.handle_remote_patch(uri, patch, is_host).await;
                        }
                        Event::LoadFromDisk { uri, content } => {
                            // Just update state, don't load into editor
                            self.workspace.get_or_create(uri.clone(), content.clone(), is_host);
                            self.last_flushed_ropes.insert(uri, Rope::from_str(&content));
                        }
                        Event::ClientDidOpen { uri, content } => {
                            self.workspace.get_or_create(uri.clone(), content.clone(), is_host);
                            self.workspace.mark_open(uri.clone());
                            self.last_flushed_ropes.insert(uri, Rope::from_str(&content));
                        }
                        Event::ClientDidClose { uri } => {
                            self.workspace.mark_closed(&uri);
                        }
                        Event::LocalCursorChange { uri, position } => {
                            let _ = self
                                .network_tx
                                .send(NetworkCommand::BroadcastCursor {
                                    uri,
                                    position: (position.line, position.character),
                                })
                                .await;
                        }
                        Event::RemoteCursorChange { uri, position } => {
                            let _ = self
                                .editor_tx
                                .send(EditorCommand::RemoteCursor { uri, position })
                                .await;
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
                                // Check if we are actually tracking this file (User has it open)
                                let is_open = self.workspace.documents.contains_key(&uri);

                                // Hydrate Memory
                                let doc = self.workspace.get_or_create_empty(uri.clone(), is_host);
                                let _ = doc.apply_remote_patch(&patch);

                                // Mark files as dirty
                                let content = doc.content.to_string();
                                files_to_write.push((uri.clone(), content));

                                if is_open {
                                    self.dirty_files.insert(uri);
                                }
                            }

                            // Write to Disk
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

                _ = flush_timer.tick() => {
                    self.flush_dirty_files(is_host).await;
                }
            }
        }
    }

    async fn handle_local_change(
        &mut self,
        uri: String,
        changes: Vec<TextDocumentContentChangeEvent>,
        is_host: bool,
    ) {
        // Get the document
        let doc = self.workspace.get_or_create_empty(uri.clone(), is_host);

        // Apply logic (The logic inside Document should return the binary patch if effective)
        let uri_ref = uri.clone();
        if let Some(patch) = doc.apply_local_changes(changes.clone()) {
            for change in changes {
                logger::log(&format!(
                    "[Core - local] Generated patch for change '{}'",
                    change.text
                ));
            }
            self.last_flushed_ropes.insert(uri_ref, doc.content.clone());
            let _ = self
                .network_tx
                .send(NetworkCommand::BroadcastPatch { uri, patch })
                .await;
        }
    }

    async fn handle_remote_patch(&mut self, uri: String, patch: Vec<u8>, is_host: bool) {
        let doc = self.workspace.get_or_create_empty(uri.clone(), is_host);
        let _ = doc.apply_remote_patch(&patch);

        if self.workspace.is_open(&uri) {
            self.dirty_files.insert(uri);
        }
    }

    async fn flush_dirty_files(&mut self, is_host: bool) {
        for uri in self.dirty_files.drain().collect::<Vec<_>>() {
            let doc = self.workspace.get_or_create_empty(uri.clone(), is_host);

            let old_rope = self
                .last_flushed_ropes
                .entry(uri.clone())
                .or_insert_with(|| Rope::from_str(""));
            let edits = crate::diff::calculate_edits(old_rope, &doc.content);

            if !edits.is_empty() {
                *old_rope = doc.content.clone();

                let _ = self
                    .editor_tx
                    .send(EditorCommand::ApplyEdits {
                        uri: uri.clone(),
                        edits,
                    })
                    .await;
                doc.pending_remote_updates.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
