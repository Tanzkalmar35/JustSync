use diamond_types::list::ListCRDT;
use ropey::Rope;
use std::{collections::HashMap, sync::atomic::AtomicUsize, time::Instant};

pub struct Workspace {
    pub state: HashMap<String, Document>,
    pub local_agent_id: String,
}

impl Workspace {
    pub fn new(agent_id: String) -> Self {
        Self {
            state: HashMap::new(),
            local_agent_id: agent_id,
        }
    }

    pub fn get_or_create(&mut self, uri: String, content: String) -> &mut Document {
        if !self.state.contains_key(&uri) {
            crate::logger::log(&format!("Key NOT FOUND: '{}'. Creating NEW.", uri));
            self.state
                .entry(uri.clone())
                .or_insert_with(|| Document::new(uri, content))
        } else {
            self.state.get_mut(&uri).unwrap()
        }
    }
}

pub struct Document {
    pub uri: String,
    pub content: Rope,
    pub crdt: ListCRDT,
    pub pending_remote_updates: AtomicUsize,
    pub last_synced: Option<(String, Instant)>,
}

impl Document {
    pub fn new(uri: String, initial_content: String) -> Self {
        let mut crdt = ListCRDT::new();
        if !initial_content.is_empty() {
            let agent = crdt.get_or_create_agent_id("init");
            crdt.insert(agent, 0, &initial_content);
        }
        Self {
            uri,
            content: Rope::from_str(&initial_content),
            crdt,
            pending_remote_updates: AtomicUsize::new(0),
            last_synced: None,
        }
    }

    // [DEBUG] Call this to see exactly what is happening inside the doc
    pub fn debug_dump(&self, label: &str) {
        let rope_len = self.content.len_chars();
        let crdt_len = self.crdt.len();

        let rope_str = self.content.to_string().replace("\n", "\\n");
        let crdt_str = self.crdt.branch.content().to_string().replace("\n", "\\n");

        crate::logger::log(&format!("=== DEBUG [{}] ===", label));
        crate::logger::log(&format!("Rope Len: {} | CRDT Len: {}", rope_len, crdt_len));
        crate::logger::log(&format!("Rope: '{}'", rope_str));
        crate::logger::log(&format!("CRDT: '{}'", crdt_str));

        if rope_len != crdt_len || rope_str != crdt_str {
            crate::logger::log("!!!! FATAL DESYNC DETECTED !!!!");
        }
        crate::logger::log("=========================");
    }

    pub fn pos_to_offset(&self, line: usize, col: usize) -> usize {
        let len_lines = self.content.len_lines();
        if line >= len_lines {
            return self.content.len_chars();
        }
        let line_start = self.content.line_to_char(line);
        let line_len = self.content.line(line).len_chars();
        let safe_col = col.min(line_len);
        (line_start + safe_col).min(self.content.len_chars())
    }

    pub fn get_offsets(
        &self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> (usize, usize) {
        (
            self.pos_to_offset(start_line, start_col),
            self.pos_to_offset(end_line, end_col),
        )
    }

    pub fn update_rope(&mut self, start_idx: usize, end_idx: usize, text: &str) {
        let len = self.content.len_chars();
        let s = start_idx.min(len);
        let e = end_idx.min(len);
        if s < e {
            self.content.remove(s..e);
        }
        if !text.is_empty() {
            self.content.insert(s, text);
        }
    }

    pub fn update_crdt(&mut self, start_idx: usize, end_idx: usize, text: &str, agent_id: &str) {
        let agent = self.crdt.get_or_create_agent_id(agent_id);
        let crdt_len = self.crdt.len();
        let safe_start = start_idx.min(crdt_len);
        let safe_end = end_idx.min(crdt_len);

        // crate::logger::log(&format!(
        //     ">> [CRDT Op] Range {}-{} (Len {}). Text '{}'",
        //     safe_start,
        //     safe_end,
        //     crdt_len,
        //     text.replace("\n", "\\n")
        // ));

        if safe_start < safe_end {
            self.crdt.delete(agent, safe_start..safe_end);
        }
        if !text.is_empty() {
            self.crdt.insert(agent, safe_start, text);
        }
    }

    pub fn is_synced(&self) -> bool {
        let crdt_text = self.crdt.branch.content().to_string();
        self.content.to_string() == crdt_text
    }
}
