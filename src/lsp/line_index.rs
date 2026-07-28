//! Cached line index for fast LSP position conversion.
//!
//! `position_to_offset`/`offset_to_position` used to re-scan the whole document
//! line-by-line on every call. Hot handlers (semantic tokens, folding ranges,
//! diagnostics) convert many offsets per request, so that O(n) scan compounded
//! to O(n·m) per request on large documents.
//!
//! This mirrors rust-analyzer's approach: precompute line-start byte offsets
//! plus a per-line wide-char table once, cache the result as a salsa query
//! keyed on the text input ([`line_index`]), and answer each conversion with a
//! binary search + a short bounded within-line walk. The index is
//! self-contained (holds no text after construction), so conversions are pure
//! arithmetic over the precomputed tables.
//!
//! The conversion semantics match the previous byte-scanning helpers exactly
//! (see the ported tests below): lines are `str::lines()`-style (a trailing
//! `\r` before `\n` is stripped from the visible line), UTF-16 columns follow
//! LSP, and out-of-bounds inputs clamp rather than panic.

use std::collections::HashMap;
use std::sync::Arc;

use lsp_types::Position;

/// A non-ASCII character within a line, recorded so UTF-8 byte columns can be
/// mapped to/from UTF-16 code-unit columns without re-reading the text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Utf16Char {
    /// Byte offset of the char within its line (relative to the line start).
    byte_start: usize,
    /// UTF-8 byte length of the char.
    utf8_len: usize,
    /// UTF-16 code-unit length of the char (1 for the BMP, 2 for astral).
    utf16_len: usize,
}

/// Precomputed line structure of a document, enabling O(log n) byte-offset
/// <-> LSP position conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineIndex {
    /// Byte offset of the start of each line (line 0 starts at 0). One entry
    /// per line, including the empty trailing line when the text ends in `\n`.
    line_starts: Vec<usize>,
    /// Visible byte length of each line, excluding its terminator (`\n` or
    /// `\r\n`). Parallel to `line_starts`.
    line_lengths: Vec<usize>,
    /// Total byte length of the document, used for clamping.
    len: usize,
    /// Wide-char tables for the lines that contain non-ASCII characters, keyed
    /// by line number. ASCII-only lines are absent (column == byte offset).
    /// Each line's `Vec` is ordered by ascending `byte_start`.
    utf16_lines: HashMap<u32, Vec<Utf16Char>>,
    /// Whether an extra addressable line exists one past the last `line_starts`
    /// entry, at byte offset `len`. True exactly when the text is non-empty and
    /// does not end in `\n` (the unterminated final line has a virtual EOF line
    /// after it, matching the old `str::lines()`-based numbering).
    has_eof_line: bool,
}

impl LineIndex {
    /// Build a line index for `text` in a single pass.
    pub(crate) fn new(text: &str) -> LineIndex {
        let bytes = text.as_bytes();
        let len = text.len();

        let mut line_starts = vec![0usize];
        let mut line_lengths: Vec<usize> = Vec::new();
        let mut utf16_lines: HashMap<u32, Vec<Utf16Char>> = HashMap::new();

        let mut line_start = 0usize;
        let mut cur_line: u32 = 0;

        for (i, ch) in text.char_indices() {
            if ch == '\n' {
                // Visible length excludes the `\n`, and a preceding `\r`.
                let mut vis = i - line_start;
                if vis > 0 && bytes[i - 1] == b'\r' {
                    vis -= 1;
                }
                line_lengths.push(vis);
                line_starts.push(i + 1);
                line_start = i + 1;
                cur_line += 1;
            } else if !ch.is_ascii() {
                utf16_lines.entry(cur_line).or_default().push(Utf16Char {
                    byte_start: i - line_start,
                    utf8_len: ch.len_utf8(),
                    utf16_len: ch.len_utf16(),
                });
            }
        }

        // The final segment after the last `\n` (or the whole text when there
        // is no `\n`). A lone trailing `\r` stays visible, matching `lines()`.
        line_lengths.push(len - line_start);

        let has_eof_line = !text.is_empty() && bytes[len - 1] != b'\n';

        LineIndex {
            line_starts,
            line_lengths,
            len,
            utf16_lines,
            has_eof_line,
        }
    }

