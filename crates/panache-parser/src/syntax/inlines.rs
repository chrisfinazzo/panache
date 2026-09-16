//! Inline AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

/// Split an executable span's payload into its runtime marker, spacing, and code.
/// Delimiter length is checked by the caller: extra backticks escape execution.
pub fn inline_execution_parts<'a>(
    content: &'a str,
    extensions: &crate::options::Extensions,
) -> Option<(&'a str, &'a str, &'a str)> {
    let marker = [
        ("r", extensions.rmarkdown_inline_code),
        ("{r}", extensions.quarto_inline_code),
        ("{python}", extensions.quarto_inline_code),
        ("{julia}", extensions.quarto_inline_code),
    ]
    .into_iter()
    .find_map(|(marker, enabled)| {
        (enabled && content.strip_prefix(marker)?.starts_with([' ', '\t'])).then_some(marker)
    })?;
    let rest = &content[marker.len()..];
    let code = rest.trim_start_matches([' ', '\t']);
    if code.trim().is_empty() {
        return None;
    }
    Some((marker, &rest[..rest.len() - code.len()], code))
}

/// A Quarto or R Markdown executable inline expression with host-aligned ranges.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlineExecutable(SyntaxNode);

impl AstNode for InlineExecutable {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INLINE_EXEC_SPAN
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl InlineExecutable {
    pub fn language(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find_map(|token| {
                (token.kind() == SyntaxKind::INLINE_EXEC_LANG)
                    .then(|| token.text().trim_matches(['{', '}']).to_string())
            })
    }

    pub fn code_source_segments(&self) -> Vec<super::CodeSourceSegment> {
        self.0
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_EXEC_CONTENT)
            .map(|token| super::CodeSourceSegment {
                text: token.text().to_string(),
                range: token.text_range(),
            })
            .collect()
    }

    pub fn code_source(&self) -> String {
        self.code_source_segments()
            .iter()
            .map(|segment| segment.text())
            .collect()
    }

    pub fn code_range(&self) -> Option<rowan::TextRange> {
        let segments = self.code_source_segments();
        Some(rowan::TextRange::new(
            segments.first()?.text_range().start(),
            segments.last()?.text_range().end(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlineMath(SyntaxNode);

impl AstNode for InlineMath {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INLINE_MATH
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl InlineMath {
    /// Ordered TeX-content and host equation-label segments between the math
    /// delimiters.
    pub fn content_segments(&self) -> impl Iterator<Item = super::math::MathContentSegment> + '_ {
        super::math::math_content_segments(&self.0)
    }

    pub fn opening_marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::INLINE_MATH_MARKER).then(|| token.text().to_string())
            })
        })
    }

    pub fn closing_marker(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_MATH_MARKER)
            .nth(1)
            .map(|token| token.text().to_string())
    }

    /// The raw math content between the delimiters, reconstructed from the
    /// ordered TeX segments and host labels (excluding container prefixes—see
    /// [`super::math::math_content_text`]).
    pub fn content(&self) -> String {
        super::math::math_content_text(&self.0)
    }

    pub fn content_range(&self) -> Option<rowan::TextRange> {
        let mut markers = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_MATH_MARKER);

        let start = markers.next()?.text_range().end();
        let end = markers.next()?.text_range().start();
        (start <= end).then(|| rowan::TextRange::new(start, end))
    }
}

pub struct CodeSpan(SyntaxNode);

impl AstNode for CodeSpan {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INLINE_CODE
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl CodeSpan {
    pub fn marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::INLINE_CODE_MARKER).then(|| token.text().to_string())
            })
        })
    }

    pub fn content(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_CODE_CONTENT)
            .map(|token| token.text().to_string())
            .collect()
    }

    pub fn content_range(&self) -> Option<rowan::TextRange> {
        let mut markers = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_CODE_MARKER);

        let start = markers.next()?.text_range().end();
        let end = markers.next()?.text_range().start();
        (start <= end).then(|| rowan::TextRange::new(start, end))
    }
}

pub struct InlineHtml(SyntaxNode);

impl AstNode for InlineHtml {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INLINE_HTML
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl InlineHtml {
    pub fn verbatim(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_HTML_CONTENT)
            .map(|token| token.text().to_string())
            .collect()
    }

