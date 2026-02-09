use std::fs;
use std::sync::atomic::Ordering;

use crate::handler::EditorCommand;
use crate::logger;
use crate::lsp::{Position, TextDocumentContentChangeEvent};
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
                Event::LoadFromDisk { uri, content } => {
                    // Just update state, don't load into editor
                    self.workspace.get_or_create(uri, content);
                }
                Event::ClientDidOpen { uri, content } => {
                    self.workspace.get_or_create(uri.clone(), content);
                    self.workspace.mark_open(uri);
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
                        let doc = self.workspace.get_or_create_empty(uri.clone());
                        let edits_opt = doc.apply_remote_patch(&patch);

                        // Capture for Disk
                        let content = doc.content.to_string();
                        files_to_write.push((uri.clone(), content));

                        // If it's not open, writing to disk (below) is sufficient.
                        if is_open {
                            if let Some(edits) = edits_opt {
                                let _ = self
                                    .editor_tx
                                    .send(EditorCommand::ApplyEdits { uri, edits })
                                    .await;
                            }
                        } else {
                            if edits_opt.is_some() {
                                doc.pending_remote_updates.fetch_sub(1, Ordering::SeqCst);
                            }
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
        let is_open = self.workspace.is_open(&uri);
        let doc = self.workspace.get_or_create_empty(uri.clone());
        let edits_opt = doc.apply_remote_patch(&patch);

        if is_open {
            // Local editor has this file open, edits go to the editor
            if let Some(edits) = edits_opt {
                if let Err(e) = self
                    .editor_tx
                    .send(EditorCommand::ApplyEdits { uri, edits })
                    .await
                {
                    logger::log(&format!("!! Failed to send edits to editor actor: {}", e));
                }
            }
        } else {
            // Local editor does not have this file open, so don't tell the editor, instead just write to disk.
            if edits_opt.is_some() {
                doc.pending_remote_updates.fetch_sub(1, Ordering::SeqCst);
            }

            let content = doc.content.to_string();
            if let Err(e) = fs::write(&uri, content) {
                logger::log(&format!("!! Failed to background-write to disk: {}", e));
            } else {
                logger::log(&format!(">> [Core] Background-wrote to disk: {}", uri));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{Position, Range, TextDocumentContentChangeEvent};
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_core_local_change_broadcasts() {
        let (core_tx, core_rx) = mpsc::channel(10);
        let (net_tx, mut net_rx) = mpsc::channel(10);
        let (edit_tx, _edit_rx) = mpsc::channel(10);

        let core = Core::new("test-agent".into(), net_tx, edit_tx);
        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        let uri = "test.rs".to_string();
        core_tx
            .send(Event::ClientDidOpen {
                uri: uri.clone(),
                content: "initial".into(),
            })
            .await
            .unwrap();

        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 7,
                },
                end: Position {
                    line: 0,
                    character: 7,
                },
            }),
            text: " modified".to_string(),
        };

        // Trigger local change
        core_tx
            .send(Event::LocalChange {
                uri: uri.clone(),
                changes: vec![change],
            })
            .await
            .unwrap();

        // Verify broadcast
        match tokio::time::timeout(Duration::from_millis(100), net_rx.recv()).await {
            Ok(Some(NetworkCommand::BroadcastPatch {
                uri: res_uri,
                patch,
            })) => {
                assert_eq!(res_uri, uri);
                assert!(!patch.is_empty());
            }
            _ => panic!("Expected BroadcastPatch"),
        }

        core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]
    async fn test_core_client_close_behavior() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("closed_after_open.txt");
        let uri = file_path.to_str().unwrap().to_string();

        let (core_tx, core_rx) = mpsc::channel(10);
        let (net_tx, _net_rx) = mpsc::channel(10);
        let (edit_tx, mut edit_rx) = mpsc::channel(10);

        let core = Core::new("test-agent".into(), net_tx, edit_tx);
        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        // 1. Open file
        core_tx
            .send(Event::ClientDidOpen {
                uri: uri.clone(),
                content: "initial".into(),
            })
            .await
            .unwrap();

        // 2. Close file
        core_tx
            .send(Event::ClientDidClose { uri: uri.clone() })
            .await
            .unwrap();

        // 3. Receive patch
        let mut peer_doc = crate::state::Document::new(uri.clone(), "initial".into(), "Peer");
        let patch = peer_doc
            .apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 7,
                    },
                    end: Position {
                        line: 0,
                        character: 7,
                    },
                }),
                text: " updated".into(),
            }])
            .unwrap();

        core_tx
            .send(Event::RemotePatch {
                uri: uri.clone(),
                patch,
            })
            .await
            .unwrap();

        // 4. Verify NO editor update (because it's closed)
        if let Ok(_) = tokio::time::timeout(Duration::from_millis(50), edit_rx.recv()).await {
            panic!("Should not send editor command after file is closed");
        }

        // 5. Verify Disk Write
        tokio::time::sleep(Duration::from_millis(100)).await;
        let content = std::fs::read_to_string(&file_path).expect("File should exist");
        assert_eq!(content, "initial updated");

        core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]
    async fn test_core_remote_patch_applies_to_editor() {
        let (core_tx, core_rx) = mpsc::channel(10);
        let (net_tx, _net_rx) = mpsc::channel(10);
        let (edit_tx, mut edit_rx) = mpsc::channel(10);

        let core = Core::new("test-agent".into(), net_tx, edit_tx);
        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        let uri = "test.rs".to_string();

        // 1. Generate a valid patch from a "peer"
        let mut peer_doc = crate::state::Document::new(uri.clone(), "hello".into(), "Peer");
        let patch = peer_doc
            .apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                }),
                text: " world".into(),
            }])
            .unwrap();

        // 2. Open file locally
        core_tx
            .send(Event::ClientDidOpen {
                uri: uri.clone(),
                content: "hello".into(),
            })
            .await
            .unwrap();

        // 3. Receive remote patch
        core_tx
            .send(Event::RemotePatch {
                uri: uri.clone(),
                patch,
            })
            .await
            .unwrap();

        // 4. Verify editor update
        match tokio::time::timeout(Duration::from_millis(100), edit_rx.recv()).await {
            Ok(Some(EditorCommand::ApplyEdits {
                uri: res_uri,
                edits,
            })) => {
                assert_eq!(res_uri, uri);
                assert!(!edits.is_empty());
                assert_eq!(edits[0].new_text, " world");
            }
            _ => panic!("Expected ApplyEdits command"),
        }

        core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]
    async fn test_core_remote_patch_closed_file_writes_to_disk() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("closed.txt");
        // Use absolute path as URI to target temp dir
        let uri = file_path.to_str().unwrap().to_string();

        let (core_tx, core_rx) = mpsc::channel(10);
        let (net_tx, _net_rx) = mpsc::channel(10);
        let (edit_tx, mut edit_rx) = mpsc::channel(10);

        let core = Core::new("test-agent".into(), net_tx, edit_tx);
        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        // 1. Generate patch
        let mut peer_doc = crate::state::Document::new(uri.clone(), "start".into(), "Peer");
        let patch = peer_doc
            .apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                }),
                text: " finish".into(),
            }])
            .unwrap();

        // 2. Receive remote patch (File NOT open)
        core_tx
            .send(Event::RemotePatch {
                uri: uri.clone(),
                patch,
            })
            .await
            .unwrap();

        // 3. Verify NO editor update
        if let Ok(_) = tokio::time::timeout(Duration::from_millis(50), edit_rx.recv()).await {
            panic!("Should not send editor command for closed file");
        }

        // 4. Verify Disk Write
        tokio::time::sleep(Duration::from_millis(100)).await;
        let content = std::fs::read_to_string(&file_path).expect("File should exist");
        assert_eq!(content, "start finish");

        core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]

    async fn test_core_full_sync_logic() {
        // --- HOST SIDE ---

        let (host_core_tx, host_core_rx) = mpsc::channel(10);

        let (host_net_tx, mut host_net_rx) = mpsc::channel(10);

        let (host_edit_tx, _) = mpsc::channel(10);

        let mut host_core = Core::new("host".into(), host_net_tx, host_edit_tx);

        // Pre-populate host workspace

        host_core
            .workspace
            .get_or_create("file:///doc1.txt".into(), "Host Content".into());

        tokio::spawn(async move {
            host_core.run(host_core_rx).await;
        });

        // Request Sync

        host_core_tx.send(Event::PeerRequestedSync).await.unwrap();

        // Capture Response

        let sync_files =
            match tokio::time::timeout(Duration::from_millis(100), host_net_rx.recv()).await {
                Ok(Some(NetworkCommand::SendFullSyncResponse { files })) => files,

                _ => panic!("Expected SendFullSyncResponse"),
            };

        assert_eq!(sync_files.len(), 1);

        assert_eq!(sync_files[0].0, "file:///doc1.txt");

        // --- PEER SIDE ---

        let temp_dir = tempfile::tempdir().unwrap();

        let file_path = temp_dir.path().join("doc1.txt");

        // Mock the payload to use our safe temp path

        let safe_uri = file_path.to_str().unwrap().to_string();

        let safe_payload = vec![(safe_uri.clone(), sync_files[0].1.clone())];

        let (peer_core_tx, peer_core_rx) = mpsc::channel(10);

        let (peer_net_tx, _) = mpsc::channel(10);

        let (peer_edit_tx, _) = mpsc::channel(10);

        let peer_core = Core::new("peer".into(), peer_net_tx, peer_edit_tx);

        tokio::spawn(async move {
            peer_core.run(peer_core_rx).await;
        });

        // Receive Full Sync

        peer_core_tx
            .send(Event::RemoteFullSync {
                files: safe_payload,
            })
            .await
            .unwrap();

        // Verify Disk

        tokio::time::sleep(Duration::from_millis(100)).await;

        let content = std::fs::read_to_string(&file_path).expect("Synced file should exist");

        assert_eq!(content, "Host Content");

        host_core_tx.send(Event::Shutdown).await.unwrap();

        peer_core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]

    async fn test_core_resilience_change_without_open() {
        // Scenario: The editor sends a didChange for a file we never saw a didOpen for.

        // This happens with some aggressive LSP clients or plugins.

        let (core_tx, core_rx) = mpsc::channel(10);

        let (net_tx, _) = mpsc::channel(10);

        let (edit_tx, _) = mpsc::channel(10);

        let core = Core::new("resilient-agent".into(), net_tx, edit_tx);

        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        let uri = "ghost_file.rs".to_string();

        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 100,
                    character: 0,
                },

                end: Position {
                    line: 100,
                    character: 0,
                },
            }),

            text: "scary stuff".into(),
        };

        // Send Change WITHOUT Open

        core_tx
            .send(Event::LocalChange {
                uri,
                changes: vec![change],
            })
            .await
            .unwrap();

        // If we are here and the test hasn't panicked, the Core is still running.

        // Let's send a Shutdown to confirm it processes the queue cleanly.

        core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]

    async fn test_core_resilience_disk_write_failure() {
        // Scenario: Remote patch received for a closed file, but we can't write to disk (permissions/invalid path).

        // Core should NOT crash; it should log error and continue.

        let (core_tx, core_rx) = mpsc::channel(10);

        let (net_tx, _) = mpsc::channel(10);

        let (edit_tx, _) = mpsc::channel(10);

        let core = Core::new("io-agent".into(), net_tx, edit_tx);

        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        // Use an invalid path that definitely cannot be written to (e.g., a directory or empty)

        // On Linux, writing to a directory path usually fails.

        let invalid_uri = if cfg!(target_os = "windows") {
            "C:\\INVALID|<|*".to_string()
        } else {
            "/".to_string()
        };

        let mut peer_doc = crate::state::Document::new(invalid_uri.clone(), "".into(), "Peer");

        let patch = peer_doc
            .apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                }),

                text: "fail".into(),
            }])
            .unwrap();

        // Send patch

        core_tx
            .send(Event::RemotePatch {
                uri: invalid_uri,
                patch,
            })
            .await
            .unwrap();

        // Give it a moment to try and fail

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Core should still be alive

        core_tx.send(Event::Shutdown).await.unwrap();
    }

    #[tokio::test]

    async fn test_core_echo_guard_prevents_loop() {
        // Scenario:

        // 1. Remote Patch Arrives -> Edits sent to Editor.

        // 2. Editor applies edits and (incorrectly) echoes them back as a LocalChange.

        // 3. Core should REJECT this LocalChange to prevent infinite loop.

        let (core_tx, core_rx) = mpsc::channel(10);

        let (net_tx, mut net_rx) = mpsc::channel(10);

        let (edit_tx, mut edit_rx) = mpsc::channel(10);

        let core = Core::new("echo-agent".into(), net_tx, edit_tx);

        tokio::spawn(async move {
            core.run(core_rx).await;
        });

        let uri = "echo.rs".to_string();

        core_tx
            .send(Event::ClientDidOpen {
                uri: uri.clone(),
                content: "A".into(),
            })
            .await
            .unwrap();

        // 1. Remote Patch

        let mut peer_doc = crate::state::Document::new(uri.clone(), "A".into(), "Peer");

        let patch = peer_doc
            .apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 1,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                }),

                text: "B".into(),
            }])
            .unwrap();

        core_tx
            .send(Event::RemotePatch {
                uri: uri.clone(),
                patch,
            })
            .await
            .unwrap();

        // Wait for Editor Command (proving remote patch was processed)

        let _ = tokio::time::timeout(Duration::from_millis(100), edit_rx.recv())
            .await
            .unwrap();

        // 2. Simulate Echo: The editor reports "AB" (which matches the remote update)

        let echo_change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 1,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            }),

            text: "B".into(),
        };

        core_tx
            .send(Event::LocalChange {
                uri: uri.clone(),
                changes: vec![echo_change],
            })
            .await
            .unwrap();

        // 3. Verify NO Broadcast (Echo Guard worked)

        // If the guard FAILED, we would see a BroadcastPatch here.

        if let Ok(_) = tokio::time::timeout(Duration::from_millis(100), net_rx.recv()).await {
            panic!("Echo guard failed! Loop detected.");
        }

        core_tx.send(Event::Shutdown).await.unwrap();
    }
}
