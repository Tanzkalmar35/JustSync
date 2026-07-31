use diamond_types::{
    LocalVersion, Time,
    list::{ListCRDT, encoding::EncodeOptions},
};
use ropey::Rope;
use std::collections::{HashMap, HashSet};
use tracing::error;

use crate::internal::{
    diff,
    lsp::{Range, TextDocumentContentChangeEvent, TextEdit},
};

#[must_use]
pub struct Workspace {
    pub documents: HashMap<String, Document>,
    pub local_agent_id: String,
    pub open_files: HashSet<String>,
}

impl Workspace {
    pub fn new(agent_id: String) -> Self {
        Self {
            documents: HashMap::new(),
            local_agent_id: agent_id,
            open_files: HashSet::new(),
        }
    }

    /// Retrieves an existing document or creates a new one with the given content.
    pub fn get_or_create(&mut self, uri: &str, content: &str, is_host: bool) -> &mut Document {
        self.documents
            .entry(uri.to_string())
            .or_insert_with(|| Document::new(content, &self.local_agent_id, is_host))
    }

    /// Retrieves a document or creates an empty one if it doesn't exist.
    pub fn get_or_create_empty(&mut self, uri: &str, is_host: bool) -> &mut Document {
        self.get_or_create(uri, "", is_host)
    }

    /// Serializes the entire state of all documents
    #[must_use]
    pub fn get_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut results = Vec::new();
        for (uri, doc) in &self.documents {
            // Encode the entire history of the document
            let data = doc
                .crdt
                .oplog
                .encode(diamond_types::list::encoding::EncodeOptions::default());
            results.push((uri.clone(), data));
        }
        results
    }

    /// Marks a document as currently open
    pub fn mark_open(&mut self, uri: String) {
        self.open_files.insert(uri);
    }

    /// Marks a document as currently closed
    pub fn mark_closed(&mut self, uri: &str) {
        self.open_files.remove(uri);
    }

    /// Checks if a document with the given uri is currently open
    #[must_use]
    pub fn is_open(&self, uri: &str) -> bool {
        self.open_files.contains(uri)
    }
}

/// A single file in the workspace.
/// Encapsulates the synchronization logic ("The Brain of the File").
#[must_use]
pub struct Document {
    /// The preliminary expected content of the document
    pub content: Rope,

    /// The conservative actual content of the document
    pub content_shadow: Rope,

    /// The latest synchronized document version
    pub last_version: LocalVersion,

    /// The "Truth" - The mathematical CRDT history.
    /// Handles conflict resolution.
    pub crdt: ListCRDT,

    /// The ID of the local agent (used for tagging CRDT ops).
    agent_id: String,
}

impl Document {
    pub fn new(initial_content: &str, agent_id: &str, is_host: bool) -> Self {
        let mut crdt = ListCRDT::new();

        // Initialize CRDT with content if present
        if !initial_content.is_empty() && is_host {
            let agent = crdt.get_or_create_agent_id("init");
            crdt.insert(agent, 0, initial_content);
        }

        let initial_version = crdt.branch.local_version();

        Self {
            content: Rope::from_str(initial_content),
            content_shadow: Rope::from_str(initial_content),
            last_version: initial_version,
            crdt,
            agent_id: agent_id.to_string(),
        }
    }

    /// Processes changes from the editor.
    ///
    /// # Arguments
    ///
    /// * `changes` - A list of changes to apply to this document.
    ///
    /// # Returns
    ///
    /// Whether changes were applied to this doc. Therefore false if the changes turned out to be
    /// an echo or a no-op.
    pub fn apply_local_changes(&mut self, changes: Vec<TextDocumentContentChangeEvent>) -> bool {
        let mut new_content = self.content_shadow.clone();
        for change in &changes {
            Document::apply_change_to_rope(&mut new_content, change);
        }

        // Echo
        if new_content == self.content {
            self.content_shadow = new_content;
            return false;
        }

        let user_edits = diff::calculate_edits(&self.content, &new_content);
        let mut crdt_changed = false;

        for edit in &user_edits {
            let (start, end) = Self::get_offsets_from_rope(&self.content, &edit.range);
            let agent = self.crdt.get_or_create_agent_id(&self.agent_id);

            if start < end {
                self.crdt.delete(agent, start..end);
                crdt_changed = true;
            }
            if !edit.new_text.is_empty() {
                self.crdt.insert(agent, start, &edit.new_text);
                crdt_changed = true;
            }
        }

        self.content_shadow = new_content;
        self.content = Rope::from_str(&self.crdt.branch.content().to_string());

        crdt_changed
    }

