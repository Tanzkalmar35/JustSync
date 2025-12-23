// src/diff.rs

use crate::lsp::{Position, Range, TextEdit};
use ropey::Rope;
use similar::{DiffTag, TextDiff};

pub fn calculate_edits(old: &Rope, new: &Rope) -> Vec<TextEdit> {
    // Identity check
    if old == new {
        return Vec::new();
    }

    let len_old = old.len_chars();
    let len_new = new.len_chars();

    // Step A: Find the first mismatch (Prefix Scan)
    let prefix_len = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();

    // Step B: Check if the REST matches perfectly
    // If we just inserted text at `prefix_len`, then the suffixes should match exactly.
    let old_suffix_start = prefix_len;
    let new_suffix_start = prefix_len + (len_new.saturating_sub(len_old));

    // If it's a pure insertion:
    // Old: [Prefix] [Suffix]
    // New: [Prefix] [INSERTED] [Suffix]
    if len_new > len_old {
        let inserted_len = len_new - len_old;
        // Check if the suffix of OLD matches the suffix of NEW (after the insertion)
        let old_slice = old.slice(prefix_len..);
        let new_slice = new.slice((prefix_len + inserted_len)..);

        if old_slice == new_slice {
            // SUCCESS: It is a clean insertion!
            let pos = offset_to_position(old, prefix_len);
            let inserted_text = new
                .slice(prefix_len..(prefix_len + inserted_len))
                .to_string();

            return vec![TextEdit {
                range: Range {
                    start: pos.clone(),
                    end: pos,
                },
                new_text: inserted_text,
            }];
        }
    }
    // If it's a pure deletion:
    // Old: [Prefix] [DELETED] [Suffix]
    // New: [Prefix] [Suffix]
    else if len_old > len_new {
        let deleted_len = len_old - len_new;
        let old_slice = old.slice((prefix_len + deleted_len)..);
        let new_slice = new.slice(prefix_len..);

        if old_slice == new_slice {
            // SUCCESS: It is a clean deletion!
            let start_pos = offset_to_position(old, prefix_len);
            let end_pos = offset_to_position(old, prefix_len + deleted_len);

            return vec![TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: String::new(),
            }];
        }
    }

    // -------------------------------------------------------------------------
    // FALLBACK: The "Dirty Middle" Diff
    // Only used for complex replaces, auto-formatting, or scattered edits.
    // -------------------------------------------------------------------------

    // We already calculated the prefix_len. Now calculate suffix.
    // We limit the suffix scan so it doesn't overlap the prefix we found.
    let common_suffix_len = old
        .chars_at(len_old) // Start iterator at the end of OLD
        .reversed() // Iterate backwards
        .zip(new.chars_at(len_new).reversed()) // Start iterator at end of NEW and reverse
        .take(len_old.min(len_new) - prefix_len) // Don't overlap with prefix
        .take_while(|&(a, b)| a == b)
        .count();

    let start = prefix_len;
    let old_end = len_old - common_suffix_len;
    let new_end = len_new - common_suffix_len;

    // Allocate only the changed region
    let old_middle = old.slice(start..old_end).to_string();
    let new_middle = new.slice(start..new_end).to_string();

    let diff = TextDiff::from_chars(&old_middle, &new_middle);
    let mut edits = Vec::new();

    for op in diff.ops() {
        if op.tag() == DiffTag::Equal {
            continue;
        }

        let local_start = op.old_range().start;
        let local_end = op.old_range().end;

        let global_start = start + local_start;
        let global_end = start + local_end;

        let range = Range {
            start: offset_to_position(old, global_start),
            end: offset_to_position(old, global_end),
        };

        let new_text_fragment = &new_middle[op.new_range()];

        edits.push(TextEdit {
            range,
            new_text: new_text_fragment.to_string(),
        });
    }

    edits
}

fn offset_to_position(rope: &Rope, char_idx: usize) -> Position {
    // Ropey handles this log(N)
    let line_idx = rope.char_to_line(char_idx);
    let line_start_char = rope.line_to_char(line_idx);
    let col = char_idx - line_start_char;
    Position {
        line: line_idx,
        character: col,
    }
}
