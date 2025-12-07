use std::{collections::HashMap, sync::atomic::AtomicUsize};

use diamond_types::list::ListCRDT;
use ropey::Rope;

pub struct Workspace {
    pub state: HashMap<String, Document>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
        }
    }

    pub fn get_mut(&mut self, uri: &str) -> Option<&mut Document> {
        self.state.get_mut(uri)
    }

    pub fn get_or_create(&mut self, uri: String, content: String) -> &mut Document {
        self.state
            .entry(uri.clone())
            .or_insert_with(|| Document::new(uri, content))
    }
}

pub struct Document {
    pub uri: String,
    pub content: Rope,
    pub crdt: ListCRDT,
    pub pending_remote_updates: AtomicUsize,
}

impl Document {
    pub fn new(uri: String, initial_content: String) -> Self {
        let mut crdt = ListCRDT::new();

        // If there is initial content, we must "type" it into the CRDT
        // so the CRDT state matches the Rope state.
        if !initial_content.is_empty() {
            // Get the internal ID for "root" (start of doc) or a local agent
            let agent = crdt.get_or_create_agent_id("init");
            crdt.insert(agent, 0, &initial_content);
        }

        Self {
            uri,
            content: Rope::from_str(&initial_content),
            crdt,
            pending_remote_updates: AtomicUsize::new(0),
        }
    }

    pub fn pos_to_offset(&self, line: usize, col: usize) -> usize {
        // Safety: Handle line out of bounds gracefully or panic?
        // For now, let's assume valid input, but in prod, use `try_line_to_char`.
        let line_start = self.content.line_to_char(line);
        line_start + col
    }

    pub fn get_offsets(
        &self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> (usize, usize) {
        let start = self.pos_to_offset(start_line, start_col);
        let end = self.pos_to_offset(end_line, end_col);
        (start, end)
    }

    // Update ONLY the Rope (The "View")
    pub fn update_rope(&mut self, start_idx: usize, end_idx: usize, text: &str) {
        self.content.remove(start_idx..end_idx);
        self.content.insert(start_idx, text);
    }

    // Update ONLY the CRDT (The "Truth")
    // We pass the raw indices so we don't recalculate them (which might be wrong after rope update)
    pub fn update_crdt(&mut self, start_idx: usize, end_idx: usize, text: &str) {
        let agent = self.crdt.get_or_create_agent_id("local-user");

        let len_to_delete = end_idx - start_idx;
        if len_to_delete > 0 {
            self.crdt
                .delete(agent, start_idx..start_idx + len_to_delete);
        }

        if !text.is_empty() {
            self.crdt.insert(agent, start_idx, text);
        }
    }

    // The Mirror Test
    pub fn is_synced(&self) -> bool {
        let crdt_text = self.crdt.branch.content().to_string();
        self.content.to_string() == crdt_text
    }
}