    /// Total byte length of the indexed document.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Convert a byte offset into an LSP position (line + UTF-16 column).
    /// Offsets past the end clamp to the document end.
    pub(crate) fn offset_to_position(&self, offset: usize) -> Position {
        let offset = offset.min(self.len);
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let byte_col = (offset - self.line_starts[line]).min(self.line_lengths[line]);
        Position {
            line: line as u32,
            character: self.utf16_column(line as u32, byte_col) as u32,
        }
    }

    /// Convert an LSP position into a byte offset, or `None` when the line is
    /// beyond the document. Columns past the end of a line clamp to the line's
    /// visible end.
    pub(crate) fn position_to_offset(&self, position: Position) -> Option<usize> {
        let line = position.line as usize;
        let (line_start, vis) = if line < self.line_starts.len() {
            (self.line_starts[line], self.line_lengths[line])
        } else if self.has_eof_line && line == self.line_starts.len() {
            (self.len, 0)
        } else {
            return None;
        };
        let byte_col = self.utf16_to_byte(line as u32, position.character as usize, vis);
        Some(line_start + byte_col)
    }

    /// UTF-8 byte column within a line -> UTF-16 column. Sums the UTF-16 length
    /// of every char whose start lies before `byte_col` (matching the old
    /// `take_while(byte_idx < line_offset)` accumulation).
    fn utf16_column(&self, line: u32, byte_col: usize) -> usize {
        match self.utf16_lines.get(&line) {
            None => byte_col,
            Some(wides) => {
                let mut ascii = byte_col;
                let mut utf16 = 0usize;
                for c in wides {
                    if c.byte_start >= byte_col {
                        break;
                    }
                    utf16 += c.utf16_len;
                    // Bytes of this char lying within [0, byte_col).
                    let consumed = (c.byte_start + c.utf8_len).min(byte_col) - c.byte_start;
                    ascii -= consumed;
                }
                ascii + utf16
            }
        }
    }

    /// UTF-16 column within a line -> UTF-8 byte column. Returns the byte offset
    /// of the first char boundary whose preceding UTF-16 count reaches
    /// `character`, clamped to the visible line length `vis`.
    fn utf16_to_byte(&self, line: u32, character: usize, vis: usize) -> usize {
        match self.utf16_lines.get(&line) {
            None => character.min(vis),
            Some(wides) => {
                let mut u16_col = 0usize;
                let mut byte = 0usize;
                let mut wi = 0usize;
                while byte < vis {
                    if u16_col >= character {
                        return byte;
                    }
                    if wi < wides.len() && wides[wi].byte_start == byte {
                        u16_col += wides[wi].utf16_len;
                        byte += wides[wi].utf8_len;
                        wi += 1;
                    } else {
                        u16_col += 1;
                        byte += 1;
                    }
                }
                vis
            }
        }
    }
}

