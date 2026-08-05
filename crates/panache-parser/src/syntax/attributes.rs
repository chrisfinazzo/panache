use crate::parser::utils::attributes::{
    AttributeBlock, decode_html_attr_entities, parse_html_attribute_list,
    try_parse_trailing_attributes,
};
use crate::syntax::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttributeNode(SyntaxNode);

impl AstNode for AttributeNode {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ATTRIBUTE | SyntaxKind::DIV_INFO | SyntaxKind::HTML_ATTRS
        )
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| AttributeNode(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl AttributeNode {
    /// Whether this node carries structured `ATTR_*` children. Pandoc `{...}`
    /// attributes (`emit_attribute_node`), `DIV_INFO`, and `HTML_ATTRS`
    /// (`emit_html_attrs_node`) all do. Only opaque fallbacks (MMD `[#id]`
    /// headers, raw-inline `{=format}`, malformed bodies) keep a single inner
    /// text token and are read via the reparse helpers below.
    fn structured_id_token(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .find(|el| el.kind() == SyntaxKind::ATTR_ID)
            .and_then(|el| el.into_token())
    }

    fn has_structured_children(&self) -> bool {
        self.0.children_with_tokens().any(|el| {
            matches!(
                el.kind(),
                SyntaxKind::ATTR_ID | SyntaxKind::ATTR_CLASS | SyntaxKind::ATTR_KEY_VALUE
            )
        })
    }

    /// Decode HTML character references in a value read straight off a
    /// structured `ATTR_*` token. Only `HTML_ATTRS` carries HTML attribute
    /// syntax; Pandoc `{...}` values are returned verbatim, since pandoc does
    /// not accept an `&`-bearing body as an attribute block at all. The
    /// [`reparse`](Self::reparse) path already decodes via
    /// [`parse_html_attribute_list`], so it must not be routed through here.
    fn decode_structured_value(&self, raw: &str) -> String {
        if self.0.kind() == SyntaxKind::HTML_ATTRS {
            decode_html_attr_entities(raw).into_owned()
        } else {
            raw.to_string()
        }
    }

    /// Reparse the opaque node text into an [`AttributeBlock`] (fallback path).
    fn reparse(&self) -> Option<AttributeBlock> {
        let text = self.0.text().to_string();
        match self.0.kind() {
            SyntaxKind::HTML_ATTRS => parse_html_attribute_list(&text),
            _ => try_parse_trailing_attributes(&text).map(|(attrs, _)| attrs),
        }
    }

    pub fn id(&self) -> Option<String> {
        if self.has_structured_children() {
            return self
                .structured_id_token()
                .map(|t| {
                    self.decode_structured_value(t.text().strip_prefix('#').unwrap_or(t.text()))
                })
                .filter(|id| !id.is_empty());
        }
        self.reparse()
            .and_then(|attrs| attrs.identifier)
            .filter(|id| !id.is_empty())
    }

    pub fn classes(&self) -> Vec<String> {
        if self.has_structured_children() {
            return self
                .0
                .children_with_tokens()
                .filter(|el| el.kind() == SyntaxKind::ATTR_CLASS)
                .filter_map(|el| el.into_token())
                .map(|t| {
                    self.decode_structured_value(t.text().strip_prefix('.').unwrap_or(t.text()))
                })
                .collect();
        }
        self.reparse().map(|a| a.classes).unwrap_or_default()
    }

    pub fn key_values(&self) -> Vec<(String, String)> {
        if self.has_structured_children() {
            return self
                .0
                .children()
                .filter(|n| n.kind() == SyntaxKind::ATTR_KEY_VALUE)
                .map(|kv| {
                    let key = child_token_text(&kv, SyntaxKind::ATTR_KEY).unwrap_or_default();
                    let value = child_token_text(&kv, SyntaxKind::ATTR_VALUE)
                        .map(|v| self.decode_structured_value(&strip_value_quotes(&v)))
                        .unwrap_or_default();
                    (key, value)
                })
                .collect();
        }
        self.reparse().map(|a| a.key_values).unwrap_or_default()
    }

    pub fn id_value_range(&self) -> Option<rowan::TextRange> {
        if self.has_structured_children() {
            // Precise inner-value range: the ATTR_ID token, minus its leading
            // `#` for Pandoc `{...}` attributes. HTML_ATTRS ids are bare (no
            // marker), so the whole token range is the value.
            let tok = self.structured_id_token()?;
            let r = tok.text_range();
            let lead = if tok.text().starts_with('#') {
                rowan::TextSize::from(1)
            } else {
                rowan::TextSize::from(0)
            };
            return Some(rowan::TextRange::new(r.start() + lead, r.end()));
        }

        // Existence guard only -- every length below is measured in SOURCE
        // bytes, never from `id()`, whose HTML value may have been shortened
        // by character-reference decoding.
        let id = self.id()?;
        let text = self.0.text().to_string();
        let node_start: usize = self.0.text_range().start().into();
        match self.0.kind() {
            SyntaxKind::HTML_ATTRS => {
                // Match `id=` followed by an optional quote and the id value.
                // The salsa indexer uses this range for highlights / renames;
                // a precise inner-value range is preferred over the full attr
                // node range.
                let marker = text.find("id")?;
                let after_id = &text[marker + 2..];
                let eq_off = after_id.bytes().position(|b| b == b'=')?;
                let after_eq = &after_id[eq_off + 1..];
                let (val_offset_in_after_eq, val_len) = match after_eq.bytes().next() {
                    Some(q @ (b'"' | b'\'')) => {
                        let inner = &after_eq[1..];
                        let len = inner.bytes().position(|b| b == q).unwrap_or(inner.len());
                        (1, len)
                    }
                    _ => {
                        let len = after_eq
                            .bytes()
                            .position(|b| b.is_ascii_whitespace())
                            .unwrap_or(after_eq.len());
                        (0, len)
                    }
                };
                let value_start_in_text = marker + 2 + eq_off + 1 + val_offset_in_after_eq;
                let start = rowan::TextSize::from((node_start + value_start_in_text) as u32);
                let end =
                    rowan::TextSize::from((node_start + value_start_in_text + val_len) as u32);
                Some(rowan::TextRange::new(start, end))
            }
            _ => {
                let marker = text.find(&format!("#{}", id))?;
                let start = rowan::TextSize::from((node_start + marker + 1) as u32);
                let end = rowan::TextSize::from((node_start + marker + 1 + id.len()) as u32);
                Some(rowan::TextRange::new(start, end))
            }
        }
    }
}

/// Text of the first child token of `node` with the given kind.
fn child_token_text(node: &SyntaxNode, kind: SyntaxKind) -> Option<String> {
    node.children_with_tokens()
        .find(|el| el.kind() == kind)
        .and_then(|el| el.into_token())
        .map(|t| t.text().to_string())
}

/// Strip a matching surrounding pair of `"`/`'` quotes from an attribute value.
fn strip_value_quotes(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 {
        let q = bytes[0];
        if (q == b'"' || q == b'\'') && bytes[bytes.len() - 1] == q {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_node_extracts_div_info_id_and_range() {
        let config = crate::ParserOptions {
            flavor: crate::options::Flavor::RMarkdown,
            ..Default::default()
        };
        // The `{...}` div body is now structured into ATTR_* children, so the
        // wrapper reads id/classes/key-values straight from the CST.
        let tree = crate::parse("::: {#mu .exercise k=v}\ntext\n:::\n", Some(config));
        let node = tree
            .descendants()
            .find_map(AttributeNode::cast)
            .expect("attribute node");
        assert_eq!(node.syntax().kind(), SyntaxKind::DIV_INFO);
        assert!(node.has_structured_children());
        assert_eq!(node.id().as_deref(), Some("mu"));
        assert_eq!(node.classes(), vec!["exercise".to_string()]);
        assert_eq!(node.key_values(), vec![("k".to_string(), "v".to_string())]);

        // The id range points at the inner `mu`, derived from the ATTR_ID token.
        let range = node.id_value_range().expect("id range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&tree.text().to_string()[start..end], "mu");
    }

    #[test]
    fn attribute_node_reads_structured_children() {
        let tree = crate::parse("# H {#x .a .b k=\"v w\"}\n", None);
        let node = tree
            .descendants()
            .find_map(AttributeNode::cast)
            .expect("attribute node");

        assert_eq!(node.id().as_deref(), Some("x"));
        assert_eq!(node.classes(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            node.key_values(),
            vec![("k".to_string(), "v w".to_string())]
        );

        let range = node.id_value_range().expect("id range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&tree.text().to_string()[start..end], "x");
    }

    /// HTML attribute values carry decoded character references, matching
    /// pandoc's `Div`/`Span` `Attr`. The range stays in SOURCE bytes, so it
    /// still covers the encoded `a&amp;b` even though `id()` is 3 bytes.
    #[test]
    fn html_attrs_decode_entities_but_keep_source_range() {
        let config = crate::ParserOptions {
            flavor: crate::options::Flavor::Pandoc,
            ..Default::default()
        };
        let src = "<div id=\"a&amp;b\" class=\"c&lt;d\" k=\"e&gt;f\">\n\ntext\n\n</div>\n";
        let tree = crate::parse(src, Some(config));
        let node = tree
            .descendants()
            .filter_map(AttributeNode::cast)
            .find(|n| n.syntax().kind() == SyntaxKind::HTML_ATTRS)
            .expect("html attrs node");

        assert_eq!(node.id().as_deref(), Some("a&b"));
        assert_eq!(node.classes(), vec!["c<d".to_string()]);
        assert_eq!(
            node.key_values(),
            vec![("k".to_string(), "e>f".to_string())]
        );

        let range = node.id_value_range().expect("id range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&tree.text().to_string()[start..end], "a&amp;b");
    }

    /// Pandoc `{...}` attribute values are NOT HTML, so they are never
    /// entity-decoded.
    #[test]
    fn brace_attrs_do_not_decode_entities() {
        let tree = crate::parse("# H {#x k=\"a&amp;b\"}\n", None);
        let node = tree
            .descendants()
            .find_map(AttributeNode::cast)
            .expect("attribute node");
        assert_eq!(
            node.key_values(),
            vec![("k".to_string(), "a&amp;b".to_string())]
        );
    }
}
