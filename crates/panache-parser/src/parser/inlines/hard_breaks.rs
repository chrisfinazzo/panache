//! Shared predicate for whitespace-form hard line breaks.
//!
//! Two independent inline paths need this rule -- the legacy recursive-descent
//! scanner in [`super::core`] and the unified IR in [`super::inline_ir`] -- and
//! they drifted apart before this module existed. Keep the rule here so they
//! cannot disagree again.

/// Where the trailing whitespace run before a line ending starts, and whether
/// it is wide enough to make that line ending a hard break.
///
/// `lower_bound` is the earliest byte the scan may walk back to, which is the
/// start of the caller's current text run -- anything before that belongs to an
/// already-emitted inline element.
///
/// Two columns is the bar. A tab is worth a whole tab stop, so a single one
/// clears it on its own: both `pandoc -f markdown` and `pandoc -f commonmark`
/// report `LineBreak` for `a\t\nb`.
pub(crate) fn trailing_ws_run(bytes: &[u8], lower_bound: usize, nl_pos: usize) -> (usize, bool) {
    let (start, width) = scan_back(bytes, lower_bound, nl_pos);
    (start, width >= 2)
}

/// Where the whitespace run immediately before `pos` starts, with no width bar.
///
/// The backslash form of a hard line break swallows whatever whitespace sits
/// in front of it, however narrow: both `pandoc -f markdown` and
/// `pandoc -f commonmark` read `foo \` + newline as `[Str "foo", LineBreak]`,
/// with no `Space` between. Losslessness means those bytes still have to live
/// somewhere, so the break token takes them.
pub(crate) fn ws_run_start(bytes: &[u8], lower_bound: usize, pos: usize) -> usize {
    scan_back(bytes, lower_bound, pos).0
}

/// Walk back over spaces and tabs from `pos`, returning where the run starts
/// and how many columns wide it is. A tab is worth a whole tab stop.
fn scan_back(bytes: &[u8], lower_bound: usize, pos: usize) -> (usize, usize) {
    let mut start = pos;
    let mut width = 0usize;

    while start > lower_bound {
        match bytes[start - 1] {
            b' ' => width += 1,
            b'\t' => width += 2,
            _ => break,
        }
        start -= 1;
    }

    (start, width)
}

/// Whether a line ending at `nl_end` closes the whole block rather than
/// separating two lines inside it.
///
/// Hard line breaks only separate inline content *within* a block, so a
/// trailing whitespace run on the final line is padding to be discarded, not a
/// break. CommonMark states this outright ("Neither syntax for hard line breaks
/// works at the end of a paragraph or other block element") and pandoc agrees:
/// `four  ` on its own parses to `Para [Str "four"]`.
///
/// Anything left over that is pure whitespace still counts as the end of the
/// block, since a whitespace-only line would have terminated the block anyway.
pub(crate) fn ends_block(text: &str, nl_end: usize, end: usize) -> bool {
    nl_end >= end || text[nl_end..end].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_spaces_up_to_the_two_column_bar() {
        assert_eq!(trailing_ws_run(b"a ", 0, 2), (1, false));
        assert_eq!(trailing_ws_run(b"a  ", 0, 3), (1, true));
        assert_eq!(trailing_ws_run(b"a   ", 0, 4), (1, true));
    }

    #[test]
    fn a_single_tab_clears_the_bar() {
        assert_eq!(trailing_ws_run(b"a\t", 0, 2), (1, true));
        assert_eq!(trailing_ws_run(b"a \t", 0, 3), (1, true));
    }

    #[test]
    fn no_trailing_whitespace_is_no_run() {
        assert_eq!(trailing_ws_run(b"ab", 0, 2), (2, false));
    }

    /// The scan must not reach back past an already-emitted inline element.
    #[test]
    fn stops_at_the_lower_bound() {
        assert_eq!(trailing_ws_run(b"  x", 2, 3), (3, false));
        assert_eq!(trailing_ws_run(b"  ", 2, 2), (2, false));
    }

    #[test]
    fn trailing_remainder_of_whitespace_still_ends_the_block() {
        assert!(ends_block("a  \n", 4, 4));
        assert!(ends_block("a  \n  ", 4, 6));
        assert!(!ends_block("a  \nb", 4, 5));
    }
}
