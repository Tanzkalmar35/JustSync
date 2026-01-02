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
