//! Citation AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

pub struct Citation(SyntaxNode);

impl AstNode for Citation {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CITATION
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

impl Citation {
    pub fn keys(&self) -> Vec<CitationKey> {
        self.0
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::CITATION_KEY)
            .map(CitationKey)
            .collect()
    }

    pub fn key_texts(&self) -> Vec<String> {
        self.keys().into_iter().map(|key| key.text()).collect()
    }
}

pub struct CitationKey(SyntaxToken);

impl CitationKey {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    pub fn text_range(&self) -> rowan::TextRange {
        self.0.text_range()
    }

    /// Range covering this key *with* its `@`/`-@` marker and, for the braced
    /// form, the surrounding braces --- `@key`, `-@key`, `@{key with spaces}`.
    ///
    /// [`text_range`](Self::text_range) covers the bare key text, which is what
    /// a rename edit must replace. This is the span to underline in a
    /// diagnostic: it points at one key inside a multi-key citation such as
    /// `[@a; @b]` instead of the whole bracketed group.
    pub fn marked_range(&self) -> rowan::TextRange {
        let range = self.0.text_range();
        let mut start = range.start();
        let mut end = range.end();

        let mut prev = self
            .0
            .prev_sibling_or_token()
            .and_then(|it| it.into_token());
        if let Some(token) = &prev
            && token.kind() == SyntaxKind::CITATION_BRACE_OPEN
        {
            start = token.text_range().start();
            prev = token.prev_sibling_or_token().and_then(|it| it.into_token());
        }
        if let Some(token) = prev
            && token.kind() == SyntaxKind::CITATION_MARKER
        {
            start = token.text_range().start();
        }

        if let Some(token) = self
            .0
            .next_sibling_or_token()
            .and_then(|it| it.into_token())
            && token.kind() == SyntaxKind::CITATION_BRACE_CLOSE
        {
            end = token.text_range().end();
        }

        rowan::TextRange::new(start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn marked_spans(input: &str) -> Vec<&str> {
        parse(input, None)
            .descendants()
            .filter_map(Citation::cast)
            .flat_map(|citation| citation.keys())
            .map(|key| {
                let range = key.marked_range();
                &input[usize::from(range.start())..usize::from(range.end())]
            })
            .collect()
    }

    #[test]
    fn marked_range_covers_one_key_of_a_group() {
        assert_eq!(marked_spans("Text [@a; @b].\n"), vec!["@a", "@b"]);
    }

    #[test]
    fn marked_range_covers_marker_variants() {
        assert_eq!(marked_spans("A [-@foo] b @bar c.\n"), vec!["-@foo", "@bar"]);
    }

    #[test]
    fn marked_range_covers_braced_keys() {
        assert_eq!(marked_spans("A [@{weird key}].\n"), vec!["@{weird key}"]);
    }
}
