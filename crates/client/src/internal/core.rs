use std::collections::HashSet;
use std::time::Duration;

use ropey::Rope;
use tokio::sync::mpsc;

use crate::internal::diff;
use crate::internal::fs::FsOps;
use crate::internal::handler::EditorCommand;
use crate::internal::lsp::{Position, TextDocumentContentChangeEvent};
use crate::internal::network::NetworkCommand;
use crate::internal::state::Workspace;
use tracing::{debug, error, info};

#[derive(Clone, Debug)]
pub enum Event {
    /// Signals an event that should be ignored
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

    /// The user moved the cursor
    LocalCursorChange {
        uri: String,
        position: Position,
    },

    /// A remote peer moved the cursor
    RemoteCursorChange {
        agent_id: String,
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

    /// The session was successfully registered on the relay
    SessionRegistered {
        name: String,
    },
}

#[must_use]
pub struct Core {
    // The State
    workspace: Workspace,

    // The Outputs
    network_tx: mpsc::Sender<NetworkCommand>, // Send patches to peers
    editor_tx: mpsc::Sender<EditorCommand>,   // Send edits to editor

    // Keeps track of "dirty" files, eg files that are tagged for sync
    dirty_files: HashSet<String>,
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
        }
    }

    /// Core's main event loop. The singular passage for events coming from either network or local
    /// editors.
    ///
    /// This event loop orchestrates the whole application. Anything that happens anywhere, will at
    /// some point pass through this function.
    ///
    /// Also, this function flushes all "dirty" files, every 5ms.
    ///
    /// # Arguments
    ///
    /// * `rx` - The channel to pull messages from.
    /// * `is_host` - Whether the localhost peer is the hosting peer of the session.
    /// * `fs` - The filesystem adapter to use.
    pub async fn run(mut self, mut rx: mpsc::Receiver<Event>, is_host: bool, fs: impl FsOps) {
        let mut flush_timer = tokio::time::interval(Duration::from_millis(5));

        loop {
            tokio::select! {
                // Network inbound
                Some(event) = rx.recv() => {
                    match event {
                        Event::Ignoring => {
                            debug!("[Core] Ignoring event");
                        },
                        Event::LocalChange { uri, changes } => {
                            debug!("[Core] Handling local change");
                            self.handle_local_change(uri, changes, is_host).await;
                        }
                        Event::RemotePatch { uri, patch } => {
                            debug!("[Core] Handling remote change");
                            self.handle_remote_patch(uri, &patch, is_host);
                        }
                        Event::LoadFromDisk { uri, content } => {
                            debug!("[Core] Loading workspace from disk");
                            self.workspace.get_or_create(uri.as_str(), content.as_str(), is_host);
                        }
                        Event::ClientDidOpen { uri, content } => {
                            debug!("[Core] Handling open file event");
                            let doc = self.workspace.get_or_create(uri.as_str(), content.as_str(), is_host);
                            doc.content_shadow = Rope::from_str(&content);
                            self.workspace.mark_open(uri.clone());
                        }
                        Event::ClientDidClose { uri } => {
                            debug!("[Core] Handling close file event");
                            self.workspace.mark_closed(&uri);
                        }
                        Event::LocalCursorChange { uri, position } => {
                            debug!("[Core] Handling local cursor change event");
                            let _ = self
                                .network_tx
                                .send(NetworkCommand::BroadcastCursor {
                                    uri,
                                    position: (position.line, position.character),
                                })
                                .await;
                        }
                        Event::RemoteCursorChange { agent_id, uri, position } => {
                            debug!("[Core] Handling remote cursor change event");
                            let _ = self
                                .editor_tx
                                .send(EditorCommand::RemoteCursor { agent_id, uri, position })
                                .await;
                        }
                        Event::PeerRequestedSync => {
                            debug!("[Core] Handling incoming full sync request");
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
                            debug!("[Core] Handling incoming full sync");

                            let mut files_to_write = Vec::new();
                            for (uri, patch) in files {
                                // Check if we are actually tracking this file (User has it open)
                                let is_open = self.workspace.documents.contains_key(&uri);

                                // Hydrate Memory
                                let doc = self.workspace.get_or_create_empty(uri.as_str(), is_host);
                                let _ = doc.apply_remote_patch(&patch);

                                // Mark files as dirty
                                let content = doc.content.to_string();
                                files_to_write.push((uri.clone(), content));

                                if is_open {
                                    self.dirty_files.insert(uri);
                                }
                            }

                            // Write to Disk
                            if let Err(e) = fs.write_project_files(files_to_write) {
                                debug!("[Core] Failed to write file to disk: {}", e);
                            } else {
                                debug!("[Core] Full sync written to disk");
                            }
                        }
                        Event::SessionRegistered { name } => {
                            info!("[Core] New session registered as {}", name);
                            if let Err(e) = self.editor_tx.send(EditorCommand::SessionCreated { name }).await {
                                error!("{}", e)
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

    /// Handles local changes made to a document.
    ///
    /// # Arguments
    ///
    /// * `uri` - The uri pointing to the changed document.
    /// * `changes` - The changes that were made.
    /// * `is_host` - Whether the localhost peer is hosting peer.
    async fn handle_local_change(
        &mut self,
        uri: String,
        changes: Vec<TextDocumentContentChangeEvent>,
        is_host: bool,
    ) {
        let doc = self.workspace.get_or_create_empty(uri.as_str(), is_host);

        if !doc.apply_local_changes(changes) {
            // Change to apply was likely an echo or a no-op
            return;
        }

        let current_version = doc.crdt.oplog.local_version().clone();
        if current_version != doc.last_version {
            let patch = doc.get_patch_since(&doc.last_version);
            if !patch.is_empty() {
                let _ = self
                    .network_tx
                    .send(NetworkCommand::BroadcastPatch { uri, patch })
                    .await;
            }
            doc.last_version = current_version;
        }
    }

    /// Handles incoming changes, a remote peer made and shared with us.
    ///
    /// # Arguments
    ///
    /// * `uri` - The uri of the changed document.
    /// * `patch` - The patch of changes to the document.
    /// * `is_host` - Whether the localhost peer is hosting peer.
    fn handle_remote_patch(&mut self, uri: String, patch: &[u8], is_host: bool) {
        let is_open = self.workspace.is_open(&uri);
        let doc = self.workspace.get_or_create_empty(uri.as_str(), is_host);

        if let Some(edits) = doc.apply_remote_patch(patch)
            && !edits.is_empty()
            && is_open
        {
            self.dirty_files.insert(uri);
        } else if !is_open {
            doc.content_shadow = doc.content.clone();
        }
    }

    /// Drains the list of "dirty" documents, eg syncing their state with the editor.
    ///
    /// # Arguments
    ///
    /// * `is_host` - Whether the localhost peer is hosting peer.
    async fn flush_dirty_files(&mut self, is_host: bool) {
        for uri in self.dirty_files.drain().collect::<Vec<_>>() {
            let doc = self.workspace.get_or_create_empty(uri.as_str(), is_host);

            let edits = diff::calculate_edits(&doc.content_shadow, &doc.content);

            if !edits.is_empty() {
                let _ = self
                    .editor_tx
                    .send(EditorCommand::ApplyEdits {
                        uri: uri.clone(),
                        edits,
                    })
                    .await;
            }
        }
    }
}
