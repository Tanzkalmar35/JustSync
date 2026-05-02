use dissimilar::Chunk;
use ropey::Rope;

use crate::internal::lsp::{Position, Range, TextEdit};

pub fn calculate_edits(old: &Rope, new: &Rope) -> Vec<TextEdit> {
    // Fast pointer comparison or deep comparison if pointers differ.
    if old == new {
        return Vec::new();
    }

    let len_old = old.len_chars();
    let len_new = new.len_chars();

    // Prefix Scan (Optimization)
    // Find how many characters at the start are identical.
    let prefix_len = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();

    // Suffix Scan (Optimization)
    // Find how many characters at the end are identical.
    // strictly ensure the suffix does not overlap with the prefix we just found.
    let common_suffix_len = old
        .chars_at(len_old)
        .reversed()
        .zip(new.chars_at(len_new).reversed())
        .take(len_old.min(len_new) - prefix_len)
        .take_while(|&(a, b)| a == b)
        .count();

    // Calculate the "Dirty Middle" Boundaries
    let start = prefix_len;
    let old_end = len_old - common_suffix_len;
    let new_end = len_new - common_suffix_len;

    // Fast Path: Pure Insertion or Deletion
    // If the middle of one side is empty, it's a simple insert/delete.
    // We don't need the expensive Diff algorithm for this.

    // Case A: Pure Insertion
    if start == old_end && start != new_end {
        let inserted_text = new.slice(start..new_end).to_string();
        let pos = offset_to_position(old, start);

        return vec![TextEdit {
            range: Range {
                start: pos.clone(),
                end: pos,
            },
            new_text: inserted_text,
        }];
    }

    // Case B: Pure Deletion
    if start != old_end && start == new_end {
        return vec![TextEdit {
            range: Range {
                start: offset_to_position(old, start),
                end: offset_to_position(old, old_end),
            },
            new_text: String::new(),
        }];
    }

    // Fallback: The "Dirty Middle" Diff
    // Used for replacements, disjoint edits, or complex changes.

    let old_middle = old.slice(start..old_end).to_string();
    let new_middle = new.slice(start..new_end).to_string();

    let chunks = dissimilar::diff(&old_middle, &new_middle);

    let mut edits = Vec::new();
    let mut current_pos = start;

    for chunk in chunks {
        match chunk {
            Chunk::Equal(text) => {
                // Just advance the cursor.
                current_pos += text.chars().count();
            }
            Chunk::Delete(text) => {
                let len = text.chars().count();
                // Emit deletion from current_pos to current_pos + len
                let start_pos = offset_to_position(old, current_pos);
                let end_pos = offset_to_position(old, current_pos + len);

                edits.push(TextEdit {
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    new_text: String::new(),
                });

                // Advance cursor past the deleted text
                current_pos += len;
            }
            Chunk::Insert(text) => {
                // Emit insertion at current_pos
                let pos = offset_to_position(old, current_pos);

                edits.push(TextEdit {
                    range: Range {
                        start: pos.clone(),
                        end: pos,
                    },
                    new_text: text.to_string(),
                });
                // Do NOT advance 'current_pos' because we inserted text at this spot;
                // the original text hasn't been consumed.
            }
        }
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
