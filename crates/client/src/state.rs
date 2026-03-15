use diamond_types::list::ListCRDT;
use ropey::Rope;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    logger,
    lsp::{TextDocumentContentChangeEvent, TextEdit},
};

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
    pub fn get_or_create(&mut self, uri: String, content: String) -> &mut Document {
        self.documents
            .entry(uri.clone())
            .or_insert_with(|| Document::new(uri, content, &self.local_agent_id))
    }

    /// Retrieves a document or creates an empty one if it doesn't exist.
    pub fn get_or_create_empty(&mut self, uri: String) -> &mut Document {
        if !self.documents.contains_key(&uri) {
            self.documents.insert(
                uri.clone(),
                Document::new(uri.clone(), String::new(), &self.local_agent_id),
            );
        }
        self.documents.get_mut(&uri).unwrap()
    }

    /// Serializes the entire state of all documents
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

    pub fn mark_open(&mut self, uri: String) {
        self.open_files.insert(uri);
    }

    pub fn mark_closed(&mut self, uri: &str) {
        self.open_files.remove(uri);
    }

    pub fn is_open(&self, uri: &str) -> bool {
        self.open_files.contains(uri)
    }
}

/// A single file in the workspace.
/// Encapsulates the synchronization logic ("The Brain of the File").
pub struct Document {
    pub uri: String,

    /// The "View" - What the user sees in the editor.
    /// Optimized for random access and slicing.
    pub content: Rope,

    /// The "Truth" - The mathematical CRDT history.
    /// Handles conflict resolution.
    pub crdt: ListCRDT,

    /// The ID of the local agent (used for tagging CRDT ops).
    agent_id: String,

    pub pending_remote_updates: AtomicUsize,
}

impl Document {
    pub fn new(uri: String, initial_content: String, agent_id: &str) -> Self {
        let mut crdt = ListCRDT::new();

        // Initialize CRDT with content if present
        if !initial_content.is_empty() {
            let agent = crdt.get_or_create_agent_id("init");
            crdt.insert(agent, 0, &initial_content);
        }

        Self {
            uri,
            content: Rope::from_str(&initial_content),
            crdt,
            agent_id: agent_id.to_string(),
            pending_remote_updates: AtomicUsize::new(0),
        }
    }

    // =========================================================================
    //  INBOUND: From Local Editor (Stdin)
    // =========================================================================

