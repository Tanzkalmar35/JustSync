use crate::lsp::{Position, Range, TextEdit};
use dissimilar::Chunk;
use ropey::Rope;

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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    macro_rules! pos {
        ($l:expr, $c:expr) => {
            Position {
                line: $l,
                character: $c,
            }
        };
    }

    // Helper to apply edits to a string
    fn apply_edits_to_string(original: &str, edits: &[TextEdit]) -> String {
        // apply edits from bottom to top so indices don't shift!
        let mut sorted_edits = edits.to_vec();
        sorted_edits.sort_by_key(|e| std::cmp::Reverse(e.range.start.line));

        let mut rope = Rope::from_str(original);

        // Sort explicitly by index descending
        sorted_edits.sort_by(|a, b| {
            let idx_a = position_to_offset(&rope, &a.range.start);
            let idx_b = position_to_offset(&rope, &b.range.start);
            idx_b.cmp(&idx_a) // Reverse order
        });

        for edit in sorted_edits {
            let start_char = position_to_offset(&rope, &edit.range.start);
            let end_char = position_to_offset(&rope, &edit.range.end);

            rope.remove(start_char..end_char);
            rope.insert(start_char, &edit.new_text);
        }

        rope.to_string()
    }

    fn position_to_offset(rope: &Rope, pos: &Position) -> usize {
        let line_char = rope.line_to_char(pos.line);
        line_char + pos.character
    }

    proptest! {
        // Run 1000 random scenarios
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn test_diff_correctness_invariant(
            old_text in "\\PC*",  // Random unicode string
            new_text in "\\PC*"   // Another random unicode string
        ) {
            let old_rope = Rope::from_str(&old_text);
            let new_rope = Rope::from_str(&new_text);

            // act: Calculate edits
            let edits = calculate_edits(&old_rope, &new_rope);

            // assert: Applying edits to Old must result in New
            let reconstructed = apply_edits_to_string(&old_text, &edits);

            prop_assert_eq!(
                &reconstructed,
                &new_text,
                "\nFailed to reconstruct!\nOld: {:?}\nNew: {:?}\nEdits: {:?}\n",
                old_text, new_text, edits
            );
        }
    }

    #[test]
    fn test_offset_to_position_mapping() {
        // Arrange
        // Line 0: "Hello" (5 chars + 1 newline = 6 chars total)
        // Line 1: "World" (5 chars)
        // Total indices: 0 to 10
        let text = "Hello\nWorld";
        let rope = Rope::from_str(text);

        let cases = vec![
            // (input_offset, expected_output, description)
            (0, pos!(0, 0), "Start of file"),
            (4, pos!(0, 4), "End of first word"),
            (5, pos!(0, 5), "The newline character itself"),
            (6, pos!(1, 0), "Start of second line"),
            (8, pos!(1, 2), "Middle of second word"),
            (10, pos!(1, 4), "Last character of file"),
        ];

        // Act & Assert loop
        for (offset, expected, desc) in cases {
            let actual = offset_to_position(&rope, offset);

            assert_eq!(
                actual, expected,
                "Failed at case: '{}' with offset {}",
                desc, offset
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_out_of_bounds_panics() {
        let rope = Rope::from_str("Small");
        offset_to_position(&rope, 100);
    }

    proptest! {
        #[test]
        fn test_offset_position_roundtrip(text in "\\PC*") {
            // "\\PC*" generates any random unicode string

            let rope = Rope::from_str(&text);
            let total_len = rope.len_chars();

            // test every character index in this random string
            for offset in 0..=total_len {
                // act
                let pos = offset_to_position(&rope, offset);

                // assert

                // Line index must be valid
                prop_assert!(pos.line < rope.len_lines(), "Line index out of bounds");

                // Check the reverse math (Roundtrip)
                let line_start = rope.line_to_char(pos.line);
                let calculated_offset = line_start + pos.character;

                prop_assert_eq!(calculated_offset, offset, "Roundtrip failed!");
            }
        }
    }
}
