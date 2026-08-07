//! `swallowed-list-marker`: flag a line that looks like a list marker but was
//! absorbed into the paragraph above as lazy continuation text.
//!
//! Pandoc-markdown never lets a list interrupt a paragraph, so
//!
//! ```text
//! Reviewing this incident, the conclusion may only be
//! - train the staff
//! - scan the coordinate column
//! ```
//!
//! is a single paragraph, and reflowing it splices the bullets into the prose.
//! Pandoc agrees (one `Para`), so this is a authoring trap rather than a parser
//! bug -- but a silent one, which is what makes it worth a diagnostic (#457).
//!
//! The CST is its own dialect gate. A marker-shaped line only survives as a
//! `TEXT` token inside a `PARAGRAPH`/`PLAIN` when the parser refused to let it
//! interrupt (see `Parser::dispatch_line` in `panache-parser`, which allows
//! interruption only under `Dialect::CommonMark`, and there only for bullets
//! and ordered markers starting at 1). So under CommonMark and GFM a bullet run
//! has already become a real `LIST` and never reaches this rule, while
//! `2. item` still does. That is why the rule needs no flavor gating:
//! `Dialect` is consulted only to word the help note.
//!
//! There is deliberately no auto-fix. Two resolutions are valid -- insert a
//! blank line to get a real list, or escape the marker to keep it as prose --
//! and inserting a single blank line is not even reliably correct: any prose
//! following the run becomes lazy continuation of the final list item.

use rowan::{TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

pub const SWALLOWED_LIST_MARKER: &str = "swallowed-list-marker";

/// Ordered markers wider than this are treated as prose. Two digits keeps
/// `1.` through `99.` -- every list a human writes by hand -- while leaving
/// `2024. was a good year` alone, which is the dominant false positive for a
/// number at the start of a wrapped prose line.
const MAX_ORDERED_DIGITS: usize = 2;

/// Columns of indent a marker may carry before it stops being a list marker.
/// Four or more would be an indented code block, so "insert a blank line"
/// would be the wrong advice.
const MAX_MARKER_INDENT: usize = 3;

pub struct SwallowedListMarkerRule;

/// One line-start inside a single paragraph that has list-marker shape.
struct Candidate {
    /// Index of the line within the enclosing node, used to group runs.
    line_idx: usize,
    /// Tight span covering just the marker (`-`, `1.`), not the space after it.
    span: TextRange,
    marker: String,
}

impl Rule for SwallowedListMarkerRule {
    fn name(&self) -> &str {
        SWALLOWED_LIST_MARKER
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: SWALLOWED_LIST_MARKER,
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning(SWALLOWED_LIST_MARKER)] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        // `PLAIN` covers inline content in tight list items and definition
        // lists, which is not wrapped in a `PARAGRAPH`.
        &[SyntaxKind::PARAGRAPH, SyntaxKind::PLAIN]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let is_commonmark = panache_parser::Dialect::for_flavor(cx.config.flavor)
            == panache_parser::Dialect::CommonMark;
        let mut diagnostics = Vec::new();
        let mut candidates = Vec::new();

        for node in cx
            .nodes(SyntaxKind::PARAGRAPH)
            .iter()
            .chain(cx.nodes(SyntaxKind::PLAIN).iter())
        {
            candidates.clear();
            collect_candidates(node, &mut candidates);

            // Collapse consecutive marker lines into one diagnostic anchored at
            // the first. The remedy is a single blank line above the run;
            // one blank line per item would instead produce a *loose* list,
            // which renders every item wrapped in its own paragraph.
            let mut i = 0;
            while i < candidates.len() {
                let start = i;
                i += 1;
                while i < candidates.len()
                    && candidates[i].line_idx == candidates[i - 1].line_idx + 1
                {
                    i += 1;
                }
                diagnostics.push(build_diagnostic(
                    &candidates[start],
                    i - start,
                    input,
                    is_commonmark,
                ));
            }
        }

        diagnostics
    }
}

fn build_diagnostic(
    first: &Candidate,
    run_len: usize,
    input: &str,
    is_commonmark: bool,
) -> Diagnostic {
    let marker = &first.marker;
    let mut diag = Diagnostic::warning(
        Location::from_range(first.span, input),
        SWALLOWED_LIST_MARKER,
        format!(
            "'{marker}' looks like a list marker, but the preceding line pulls it into a paragraph"
        ),
    );

    if run_len > 1 {
        diag = diag.with_note(
            DiagnosticNoteKind::Note,
            format!(
                "{run_len} consecutive lines here start with a list marker; all of them are \
                 reflowed into the paragraph above"
            ),
        );
    }

    let help = if is_commonmark {
        "CommonMark only lets an ordered list interrupt a paragraph when it starts at 1. \
         Insert a blank line above this line, or renumber the list to start at 1"
            .to_owned()
    } else {
        format!(
            "Pandoc-markdown never lets a list interrupt a paragraph. Insert a blank line \
             above this line to start a real list, or escape the marker ('{}') if it is meant \
             as prose",
            escaped_marker(marker)
        )
    };
    diag.with_note(DiagnosticNoteKind::Help, help)
}

