use std::collections::HashSet;

use rowan::{TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub struct BlankLineInInlineFootnoteRule;

impl Rule for BlankLineInInlineFootnoteRule {
    fn name(&self) -> &str {
        "blank-line-in-inline-footnote"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "blank-line-in-inline-footnote",
            default_on: true,
            requires: Requirement::InlineFootnotes,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("blank-line-in-inline-footnote")] },
        }
    }

    fn wants_text_tokens(&self) -> bool {
        true
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let paragraphs_with_closer: HashSet<_> = cx
            .text_tokens()
            .iter()
            .filter(|token| token.text().contains(']'))
            .filter_map(containing_paragraph)
            .map(|paragraph| paragraph.text_range())
            .collect();

        for token in cx.text_tokens() {
            let Some(paragraph) = containing_paragraph(token) else {
                continue;
            };
            let Some(next_paragraph) = paragraph_after_blank_line(&paragraph) else {
                continue;
            };
            if !paragraphs_with_closer.contains(&next_paragraph.text_range()) {
                continue;
            }

            for (offset, _) in token.text().match_indices("^[") {
                let start = token.text_range().start()
                    + TextSize::try_from(offset).expect("token offset fits in TextSize");
                let marker = TextRange::at(start, TextSize::from(2));
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(marker, cx.input),
                    "blank-line-in-inline-footnote",
                    "Blank line ends the paragraph before this inline footnote closes, so \
                     pandoc renders the footnote markers as literal text",
                ));
            }
        }

        diagnostics
    }
}

fn containing_paragraph(token: &SyntaxToken) -> Option<SyntaxNode> {
    token
        .parent_ancestors()
        .find(|node| node.kind() == SyntaxKind::PARAGRAPH)
}

/// The paragraph immediately following a blank line after `paragraph`.
fn paragraph_after_blank_line(paragraph: &SyntaxNode) -> Option<SyntaxNode> {
    let blank_line = paragraph.next_sibling()?;
    if blank_line.kind() != SyntaxKind::BLANK_LINE {
        return None;
    }

    blank_line
        .next_sibling()
        .filter(|node| node.kind() == SyntaxKind::PARAGRAPH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        BlankLineInInlineFootnoteRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_inline_footnote_split_by_blank_line() {
        let input = "Text^[first paragraph\n\nsecond paragraph]\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, "blank-line-in-inline-footnote");
        assert_eq!(diagnostics[0].location.column, 5);
        assert_eq!(u32::from(diagnostics[0].location.range.len()), 2);
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn accepts_multiline_inline_footnote_without_blank_line() {
        assert!(parse_and_lint("Text^[first line\nsecond line]\n").is_empty());
    }

    #[test]
    fn accepts_escaped_opener() {
        assert!(parse_and_lint("Text\\^[literal\n\nclosing bracket]\n").is_empty());
    }

    #[test]
    fn accepts_unclosed_literal_without_later_bracket() {
        assert!(parse_and_lint("Use ^[ as notation.\n\nAnother paragraph.\n").is_empty());
    }
}
