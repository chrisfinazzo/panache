use rowan::TextRange;

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub struct BlankLineInDisplayMathRule;

impl Rule for BlankLineInDisplayMathRule {
    fn name(&self) -> &str {
        "blank-line-in-display-math"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "blank-line-in-display-math",
            default_on: true,
            requires: Requirement::TexMathDollars,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("blank-line-in-display-math")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::PARAGRAPH, SyntaxKind::PLAIN]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        if !cx.config.extensions.tex_math_dollars {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();
        for kind in [SyntaxKind::PARAGRAPH, SyntaxKind::PLAIN] {
            for block in cx.nodes(kind) {
                let Some(marker) = standalone_dollar_delimiter(block) else {
                    continue;
                };
                let Some(next_block) = text_block_after_blank_line(block) else {
                    continue;
                };
                if standalone_dollar_delimiter(&next_block).is_none() {
                    continue;
                }

                diagnostics.push(Diagnostic::warning(
                    Location::from_range(marker, cx.input),
                    "blank-line-in-display-math",
                    "Blank line ends the paragraph before this display-math delimiter closes, so \
                     pandoc renders both `$$` markers as literal text",
                ));
            }
        }
        diagnostics
    }
}

/// Return the range of a `$$` line whose surrounding characters are whitespace.
fn standalone_dollar_delimiter(block: &SyntaxNode) -> Option<TextRange> {
    let mut line = Vec::new();
    for token in block
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        if matches!(
            token.kind(),
            SyntaxKind::NEWLINE | SyntaxKind::HARD_LINE_BREAK
        ) {
            if let Some(range) = dollar_delimiter_in_line(&line) {
                return Some(range);
            }
            line.clear();
        } else {
            line.push(token);
        }
    }
    dollar_delimiter_in_line(&line)
}

fn dollar_delimiter_in_line(tokens: &[SyntaxToken]) -> Option<TextRange> {
    let non_whitespace: Vec<_> = tokens
        .iter()
        .filter(|token| !token.text().trim().is_empty())
        .collect();
    match non_whitespace.as_slice() {
        [first, second]
            if first.kind() == SyntaxKind::TEXT
                && first.text() == "$"
                && second.kind() == SyntaxKind::TEXT
                && second.text() == "$"
                && first.text_range().end() == second.text_range().start() =>
        {
            Some(TextRange::new(
                first.text_range().start(),
                second.text_range().end(),
            ))
        }
        _ => None,
    }
}

/// The immediately following paragraph-like block, when separated only by a blank line and
/// container prefixes.
fn text_block_after_blank_line(block: &SyntaxNode) -> Option<SyntaxNode> {
    let mut next = block.next_sibling();
    while next.as_ref().is_some_and(is_container_prefix) {
        next = next.and_then(|node| node.next_sibling());
    }
    let blank_line = next.filter(|node| node.kind() == SyntaxKind::BLANK_LINE)?;

    let mut next = blank_line.next_sibling();
    while next.as_ref().is_some_and(is_container_prefix) {
        next = next.and_then(|node| node.next_sibling());
    }
    next.filter(|node| matches!(node.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN))
}

fn is_container_prefix(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::BLOCK_QUOTE_MARKER | SyntaxKind::WHITESPACE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str, config: Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        BlankLineInDisplayMathRule.check_tree(&tree, input, &config, None)
    }

    fn pandoc_config() -> Config {
        let mut config = Config::default();
        config.extensions.tex_math_dollars = true;
        config
    }

    #[test]
    fn flags_display_math_delimiters_split_by_blank_line() {
        let input = "$$\na\n\n$$\n";
        let diagnostics = parse_and_lint(input, pandoc_config());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code, "blank-line-in-display-math");
        assert_eq!(diagnostics[0].location.column, 1);
        assert_eq!(u32::from(diagnostics[0].location.range.len()), 2);
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn accepts_display_math_without_blank_line() {
        assert!(parse_and_lint("$$\na\n$$\n", pandoc_config()).is_empty());
    }

    #[test]
    fn does_not_confuse_display_math_with_literal_dollars() {
        assert!(parse_and_lint("$$\na\n$$\n\n$$\n", pandoc_config()).is_empty());
    }

    #[test]
    fn requires_dollar_math() {
        let mut config = pandoc_config();
        config.extensions.tex_math_dollars = false;
        config.extensions.tex_math_single_backslash = true;
        assert!(parse_and_lint("$$\na\n\n$$\n", config).is_empty());
    }
}
