//! Raw TeX AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};
use crate::parser::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TexBlock(SyntaxNode);

impl AstNode for TexBlock {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TEX_BLOCK
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl TexBlock {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LatexCommand(SyntaxNode);

impl AstNode for LatexCommand {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LATEX_COMMAND
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl LatexCommand {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    /// Return the standalone math-environment parts represented by this raw
    /// TeX inline.
    ///
    /// Container prefixes are host syntax, so they are excluded from the
    /// returned source before the environment body is formatted.
    pub fn math_environment(&self) -> Option<LatexMathEnvironment> {
        let mut source = String::new();
        for element in self.0.descendants_with_tokens() {
            let Some(token) = element.into_token() else {
                continue;
            };
            if token.kind() != SyntaxKind::LINE_PREFIX {
                source.push_str(token.text());
            }
        }

        let name = extract_environment_name(&source)?;
        if !is_inline_math_environment(name) {
            return None;
        }

        let opening_len = source.find('}')? + 1;
        let closing = format!("\\end{{{name}}}");
        let closing_start = opening_len + source[opening_len..].find(&closing)?;
        let closing_end = closing_start + closing.len();
        if closing_end != source.len() {
            return None;
        }

        Some(LatexMathEnvironment {
            opening: source[..opening_len].to_string(),
            body: source[opening_len..closing_start].to_string(),
            closing,
        })
    }
}

/// A standalone raw TeX math environment split at its outer markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexMathEnvironment {
    opening: String,
    body: String,
    closing: String,
}

impl LatexMathEnvironment {
    /// The `\\begin{...}` marker.
    pub fn opening(&self) -> &str {
        &self.opening
    }

    /// The source bytes between the environment markers.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The matching `\\end{...}` marker.
    pub fn closing(&self) -> &str {
        &self.closing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn tex_block_wrapper_casts_and_exposes_text() {
        let tree = parse("\\newcommand{\\foo}{bar}\n", None);
        let block = tree
            .descendants()
            .find_map(TexBlock::cast)
            .expect("tex block");
        assert!(block.text().contains("\\newcommand"));
    }

    #[test]
    fn latex_command_wrapper_casts_and_exposes_text() {
        let tree = parse("Inline \\cite{ref} text\n", None);
        let cmd = tree
            .descendants()
            .find_map(LatexCommand::cast)
            .expect("latex command");
        assert_eq!(cmd.text(), "\\cite{ref}");
    }

    #[test]
    fn latex_math_environment_excludes_container_prefixes() {
        let tree = parse(
            "> \\begin{equation}\n> (\\#eq:oracle)\n> h_N = x.\n> \\end{equation}\n",
            None,
        );
        let environment = tree
            .descendants()
            .find_map(LatexCommand::cast)
            .and_then(|command| command.math_environment())
            .expect("raw TeX math environment");

        assert_eq!(environment.opening(), "\\begin{equation}");
        assert_eq!(environment.body(), "\n(\\#eq:oracle)\nh_N = x.\n");
        assert_eq!(environment.closing(), "\\end{equation}");
    }
}