    /// Processes changes from the editor.
    /// Returns: `Some(Vec<u8>)` (the patch bytes) if the network needs to be notified.
    /// Returns: `None` if the change was an echo or no-op.
    pub fn apply_local_changes(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> Option<Vec<u8>> {
        // Echo guard
        if self.pending_remote_updates.load(Ordering::SeqCst) > 0 {
            logger::log("Received update request, but blocking due to pending counter");
            self.pending_remote_updates.fetch_sub(1, Ordering::SeqCst);
            return None;
        }

        let mut patch_generated = false;

        for change in changes {
            // Calculate change offsets
            if let Some(range) = &change.range {
                let (start, end) = Self::get_offsets_from_rope(&self.content, range);
                let agent = self.crdt.get_or_create_agent_id(&self.agent_id);

                // Apply changes
                if start < end {
                    self.crdt.delete(agent, start..end);
                }
                if !change.text.is_empty() {
                    self.crdt.insert(agent, start, &change.text);
                }
                patch_generated = true;
            }

            // Update editor view (rope)
            Self::apply_change_to_rope(&mut self.content, &change);
        }

        if patch_generated {
            logger::log(">> Generating Patch for User Edit");
            Some(
                self.crdt
                    .oplog
                    .encode(diamond_types::list::encoding::EncodeOptions::default()),
            )
        } else {
            None
        }
    }

    // =========================================================================
    //  INBOUND: From Network (QUIC)
    // =========================================================================

    /// Processes a patch from a peer.
    /// Returns: `Some(Vec<TextEdit>)` if the editor needs to be updated.
    pub fn apply_remote_patch(&mut self, patch: &[u8]) -> Option<Vec<TextEdit>> {
        let old_rope = self.content.clone();

        // Merge CRDT Patch into Oplog
        let merge_result = self.crdt.oplog.decode_and_add(patch);

        match merge_result {
            Ok(_) => {
                // Fast-forward the current branch state
                // Without this, 'branch.content()' returns empty string,
                // causing the system to think it needs to re-insert everything.
                self.crdt
                    .branch
                    .merge(&self.crdt.oplog, self.crdt.oplog.local_version_ref());

                // Reconstruct text
                let new_text = self.crdt.branch.content().to_string();
                let new_rope = Rope::from_str(&new_text);
                self.content = new_rope.clone();

                let edits = crate::diff::calculate_edits(&old_rope, &new_rope);
                logger::log(&format!("Calculated edits: {:?}", edits));
                if edits.is_empty() {
                    None
                } else {
                    self.pending_remote_updates.fetch_add(1, Ordering::SeqCst);
                    Some(edits)
                }
            }
            Err(e) => {
                eprintln!("!! [CRDT] Failed to merge: {:?}", e);
                None
            }
        }
    }

    // =========================================================================
    //  HELPERS
    // =========================================================================

    /// Converts LSP Position (Line, Char) to Byte Offset
    fn get_offsets_from_rope(rope: &Rope, range: &crate::lsp::Range) -> (usize, usize) {
        let len_lines = rope.len_lines();

        // Safety: Clamp line index
        let start_line = range.start.line.min(len_lines.saturating_sub(1));
        let end_line = range.end.line.min(len_lines.saturating_sub(1));

        let start_char_idx = rope.line_to_char(start_line) + range.start.character;
        let end_char_idx = rope.line_to_char(end_line) + range.end.character;

        let len_chars = rope.len_chars();
        (start_char_idx.min(len_chars), end_char_idx.min(len_chars))
    }

    /// Helper to mutate a Rope based on an LSP change event
    fn apply_change_to_rope(rope: &mut Rope, change: &TextDocumentContentChangeEvent) {
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
            // Full text replacement (uncommon in incremental sync but possible)
            *rope = Rope::from_str(&change.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lsp::{Position, Range};

    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_workspace_lifecycle() {
        let mut ws = Workspace::new("agent-A".to_string());
        let uri = "file:///test.txt".to_string();

        // Get or Create Empty
        let doc = ws.get_or_create_empty(uri.clone());
        assert_eq!(doc.content.len_chars(), 0);
        assert_eq!(doc.uri, uri);

        // Mark Open/Closed
        ws.mark_open(uri.clone());
        assert!(ws.is_open(&uri));
        ws.mark_closed(&uri);
        assert!(!ws.is_open(&uri));
    }

    #[test]
    fn test_apply_local_insertion() {
        let mut doc = Document::new("doc1".into(), "Hello".into(), "agent-A");

        // Simulate LSP Change: Insert " World" at the end
        let change = TextDocumentContentChangeEvent {
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
            text: " World".to_string(),
        };

        let patch = doc.apply_local_changes(vec![change]);

        // Verify View (Rope)
        assert_eq!(doc.content.to_string(), "Hello World");

        // Verify Truth (CRDT)
        assert_eq!(doc.crdt.branch.content().to_string(), "Hello World");

        // Verify Patch was generated
        assert!(patch.is_some());
    }

    #[test]
    fn test_apply_local_deletion() {
        let mut doc = Document::new("doc1".into(), "Hello World".into(), "agent-A");

        // Simulate LSP Change: Delete "Hello "
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            }),
            text: "".to_string(),
        };

        doc.apply_local_changes(vec![change]);

        assert_eq!(doc.content.to_string(), "World");
        assert_eq!(doc.crdt.branch.content().to_string(), "World");
    }

