//! Implicit-figure promotion (`implicit_figures`).
//!
//! Pandoc promotes a paragraph to a `Figure` only when the image is *alone*
//! in its paragraph. Any lazy continuation line keeps the whole thing a
//! `Para [Image, SoftBreak, ...]`.

use super::helpers::{assert_block_kinds, parse_blocks};
use crate::syntax::SyntaxKind;

#[test]
fn standalone_image_is_promoted_to_figure() {
    assert_block_kinds("![Cap.](img.jpg)\n", &[SyntaxKind::FIGURE]);
}

#[test]
fn indented_standalone_image_is_promoted_to_figure() {
    assert_block_kinds("  ![Cap.](img.jpg)\n", &[SyntaxKind::FIGURE]);
}

#[test]
fn image_with_trailing_line_stays_a_paragraph() {
    assert_block_kinds(
        "![Cap.](img.jpg)\ntrailing prose\n",
        &[SyntaxKind::PARAGRAPH],
    );
}

#[test]
fn image_with_leading_line_stays_a_paragraph() {
    assert_block_kinds("lead in\n![Cap.](img.jpg)\n", &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn image_followed_by_non_interrupting_block_shape_stays_a_paragraph() {
    assert_block_kinds("![Cap.](img.jpg)\n# Heading\n", &[SyntaxKind::PARAGRAPH]);
    assert_block_kinds("![Cap.](img.jpg)\n> quote\n", &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn image_followed_by_interrupting_fence_is_still_a_figure() {
    assert_block_kinds(
        "![Cap.](img.jpg)\n```py\ncode\n```\n",
        &[SyntaxKind::FIGURE, SyntaxKind::CODE_BLOCK],
    );
}

#[test]
fn two_images_on_consecutive_lines_stay_a_paragraph() {
    assert_block_kinds("![a](1.jpg)\n![b](2.jpg)\n", &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn figure_in_blockquote_is_lossless() {
    let input = "> ![a](x.jpg)\n";
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input);
    assert!(
        tree.descendants().any(|n| n.kind() == SyntaxKind::FIGURE),
        "expected a FIGURE inside the blockquote, got:\n{tree:#?}"
    );
}

#[test]
fn figure_with_trailing_line_in_blockquote_stays_a_paragraph() {
    let input = "> ![a](x.jpg)\n> trailing\n";
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input);
    assert!(
        !tree.descendants().any(|n| n.kind() == SyntaxKind::FIGURE),
        "expected no FIGURE, got:\n{tree:#?}"
    );
}

fn assert_figure(input: &str, expected: bool) {
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "not lossless: {input:?}");
    let found = tree.descendants().any(|n| n.kind() == SyntaxKind::FIGURE);
    assert_eq!(found, expected, "for input {input:?}, got:\n{tree:#?}");
}

#[test]
fn standalone_image_in_list_item_is_a_figure() {
    assert_figure("- ![Cap.](a.jpg)\n", true);
}

#[test]
fn standalone_image_in_ordered_list_item_is_a_figure() {
    assert_figure("1. ![Cap.](a.jpg)\n", true);
}

#[test]
fn standalone_image_in_each_list_item_is_a_figure() {
    let input = "- ![One.](a.jpg)\n\n- ![Two.](b.jpg)\n";
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input);
    assert_eq!(
        tree.descendants()
            .filter(|n| n.kind() == SyntaxKind::FIGURE)
            .count(),
        2,
        "{tree:#?}"
    );
}

#[test]
fn image_with_trailing_line_in_list_item_stays_plain() {
    assert_figure("- ![Cap.](a.jpg)\n  trailing prose\n", false);
}

#[test]
fn image_in_later_list_item_chunk_is_a_figure() {
    assert_figure("- text\n\n  ![Cap.](a.jpg)\n", true);
}

#[test]
fn standalone_image_in_definition_body_is_a_figure() {
    assert_figure("Term\n\n:   ![Cap.](a.jpg)\n", true);
}

#[test]
fn standalone_image_in_tight_definition_body_is_a_figure() {
    assert_figure("Term\n:   ![Cap.](a.jpg)\n", true);
}

#[test]
fn image_with_trailing_line_in_definition_body_stays_plain() {
    assert_figure("Term\n:   ![Cap.](a.jpg)\n    trailing prose\n", false);
}

#[test]
fn list_item_figure_is_gated_on_implicit_figures() {
    use crate::options::{Extensions, Flavor};

    let config = crate::options::ParserOptions {
        flavor: Flavor::Gfm,
        dialect: crate::options::Dialect::for_flavor(Flavor::Gfm),
        extensions: Extensions::for_flavor(Flavor::Gfm),
        ..Default::default()
    };
    let tree = super::helpers::parse_blocks_with_config("- ![Cap.](a.jpg)\n", &config);
    assert!(
        !tree.descendants().any(|n| n.kind() == SyntaxKind::FIGURE),
        "{tree:#?}"
    );
}
