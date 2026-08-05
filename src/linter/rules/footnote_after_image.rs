//! `footnote-after-image`: flag a footnote hanging off a standalone image,
//! which silently demotes the figure back to an ordinary paragraph.
//!
//! Pandoc's `implicit_figures` promotes an image to a `Figure` only when the
//! image is *alone* in its paragraph. A trailing footnote -- on the next line
//! as lazy continuation, or on the same line -- keeps the paragraph from being
//! image-only, so
//!
//! ```text
//! ![A caption here.](img.jpg){#fig-1}
//! ^[A note about the figure.]
//! ```
//!
//! parses as `Para [Image, SoftBreak, Note]` rather than `Figure`. The caption
//! text survives only as the image's alt attribute, never rendering as a
//! caption, and under Quarto the `#fig-` id stops being a figure so `@fig-1`
//! renders as an unresolved `**?@fig-1**`. Pandoc parses it exactly as panache
//! does, which makes this an authoring trap the formatter cannot fix on its
//! own (#456).
//!
//! `extensions.implicit_figures` is the gate: with it off there is no figure
//! promotion to lose, so nothing here is a trap. For the next-line shape the
//! CST is a second, implicit gate -- a `FIGURE` node only exists when the
//! extension is on.
//!
//! Note that panache's parser diverges from pandoc here: it closes the `FIGURE`
//! at the newline and starts a fresh `PARAGRAPH`, where pandoc keeps one `Para`
//! via lazy continuation. The divergence is what the rule matches on, but it is
//! the parser's to fix; this rule only reports the authoring hazard.
//!
//! There is deliberately no auto-fix. Two resolutions are valid and they mean
//! different things: move the footnote inside the caption (it becomes part of
//! the caption, which is what #456 asks for), or insert a blank line (the
//! figure is restored and the footnote becomes a separate paragraph). Picking
//! one for the author would be guessing, and splicing a multi-line footnote
//! body into a caption is not a mechanical edit.

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

pub const FOOTNOTE_AFTER_IMAGE: &str = "footnote-after-image";

pub struct FootnoteAfterImageRule;

/// What the demotion actually costs for a given image. At least one must hold
/// for the diagnostic to be worth emitting -- `![](img.jpg)` has neither a
/// caption nor a crossref target, so losing its figure wrapper is a non-event.
struct Stakes {
    caption: bool,
    id: bool,
}

impl Stakes {
    fn worth_reporting(&self) -> bool {
        self.caption || self.id
    }
}

impl Rule for FootnoteAfterImageRule {
    fn name(&self) -> &str {
        FOOTNOTE_AFTER_IMAGE
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: FOOTNOTE_AFTER_IMAGE,
            default_on: true,
            requires: Requirement::Footnotes,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning(FOOTNOTE_AFTER_IMAGE)] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::FIGURE, SyntaxKind::PARAGRAPH]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        // Without implicit figures there is no promotion to lose.
        if !cx.config.extensions.implicit_figures {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();

        // Next-line shape: a `FIGURE` whose immediately following sibling is a
        // footnote-only `PARAGRAPH`. No `BLANK_LINE` sits between them, so
        // pandoc reads the footnote as lazy continuation of the image's
        // paragraph and never promotes the figure.
        for figure in cx.nodes(SyntaxKind::FIGURE) {
            let Some(image) = child_of_kind(figure, SyntaxKind::IMAGE_LINK) else {
                continue;
            };
            let Some(next) = figure.next_sibling() else {
                continue;
            };
            if next.kind() != SyntaxKind::PARAGRAPH {
                continue;
            }
            let Some(footnote) = sole_footnote(&next) else {
                continue;
            };
            if let Some(diagnostic) = report(cx, &image, &footnote) {
                diagnostics.push(diagnostic);
            }
        }

        // Same-line shape: a `PARAGRAPH` that is exactly an image followed by a
        // footnote. The parser already refused to make this a `FIGURE`, which
        // matches pandoc -- but the author's intent is the same trap.
        for paragraph in cx.nodes(SyntaxKind::PARAGRAPH) {
            let significant = significant_children(paragraph);
            let [first, second] = significant.as_slice() else {
                continue;
            };
            if first.kind() != SyntaxKind::IMAGE_LINK || !is_footnote(second.kind()) {
                continue;
            }
            let (Some(image), Some(footnote)) = (first.as_node(), second.as_node()) else {
                continue;
            };
            if let Some(diagnostic) = report(cx, image, footnote) {
                diagnostics.push(diagnostic);
            }
        }

        diagnostics.sort_by_key(|d| d.location.range.start());
        diagnostics
    }
}

/// Build the diagnostic for one image/footnote pair, or `None` when the
/// demotion costs the document nothing.
fn report(cx: &LintContext, image: &SyntaxNode, footnote: &SyntaxNode) -> Option<Diagnostic> {
    let stakes = stakes(image);
    if !stakes.worth_reporting() {
        return None;
    }

    let location = Location::from_node(footnote, cx.input);
    let mut diagnostic = Diagnostic::warning(
        location,
        FOOTNOTE_AFTER_IMAGE,
        "footnote attached to a standalone image keeps it from becoming a figure",
    )
    .with_note(
        DiagnosticNoteKind::Note,
        "an image becomes a figure only when it is alone in its paragraph; \
         the trailing footnote demotes it",
    );

    if stakes.caption {
        diagnostic = diagnostic.with_note(
            DiagnosticNoteKind::Note,
            "the caption text will render as the image's alt attribute, not as a caption",
        );
    }

    if stakes.id {
        diagnostic = diagnostic.with_note(
            DiagnosticNoteKind::Note,
            "the id no longer labels a figure, so cross-references to it will not resolve",
        );
    }

    Some(diagnostic.with_note(
        DiagnosticNoteKind::Help,
        "move the footnote inside the caption, as in `![Caption. ^[note]](img.jpg)`, \
         or separate it from the image with a blank line to keep the figure",
    ))
}

