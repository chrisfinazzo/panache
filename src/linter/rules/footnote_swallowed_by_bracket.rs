//! Flags `^[note]` inline footnotes that pandoc will not read as footnotes
//! because a `[` or `(` follows the closing bracket.
//!
//! Pandoc consumes the bracket run as a link label, so `^` degrades to
//! literal text and the note silently disappears (or, in the `(` case,
//! silently becomes a link). Panache's parser matches that behavior, which
//! means the CST shows a `LINK`/`UNRESOLVED_REFERENCE` preceded by a stray
//! `^` --- the shape this rule looks for.

use rowan::{NodeOrToken, TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxKind, SyntaxNode};

pub struct FootnoteSwallowedByBracketRule;

/// Which continuation swallowed the note. The two differ in how confident we
/// can be about the author's intent, and so in fix safety.
#[derive(Clone, Copy)]
enum Swallower {
    /// `^[note][ref]` --- a reference-style label. The author wrote a footnote
    /// and then a second bracket construct; inserting a space is unambiguous.
    Bracket,
    /// `^[note](dest)` --- an inline destination. This could equally be a
    /// stray `^` in front of an intended link, so the fix is offered as unsafe.
    Paren,
}

impl Rule for FootnoteSwallowedByBracketRule {
    fn name(&self) -> &str {
        "footnote-swallowed-by-bracket"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "footnote-swallowed-by-bracket",
            default_on: true,
            requires: Requirement::InlineFootnotes,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("footnote-swallowed-by-bracket")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::LINK, SyntaxKind::UNRESOLVED_REFERENCE]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for kind in [SyntaxKind::LINK, SyntaxKind::UNRESOLVED_REFERENCE] {
            for node in cx.nodes(kind) {
                let Some(caret_start) = preceding_caret(node) else {
                    continue;
                };
                let Some(swallower) = classify(node) else {
                    continue;
                };

                // Point at the `^[` that failed to open a footnote.
                let opener_end = node.text_range().start() + TextSize::from(1);
                let location = Location::from_range(TextRange::new(caret_start, opener_end), input);

                let mut diagnostic = Diagnostic::warning(
                    location,
                    "footnote-swallowed-by-bracket",
                    "Inline footnote is followed directly by a bracket, so pandoc reads \
                     the label as a link instead and drops the footnote; insert a space \
                     after the closing `]`",
                );

                if let Some(insert_at) = label_close_end(node) {
                    let edits = vec![Edit {
                        range: TextRange::new(insert_at, insert_at),
                        replacement: " ".to_string(),
                    }];
                    let message = "Insert a space after the inline footnote";
                    diagnostic = diagnostic.with_fix(match swallower {
                        Swallower::Bracket => Fix::safe(message, edits),
                        Swallower::Paren => Fix::unsafe_fix(message, edits),
                    });
                }

                diagnostics.push(diagnostic);
            }
        }

        diagnostics
    }
}

/// Offset of a `^` sitting immediately before `node`, if there is one.
///
/// An escaped caret lands in an `ESCAPED_CHAR` node rather than a `TEXT`
/// token, so this naturally skips `\^[note](u)`.
fn preceding_caret(node: &SyntaxNode) -> Option<TextSize> {
    let prev = node.prev_sibling_or_token()?;
    let NodeOrToken::Token(token) = prev else {
        return None;
    };
    if token.kind() != SyntaxKind::TEXT || !token.text().ends_with('^') {
        return None;
    }
    Some(token.text_range().end() - TextSize::from(1))
}

/// Distinguish the reference-style label from an inline destination.
fn classify(node: &SyntaxNode) -> Option<Swallower> {
    let after_label = node
        .children_with_tokens()
        .skip_while(|child| child.kind() != SyntaxKind::LINK_TEXT)
        .nth(2)?;
    match after_label.kind() {
        SyntaxKind::LINK_DEST_START => Some(Swallower::Paren),
        _ if after_label.as_token().is_some_and(|t| t.text() == "[") => Some(Swallower::Bracket),
        _ => None,
    }
}

/// Offset just past the `]` that closes the would-be footnote body --- the
/// point where a space restores the footnote reading.
fn label_close_end(node: &SyntaxNode) -> Option<TextSize> {
    let label = node
        .children_with_tokens()
        .find(|child| child.kind() == SyntaxKind::LINK_TEXT)?;
    let close = node
        .children_with_tokens()
        .skip_while(|child| child.kind() != SyntaxKind::LINK_TEXT)
        .nth(1)?;
    if close.as_token().is_none_or(|t| t.text() != "]") {
        return None;
    }
    debug_assert_eq!(close.text_range().start(), label.text_range().end());
    Some(close.text_range().end())
}
