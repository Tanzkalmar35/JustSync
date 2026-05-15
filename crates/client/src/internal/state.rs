use diamond_types::list::ListCRDT;
use ropey::Rope;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    internal::{
        diff,
        lsp::{Range, TextDocumentContentChangeEvent, TextEdit},
    },
    logger,
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

    pub fn mark_open(&mut self, uri: String) {
        self.open_files.insert(uri);
    }

    pub fn mark_closed(&mut self, uri: &str) {
        self.open_files.remove(uri);
    }

    #[must_use]
    pub fn is_open(&self, uri: &str) -> bool {
        self.open_files.contains(uri)
    }
}

/// A single file in the workspace.
/// Encapsulates the synchronization logic ("The Brain of the File").
#[must_use]
pub struct Document {
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
    pub fn new(initial_content: &str, agent_id: &str, is_host: bool) -> Self {
        let mut crdt = ListCRDT::new();

        // Initialize CRDT with content if present
        if !initial_content.is_empty() && is_host {
            let agent = crdt.get_or_create_agent_id("init");
            crdt.insert(agent, 0, initial_content);
        }

        Self {
            content: Rope::from_str(initial_content),
            crdt,
            agent_id: agent_id.to_string(),
            pending_remote_updates: AtomicUsize::new(0),
        }
    }

    /// Processes changes from the editor.
    /// Returns: `Some(Vec<u8>)` (the patch bytes) if the network needs to be notified.
    /// Returns: `None` if the change was an echo or no-op.
    pub fn apply_local_changes(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> Option<Vec<u8>> {
        // Echo guard
        if self.pending_remote_updates.load(Ordering::SeqCst) > 0 {
            logger::log(&format!(
                "Received update request - blocking. Pending counter: {}",
                self.pending_remote_updates.load(Ordering::SeqCst)
            ));
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

                let edits = diff::calculate_edits(&old_rope, &new_rope);
                logger::log(&format!("Calculated edits: {edits:?}"));
                if edits.is_empty() { None } else { Some(edits) }
            }
            Err(e) => {
                logger::log(&format!("!! [CRDT] Failed to merge: {e:?}"));
                None
            }
        }
    }

    // =========================================================================
    //  HELPERS
    // =========================================================================

    /// Converts LSP Position (Line, Char) to Byte Offset
    fn get_offsets_from_rope(rope: &Rope, range: &Range) -> (usize, usize) {
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
    use super::*;
    use crate::internal::lsp::{Position, Range};

    #[test]
    fn test_echo_suppression() {
        let mut doc = Document::new("initial", "agent-1", true);
        
        // Simulate a remote update pending
        doc.pending_remote_updates.store(1, Ordering::SeqCst);

        // A local change comes in (echo)
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 0, character: 0 },
            }),
            text: "ignored".to_string(),
        };

        let patch = doc.apply_local_changes(vec![change]);
        
        // Must be None because it was suppressed
        assert!(patch.is_none());
        // Counter should be decremented
        assert_eq!(doc.pending_remote_updates.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_crdt_convergence_with_hydration() {
        // 1. Host starts with "Hello"
        let mut host = Document::new("Hello", "host-agent", true);
        
        // 2. Peer starts empty
        let mut peer = Document::new("", "peer-agent", false);

        // 3. Initial Hydration: Host sends its current state to Peer
        let hydration_patch = host.crdt.oplog.encode(diamond_types::list::encoding::EncodeOptions::default());
        peer.apply_remote_patch(&hydration_patch);
        
        assert_eq!(peer.content.to_string(), "Hello");

        // 4. Concurrent changes
        let change_host = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position { line: 0, character: 5 },
                end: Position { line: 0, character: 5 },
            }),
            text: " Alice".to_string(),
        };
        let patch_host = host.apply_local_changes(vec![change_host]).unwrap();

        let change_peer = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position { line: 0, character: 5 },
                end: Position { line: 0, character: 5 },
            }),
            text: " Bob".to_string(),
        };
        let patch_peer = peer.apply_local_changes(vec![change_peer]).unwrap();

        // 5. Swap patches
        host.apply_remote_patch(&patch_peer);
        peer.apply_remote_patch(&patch_host);

        // 6. They must converge
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
        
        assert_eq!(new_ws.documents.get("file1.txt").unwrap().content.to_string(), "content1");
        assert_eq!(new_ws.documents.get("file2.txt").unwrap().content.to_string(), "content2");
    }

    #[test]
    fn test_complex_convergence() {
        let mut peer_a = Document::new("", "a", true);
        let mut peer_b = Document::new("", "b", false);

        // 1. A types something
        let patch1 = peer_a.apply_local_changes(vec![TextDocumentContentChangeEvent {
            range: Some(Range { start: Position { line: 0, character: 0 }, end: Position { line: 0, character: 0 } }),
            text: "The quick brown fox".to_string(),
        }]).unwrap();
        
        // 2. B receives it
        peer_b.apply_remote_patch(&patch1);

        // 3. Concurrent edits: A deletes "quick", B inserts "lazy " before "fox"
        let patch_a = peer_a.apply_local_changes(vec![TextDocumentContentChangeEvent {
            range: Some(Range { start: Position { line: 0, character: 4 }, end: Position { line: 0, character: 10 } }),
            text: "".to_string(),
        }]).unwrap();

        let patch_b = peer_b.apply_local_changes(vec![TextDocumentContentChangeEvent {
            range: Some(Range { start: Position { line: 0, character: 16 }, end: Position { line: 0, character: 16 } }),
            text: "lazy ".to_string(),
        }]).unwrap();

        // 4. Swap patches
        peer_a.apply_remote_patch(&patch_b);
        peer_b.apply_remote_patch(&patch_a);

        // 5. Verify convergence
        assert_eq!(peer_a.content.to_string(), peer_b.content.to_string());
        assert!(peer_a.content.to_string().contains("lazy"));
        assert!(!peer_a.content.to_string().contains("quick"));
    }
}
