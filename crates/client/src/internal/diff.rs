use ropey::Rope;

use crate::internal::lsp::{Position, Range, TextEdit};

/// Calculates the edit operations needed to move from one state to another.
///
/// Given two `ropey::Rope` elements, this function calculates a list of text edits, that if
/// applied in correct order on the first rope result in the first rope having the same content as
/// the second one.
///
/// # Arguments
///
/// * `old` - The old state, eg the baseline to calculate edits upon.
/// * `new` - The new state, eg the desired state of the old state.
///
/// # Returns
///
/// A list of `TextEdit` operations, that if applied to the `old` rope in order, result in it
/// having the exact same content as `new`.
#[must_use]
pub fn calculate_edits(old: &Rope, new: &Rope) -> Vec<TextEdit> {
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

    // Pure Insertion
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

    // Pure Deletion
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

    let mut edits = Vec::new();

    if !old_middle.is_empty() || !new_middle.is_empty() {
        edits.push(TextEdit {
            range: Range {
                start: offset_to_position(old, start),
                end: offset_to_position(old, old_end),
            },
            new_text: new_middle,
        });
    }

    edits
}

/// Calculates the position inside of a document content string, based on a given char offset.
///
/// # Arguments
///
/// * `rope` - The content to calculate the position of the offset in.
/// * `char_idx` - The offset to use for position calculation.
///
/// # Returns
///
/// The calculated position, including row and column.
fn offset_to_position(rope: &Rope, char_idx: usize) -> Position {
    // Ropey handles this in log(n)
    let line_idx = rope.char_to_line(char_idx);
    let line_start_char = rope.line_to_char(line_idx);
    let col = char_idx - line_start_char;
    Position {
        line: line_idx,
        character: col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Helper to apply edits to a string for verification
    fn apply_edits_to_string(text: &str, edits: &[TextEdit]) -> String {
        let mut rope = Rope::from_str(text);
        // We must apply edits in reverse order if they overlap, but since
        // calculate_edits should return non-overlapping edits in order,
        // we can apply them by calculating the character offsets.
        // However, the safest way to verify is to apply them one by one,
        // but note that each edit's range refers to the ORIGINAL rope.
        // Thus, we apply them from back to front to avoid shifting indices.
        let mut sorted_edits = edits.to_vec();
        sorted_edits.sort_by(|a, b| {
            let a_start = a.range.start.line * 1000000 + a.range.start.character;
            let b_start = b.range.start.line * 1000000 + b.range.start.character;
            b_start.cmp(&a_start)
        });

        for edit in sorted_edits {
            let start_line_idx = rope.line_to_char(edit.range.start.line);
            let start_idx = start_line_idx + edit.range.start.character;

            let end_line_idx = rope.line_to_char(edit.range.end.line);
            let end_idx = end_line_idx + edit.range.end.character;

            rope.remove(start_idx..end_idx);
            rope.insert(start_idx, &edit.new_text);
        }
        rope.to_string()
    }

    #[test]
    fn equal_content_gives_no_edits_end() {
        let old_content = Rope::from_str("Some content");
        let new_content = Rope::from_str("Some content");

        let diff = calculate_edits(&old_content, &new_content);

        assert!(diff.is_empty());
    }

    #[test]
    fn test_multiline_edit() {
        let old = "line1\nline2\nline3";
        let new = "line1\nchanged\nline3";
        let old_rope = Rope::from_str(old);
        let new_rope = Rope::from_str(new);

        let edits = calculate_edits(&old_rope, &new_rope);
        assert_eq!(apply_edits_to_string(old, &edits), new);
    }

    #[test]
    fn test_unicode_emoji() {
        let old = "Hello 🦀 World";
        let new = "Hello 🦀 Rust";
        let old_rope = Rope::from_str(old);
        let new_rope = Rope::from_str(new);

        let edits = calculate_edits(&old_rope, &new_rope);
        assert_eq!(apply_edits_to_string(old, &edits), new);
    }

    #[test]
    fn test_newline_insertion() {
        let old = "line1line2";
        let new = "line1\nline2";
        let old_rope = Rope::from_str(old);
        let new_rope = Rope::from_str(new);

        let edits = calculate_edits(&old_rope, &new_rope);
        assert_eq!(apply_edits_to_string(old, &edits), new);
    }

    proptest! {
        #[test]
        fn prop_diff_is_correct(old in "\\PC*", new in "\\PC*") {
            let old_rope = Rope::from_str(&old);
            let new_rope = Rope::from_str(&new);
            let edits = calculate_edits(&old_rope, &new_rope);
            let result = apply_edits_to_string(&old, &edits);
            prop_assert_eq!(result, new);
        }
    }
}