    /// Processes a patch from a peer.
    ///
    /// # Arguments
    ///
    /// * `patch` - The patch to apply on this doc.
    ///
    /// # Returns
    ///
    /// * `Some(Vec<TextEdit>)` if the editor needs to be updated.
    /// * `None` if not.
    pub fn apply_remote_patch(&mut self, patch: &[u8]) -> Option<Vec<TextEdit>> {
        let old_shadow = self.content_shadow.clone();

        match self.crdt.oplog.decode_and_add(patch) {
            Ok(_) => {
                self.crdt
                    .branch
                    .merge(&self.crdt.oplog, self.crdt.oplog.local_version_ref());

                let new_content = Rope::from_str(&self.crdt.branch.content().to_string());
                self.content = new_content;

                let edits = diff::calculate_edits(&old_shadow, &self.content);

                self.last_version = self.crdt.oplog.local_version();

                if !edits.is_empty() { Some(edits) } else { None }
            }
            Err(e) => {
                error!("[Core] Failed to merge: {:?}", e);
                None
            }
        }
    }

    /// Returns the crdt history, starting at a given time
    pub fn get_patch_since(&self, since: &[Time]) -> Vec<u8> {
        self.crdt.oplog.encode_from(EncodeOptions::default(), since)
    }

    // =========================================================================
    //  HELPERS
    // =========================================================================

    /// Converts LSP Position (Line, Char) to Byte Offset
    fn get_offsets_from_rope(rope: &Rope, range: &Range) -> (usize, usize) {
        let len_lines = rope.len_lines();

        // Clamp line index
        let start_line = range.start.line.min(len_lines.saturating_sub(1));
        let end_line = range.end.line.min(len_lines.saturating_sub(1));

        let start_char_idx = rope.line_to_char(start_line) + range.start.character;
        let end_char_idx = rope.line_to_char(end_line) + range.end.character;

        let len_chars = rope.len_chars();
        (start_char_idx.min(len_chars), end_char_idx.min(len_chars))
    }