    #[test]
    fn test_remote_patch_merging() {
        // Create two documents representing two users
        let mut doc_a = Document::new("uri".into(), "Init".into(), "A");
        let mut doc_b = Document::new("uri".into(), "Init".into(), "B");

        // User A makes a change
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 4,
                },
                end: Position {
                    line: 0,
                    character: 4,
                },
            }),
            text: "ialized".to_string(),
        };

        let patch_bytes = doc_a
            .apply_local_changes(vec![change])
            .expect("Should gen patch");

        // User B receives patch
        let _edits = doc_b.apply_remote_patch(&patch_bytes);

        // Assert B is now "Initialized"
        assert_eq!(doc_b.content.to_string(), "Initialized");
        assert_eq!(doc_b.crdt.branch.content().to_string(), "Initialized");

        // Assert Pending Updates counter incremented (indicating UI needs redraw)
        assert_eq!(doc_b.pending_remote_updates.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_crdt_convergence() {
        // The "Diamond" Problem: Two agents edit the same spot concurrently.
        let mut doc_a = Document::new("uri".into(), "Start".into(), "A");
        let mut doc_b = Document::new("uri".into(), "Start".into(), "B");

        // A Inserts "X" at start -> "XStart"
        let change_a = TextDocumentContentChangeEvent {
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
            text: "X".to_string(),
        };
        let patch_from_a = doc_a.apply_local_changes(vec![change_a]).unwrap();

        // B Inserts "Y" at start -> "YStart"
        let change_b = TextDocumentContentChangeEvent {
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
            text: "Y".to_string(),
        };
        let patch_from_b = doc_b.apply_local_changes(vec![change_b]).unwrap();

        // Sync A <- B
        doc_a.apply_remote_patch(&patch_from_b);

        // Sync B <- A
        doc_b.apply_remote_patch(&patch_from_a);

        // Both must match exactly.
        // Diamond Types (SE-2) usually sorts by Agent ID for concurrent insertions at same site.
        assert_eq!(doc_a.content.to_string(), doc_b.content.to_string());

        // It will be either XYStart or YXStart, but must be consistent.
        println!("Converged state: {}", doc_a.content);
    }

    #[test]
    fn test_snapshot_restore() {
        // 1. Create a workspace with history
        let mut ws = Workspace::new("A".to_string());
        let uri = "file:///save.txt".to_string();
        let doc = ws.get_or_create(uri.clone(), "Initial".to_string());

        // Make an edit so history is non-empty
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
            text: " Saved".to_string(),
        };
        doc.apply_local_changes(vec![change]);

        // 2. Take Snapshot
        let snapshot = ws.get_snapshot();
        let (saved_uri, saved_data) = &snapshot[0];

        // 3. Rehydrate into a NEW Workspace
        // Note: You might need a method to load from snapshot,
        // or manually reconstruct specifically for this test:
        let mut crdt_new = ListCRDT::new();
        crdt_new
            .oplog
            .decode_and_add(saved_data)
            .expect("Snapshot decode failed");
        crdt_new
            .branch
            .merge(&crdt_new.oplog, crdt_new.oplog.local_version_ref());

        assert_eq!(saved_uri, &uri);
        assert_eq!(crdt_new.branch.content().to_string(), "Initial Saved");
    }

    // =========================================================================
    //  PROPTESTS (Fuzzing)
    // =========================================================================

    proptest! {
        #[test]
        fn test_apply_changes_resilience(
            initial_text in "\\PC*",
            line in 0usize..500,
            character in 0usize..500
        ) {
            let mut doc = Document::new("safe".into(), initial_text, "tester");

            let change = TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position { line, character },
                    end: Position { line, character: character + 1 }
                }),
                text: "safe".to_string()
            };

            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                 let _ = doc.apply_local_changes(vec![change]);
            }));
        }

        // Edit Fuzzing
        // Performs random insertions and deletions and ensures CRDT == Rope
        #[test]
        fn test_fuzz_local_edits(
            initial_text in "[a-z0-9]{0,20}",
            ref edits in prop::collection::vec(
                (0usize..20, "[a-z0-9]{0,5}", 0usize..20), 1..10 // (index, text, delete_len)
            )
        ) {
            let mut doc = Document::new("fuzz".into(), initial_text.clone(), "fuzzer");

            for &(mut idx, ref insert_text, ref delete_len) in edits {
                // normalize index to current length
                let current_len = doc.content.len_chars();
                if current_len == 0 {
                    idx = 0;
                } else {
                    idx %= current_len;
                }

                let mut end_idx = idx + delete_len;
                if end_idx > current_len { end_idx = current_len; }

                // Convert flat index back to LSP Position (reverse of your logic)
                let start_line = doc.content.char_to_line(idx);
                let start_col = idx - doc.content.line_to_char(start_line);

                let end_line = doc.content.char_to_line(end_idx);
                let end_col = end_idx - doc.content.line_to_char(end_line);

                let change = TextDocumentContentChangeEvent {
                    range: Some(Range {
                        start: Position { line: start_line, character: start_col },
                        end: Position { line: end_line, character: end_col },
                    }),
                    text: insert_text.to_string(),
                };

                doc.apply_local_changes(vec![change]);
            }

            // Invariant Check
            assert_eq!(doc.content.to_string(), doc.crdt.branch.content().to_string(), "Rope and CRDT desynced!");
        }
    }

    #[test]
    fn test_patch_idempotency() {
        // Applying the same patch twice should have no effect the second time
        let mut doc_a = Document::new("uri".into(), "Init".into(), "A");
        let mut doc_b = Document::new("uri".into(), "Init".into(), "B");

        // A makes change
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 4,
                },
                end: Position {
                    line: 0,
                    character: 4,
                },
            }),
            text: "ialized".to_string(),
        };
        let patch = doc_a.apply_local_changes(vec![change]).unwrap();

        // B applies ONCE
        let edits_1 = doc_b.apply_remote_patch(&patch);
        assert!(edits_1.is_some());
        assert_eq!(doc_b.content.to_string(), "Initialized");

        // B applies TWICE (Duplicate packet)
        let edits_2 = doc_b.apply_remote_patch(&patch);

        // Diamond Types handles duplicates gracefully (idempotent),
        // but depending on version it might return "no edits" or "empty edits".
        // Crucially, the content must remain correct.
        assert_eq!(doc_b.content.to_string(), "Initialized");

        // If it detected it as already applied, it should ideally return None or empty vector
        if let Some(e) = edits_2 {
            assert!(
                e.is_empty(),
                "Should not generate text edits for duplicate patch"
            );
        }
    }
}