/// How the author would write this marker to keep it as prose: a bullet is
/// escaped whole (`\-`), an ordered marker only needs its punctuation escaped
/// (`1\.`).
fn escaped_marker(marker: &str) -> String {
    match marker.char_indices().next_back() {
        Some((last, _)) => format!("{}\\{}", &marker[..last], &marker[last..]),
        None => marker.to_owned(),
    }
}

/// Walk the node's direct children, recording every line-start that has list
/// marker shape.
///
/// Direct children rather than `descendants_with_tokens` on purpose: a line
/// start nested inside an inline span (`hello *emph\n- item* more`) is not a
/// swallowed list -- inserting a blank line there would tear the span in half.
/// The blockquote case still works, because `BLOCK_QUOTE_MARKER` and its
/// `WHITESPACE` are direct children of the enclosing `PARAGRAPH`.
fn collect_candidates(node: &SyntaxNode, out: &mut Vec<Candidate>) {
    // Every container's continuation indent is its own token, so a marker's
    // indent is already measured from the container's content column and needs
    // no baseline: blockquote prefixes are separate tokens, footnote
    // definitions and admonitions hold their content indent out as
    // `WHITESPACE`, and list items and definition lists strip their own
    // container indent (which also means a nested list there parses for real).
    let mut line_idx = 0usize;
    // Set by a newline and preserved across container prefixes, so that
    // `> text\n> - item` still registers `- item` as a line start.
    let mut at_line_start = false;
    // Whether the node already has content above this line. A marker on the
    // node's own first line is a genuine block start, not a swallowed one.
    let mut seen_content = false;

    for elem in node.children_with_tokens() {
        let Some(token) = elem.as_token() else {
            at_line_start = false;
            seen_content = true;
            continue;
        };

        match token.kind() {
            SyntaxKind::NEWLINE => {
                line_idx += 1;
                at_line_start = true;
            }
            SyntaxKind::BLOCK_QUOTE_MARKER | SyntaxKind::WHITESPACE => {}
            SyntaxKind::TEXT => {
                let text = token.text();
                if at_line_start
                    && seen_content
                    && let Some((mstart, mend)) = marker_shape(text)
                    && has_content_after(&elem, &text[mend..])
                {
                    let base = token.text_range().start();
                    out.push(Candidate {
                        line_idx,
                        span: TextRange::new(
                            base + TextSize::from(mstart as u32),
                            base + TextSize::from(mend as u32),
                        ),
                        marker: text[mstart..mend].to_owned(),
                    });
                }
                at_line_start = false;
                if !text.trim().is_empty() {
                    seen_content = true;
                }
            }
            _ => {
                at_line_start = false;
                seen_content = true;
            }
        }
    }
}

/// Whether the marker is followed by list-item content. `tail` is the rest of
/// the marker's own token; when that is blank the content may still live in a
/// following sibling, as in `- *emph*`, which splits into `TEXT "- "` plus an
/// `EMPHASIS` node.
fn has_content_after(elem: &SyntaxElement, tail: &str) -> bool {
    if !tail.trim().is_empty() {
        return true;
    }
    let mut next = elem.next_sibling_or_token();
    while let Some(sibling) = next {
        match sibling.as_token() {
            Some(token) if token.kind() == SyntaxKind::WHITESPACE => {
                next = sibling.next_sibling_or_token();
            }
            Some(token) => return token.kind() != SyntaxKind::NEWLINE,
            None => return true,
        }
    }
    false
}

