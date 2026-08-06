//! Flags `[^...]` brackets that pandoc will not read as a footnote reference,
//! which almost always means the author reversed the inline-footnote marker and
//! wrote `[^note]` where `^[note]` was meant.
//!
//! Pandoc's reference form takes a bare label: `[^` must be followed by
//! non-whitespace label characters and a closing `]` on the same line. Prose
//! between the brackets breaks all of that, and the bracket run silently
//! degrades into something else --- literal text, or a citation when the prose
//! happens to contain an `@key`. Nothing is dropped from the output, so the
//! missing note is easy to miss.
//!
//! The three CST shapes this looks for are the three landing spots for such a
//! bracket: `FOOTNOTE_REFERENCE` with whitespace in the label (panache is more
//! permissive than pandoc here), `UNRESOLVED_REFERENCE` when the prose spans a
//! line break, and `CITATION` when it carries an `@key`. Resolved `LINK`s are
//! deliberately excluded: `[^text](dest)` renders as a working link, so a stray
//! caret in the link text is the likelier reading.

use rowan::{TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AstNode, FootnoteReference, SyntaxKind, SyntaxNode};

pub struct ReversedFootnoteMarkerRule;

impl Rule for ReversedFootnoteMarkerRule {
    fn name(&self) -> &str {
        "reversed-footnote-marker"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "reversed-footnote-marker",
            default_on: true,
            requires: Requirement::InlineFootnotes,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("reversed-footnote-marker")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[
            SyntaxKind::FOOTNOTE_REFERENCE,
            SyntaxKind::UNRESOLVED_REFERENCE,
            SyntaxKind::CITATION,
        ]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for node in cx.nodes(SyntaxKind::FOOTNOTE_REFERENCE) {
            let Some(footnote_ref) = FootnoteReference::cast(node.clone()) else {
                continue;
            };
            if !footnote_ref.id().chars().any(char::is_whitespace) {
                continue;
            }
            diagnostics.push(diagnostic(
                node,
                input,
                "Footnote label contains whitespace, so pandoc does not read `[^...]` as a \
                 footnote reference; write `^[...]` for an inline footnote",
            ));
        }

        for node in cx.nodes(SyntaxKind::UNRESOLVED_REFERENCE) {
            if !opens_with_caret(node, input) {
                continue;
            }
            diagnostics.push(diagnostic(
                node,
                input,
                "`[^...]` is not a footnote reference here, so pandoc renders these brackets \
                 as literal text; write `^[...]` for an inline footnote",
            ));
        }

        for node in cx.nodes(SyntaxKind::CITATION) {
            if !opens_with_caret(node, input) {
                continue;
            }
            diagnostics.push(diagnostic(
                node,
                input,
                "`[^...]` is not a footnote reference here, so pandoc reads the bracket as a \
                 citation instead; write `^[...]` for an inline footnote",
            ));
        }

        diagnostics
    }
}

/// Whether `node` starts with a literal `[^`.
///
/// Bare `@key` citations and image-shaped references (`![...]`) start with
/// something else, so this doubles as the filter that keeps them out.
fn opens_with_caret(node: &SyntaxNode, input: &str) -> bool {
    let start = usize::from(node.text_range().start());
    input[start..].starts_with("[^")
}

fn diagnostic(node: &SyntaxNode, input: &str, message: &str) -> Diagnostic {
    // Point at the `[^` that failed to open a footnote reference; the bracket
    // itself can run to several lines.
    let start = node.text_range().start();
    let marker = TextRange::new(start, start + TextSize::from(2));

    let diagnostic = Diagnostic::warning(
        Location::from_range(marker, input),
        "reversed-footnote-marker",
        message,
    )
    .with_note(
        DiagnosticNoteKind::Help,
        "the reference form `[^id]` takes a bare label and needs a matching `[^id]:` definition",
    );

    if !swap_yields_a_footnote(node, input) {
        return diagnostic;
    }

    diagnostic.with_fix(Fix::unsafe_fix(
        "Swap `[^` for the inline-footnote opener `^[`",
        vec![Edit {
            range: marker,
            replacement: "^[".to_string(),
        }],
    ))
}