/// Whether the image carries a caption or an id, i.e. whether demotion loses
/// anything the author will notice.
fn stakes(image: &SyntaxNode) -> Stakes {
    let caption = child_of_kind(image, SyntaxKind::IMAGE_ALT)
        .is_some_and(|alt| !alt.text().to_string().trim().is_empty());
    let id = image
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::ATTR_ID);

    Stakes { caption, id }
}

/// The single footnote making up `paragraph`, if that is all it contains.
///
/// Requiring the paragraph to hold *only* the footnote keeps the diagnostic
/// actionable: with trailing prose alongside it, removing the footnote would
/// not restore the figure, so the advice would be wrong.
fn sole_footnote(paragraph: &SyntaxNode) -> Option<SyntaxNode> {
    let significant = significant_children(paragraph);
    let [only] = significant.as_slice() else {
        return None;
    };
    if !is_footnote(only.kind()) {
        return None;
    }
    only.as_node().cloned()
}

fn is_footnote(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::INLINE_FOOTNOTE | SyntaxKind::FOOTNOTE_REFERENCE
    )
}

fn significant_children(node: &SyntaxNode) -> Vec<SyntaxElement> {
    node.children_with_tokens()
        .filter(|element| !is_trivia(element))
        .collect()
}

/// Whitespace separating the image from the footnote. The gap is emitted as a
/// blank `TEXT` token rather than `WHITESPACE` in inline position (see
/// `![Cap.](img.jpg) ^[note]`), so a kind check alone is not enough.
fn is_trivia(element: &SyntaxElement) -> bool {
    match element {
        SyntaxElement::Token(token) => {
            matches!(
                token.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE
            ) || (token.kind() == SyntaxKind::TEXT && token.text().trim().is_empty())
        }
        SyntaxElement::Node(node) => matches!(node.kind(), SyntaxKind::BLANK_LINE),
    }
}

fn child_of_kind(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|child| child.kind() == kind)
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
        FootnoteAfterImageRule.check_tree(&tree, input, &config, None)
    }

    fn quarto() -> Config {
        Config {
            flavor: Flavor::Quarto,
            ..Config::default()
        }
    }

    // --- positive ---------------------------------------------------------

    #[test]
    fn issue_456_repro() {
        let input = "![A caption here.](img.jpg){#fig-1}\n^[A note about the figure.]\n";
        let diagnostics = lint_with(input, quarto());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, FOOTNOTE_AFTER_IMAGE);
        assert_eq!(diagnostics[0].location.line, 2);
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn span_covers_only_the_footnote() {
        let input = "![Cap.](img.jpg)\n^[note]\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "^[note]");
    }

    #[test]
    fn flags_same_line_footnote() {
        let input = "![Cap.](img.jpg)^[note]\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 1);
    }

    #[test]
    fn flags_same_line_footnote_after_space() {
        let diagnostics = parse_and_lint("![Cap.](img.jpg) ^[note]\n");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    }

    #[test]
    fn flags_footnote_reference() {
        let input = "![Cap.](img.jpg)\n[^1]\n\n[^1]: The note body.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn flags_multiline_inline_footnote() {
        let input = "![Cap.](img.jpg){#fig-1}\n^[First line.\n    Second line.]\n";
        let diagnostics = lint_with(input, quarto());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn flags_image_with_id_but_no_caption() {
        let diagnostics = lint_with("![](img.jpg){#fig-1}\n^[note]\n", quarto());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert!(
            diagnostics[0]
                .notes
                .iter()
                .any(|note| note.message.contains("cross-references")),
            "{diagnostics:#?}"
        );
    }

    #[test]
    fn flags_each_occurrence() {
        let input = "![One.](a.jpg)\n^[first]\n\n![Two.](b.jpg)\n^[second]\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].location.line, 2);
        assert_eq!(diagnostics[1].location.line, 5);
    }

    // --- negative ---------------------------------------------------------

    #[test]
    fn does_not_flag_footnote_inside_caption() {
        let input = "![A caption ^[note here.] end.](img.jpg){#fig-1}\n";
        assert!(lint_with(input, quarto()).is_empty());
    }

    #[test]
    fn does_not_flag_footnote_after_blank_line() {
        let input = "![Cap.](img.jpg){#fig-1}\n\n^[A separate remark.]\n";
        assert!(lint_with(input, quarto()).is_empty());
    }

    #[test]
    fn does_not_flag_image_without_caption_or_id() {
        assert!(parse_and_lint("![](img.jpg)\n^[note]\n").is_empty());
    }

    #[test]
    fn does_not_flag_when_paragraph_has_other_prose() {
        // Removing the footnote would not restore the figure, so the advice
        // would be wrong.
        let input = "![Cap.](img.jpg)\n^[note] and trailing prose\n";
        assert!(parse_and_lint(input).is_empty(), "{input:?}");
    }

    #[test]
    fn does_not_flag_inline_image_amid_prose() {
        let input = "See ![Cap.](img.jpg) here^[note] for details.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn does_not_flag_footnote_after_plain_paragraph() {
        assert!(parse_and_lint("Just prose.\n^[note]\n").is_empty());
    }

    #[test]
    fn does_not_flag_without_implicit_figures() {
        let mut config = Config::default();
        config.extensions.implicit_figures = false;
        // Both shapes must stay silent: there is no figure promotion to lose.
        assert!(lint_with("![Cap.](img.jpg)\n^[note]\n", config.clone()).is_empty());
        assert!(lint_with("![Cap.](img.jpg)^[note]\n", config).is_empty());
    }
}