    /// Helper to mutate a Rope based on an LSP change event
    pub(crate) fn apply_change_to_rope(rope: &mut Rope, change: &TextDocumentContentChangeEvent) {
        if let Some(range) = &change.range {
            let (s, e) = Self::get_offsets_from_rope(rope, range);

            // Remove old text
            if s < e {
                rope.remove(s..e);
            }
            // Insert new text
            if !change.text.is_empty() {
                rope.insert(s, &change.text);
            }
        } else {
            // Full text replacement
            *rope = Rope::from_str(&change.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::lsp::{Position, Range};

    #[test]
    fn test_sync() {
        let mut doc = Document::new("hello", "agent-1", true);

        let change = TextDocumentContentChangeEvent {
            range: None,
            text: "world".to_string(),
        };
        assert!(doc.apply_local_changes(vec![change]));

        assert_eq!(doc.content.to_string(), "world");
        assert_eq!(doc.crdt.branch.content().to_string(), "world");

        let incremental_change = TextDocumentContentChangeEvent {
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
            text: "hello ".to_string(),
        };
        assert!(doc.apply_local_changes(vec![incremental_change]));

        assert_eq!(doc.content.to_string(), "hello world");
        assert_eq!(doc.crdt.branch.content().to_string(), "hello world");
    }

    #[test]
    fn test_local_change_without_pending_remote() {
        let mut doc = Document::new("initial", "agent-1", true);

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
            text: " extra".to_string(),
        };

        assert!(doc.apply_local_changes(vec![change]));
        assert_eq!(doc.content.to_string(), "initial extra");
        assert_eq!(doc.content_shadow.to_string(), "initial extra");
    }

    #[test]
    fn test_pure_echo_suppression() {
        let mut local = Document::new("hello", "agent-local", true);
        let mut remote = Document::new("", "agent-remote", false);

        // Hydrate remote with the initial state so its CRDT has "hello"
        let hydration_patch = local
            .crdt
            .oplog
            .encode(diamond_types::list::encoding::EncodeOptions::default());
        remote.apply_remote_patch(&hydration_patch);
        remote.content_shadow = remote.content.clone();

        // Remote peer inserts " world"
        let remote_change = TextDocumentContentChangeEvent {
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
            text: " world".to_string(),
        };
        assert!(remote.apply_local_changes(vec![remote_change]));

        // Send the remote patch to the local document
        let patch = remote
            .crdt
            .oplog
            .encode(diamond_types::list::encoding::EncodeOptions::default());
        let edits = local
            .apply_remote_patch(&patch)
            .expect("remote patch should produce edits");
        assert!(!edits.is_empty());

        // Local editor has not yet echoed, so shadow is still the old view
        assert_eq!(local.content_shadow.to_string(), "hello");
        assert_eq!(local.content.to_string(), "hello world");

        // Editor echoes the remote edit
        let echo = TextDocumentContentChangeEvent {
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
            text: " world".to_string(),
        };

        let result = local.apply_local_changes(vec![echo]);
        assert!(!result, "echo should be suppressed");

        assert_eq!(local.content.to_string(), "hello world");
        assert_eq!(local.content_shadow.to_string(), "hello world");
    }

    #[test]
    fn test_mixed_echo_and_user_input() {
        let mut local = Document::new("hello", "agent-local", true);
        let mut remote = Document::new("", "agent-remote", false);

        // Hydrate remote with the initial state so its CRDT has "hello"
        let hydration_patch = local
            .crdt
            .oplog
            .encode(diamond_types::list::encoding::EncodeOptions::default());
        remote.apply_remote_patch(&hydration_patch);
        remote.content_shadow = remote.content.clone();

        // Remote peer inserts " remote"
        let remote_change = TextDocumentContentChangeEvent {
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
            text: " remote".to_string(),
        };
        assert!(remote.apply_local_changes(vec![remote_change]));

        let patch = remote
            .crdt
            .oplog
            .encode(diamond_types::list::encoding::EncodeOptions::default());
        local.apply_remote_patch(&patch);

        // Editor echoes the remote edit, then the user types " user"
        let mixed_changes = vec![
            TextDocumentContentChangeEvent {
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
                text: " remote".to_string(),
            },
            TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 12,
                    },
                    end: Position {
                        line: 0,
                        character: 12,
                    },
                }),
                text: " user".to_string(),
            },
        ];

        let result = local.apply_local_changes(mixed_changes);
        assert!(result, "user portion should be applied");

        assert_eq!(local.content.to_string(), "hello remote user");
        assert_eq!(local.content_shadow.to_string(), "hello remote user");
    }

    #[test]
    fn test_crdt_convergence_with_hydration() {
        let mut host = Document::new("Hello", "host-agent", true);
        let mut peer = Document::new("", "peer-agent", false);

        // Initial Hydration: Host sends its current state to Peer
        let hydration_patch = host
            .crdt
            .oplog
            .encode(diamond_types::list::encoding::EncodeOptions::default());
        peer.apply_remote_patch(&hydration_patch);
        peer.content_shadow = peer.content.clone();

        assert_eq!(peer.content.to_string(), "Hello");

        // Concurrent changes
        let change_host = TextDocumentContentChangeEvent {
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
            text: " Alice".to_string(),
        };
        assert!(host.apply_local_changes(vec![change_host]));
        let patch_host = host.get_patch_since(&host.last_version);
        host.last_version = host.crdt.oplog.local_version();

        let change_peer = TextDocumentContentChangeEvent {
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
            text: " Bob".to_string(),
        };
        assert!(peer.apply_local_changes(vec![change_peer]));
        let patch_peer = peer.get_patch_since(&peer.last_version);
        peer.last_version = peer.crdt.oplog.local_version();

        // Swap patches
        host.apply_remote_patch(&patch_peer);
        peer.apply_remote_patch(&patch_host);

        // They must converge
        assert_eq!(host.content.to_string(), peer.content.to_string());
    }

    #[test]
    fn test_workspace_snapshot() {
        let mut ws = Workspace::new("host".to_string());
        ws.get_or_create("file1.txt", "content1", true);
        ws.get_or_create("file2.txt", "content2", true);

        let snapshot = ws.get_snapshot();
        assert_eq!(snapshot.len(), 2);

        // Verify we can hydrate a new workspace from this snapshot
        let mut new_ws = Workspace::new("peer".to_string());
        for (uri, patch) in snapshot {
            let doc = new_ws.get_or_create_empty(uri.as_str(), false);
            doc.apply_remote_patch(&patch);
        }

        assert_eq!(
            new_ws
                .documents
                .get("file1.txt")
                .unwrap()
                .content
                .to_string(),
            "content1"
        );
        assert_eq!(
            new_ws
                .documents
                .get("file2.txt")
                .unwrap()
                .content
                .to_string(),
            "content2"
        );
    }

    #[test]
    fn test_complex_convergence() {
        let mut peer_a = Document::new("", "a", true);
        let mut peer_b = Document::new("", "b", false);

        // 1. A types something
        assert!(
            peer_a.apply_local_changes(vec![TextDocumentContentChangeEvent {
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
                text: "The quick brown fox".to_string(),
            }])
        );

        let patch1 = peer_a
            .crdt
            .oplog
            .encode(diamond_types::list::encoding::EncodeOptions::default());
        peer_b.apply_remote_patch(&patch1);
        peer_b.content_shadow = peer_b.content.clone();

        // 2. Concurrent edits: A deletes "quick", B inserts "lazy " before "fox"
        assert!(
            peer_a.apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 4,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                }),
                text: "".to_string(),
            }])
        );
        let patch_a = peer_a.get_patch_since(&peer_a.last_version);
        peer_a.last_version = peer_a.crdt.oplog.local_version();

        assert!(
            peer_b.apply_local_changes(vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 16,
                    },
                    end: Position {
                        line: 0,
                        character: 16,
                    },
                }),
                text: "lazy ".to_string(),
            }])
        );
        let patch_b = peer_b.get_patch_since(&peer_b.last_version);
        peer_b.last_version = peer_b.crdt.oplog.local_version();

        // 3. Swap patches
        peer_a.apply_remote_patch(&patch_b);
        peer_b.apply_remote_patch(&patch_a);

        // 4. Verify convergence
        assert_eq!(peer_a.content.to_string(), peer_b.content.to_string());
        assert!(peer_a.content.to_string().contains("lazy"));
        assert!(!peer_a.content.to_string().contains("quick"));
    }
}