/// Whether swapping the marker actually produces an inline footnote.
///
/// When a `[` or `(` follows the closing `]`, pandoc consumes the bracket run
/// as a link label instead, so `^[note](dest)` is a stray caret plus a link and
/// `^[note][ref]` is literal text --- in both cases the note still does not
/// exist, and the edit has quietly manufactured the defect
/// [`footnote-swallowed-by-bracket`](super::footnote_swallowed_by_bracket)
/// exists to catch. Report those without a fix; the author has to decide where
/// the note ends, which is not something an edit can infer.
fn swap_yields_a_footnote(node: &SyntaxNode, input: &str) -> bool {
    // Every shape this rule matches ends at its closing `]`.
    let end = usize::from(node.text_range().end());
    !matches!(input[end..].chars().next(), Some('[') | Some('('))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};
    use crate::linter::diagnostics::FixSafety;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        lint_with(input, Config::default())
    }

    fn lint_with(input: &str, config: Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        ReversedFootnoteMarkerRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_prose_in_a_footnote_reference() {
        let input = "Coordinates [^also a note about them] follow.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, "reversed-footnote-marker");
        assert_eq!(u32::from(diagnostics[0].location.range.len()), 2);
        assert_eq!(diagnostics[0].location.column, 13);
    }

    #[test]
    fn flags_prose_spanning_a_line_break() {
        let input = "Coordinates [^also a note\nabout them] follow.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, "reversed-footnote-marker");
        assert!(diagnostics[0].message.contains("literal text"));
    }

    #[test]
    fn flags_prose_that_degrades_into_a_citation() {
        // The reported shape from issue #460: prose carrying an `@key` makes
        // pandoc read the whole bracket as a citation.
        let input = "Coordinates [^also a note\nabout them (@doe2026)] follow.\n";
        let mut config = Config::default();
        config.extensions.citations = true;
        let diagnostics = lint_with(input, config);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, "reversed-footnote-marker");
        assert!(diagnostics[0].message.contains("citation"));
    }

    #[test]
    fn fix_swaps_the_marker_and_is_unsafe() {
        let input = "Coordinates [^also a note about them] follow.\n";
        let diagnostics = parse_and_lint(input);
        let fix = diagnostics[0].fix.as_ref().expect("fix");
        assert_eq!(fix.safety, FixSafety::Unsafe);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "^[");

        let edit = &fix.edits[0];
        let mut fixed = input.to_string();
        fixed.replace_range(
            usize::from(edit.range.start())..usize::from(edit.range.end()),
            &edit.replacement,
        );
        assert_eq!(fixed, "Coordinates ^[also a note about them] follow.\n");
        assert!(lint_with(&fixed, Config::default()).is_empty());
    }

    #[test]
    fn no_fix_when_the_swap_would_build_a_link_instead() {
        // `^[note](dest)` is a stray caret plus a link, and `^[note][ref]` is
        // literal text: the swap would leave the note just as absent while
        // manufacturing a `footnote-swallowed-by-bracket` defect.
        for input in [
            "See [^some note](https://example.com) here.\n",
            "See [^some note][ref] here.\n\n[ref]: /r\n",
        ] {
            let diagnostics = parse_and_lint(input);
            assert_eq!(diagnostics.len(), 1, "{input:?} -> {diagnostics:#?}");
            assert!(
                diagnostics[0].fix.is_none(),
                "{input:?} should report without a fix, got {:#?}",
                diagnostics[0].fix
            );
        }
    }

    #[test]
    fn accepts_a_bare_footnote_label() {
        // Undefined or not, `[^id]` is a well-formed reference; that is
        // `undefined-footnote-id`'s business, not this rule's.
        let input = "Coordinates [^note] follow.\n\n[^note]: A note.\n";
        assert!(parse_and_lint(input).is_empty());
        assert!(parse_and_lint("Coordinates [^missing] follow.\n").is_empty());
    }

    #[test]
    fn accepts_an_inline_footnote() {
        let input = "Coordinates^[also a note about them] follow.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn accepts_ordinary_brackets_and_citations() {
        let input = "See [the docs] and [see @doe2026 for more].\n\n[the docs]: /docs\n";
        let mut config = Config::default();
        config.extensions.citations = true;
        assert!(lint_with(input, config).is_empty());
    }

    #[test]
    fn accepts_a_link_whose_text_starts_with_a_caret() {
        // `[^text](dest)` renders as a working link, so a stray caret in the
        // link text is the likelier reading; leave it alone.
        let input = "See [^multi\nline](https://example.com) here.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn gated_off_without_inline_footnotes() {
        let input = "Coordinates [^also a note about them] follow.\n";
        let config = Config {
            flavor: Flavor::Gfm,
            extensions: crate::config::Extensions::for_flavor(Flavor::Gfm),
            ..Default::default()
        };
        assert!(!config.extensions.inline_footnotes);
        let tree = crate::parser::parse(input, Some(config.clone()));
        let diagnostics = crate::linter::lint(&tree, input, &config);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == "reversed-footnote-marker"),
            "GFM footnote labels go through markdown-it, which accepts spaces: {diagnostics:#?}"
        );
    }
}
