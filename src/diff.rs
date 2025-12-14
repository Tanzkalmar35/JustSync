use crate::lsp::{Position, Range, TextEdit};
use ropey::Rope;
use similar::{DiffTag, TextDiff};

/// Calculates character-level edits to transform `old` into `new`.
pub fn calculate_edits(old: &Rope, new: &Rope) -> Vec<TextEdit> {
    let old_str = old.to_string();
    let new_str = new.to_string();

    // Compute Diff (Character Level)
    // This is more expensive than lines but gives surgical precision.
    let diff = TextDiff::from_chars(&old_str, &new_str);
    let mut edits = Vec::new();

    for op in diff.ops() {
        // Ignore unchanged regions
        if op.tag() == DiffTag::Equal {
            continue;
        }

        // Calculate the Range in the OLD text
        let old_start_char_idx = op.old_range().start;
        let old_end_char_idx = op.old_range().end;

        let start_line = old.char_to_line(old_start_char_idx);
        let start_col = old_start_char_idx - old.line_to_char(start_line);
        let end_line = old.char_to_line(old_end_char_idx);
        let end_col = old_end_char_idx - old.line_to_char(end_line);

        let range = Range {
            start: Position {
                line: start_line,
                character: start_col,
            },
            end: Position {
                line: end_line,
                character: end_col,
            },
        };

        // Extract the New Text
        let new_start_char_idx = op.new_range().start;
        let new_end_char_idx = op.new_range().end;

        // Slice the new string directly
        let new_text_fragment = if new_start_char_idx < new_end_char_idx {
            new_str[new_start_char_idx..new_end_char_idx].to_string()
        } else {
            String::new()
        };

        edits.push(TextEdit {
            range,
            new_text: new_text_fragment,
        });
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_insert_word() {
        let old = Rope::from_str("Hello World");
        let new = Rope::from_str("Hello Beautiful World");

        let edits = calculate_edits(&old, &new);

        // Expected: Insert "Beautiful " at index 6
        // "Hello " is 6 chars.
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.line, 0);
        assert_eq!(edits[0].range.start.character, 6);
        assert_eq!(edits[0].range.end.character, 6); // 0 length = Insert
        assert_eq!(edits[0].new_text, "Beautiful ");
    }

    #[test]
    fn test_diff_replace_char() {
        let old = Rope::from_str("Rust");
        let new = Rope::from_str("Bust");

        let edits = calculate_edits(&old, &new);

        // Expected: Replace 'R' (0..1) with 'B'
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.character, 1);
        assert_eq!(edits[0].new_text, "B");
    }

    #[test]
    fn test_diff_delete_multiline() {
        let old = Rope::from_str("Line 1\nDelete Me\nLine 3");
        let new = Rope::from_str("Line 1\nLine 3");

        let edits = calculate_edits(&old, &new);

        // Expected: Delete "Delete Me\n" (starts Line 1, ends Line 2)
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.line, 1);
        assert_eq!(edits[0].range.end.line, 2);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn test_diff_unicode_emoji() {
        // Emojis can be tricky with char counts vs byte counts
        let old = Rope::from_str("Hello 🌍");
        let new = Rope::from_str("Hello World 🌍");

        let edits = calculate_edits(&old, &new);

        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "World ");
        // "Hello " is 6 chars
        assert_eq!(edits[0].range.start.character, 6);
    }
}
