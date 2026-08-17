//! Cached line index for fast LSP position conversion.
//!
//! `position_to_offset`/`offset_to_position` used to re-scan the whole document
//! line-by-line on every call. Hot handlers (semantic tokens, folding ranges,
//! diagnostics) convert many offsets per request, so that O(n) scan compounded
//! to O(n·m) per request on large documents.
//!
//! This mirrors rust-analyzer's approach: precompute line-start byte offsets
//! once, cache the result as a salsa query keyed on the text input
//! ([`line_index`]), and answer each conversion with a binary search plus a
//! short bounded within-line walk.
//!
//! The index owns a shared handle to the text it indexes rather than a copy, so
//! it is still `'static` and still cheap to clone. UTF-16 columns are answered
//! by walking the one line concerned, guarded by a per-line "contains any
//! non-ASCII byte" flag so an all-ASCII line -- which is nearly every line --
//! stays pure arithmetic. Precomputing a wide-char table for the whole document
//! instead (a hash entry per non-ASCII character) cost more to build than every
//! conversion it ever answered.
//!
//! The conversion semantics match the previous byte-scanning helpers exactly
//! (see the ported tests below): lines are `str::lines()`-style (a trailing
//! `\r` before `\n` is stripped from the visible line), UTF-16 columns follow
//! LSP, and out-of-bounds inputs clamp rather than panic.

use std::ops::Range;
use std::sync::Arc;

use lsp_types::Position;

/// Precomputed line structure of a document, enabling O(log n) byte-offset
/// <-> LSP position conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineIndex {
    /// The indexed text. An `Arc` so this is the same allocation salsa holds:
    /// indexing a document costs a refcount bump, not a copy.
    text: Arc<str>,
    /// Byte offset of the start of each line (line 0 starts at 0). One entry
    /// per line, including the empty trailing line when the text ends in `\n`.
    line_starts: Vec<usize>,
    /// Per line, whether it contains any non-ASCII byte. Parallel to
    /// `line_starts`. A clear flag means the UTF-16 column equals the byte
    /// column, which is the O(1) path this array exists to preserve: a
    /// semantic-tokens request converts one position per token, and a Markdown
    /// paragraph is routinely one very long line.
    wide_lines: Vec<bool>,
}

impl LineIndex {
    /// Build a line index for `text`.
    pub(crate) fn new(text: &str) -> LineIndex {
        LineIndex::from_arc(Arc::from(text))
    }

    /// Build a line index that shares `text` rather than copying it.
    pub(crate) fn from_arc(text: Arc<str>) -> LineIndex {
        let mut line_starts = Vec::with_capacity(text.len() / 40 + 1);
        line_starts.push(0usize);
        line_starts.extend(memchr::memchr_iter(b'\n', text.as_bytes()).map(|at| at + 1));
        let mut index = LineIndex {
            text,
            wide_lines: vec![false; line_starts.len()],
            line_starts,
        };
        for line in 0..index.line_starts.len() {
            index.wide_lines[line] = index.recompute_wide(line);
        }
        index
    }

    /// The indexed text as a shared handle: an O(1) clone, for handing the
    /// document to salsa or to a worker.
    pub(crate) fn text_arc(&self) -> Arc<str> {
        Arc::clone(&self.text)
    }

    /// Whether this index was built for exactly `text` --- the same allocation,
    /// not merely equal bytes.
    ///
    /// An `Arc<str>` is immutable and an index only ever *replaces* its handle
    /// (see [`replace_range`](Self::replace_range)), never mutates through it,
    /// so a shared allocation proves the tables describe those bytes without
    /// reading one. The holder keeps a strong reference, so the allocation
    /// cannot be freed and a different string land at the same address.
    pub(crate) fn indexes(&self, text: &Arc<str>) -> bool {
        Arc::ptr_eq(&self.text, text)
    }

    /// Total byte length of the indexed document.
    pub(crate) fn len(&self) -> usize {
        self.text.len()
    }

    /// The byte range of `line` including its terminator.
    fn line_span(&self, line: usize) -> Range<usize> {
        let start = self.line_starts[line];
        let end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        start..end
    }

    /// Whether `line` contains a non-ASCII byte, read from the text. The
    /// terminator bytes are ASCII, so scanning the whole span is equivalent to
    /// scanning the visible part.
    fn recompute_wide(&self, line: usize) -> bool {
        !self.text.as_bytes()[self.line_span(line)].is_ascii()
    }

