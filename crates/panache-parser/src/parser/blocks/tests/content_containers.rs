//! Continuation-indent gobbling for the *content containers* — footnote
//! definitions and admonitions, whose bodies are indented rather than
//! marker-prefixed.
//!
//! Pandoc re-reads such a body from its content column, so those columns are
//! held out of the text handed to the inline parser and re-injected as
//! `WHITESPACE` at emission. A construct that preserves interior whitespace
//! (a code span, inline math) therefore measures from the content column, not
//! from column 0, while the parse stays byte-lossless.
//!
//! The footnote side has a pandoc oracle and is pinned end-to-end by the
//! `footnote_continuation_*` parser fixtures. Admonitions are python-markdown,
//! so there is no pandoc ground truth for them — hence these unit tests.

use super::helpers::{find_first, parse_blocks, parse_blocks_with_config};
use crate::options::{Extensions, ParserOptions};
use crate::syntax::{SyntaxKind, SyntaxNode};

/// Concatenated `INLINE_CODE_CONTENT` of the first code span, i.e. the payload
/// with any held-out container indent already excluded.
fn first_code_payload(tree: &SyntaxNode) -> String {
    let code = find_first(tree, SyntaxKind::INLINE_CODE).expect("should find inline code");
    code.children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::INLINE_CODE_CONTENT)
        .map(|t| t.text().to_string())
        .collect()
}

fn admonition_config() -> ParserOptions {
    let mut config = ParserOptions::default();
    config.extensions = Extensions {
        python_markdown_admonitions: true,
        ..config.extensions
    };
    config
}

#[test]
fn footnote_continuation_indent_is_stripped_from_inline_code_content() {
    // `noteBlock` strips the body's 4-column indent off every continuation
    // line, so the span reads `x\ny` (pandoc: `Code "x y"`), not `x\n    y`.
    let input = "a[^1]\n\n[^1]: d\n    `x\n    y`\n";
    let tree = parse_blocks(input);
    assert_eq!(first_code_payload(&tree), "x\ny");
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn footnote_continuation_keeps_surplus_indent_as_payload() {
    // Only the container's own columns are gobbled; indent past the content
    // column is content. Pandoc: `Code "x   y"`.
    let input = "a[^1]\n\n[^1]: d\n      `x\n      y`\n";
    let tree = parse_blocks(input);
    assert_eq!(first_code_payload(&tree), "x\n  y");
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn footnote_lazy_continuation_keeps_its_whitespace() {
    // A lazy line never reaches the content column, so pandoc takes nothing
    // off it and its leading whitespace stays payload.
    let input = "a[^1]\n\n[^1]: d\n  `x\n  y`\n";
    let tree = parse_blocks(input);
    assert_eq!(first_code_payload(&tree), "x\n  y");
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn footnote_continuation_indent_is_held_out_as_line_prefix() {
    // The gobbled bytes are re-injected as a `LINE_PREFIX` token rather than
    // dropped -- that is what keeps the CST byte-lossless.
    let input = "a[^1]\n\n[^1]: d\n    text\n";
    let tree = parse_blocks(input);
    let definition =
        find_first(&tree, SyntaxKind::FOOTNOTE_DEFINITION).expect("should find footnote body");
    let paragraph =
        find_first(&definition, SyntaxKind::PARAGRAPH).expect("should find body paragraph");
    let held: Vec<String> = paragraph
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::LINE_PREFIX)
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(held, vec!["    ".to_string()]);
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn admonition_continuation_indent_is_stripped_from_inline_code_content() {
    // Admonitions share the `content_col` machinery, so the same gobble
    // applies to their 4-space-indented bodies.
    let input = "!!! note \"T\"\n    d\n    `x\n    y`\n";
    let tree = parse_blocks_with_config(input, &admonition_config());
    assert!(
        find_first(&tree, SyntaxKind::ADMONITION).is_some(),
        "admonition extension should be active"
    );
    assert_eq!(first_code_payload(&tree), "x\ny");
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn admonition_continuation_keeps_surplus_indent_as_payload() {
    let input = "!!! note \"T\"\n    d\n      `x\n      y`\n";
    let tree = parse_blocks_with_config(input, &admonition_config());
    assert_eq!(first_code_payload(&tree), "x\n  y");
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}
