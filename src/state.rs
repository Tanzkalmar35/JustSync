use diamond_types::list::ListCRDT;
use ropey::Rope;
use std::collections::HashMap;

use crate::lsp::{TextDocumentContentChangeEvent, TextEdit};

pub struct Workspace {
    pub documents: HashMap<String, Document>,
    pub local_agent_id: String,
}

impl Workspace {
    pub fn new(agent_id: String) -> Self {
        Self {
            documents: HashMap::new(),
            local_agent_id: agent_id,
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

    /// ECHO GUARD:
    /// Stores the content state we expect the editor to have after a sync.
    /// If the editor sends us this exact state back, we ignore it.
    last_synced_content: Option<String>,
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
            last_synced_content: None,
        }
    }

    // =========================================================================
    //  INBOUND: From Local Editor (Stdin)
    // =========================================================================

    /// Processes changes from the editor.
    /// Returns: `Some(Vec<u8>)` (the patch bytes) if the network needs to be notified.
    /// Returns: `None` if the change was an echo or no-op.
    // pub fn apply_local_changes(
    //     &mut self,
    //     changes: Vec<TextDocumentContentChangeEvent>,
    // ) -> Option<Vec<u8>> {
    //     let mut patch_generated = false;

    //     // Apply changes to a temporary Rope first to check the result.
    //     let mut temp_rope = self.content.clone();
    //     for change in &changes {
    //         self.apply_change_to_rope(&mut temp_rope, change);
    //     }

    //     // Echo guard check
    //     let new_content_str = temp_rope.to_string();

    //     if let Some(expected) = &self.last_synced_content {
    //         // // Compare trimmed strings to avoid whitespace noise often caused by different editors
    //         // if expected.trim() == new_content_str.trim() {
    //         //     // Match! This is an echo.
    //         //     self.last_synced_content = None;
    //         //     self.content = temp_rope; // Sync our View to match the Editor
    //         //     return None; // Do NOT send to network
    //         // }

    //         // [FIX] Normalize both strings to ignore CRLF vs LF differences
    //         let norm_expected = expected.replace("\r", "");
    //         let norm_new = new_content_str.replace("\r", "");

    //         if norm_expected == norm_new {
    //             // Perfect match (ignoring line endings)
    //             self.last_synced_content = None;
    //             self.content = temp_rope;
    //             return None;
    //         }

    //         // [OPTIONAL] Keep trim check as a fallback for trailing newlines
    //         if norm_expected.trim() == norm_new.trim() {
    //             self.last_synced_content = None;
    //             self.content = temp_rope;
    //             return None;
    //         }

    //         // Log if still failing
    //         crate::logger::log(&format!(
    //             "!! [Guard] Mismatch on {}.\nExp len: {}\nGot len: {}",
    //             self.uri,
    //             norm_expected.len(),
    //             norm_new.len()
    //         ));
    //     }

    //     // User Edit Confirmed, update CRDT.

    //     self.last_synced_content = None; // Reset guard since state diverged

    //     for change in changes {
    //         // Re-apply to self.content so we can calculate CRDT offsets correctly
    //         if let Some(range) = &change.range {
    //             let (start, end) = self.get_offsets_from_rope(&self.content, range);

    //             let agent = self.crdt.get_or_create_agent_id(&self.agent_id);

    //             // Update CRDT (The Truth)
    //             if start < end {
    //                 self.crdt.delete(agent, start..end);
    //             }
    //             if !change.text.is_empty() {
    //                 self.crdt.insert(agent, start, &change.text);
    //             }
    //             patch_generated = true;
    //         }

    //         // Update the authoritative Rope (The View) for the next iteration of the loop
    //         self.apply_change_to_rope(&mut self.content.clone(), &change);
    //     }

    //     if patch_generated {
    //         // Generate OpLog Patch
    //         Some(
    //             self.crdt
    //                 .oplog
    //                 .encode(diamond_types::list::encoding::EncodeOptions::default()),
    //         )
    //     } else {
    //         None
    //     }
    // }

    pub fn apply_local_changes(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> Option<Vec<u8>> {
        // 1. Apply changes to a temporary rope first
        let mut temp_rope = self.content.clone();
        for change in &changes {
            self.apply_change_to_rope(&mut temp_rope, change);
        }

        let new_content_str = temp_rope.to_string();

        // 2. ECHO GUARD CHECK
        if let Some(expected) = &self.last_synced_content {
            // Helper to clean strings for comparison
            // Removes Carriage Returns AND strips surrounding whitespace (handling trailing \n)
            let normalize = |s: &str| -> String { s.replace("\r", "").trim().to_string() };

            let norm_expected = normalize(expected);
            let norm_new = normalize(&new_content_str);

            if norm_expected == norm_new {
                // It matches! It was just an echo or a formatter newline.
                // Treat it as "Synced" and do NOT generate a patch.
                self.last_synced_content = None;
                self.content = temp_rope; // Update our view to match VS Code (accept the newline)
                return None;
            }

            // [DEBUG] Log the failure details if it still fails
            crate::logger::log(&format!(
                "!! [Guard] Mismatch on {}.\nExp len: {} (Norm: {})\nGot len: {} (Norm: {})",
                self.uri,
                expected.len(),
                norm_expected.len(),
                new_content_str.len(),
                norm_new.len()
            ));
        }

        // 3. If we got here, it's a real user edit.
        self.last_synced_content = None;
        self.content = temp_rope;

        // ... generate patch logic (crate::diff::calculate_diff ...) ...
        let patch = generate_patch(&self.crdt.oplog, &self.content.to_string());
        Some(patch)
    }

    // =========================================================================
    //  INBOUND: From Network (QUIC)
    // =========================================================================

    /// Processes a patch from a peer.
    /// Returns: `Some(Vec<TextEdit>)` if the editor needs to be updated.
    pub fn apply_remote_patch(&mut self, patch: &[u8]) -> Option<Vec<TextEdit>> {
        // 1. Snapshot old state
        let old_rope = self.content.clone();

        // 2. Merge CRDT Patch into Oplog
        let merge_result = self.crdt.oplog.decode_and_add(patch);

        match merge_result {
            Ok(_) => {
                // Fast-forward the current branch state
                // Without this, 'branch.content()' returns empty string,
                // causing the system to think it needs to re-insert everything.
                self.crdt
                    .branch
                    .merge(&self.crdt.oplog, self.crdt.oplog.local_version_ref());

                // 2. Reconstruct text
                let new_text = self.crdt.branch.content().to_string();
                let new_rope = Rope::from_str(&new_text);

                self.last_synced_content = Some(new_text);
                self.content = new_rope.clone();

                let edits = crate::diff::calculate_edits(&old_rope, &new_rope);
                if edits.is_empty() { None } else { Some(edits) }
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
    fn get_offsets_from_rope(&self, rope: &Rope, range: &crate::lsp::Range) -> (usize, usize) {
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
    fn apply_change_to_rope(&self, rope: &mut Rope, change: &TextDocumentContentChangeEvent) {
        if let Some(range) = &change.range {
            let (s, e) = self.get_offsets_from_rope(rope, range);

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