    pub fn is_comment(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::INLINE_HTML_CONTENT)
            .is_some_and(|token| token.text().starts_with("<!--"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_execution_obeys_flavor_and_extension_gates() {
        use crate::options::{Extensions, Flavor, ParserOptions};
        let input = "`r x` `{r} x` `{python} x` `{julia} x`\n";
        for (flavor, languages) in [
            (Flavor::Quarto, vec!["r", "r", "python", "julia"]),
            (Flavor::RMarkdown, vec!["r"]),
            (Flavor::Pandoc, vec![]),
            (Flavor::CommonMark, vec![]),
        ] {
            let options = ParserOptions {
                flavor,
                dialect: crate::options::Dialect::for_flavor(flavor),
                extensions: Extensions::for_flavor(flavor),
                ..Default::default()
            };
            let tree = crate::parse(input, Some(options));
            let actual: Vec<_> = tree
                .descendants()
                .filter_map(InlineExecutable::cast)
                .map(|inline| inline.language().unwrap())
                .collect();
            assert_eq!(actual, languages);
            assert_eq!(tree.text().to_string(), input);
        }
        for (classic, braced, expected) in [(false, false, 0), (true, false, 1), (false, true, 3)] {
            let mut options = ParserOptions::default();
            options.extensions.rmarkdown_inline_code = classic;
            options.extensions.quarto_inline_code = braced;
            let tree = crate::parse(input, Some(options));
            assert_eq!(
                tree.descendants()
                    .filter_map(InlineExecutable::cast)
                    .count(),
                expected
            );
        }
    }

    #[test]
    fn inline_execution_excludes_literals_and_empty_expressions() {
        use crate::options::{Extensions, Flavor, ParserOptions};
        let input = "``r x`` ``{python} x`` `{{r}} x` `{{julia}} x` `r ` `{r}` `rust x`\n\n```text\n`r x`\n```\n";
        let tree = crate::parse(
            input,
            Some(ParserOptions {
                flavor: Flavor::Quarto,
                extensions: Extensions::for_flavor(Flavor::Quarto),
                ..Default::default()
            }),
        );
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            tree.descendants()
                .filter_map(InlineExecutable::cast)
                .count(),
            0
        );
    }

    #[test]
    fn inline_execution_preserves_host_ranges_in_multiline_containers() {
        use crate::options::{Extensions, Flavor, ParserOptions};
        let input = "> Résultat `{python}\t(x +\r\n>   1)`{.result}.\r\n";
        let tree = crate::parse(
            input,
            Some(ParserOptions {
                flavor: Flavor::Quarto,
                extensions: Extensions::for_flavor(Flavor::Quarto),
                ..Default::default()
            }),
        );
        assert_eq!(tree.text().to_string(), input);
        let inline = tree.descendants().find_map(InlineExecutable::cast).unwrap();
        assert_eq!(inline.language().as_deref(), Some("python"));
        assert_eq!(inline.code_source(), "(x +\r\n  1)");
        for segment in inline.code_source_segments() {
            let range = segment.text_range();
            assert_eq!(
                &input[usize::from(range.start())..usize::from(range.end())],
                segment.text()
            );
        }
    }

    #[test]
    fn inline_html_discriminates_comments_from_tags() {
        let input = "Hi <!-- x --> <br/>\n";
        let tree = crate::parse(input, None);
        let spans: Vec<_> = tree.descendants().filter_map(InlineHtml::cast).collect();
        assert_eq!(spans.len(), 2, "expected 2 InlineHtml nodes");
        assert!(spans[0].is_comment(), "first span should be comment");
        assert_eq!(spans[0].verbatim(), "<!-- x -->");
        assert!(!spans[1].is_comment(), "second span should not be comment");
        assert_eq!(spans[1].verbatim(), "<br/>");
    }

    #[test]
    fn inline_math_extracts_markers_and_content() {
        let input = "Before $x^2 + y^2$ after\n";
        let tree = crate::parse(input, None);
        let math = tree
            .descendants()
            .find_map(InlineMath::cast)
            .expect("inline math");

        assert_eq!(math.opening_marker().as_deref(), Some("$"));
        assert_eq!(math.closing_marker().as_deref(), Some("$"));
        assert_eq!(math.content(), "x^2 + y^2");
        let range = math.content_range().expect("content range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "x^2 + y^2");
    }

    #[test]
    fn code_span_extracts_marker_and_content() {
        let input = "Use `code` here\n";
        let tree = crate::parse(input, None);
        let code = tree
            .descendants()
            .find_map(CodeSpan::cast)
            .expect("code span");

        assert_eq!(code.marker().as_deref(), Some("`"));
        assert_eq!(code.content(), "code");
        let range = code.content_range().expect("content range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "code");
    }
}