/// Byte range of the list marker at the start of `text`, if it has one.
///
/// A minimal recognizer rather than the parser's `try_parse_list_marker`, which
/// is `pub(crate)` in `panache-parser`; the same trade-off
/// `stray-fenced-div-markers` makes, and this one is deliberately narrower than
/// the parser (no alpha, roman, or example-list markers, and no long numbers)
/// because those shapes are common in ordinary prose.
fn marker_shape(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();

    let mut col = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' => col += 1,
            b'\t' => col += 4 - (col % 4),
            _ => break,
        }
        i += 1;
    }
    if col > MAX_MARKER_INDENT {
        return None;
    }

    // Leading-byte gate. Every line that is not marker-shaped leaves here,
    // which keeps the rule off the hot path for ordinary prose.
    match *bytes.get(i)? {
        first @ (b'-' | b'*' | b'+') => {
            if !matches!(bytes.get(i + 1), Some(b' ' | b'\t')) {
                return None;
            }
            // `- - -` is marker-shaped but becomes a thematic break once the
            // blank line this rule asks for is inserted.
            if matches!(first, b'-' | b'*') && is_thematic_break_shape(&text[i..], first) {
                return None;
            }
            Some((i, i + 1))
        }
        b'0'..=b'9' => {
            let mut j = i;
            while bytes.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j - i > MAX_ORDERED_DIGITS {
                return None;
            }
            if !matches!(bytes.get(j), Some(b'.' | b')')) {
                return None;
            }
            if !matches!(bytes.get(j + 1), Some(b' ' | b'\t')) {
                return None;
            }
            Some((i, j + 1))
        }
        _ => None,
    }
}

