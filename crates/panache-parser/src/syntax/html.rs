//! Typed HTML tags and source diagnostics for retained HTML-like text.

use crate::Dialect;
use crate::parser::inlines::inline_html::inspect_inline_html;

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HtmlBlockTag(SyntaxNode);

impl AstNode for HtmlBlockTag {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::HTML_BLOCK_TAG
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl HtmlBlockTag {
    /// Whether this block tag ends the given literal element's contents.
    pub fn contains_literal_close(&self, name: &str) -> bool {
        name != "plaintext"
            && crate::parser::utils::raw_html::raw_text_end(&self.0.text().to_string(), 0, name)
                .is_some()
    }
}

/// An unterminated quoted HTML attribute, anchored to its opening quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlAttributeDiagnostic {
    pub attribute: String,
    pub range: TextRange,
}

pub(super) fn text_html_attribute_diagnostics(token: &SyntaxToken) -> Vec<HtmlAttributeDiagnostic> {
    if token.kind() != SyntaxKind::TEXT || !token.text().contains('<') {
        return Vec::new();
    }
    let Some((context, opening_tag_only)) = html_text_context(token) else {
        return Vec::new();
    };

    // Complete tags need no surrounding context. Avoid reconstructing a large
    // HTML block once for each tag it contains.
    let mut candidate = 0;
    loop {
        let Some(relative) = token.text()[candidate..].find('<') else {
            return Vec::new();
        };
        candidate += relative;
        match inspect_inline_html(&token.text()[candidate..], Dialect::CommonMark) {
            Ok(Some(_)) if opening_tag_only => return Vec::new(),
            Ok(Some(len)) => candidate += len,
            _ => break,
        }
    }

    // A tag may span several tokens and physical lines. Container prefixes
    // belong to the host document, so omit them while retaining host offsets.
    let mut source = String::new();
    let mut segments = Vec::new();
    let mut token_start = 0;
    for part in context
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|part| part.kind() != SyntaxKind::LINE_PREFIX)
    {
        if part == *token {
            token_start = source.len();
        }
        segments.push((source.len(), part.text_range()));
        source.push_str(part.text());
    }

    let mut diagnostics = Vec::new();
    let mut consumed = 0;
    for (relative, _) in token.text().match_indices('<') {
        let start = token_start + relative;
        if start < consumed {
            continue;
        }
        match inspect_inline_html(&source[start..], Dialect::CommonMark) {
            Ok(Some(_)) if opening_tag_only => break,
            Ok(Some(len)) => consumed = start + len,
            // An unfinished comment, declaration, or processing instruction
            // makes later tag-like bytes ambiguous, so leave its payload alone.
            Ok(None) if source[start..].starts_with("<!") || source[start..].starts_with("<?") => {
                break;
            }
            Ok(None) => {}
            Err(error) => {
                let quote = start + error.quote;
                let host_quote = segments.iter().find_map(|&(offset, range)| {
                    (offset <= quote && quote < offset + usize::from(range.len()))
                        .then(|| range.start() + TextSize::from((quote - offset) as u32))
                });
                if let Some(host_quote) = host_quote {
                    diagnostics.push(HtmlAttributeDiagnostic {
                        attribute: source
                            [start + error.attribute.start..start + error.attribute.end]
                            .to_string(),
                        range: TextRange::at(host_quote, TextSize::from(1)),
                    });
                }
                // Further '<' bytes may belong to this unfinished value.
                break;
            }
        }
    }
    diagnostics
}

