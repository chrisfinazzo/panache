//! [`ParseOrigin::Fragment`]: parsing a slice lifted out of a larger document.
//!
//! The parser derives `at_document_start` from `self.pos == 0`, which is a lie
//! for a fragment that began life at a non-zero offset. Three constructs read
//! that flag with no `|| has_blank_before` escape, so a fragment parse would
//! manufacture them out of what is ordinary prose in the document it came
//! from. Every other consumer is `||`-ed with `has_blank_before`, which is
//! already `true` at `pos == 0`, so those agree either way -- which is what
//! [`fragment_agrees_with_document_on_everything_else`] pins.

use crate::options::{Dialect, Flavor, ParserOptions};
use crate::parser::core::ParseOrigin;
use crate::parser::{Parser, fingerprint};
use crate::syntax::{SyntaxKind, SyntaxNode};

use super::helpers::find_first;

fn parse_document(input: &str, config: &ParserOptions) -> SyntaxNode {
    Parser::new(input, config).parse()
}

fn parse_fragment(input: &str, config: &ParserOptions) -> SyntaxNode {
    Parser::new_fragment(input, config).parse()
}

fn commonmark() -> ParserOptions {
    ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::CommonMark,
        ..Default::default()
    }
}

#[test]
fn document_parses_a_pandoc_title_block_and_a_fragment_does_not() {
    let input = "% Title\n% Author\n\nBody\n";
    let config = ParserOptions::default();
    assert!(
        find_first(
            &parse_document(input, &config),
            SyntaxKind::PANDOC_TITLE_BLOCK
        )
        .is_some(),
        "a document starting with `%` lines has a pandoc title block"
    );
    assert!(
        find_first(
            &parse_fragment(input, &config),
            SyntaxKind::PANDOC_TITLE_BLOCK
        )
        .is_none(),
        "the same bytes lifted out of a document are ordinary paragraph text"
    );
}

#[test]
fn document_parses_an_mmd_title_block_and_a_fragment_does_not() {
    let input = "Title: My Title\nAuthor: Jane Doe\n\nBody\n";
    let mut config = ParserOptions::default();
    config.extensions.pandoc_title_block = false;
    config.extensions.mmd_title_block = true;

    assert!(
        find_first(&parse_document(input, &config), SyntaxKind::MMD_TITLE_BLOCK).is_some(),
        "a document starting with `Key: value` lines has an MMD title block"
    );
    assert!(
        find_first(&parse_fragment(input, &config), SyntaxKind::MMD_TITLE_BLOCK).is_none(),
        "the same bytes mid-document are a paragraph, not a title block"
    );
}

#[test]
fn commonmark_reads_a_leading_dash_rule_as_yaml_only_in_a_document() {
    let input = "---\ntitle: t\n---\n\nBody\n";
    let config = commonmark();

    assert!(
        find_first(&parse_document(input, &config), SyntaxKind::YAML_METADATA).is_some(),
        "at byte 0 of a CommonMark document this is frontmatter"
    );
    assert!(
        find_first(&parse_fragment(input, &config), SyntaxKind::YAML_METADATA).is_none(),
        "mid-document the same bytes are a thematic break and a paragraph"
    );
}

/// The whole risk of the flag: it must move *only* the three constructs above.
///
/// Every snippet here is one whose first line reaches a consumer that is
/// `||`-ed with `has_blank_before`, which is already `true` at `pos == 0`, so
/// document and fragment must agree byte for byte.
#[test]
fn fragment_agrees_with_document_on_everything_else() {
    let cases = [
        "# Heading\n\nBody\n",
        "Setext\n======\n\nBody\n",
        "- a\n- b\n\npara\n",
        "> quoted\n\npara\n",
        "```rust\ncode\n```\n\npara\n",
        "::: note\nbody\n:::\n\npara\n",
        "    indented code\n\npara\n",
        "| a | b |\n| - | - |\n| 1 | 2 |\n\npara\n",
        "Term\n: Definition\n\npara\n",
        "[ref]: https://example.com\n\nA [ref] link.\n",
        "***\n\npara\n",
        "<div>\nhtml\n</div>\n\npara\n",
        "$$\nx = 1\n$$\n\npara\n",
        "plain prose with *emphasis* and `code`\n",
        "\r\n# CRLF heading\r\n\r\nBody\r\n",
    ];

    for flavor in [Flavor::Pandoc, Flavor::Quarto, Flavor::Gfm] {
        let config = ParserOptions {
            flavor,
            extensions: crate::options::Extensions::for_flavor(flavor),
            ..Default::default()
        };
        for input in cases {
            assert_eq!(
                fingerprint(&parse_fragment(input, &config)),
                fingerprint(&parse_document(input, &config)),
                "fragment and document parses diverged on {flavor:?} for {input:?}"
            );
        }
    }
}

/// A fragment is still lossless -- the flag changes what is recognized, never
/// how many bytes are kept.
#[test]
fn fragment_parses_are_lossless() {
    let config = ParserOptions::default();
    for input in [
        "% Title\n% Author\n\nBody\n",
        "Title: My Title\n\nBody\n",
        "---\ntitle: t\n---\n\nBody\n",
        "# Heading\n\nBody\n",
    ] {
        let tree = parse_fragment(input, &config);
        assert_eq!(tree.text().to_string(), input, "fragment parse lost bytes");
    }
}

#[test]
fn parse_origin_defaults_to_document() {
    assert_eq!(ParseOrigin::default(), ParseOrigin::Document);
}