    /// Visible byte length of `line`, excluding its terminator (`\n` or
    /// `\r\n`). The final segment after the last `\n` (or the whole text when
    /// there is none) has no terminator to strip, so a lone trailing `\r` stays
    /// visible there --- matching `str::lines()`.
    fn line_len(&self, line: usize) -> usize {
        let Range { start, end } = self.line_span(line);
        if line + 1 == self.line_starts.len() {
            // Every entry after the first is one past a `\n`, so only the last
            // line can lack a terminator -- and it always does.
            return end - start;
        }
        // `end` is one past the `\n` that terminates this line.
        let mut vis = end - 1 - start;
        if vis > 0 && self.text.as_bytes()[end - 2] == b'\r' {
            vis -= 1;
        }
        vis
    }

    /// Whether an extra addressable line exists one past the last `line_starts`
    /// entry, at byte offset `len()`. True exactly when the text is non-empty
    /// and does not end in `\n` (the unterminated final line has a virtual EOF
    /// line after it, matching the old `str::lines()`-based numbering).
    fn has_eof_line(&self) -> bool {
        !self.text.is_empty() && !self.text.as_bytes().ends_with(b"\n")
    }

    /// Convert a byte offset into an LSP position (line + UTF-16 column).
    /// Offsets past the end clamp to the document end.
    pub(crate) fn offset_to_position(&self, offset: usize) -> Position {
        let offset = offset.min(self.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let byte_col = (offset - self.line_starts[line]).min(self.line_len(line));
        Position {
            line: line as u32,
            character: self.utf16_column(line, byte_col) as u32,
        }
    }

    /// Convert an LSP position into a byte offset, or `None` when the line is
    /// beyond the document. Columns past the end of a line clamp to the line's
    /// visible end.
    pub(crate) fn position_to_offset(&self, position: Position) -> Option<usize> {
        let line = position.line as usize;
        let (line_start, vis) = if line < self.line_starts.len() {
            (self.line_starts[line], self.line_len(line))
        } else if self.has_eof_line() && line == self.line_starts.len() {
            (self.len(), 0)
        } else {
            return None;
        };
        let byte_col = self.utf16_to_byte(line, position.character as usize, vis);
        Some(line_start + byte_col)
    }

    /// UTF-8 byte column within a line -> UTF-16 column. Sums the UTF-16 length
    /// of every char whose start lies before `byte_col`.
    fn utf16_column(&self, line: usize, byte_col: usize) -> usize {
        if !self.wide_lines[line] {
            return byte_col;
        }
        let start = self.line_starts[line];
        let mut utf16 = 0usize;
        let mut byte = 0usize;
        for ch in self.text[start..].chars() {
            if byte >= byte_col {
                break;
            }
            utf16 += ch.len_utf16();
            byte += ch.len_utf8();
        }
        utf16
    }

    /// UTF-16 column within a line -> UTF-8 byte column. Returns the byte offset
    /// of the first char boundary whose preceding UTF-16 count reaches
    /// `character`, clamped to the visible line length `vis`.
    fn utf16_to_byte(&self, line: usize, character: usize, vis: usize) -> usize {
        // `line` can be the virtual EOF line, one past the table, which is
        // empty and therefore never wide.
        if !self.wide_lines.get(line).copied().unwrap_or(false) {
            return character.min(vis);
        }
        let start = self.line_starts[line];
        let mut chars = self.text[start..start + vis].chars();
        let mut u16_col = 0usize;
        let mut byte = 0usize;
        while byte < vis {
            if u16_col >= character {
                return byte;
            }
            // `byte < vis` guarantees another char inside the visible line.
            let ch = chars.next().expect("visible line has a char at `byte`");
            u16_col += ch.len_utf16();
            byte += ch.len_utf8();
        }
        vis
    }

    /// Replace the bytes in `range` with `insert`, patching the tables rather
    /// than rescanning the document.
    ///
    /// Line starts fall into three groups. Those at or before `range.start` are
    /// untouched (a newline ending such a line sits before the replaced bytes);
    /// those inside the replaced span are gone; those past `range.end` shift by
    /// the edit's byte delta -- one add per line, not a scan per byte. The
    /// wide-line flags splice alongside them, but the lines the edit *creates*
    /// have to be re-derived from the new text: the joined line's contents come
    /// from the surviving prefix, the insert, and the surviving suffix, so
    /// `insert` alone cannot answer for it.
    ///
    /// The text itself is rebuilt around the splice. That is the one linear
    /// pass an edit pays, and what buys the O(1) sharing everywhere else.
    ///
    /// Panics on a range that is out of bounds or not on a char boundary, as
    /// [`String::replace_range`] does.
    pub(crate) fn replace_range(&mut self, range: Range<usize>, insert: &str) {
        let Range { start, end } = range;
        assert!(start <= end, "reversed edit range {start}..{end}");

        let first = self.line_starts.partition_point(|&at| at <= start);
        let last = self.line_starts.partition_point(|&at| at <= end);
        let delta = insert.len() as isize - (end - start) as isize;
        if delta != 0 {
            for at in &mut self.line_starts[last..] {
                *at = at.wrapping_add_signed(delta);
            }
        }
        let inserted: Vec<usize> = memchr::memchr_iter(b'\n', insert.as_bytes())
            .map(|at| start + at + 1)
            .collect();
        let inserted_count = inserted.len();
        self.line_starts.splice(first..last, inserted);

        let old = &self.text;
        let mut text = String::with_capacity(old.len() - (end - start) + insert.len());
        text.push_str(&old[..start]);
        text.push_str(insert);
        text.push_str(&old[end..]);
        self.text = Arc::from(text);

        self.wide_lines
            .splice(first..last, std::iter::repeat_n(false, inserted_count));
        // `first` is at least 1 (line 0 starts at 0, which is `<= start`), so
        // the edit's own line is `first - 1`, and the lines it created run from
        // `first` through `first + inserted_count - 1`.
        for line in (first - 1)..(first + inserted_count) {
            self.wide_lines[line] = self.recompute_wide(line);
        }
        self.debug_assert_in_step();
    }

    /// The invariant the patch upholds: the tables are always exactly what a
    /// rescan would produce. Debug-only --- it is linear in the document, which
    /// is the cost [`replace_range`](Self::replace_range) exists to avoid. Every
    /// LSP test in the suite therefore doubles as a patch oracle.
    fn debug_assert_in_step(&self) {
        debug_assert!(
            *self == LineIndex::from_arc(Arc::clone(&self.text)),
            "line index drifted from the text it indexes"
        );
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
    let text = file.text(db).clone().unwrap_or_else(|| Arc::from(""));
    Arc::new(LineIndex::from_arc(text))
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

    // --- patching ---

    /// Every replacement of every char-boundary range of a set of awkward texts
    /// must leave the index exactly as a rescan would. This is the whole
    /// correctness argument for patching instead of rebuilding, so it is
    /// checked exhaustively rather than by example: a few thousand cases, and
    /// the equality is over the whole struct, tables included.
    #[test]
    fn patching_matches_a_rescan() {
        let texts = [
            "",
            "\n",
            "\n\n",
            "abc",
            "ab\ncd\nef\n",
            "a\r\nb\r\n",
            "\u{1F600}\nx\n",
            "café\r\nx",
            "a\rb\n",
            "ä",
        ];
        let inserts = [
            "",
            "z",
            "\n",
            "\n\n",
            "x\ny\n",
            "\r\n",
            "\u{1F600}",
            "é",
            "\r",
        ];

        for text in texts {
            for start in 0..=text.len() {
                if !text.is_char_boundary(start) {
                    continue;
                }
                for end in start..=text.len() {
                    if !text.is_char_boundary(end) {
                        continue;
                    }
                    for insert in inserts {
                        let mut patched = LineIndex::new(text);
                        patched.replace_range(start..end, insert);

                        let mut edited = text.to_string();
                        edited.replace_range(start..end, insert);

                        assert_eq!(
                            patched,
                            LineIndex::new(&edited),
                            "patching {text:?}[{start}..{end}] with {insert:?} \
                             diverged from a rescan"
                        );
                    }
                }
            }
        }
    }

    /// The write phase reuses an index only when it still names the allocation
    /// salsa holds, so `indexes` must be strictly about identity: equal bytes in
    /// a second allocation are exactly the case where reuse would be a guess.
    #[test]
    fn an_index_only_claims_the_allocation_it_was_built_for() {
        let text: Arc<str> = Arc::from("ab\ncd\n");
        let mut index = LineIndex::from_arc(Arc::clone(&text));
        assert!(index.indexes(&text));

        let twin: Arc<str> = Arc::from("ab\ncd\n");
        assert_eq!(&*twin, &*text);
        assert!(!index.indexes(&twin));

        index.replace_range(2..2, "x");
        assert!(!index.indexes(&text));
        assert!(index.indexes(&index.text_arc()));
    }

    /// An edit replaces the text allocation rather than mutating it, so a handle
    /// taken before the edit still reads the text it was taken for --- which is
    /// what lets salsa, a reparse base, and an in-flight read job hold the
    /// document without copying it.
    #[test]
    fn an_edit_leaves_earlier_text_handles_alone() {
        let mut index = LineIndex::new("ab\ncd");
        let before = index.text_arc();
        assert!(Arc::ptr_eq(&before, &index.text_arc()));

        index.replace_range(2..2, "\nxy");

        assert!(
            !Arc::ptr_eq(&before, &index.text_arc()),
            "an edit must not mutate a shared allocation"
        );
        assert_eq!(&*before, "ab\ncd");
        assert_eq!(&*index.text_arc(), "ab\nxy\ncd");
    }
}
