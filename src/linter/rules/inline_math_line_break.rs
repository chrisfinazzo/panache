use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AstNode, MathLineBreak, SyntaxKind};

pub struct InlineMathLineBreakRule;

impl Rule for InlineMathLineBreakRule {
    fn name(&self) -> &str {
        "inline-math-line-break"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "inline-math-line-break",
            default_on: true,
            requires: Requirement::TexMath,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("inline-math-line-break")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::MATH_LINE_BREAK]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        cx.nodes(SyntaxKind::MATH_LINE_BREAK)
            .iter()
            .filter_map(|node| MathLineBreak::cast(node.clone()))
            .filter(is_top_level_inline_break)
            .filter_map(|line_break| {
                let marker = line_break.marker_token()?;
                let mut diagnostic = Diagnostic::warning(
                    Location::from_range(marker.text_range(), cx.input),
                    "inline-math-line-break",
                    r"top-level `\\` is not portable in inline math",
                );

                if let Some(name) = following_control_word_candidate(&line_break) {
                    diagnostic = diagnostic.with_note(
                        DiagnosticNoteKind::Help,
                        format!(
                            r"`\\{name}` is parsed as a line break followed by `{name}`; use `\{name}` if you intended a command"
                        ),
                    );
                }

                Some(diagnostic)
            })
            .collect()
    }
}

fn is_top_level_inline_break(line_break: &MathLineBreak) -> bool {
    line_break
        .syntax()
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::MATH_CONTENT)
        .and_then(|content| content.parent())
        .is_some_and(|host| host.kind() == SyntaxKind::INLINE_MATH)
}

fn following_control_word_candidate(line_break: &MathLineBreak) -> Option<String> {
    let token = line_break.syntax().next_sibling_or_token()?.into_token()?;
    if token.kind() != SyntaxKind::MATH_WORD {
        return None;
    }

    let name: String = token
        .text()
        .chars()
        .take_while(|character| character.is_ascii_alphabetic() || *character == '@')
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let mut config = Config::default();
        config.extensions.tex_math_dollars = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        InlineMathLineBreakRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_only_top_level_inline_breaks() {
        let input = "$a \\\\ b$ $\\begin{matrix}a \\\\ b\\end{matrix}$ $\\substack{a \\\\ b}$\n\n$$a \\\\ b$$\n";
        let diagnostics = parse_and_lint(input);

        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        let range = diagnostics[0].location.range;
        assert_eq!(
            &input[usize::from(range.start())..usize::from(range.end())],
            r"\\"
        );
    }

    #[test]
    fn explains_an_adjacent_control_word_candidate() {
        let diagnostics = parse_and_lint(r"$a \\mathrm{x}$ and $a \\ b$");

        assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].notes.len(), 1);
        assert!(diagnostics[0].notes[0].message.contains(r"use `\mathrm`"));
        assert!(diagnostics[1].notes.is_empty());
    }
}