/// Salsa-cached line index for `file`, keyed on the text input only (line
/// structure is config-independent, so this is shared across configs). Returns
/// an `Arc` so worker helpers can thread the index around cheaply.
#[salsa::tracked(lru = 512)]
pub(crate) fn line_index(
    db: &dyn crate::salsa::Db,
    file: crate::salsa::FileText,
) -> Arc<LineIndex> {
    Arc::new(LineIndex::new(file.content_or_empty(db)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    // --- offset_to_position, ported from conversions.rs ---

    #[test]
    fn offset_to_position_simple() {
        let idx = LineIndex::new("hello\nworld\n");
        assert_eq!(idx.offset_to_position(0), pos(0, 0));
        assert_eq!(idx.offset_to_position(3), pos(0, 3));
        assert_eq!(idx.offset_to_position(6), pos(1, 0));
        assert_eq!(idx.offset_to_position(9), pos(1, 3));
    }

    #[test]
    fn offset_to_position_utf16() {
        // "café" = 5 UTF-8 bytes, 4 UTF-16 code units.
        let idx = LineIndex::new("café\n");
        assert_eq!(idx.offset_to_position(0).character, 0);
        assert_eq!(idx.offset_to_position(3).character, 3);
        // After é (2 UTF-8 bytes, 1 UTF-16 code unit).
        assert_eq!(idx.offset_to_position(5).character, 4);
    }

    #[test]
    fn offset_to_position_emoji() {
        // "👋" = 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair).
        let idx = LineIndex::new("hi👋\n");
        assert_eq!(idx.offset_to_position(2).character, 2);
        assert_eq!(idx.offset_to_position(6).character, 4);
    }

    #[test]
    fn offset_to_position_crlf() {
        let idx = LineIndex::new("hello\r\nworld\r\n");
        assert_eq!(idx.offset_to_position(0), pos(0, 0));
        assert_eq!(idx.offset_to_position(3), pos(0, 3));
        assert_eq!(idx.offset_to_position(7), pos(1, 0));
        assert_eq!(idx.offset_to_position(10), pos(1, 3));
    }

    #[test]
    fn offset_to_position_inside_multibyte_char() {
        let idx = LineIndex::new("ä\n");
        assert_eq!(idx.offset_to_position(1), pos(0, 1));
    }

    #[test]
    fn offset_to_position_inside_multibyte_char_crlf() {
        let idx = LineIndex::new("åäö\r\nnext\r\n");
        assert_eq!(idx.offset_to_position(1), pos(0, 1));
        assert_eq!(idx.offset_to_position(5), pos(0, 3));
        assert_eq!(idx.offset_to_position(8), pos(1, 0));
    }

    // --- position_to_offset, ported from conversions.rs ---

    #[test]
    fn position_to_offset_simple() {
        let idx = LineIndex::new("hello\nworld\n");
        assert_eq!(idx.position_to_offset(pos(0, 0)), Some(0));
        assert_eq!(idx.position_to_offset(pos(0, 3)), Some(3));
        assert_eq!(idx.position_to_offset(pos(0, 5)), Some(5));
        assert_eq!(idx.position_to_offset(pos(1, 0)), Some(6));
        assert_eq!(idx.position_to_offset(pos(1, 3)), Some(9));
    }

    #[test]
    fn position_to_offset_utf8() {
        let idx = LineIndex::new("café\nworld\n");
        assert_eq!(idx.position_to_offset(pos(0, 0)), Some(0));
        assert_eq!(idx.position_to_offset(pos(0, 1)), Some(1));
        assert_eq!(idx.position_to_offset(pos(0, 2)), Some(2));
        assert_eq!(idx.position_to_offset(pos(0, 3)), Some(3));
        // café = 5 bytes, 4 UTF-16 units.
        assert_eq!(idx.position_to_offset(pos(0, 4)), Some(5));
    }

    #[test]
    fn position_to_offset_emoji() {
        let idx = LineIndex::new("hi👋\n");
        assert_eq!(idx.position_to_offset(pos(0, 2)), Some(2));
        assert_eq!(idx.position_to_offset(pos(0, 4)), Some(6));
    }

    #[test]
    fn position_to_offset_crlf() {
        let idx = LineIndex::new("hello\r\nworld\r\n");
        assert_eq!(idx.position_to_offset(pos(0, 0)), Some(0));
        assert_eq!(idx.position_to_offset(pos(0, 3)), Some(3));
        assert_eq!(idx.position_to_offset(pos(1, 0)), Some(7));
        assert_eq!(idx.position_to_offset(pos(1, 3)), Some(10));
    }

    // --- trailing-line / out-of-bounds parity with the old scanner ---

    #[test]
    fn position_to_offset_trailing_lines() {
        // Text ending in a newline has an empty trailing line at `len`.
        let idx = LineIndex::new("hello\nworld\n");
        assert_eq!(idx.position_to_offset(pos(2, 0)), Some(12));
        assert_eq!(idx.position_to_offset(pos(3, 0)), None);

        // Text without a trailing newline has a virtual EOF line at `len`.
        let idx = LineIndex::new("hello\nworld");
        assert_eq!(idx.position_to_offset(pos(2, 0)), Some(11));
        assert_eq!(idx.position_to_offset(pos(2, 5)), Some(11));
        assert_eq!(idx.position_to_offset(pos(3, 0)), None);
    }

    #[test]
    fn empty_document() {
        let idx = LineIndex::new("");
        assert_eq!(idx.offset_to_position(0), pos(0, 0));
        assert_eq!(idx.position_to_offset(pos(0, 0)), Some(0));
        assert_eq!(idx.position_to_offset(pos(1, 0)), None);
    }

    #[test]
    fn offset_past_end_clamps() {
        let idx = LineIndex::new("hi");
        assert_eq!(idx.offset_to_position(999), pos(0, 2));
    }

    #[test]
    fn position_column_past_line_clamps() {
        let idx = LineIndex::new("hi\nthere\n");
        assert_eq!(idx.position_to_offset(pos(0, 99)), Some(2));
    }
}