fn html_text_context(token: &SyntaxToken) -> Option<(SyntaxNode, bool)> {
    let parent = token.parent()?;
    match parent.kind() {
        SyntaxKind::PARAGRAPH
        | SyntaxKind::PLAIN
        | SyntaxKind::HEADING_CONTENT
        | SyntaxKind::TERM
        | SyntaxKind::LINE_BLOCK_LINE
        | SyntaxKind::TABLE_CELL
        | SyntaxKind::TABLE_CAPTION
        | SyntaxKind::LINK_TEXT
        | SyntaxKind::IMAGE_ALT
        | SyntaxKind::SPAN_CONTENT
        | SyntaxKind::EMPHASIS
        | SyntaxKind::STRONG
        | SyntaxKind::STRIKEOUT
        | SyntaxKind::MARK
        | SyntaxKind::SUPERSCRIPT
        | SyntaxKind::SUBSCRIPT => Some((parent, false)),
        // CommonMark can put the rest of a valid multiline opening tag in
        // HTML_BLOCK_CONTENT, so recognition needs the enclosing block.
        SyntaxKind::HTML_BLOCK_TAG => {
            let first_tag_token = parent
                .descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .find(|part| part.text().contains('<'))?;
            if first_tag_token != *token {
                return None;
            }
            parent.parent().map(|block| (block, true))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::syntax::{InlineNode, SyntaxKind};
    use crate::{Dialect, Extensions, Flavor, ParserOptions, parse};

    fn diagnostics(input: &str, flavor: Flavor) -> Vec<super::HtmlAttributeDiagnostic> {
        let tree = parse(
            input,
            Some(ParserOptions {
                flavor,
                dialect: Dialect::for_flavor(flavor),
                extensions: Extensions::for_flavor(flavor),
                ..Default::default()
            }),
        );
        assert_eq!(tree.text().to_string(), input);
        tree.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .flat_map(|token| match InlineNode::cast(token.into()) {
                InlineNode::Text(text) => text.html_attribute_diagnostics(),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn unclosed_html_attribute_points_at_the_opening_quote() {
        for flavor in [Flavor::Pandoc, Flavor::CommonMark] {
            for (input, attribute) in [
                ("仍會面臨 <a id=\"goal></a>\n", "id"),
                ("<img alt='unfinished>\n", "alt"),
                ("<div data-title=\"unfinished>\n", "data-title"),
                ("**<a title='unfinished>**\n", "title"),
            ] {
                let errors = diagnostics(input, flavor);
                assert_eq!(errors.len(), 1, "{flavor:?}: {input:?}");
                let error = &errors[0];
                assert_eq!(error.attribute, attribute);
                let range = std::ops::Range::<usize>::from(error.range);
                assert!(matches!(&input[range.clone()], "\"" | "'"));
                assert_eq!(range.start, input.find(['\"', '\'']).unwrap());
            }
        }
    }

    #[test]
    fn unclosed_html_attribute_maps_container_prefixes_and_crlf() {
        for input in [
            "> <a\n> title=\"unfinished>\n",
            "> <a\r\n> title=\"unfinished>\r\n",
            "- <a\n  title=\"unfinished>\n",
            "> - <a\n>   title=\"unfinished>\n",
        ] {
            let errors = diagnostics(input, Flavor::Pandoc);
            assert_eq!(errors.len(), 1, "{input:?}");
            assert_eq!(errors[0].attribute, "title");
            assert_eq!(
                usize::from(errors[0].range.start()),
                input.find('"').unwrap()
            );
        }
    }

    #[test]
    fn valid_html_attribute_quotes_can_span_tokens_and_lines() {
        for flavor in [Flavor::Pandoc, Flavor::CommonMark] {
            for input in [
                "<a title=\"a > b\">ok</a>\n",
                "<a title=\"one\ntwo\">ok</a>\n",
                "<div title=\"one\ntwo\">ok</div>\n",
                "> <div title=\"one\n> two\">ok</div>\n",
                "<a _bad=\">**text**\">ok</a>\n",
                "<a _bad=\"<b title='text'>\">ok</a>\n",
                "<custom-element data-title=\"ok\" disabled>\n",
            ] {
                assert!(
                    diagnostics(input, flavor).is_empty(),
                    "{flavor:?}: {input:?}"
                );
            }
        }
    }

    #[test]
    fn html_attribute_checks_skip_literal_contexts_and_escaped_openers() {
        for flavor in [Flavor::Pandoc, Flavor::CommonMark] {
            for input in [
                "`<a title=\"unfinished>`\n",
                "```html\n<a title=\"unfinished>\n```\n",
                "    <a title=\"unfinished>\n",
                "\\<a title=\"unfinished>\n",
                "<!-- <a title=\"unfinished> -->\n",
                "<!-- <a title=\"unfinished>\n",
                "<?xml <a title=\"unfinished>\n",
                "<script>\nconst s = '<a title=\"unfinished>';\n</script>\n",
                "<script>const s = '<a title=\"unfinished>';</script>\n",
                "<pre><a title=\"unfinished></pre>\n",
                "a < b and <https://example.org>\n",
            ] {
                assert!(
                    diagnostics(input, flavor).is_empty(),
                    "{flavor:?}: {input:?}"
                );
            }
        }
    }
}