/// Whether `rest` is only repetitions of `ch` and spaces, with at least three
/// of them -- the thematic-break shape.
fn is_thematic_break_shape(rest: &str, ch: u8) -> bool {
    let mut count = 0usize;
    for b in rest.bytes() {
        if b == ch {
            count += 1;
        } else if !matches!(b, b' ' | b'\t') {
            return false;
        }
    }
    count >= 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        lint_with(input, Config::default())
    }

    fn lint_with(input: &str, config: Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        SwallowedListMarkerRule.check_tree(&tree, input, &config, None)
    }

    fn commonmark() -> Config {
        Config {
            flavor: Flavor::CommonMark,
            ..Config::default()
        }
    }

    // --- positive ---------------------------------------------------------

    #[test]
    fn issue_457_repro() {
        let input = "檢討此類事件，結論可能僅\n- 加強戶政人員訓練\n- 全國定時掃描座標欄\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, SWALLOWED_LIST_MARKER);
        assert_eq!(diagnostics[0].location.line, 2);
        assert!(diagnostics[0].message.contains("'-'"));
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn flags_asterisk_and_plus_bullets() {
        for input in ["text\n* item\n", "text\n+ item\n"] {
            let diagnostics = parse_and_lint(input);
            assert_eq!(diagnostics.len(), 1, "{input:?} -> {diagnostics:#?}");
            assert_eq!(diagnostics[0].location.line, 2);
        }
    }

    #[test]
    fn flags_ordered_decimal_and_paren() {
        for input in ["text\n1. item\n", "text\n3) item\n", "text\n12. item\n"] {
            let diagnostics = parse_and_lint(input);
            assert_eq!(diagnostics.len(), 1, "{input:?} -> {diagnostics:#?}");
        }
    }

    #[test]
    fn flags_inside_blockquote() {
        // The line-start TEXT is preceded by BLOCK_QUOTE_MARKER + WHITESPACE,
        // not directly by the NEWLINE.
        let diagnostics = parse_and_lint("> text\n> - item\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn flags_lazy_blockquote_continuation() {
        let diagnostics = parse_and_lint("> text\n- item\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn flags_inside_footnote_definition() {
        // The four-space continuation indent is held out as its own
        // `WHITESPACE` token, so the marker sits at the TEXT token's start.
        let diagnostics = parse_and_lint("[^1]: text\n    - item\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn flags_footnote_definition_with_tab_indent() {
        let diagnostics = parse_and_lint("[^1]: text\n\t- item\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    }

    #[test]
    fn flags_marker_followed_by_inline_node() {
        // Splits into TEXT "- " plus an EMPHASIS node.
        let diagnostics = parse_and_lint("text\n- *emph*\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn flags_up_to_three_leading_spaces() {
        let diagnostics = parse_and_lint("text\n   - item\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    }

    #[test]
    fn flags_crlf_line_endings() {
        let diagnostics = parse_and_lint("text\r\n- item\r\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn span_covers_only_the_marker() {
        let input = "text\n12. item\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let span = diagnostics[0].location.range;
        assert_eq!(&input[span], "12.");
    }

    // --- negative ---------------------------------------------------------

    #[test]
    fn real_list_after_blank_line_is_clean() {
        assert!(parse_and_lint("text\n\n- a\n- b\n").is_empty());
    }

    #[test]
    fn ignores_marker_on_first_line_of_paragraph() {
        assert!(parse_and_lint("- real item\n").is_empty());
    }

    #[test]
    fn ignores_four_space_indent() {
        assert!(parse_and_lint("text\n    - item\n").is_empty());
    }

    #[test]
    fn ignores_top_level_tab_indent() {
        assert!(parse_and_lint("text\n\t- item\n").is_empty());
    }

    #[test]
    fn ignores_marker_without_following_space() {
        assert!(parse_and_lint("text\n-item\n").is_empty());
    }

    #[test]
    fn ignores_thematic_break_shapes() {
        for input in ["text\n- - -\n", "text\n* * *\n", "text\n- - - -\n"] {
            assert!(parse_and_lint(input).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn ignores_four_digit_number() {
        assert!(parse_and_lint("Prices rose sharply\n2024. was a good year\n").is_empty());
    }

    #[test]
    fn ignores_escaped_marker() {
        assert!(parse_and_lint("text\n\\- not a marker\n").is_empty());
    }

    #[test]
    fn ignores_alpha_roman_and_parenthesized_markers() {
        for input in ["text\na. thing\n", "text\ni. thing\n", "text\n(1) thing\n"] {
            assert!(parse_and_lint(input).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn ignores_hash_heading_shape() {
        // An absorbed ATX heading is a different problem, not a list one.
        assert!(parse_and_lint("text\n# heading shape\n").is_empty());
    }

    #[test]
    fn ignores_marker_inside_emphasis_span() {
        assert!(parse_and_lint("hello *emph\n- item* more\n").is_empty());
    }

    #[test]
    fn ignores_nested_list_in_list_item() {
        assert!(parse_and_lint("* outer\n  - inner\n").is_empty());
    }

    #[test]
    fn ignores_definition_list_body_list() {
        assert!(parse_and_lint("Term\n\n:   def\n    - item\n").is_empty());
    }

    // --- run collapsing ---------------------------------------------------

    #[test]
    fn collapses_consecutive_run_to_one_diagnostic() {
        let diagnostics = parse_and_lint("text\n- a\n- b\n- c\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
        assert!(
            diagnostics[0]
                .notes
                .iter()
                .any(|n| n.message.contains("3 consecutive lines")),
            "{:#?}",
            diagnostics[0].notes
        );
    }

    #[test]
    fn separate_runs_get_separate_diagnostics() {
        let diagnostics = parse_and_lint("text\n- a\nprose\n- b\n");
        assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
        let lines: Vec<usize> = diagnostics.iter().map(|d| d.location.line).collect();
        assert_eq!(lines, vec![2, 4]);
    }

    #[test]
    fn single_line_run_has_no_count_note() {
        let diagnostics = parse_and_lint("text\n- a\n");
        assert_eq!(diagnostics.len(), 1);
        assert!(
            !diagnostics[0]
                .notes
                .iter()
                .any(|n| n.message.contains("consecutive lines"))
        );
    }

    #[test]
    fn mixed_bullet_and_ordered_stay_one_run() {
        // The remedy is still a single blank line, so this is one diagnostic.
        let diagnostics = parse_and_lint("text\n- a\n1. b\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    // --- dialect ----------------------------------------------------------

    #[test]
    fn commonmark_bullets_are_real_lists_and_stay_clean() {
        // Under CommonMark these interrupt the paragraph, so the rule never
        // sees them -- the CST is the gate.
        assert!(lint_with("text\n- a\n", commonmark()).is_empty());
        assert!(lint_with("text\n1. a\n", commonmark()).is_empty());
    }

    #[test]
    fn commonmark_flags_ordered_not_starting_at_one() {
        let diagnostics = lint_with("text\n2. a\n3. b\n", commonmark());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
        assert!(
            diagnostics[0]
                .notes
                .iter()
                .any(|n| n.message.contains("start at 1")),
            "{:#?}",
            diagnostics[0].notes
        );
    }

    #[test]
    fn pandoc_help_note_mentions_blank_line() {
        let diagnostics = parse_and_lint("text\n- a\n");
        assert!(
            diagnostics[0]
                .notes
                .iter()
                .any(|n| n.message.contains("Insert a blank line"))
        );
    }

    #[test]
    fn help_note_escapes_the_actual_marker() {
        for (input, escaped) in [
            ("text\n- a\n", "'\\-'"),
            ("text\n+ a\n", "'\\+'"),
            ("text\n1. a\n", "'1\\.'"),
            ("text\n12) a\n", "'12\\)'"),
        ] {
            let diagnostics = parse_and_lint(input);
            assert_eq!(diagnostics.len(), 1, "{input:?}");
            assert!(
                diagnostics[0]
                    .notes
                    .iter()
                    .any(|n| n.message.contains(escaped)),
                "{input:?} should suggest {escaped}, got {:#?}",
                diagnostics[0].notes
            );
        }
    }
}
