//! Soft-wrapping a document into visual lines.
//!
//! A visual line is a half-open range of character offsets. Newline
//! characters are never included in a line's range: they are the
//! boundary between two lines, not content on either.

/// 65 characters is a 6.5in line at 10 characters per inch, which is what
/// an 8.5in page with 1in margins gives you.
pub const TEXT_COLS: usize = 65;

/// Left margin that centres the 65-column measure in the 80-column grid.
pub const TEXT_LEFT: usize = (80 - TEXT_COLS) / 2;

/// Top margin: one blank row above the text. The menu bar drops onto row
/// 0, so without this it would land on the first line and bury the
/// cursor. It is a property of the screen, not the document -- nothing
/// is inserted into the buffer to make it.
pub const TEXT_TOP: usize = 1;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VisualLine {
    pub start: usize,
    pub end: usize,
}

impl VisualLine {
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[allow(dead_code)] // paired with `len` to satisfy clippy
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Break `text` into visual lines of at most `width` characters.
///
/// Breaks prefer the last space at or before the width limit, and that
/// space is consumed rather than starting the next line. A word longer
/// than `width` is broken hard at the limit.
pub fn wrap(text: &[char], width: usize) -> Vec<VisualLine> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut start = 0usize;

    loop {
        let para_end = text[start..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| start + i)
            .unwrap_or(text.len());

        let mut cursor = start;
        loop {
            if para_end - cursor <= width {
                lines.push(VisualLine {
                    start: cursor,
                    end: para_end,
                });
                break;
            }
            let limit = cursor + width;
            // Search through `limit` inclusive: if the first `width`
            // characters fit exactly and a space follows, that space is
            // the break point. `limit < para_end` here, so this is safe.
            let break_at = text[cursor..=limit]
                .iter()
                .rposition(|&c| c == ' ')
                .map(|i| cursor + i);

            match break_at {
                Some(sp) => {
                    lines.push(VisualLine {
                        start: cursor,
                        end: sp,
                    });
                    cursor = sp + 1; // consume the space
                }
                None => {
                    lines.push(VisualLine {
                        start: cursor,
                        end: limit,
                    });
                    cursor = limit;
                }
            }
        }

        if para_end >= text.len() {
            break;
        }
        start = para_end + 1; // step over the newline
    }

    lines
}

/// The visual (line, column) of a character offset.
///
/// An offset sitting exactly at a line's end belongs to that line, which
/// is what puts the cursor at the end of a line rather than the start of
/// the next one.
pub fn position_of(lines: &[VisualLine], offset: usize) -> (usize, usize) {
    for (i, line) in lines.iter().enumerate() {
        if offset <= line.end {
            let col = offset.saturating_sub(line.start);
            return (i, col);
        }
    }
    match lines.last() {
        Some(last) => (lines.len() - 1, last.len()),
        None => (0, 0),
    }
}

/// The character offset of a visual (line, column). Both are clamped.
pub fn offset_at(lines: &[VisualLine], line: usize, col: usize) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let line = line.min(lines.len() - 1);
    let l = lines[line];
    l.start + col.min(l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn rendered(text: &str, width: usize) -> Vec<String> {
        let cs = chars(text);
        wrap(&cs, width)
            .iter()
            .map(|l| cs[l.start..l.end].iter().collect())
            .collect()
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        assert_eq!(wrap(&[], 10), vec![VisualLine { start: 0, end: 0 }]);
    }

    #[test]
    fn short_text_is_one_line() {
        assert_eq!(rendered("hello", 10), vec!["hello"]);
    }

    #[test]
    fn hard_newlines_split_lines() {
        assert_eq!(rendered("a\nb\nc", 10), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_trailing_newline_produces_a_final_empty_line() {
        assert_eq!(rendered("a\n", 10), vec!["a", ""]);
    }

    #[test]
    fn blank_lines_are_preserved() {
        assert_eq!(rendered("a\n\nb", 10), vec!["a", "", "b"]);
    }

    #[test]
    fn wraps_at_the_last_space_within_the_width() {
        assert_eq!(rendered("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
    }

    #[test]
    fn the_space_at_a_wrap_point_is_consumed() {
        let cs = chars("aaa bbb ccc");
        let lines = wrap(&cs, 7);
        assert_eq!(
            lines[1].start, 8,
            "line 2 starts after the space at index 7"
        );
    }

    #[test]
    fn a_word_longer_than_the_width_is_broken_hard() {
        assert_eq!(rendered("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn a_long_word_after_a_short_one_wraps_then_breaks() {
        assert_eq!(rendered("ab cdefghij", 4), vec!["ab", "cdef", "ghij"]);
    }

    #[test]
    fn text_exactly_the_width_does_not_wrap() {
        assert_eq!(rendered("abcd", 4), vec!["abcd"]);
    }

    #[test]
    fn position_of_start_and_end_of_a_single_line() {
        let lines = wrap(&chars("hello"), 10);
        assert_eq!(position_of(&lines, 0), (0, 0));
        assert_eq!(
            position_of(&lines, 5),
            (0, 5),
            "cursor may sit past the last char"
        );
    }

    #[test]
    fn position_of_across_a_hard_newline() {
        let lines = wrap(&chars("ab\ncd"), 10);
        assert_eq!(position_of(&lines, 2), (0, 2), "end of line 1");
        assert_eq!(position_of(&lines, 3), (1, 0), "start of line 2");
        assert_eq!(position_of(&lines, 5), (1, 2));
    }

    #[test]
    fn position_of_across_a_soft_wrap() {
        let lines = wrap(&chars("aaa bbb"), 3);
        assert_eq!(position_of(&lines, 3), (0, 3));
        assert_eq!(position_of(&lines, 4), (1, 0));
    }

    #[test]
    fn position_of_clamps_past_the_end() {
        let lines = wrap(&chars("ab"), 10);
        assert_eq!(position_of(&lines, 999), (0, 2));
    }

    #[test]
    fn offset_at_is_the_inverse_of_position_of() {
        let cs = chars("the quick brown fox\njumped over it");
        let lines = wrap(&cs, 9);
        for offset in 0..=cs.len() {
            let (l, c) = position_of(&lines, offset);
            let back = offset_at(&lines, l, c);
            let (l2, c2) = position_of(&lines, back);
            assert_eq!((l, c), (l2, c2), "offset {offset} did not round trip");
        }
    }

    #[test]
    fn offset_at_clamps_column_to_the_line_end() {
        let lines = wrap(&chars("ab\ncdef"), 10);
        assert_eq!(offset_at(&lines, 0, 99), 2, "column clamps to the line end");
        assert_eq!(
            offset_at(&lines, 99, 0),
            3,
            "line clamps to the last line's start"
        );
        assert_eq!(offset_at(&lines, 99, 99), 7, "both clamp: the document end");
    }
}
