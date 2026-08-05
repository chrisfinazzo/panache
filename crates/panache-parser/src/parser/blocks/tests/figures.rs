//! Implicit-figure promotion (`implicit_figures`).
//!
//! Pandoc promotes a paragraph to a `Figure` only when the image is *alone*
//! in its paragraph. Any lazy continuation line keeps the whole thing a
//! `Para [Image, SoftBreak, ...]`.

use super::helpers::{assert_block_kinds, parse_blocks};
use crate::syntax::SyntaxKind;

#[test]
fn standalone_image_is_promoted_to_figure() {
    // pandoc -f markdown: Figure ... [Plain [Image ...]]
    assert_block_kinds("![Cap.](img.jpg)\n", &[SyntaxKind::FIGURE]);
}

#[test]
fn indented_standalone_image_is_promoted_to_figure() {
    assert_block_kinds("  ![Cap.](img.jpg)\n", &[SyntaxKind::FIGURE]);
}

#[test]
fn image_with_trailing_line_stays_a_paragraph() {
    // pandoc -f markdown:
    //   Para [Image .., SoftBreak, Str "trailing", Space, Str "prose"]
    assert_block_kinds(
        "![Cap.](img.jpg)\ntrailing prose\n",
        &[SyntaxKind::PARAGRAPH],
    );
}

#[test]
fn image_with_leading_line_stays_a_paragraph() {
    // pandoc -f markdown: Para [Str "lead", .., SoftBreak, Image ..]
    assert_block_kinds("lead in\n![Cap.](img.jpg)\n", &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn image_followed_by_non_interrupting_block_shape_stays_a_paragraph() {
    // ATX headings, blockquotes, and list markers do not interrupt a
    // paragraph in pandoc-markdown, so the image is not alone in its Para.
    assert_block_kinds("![Cap.](img.jpg)\n# Heading\n", &[SyntaxKind::PARAGRAPH]);
    assert_block_kinds("![Cap.](img.jpg)\n> quote\n", &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn image_followed_by_interrupting_fence_is_still_a_figure() {
    // A fenced code block *does* interrupt, so the image is alone in its
    // paragraph and pandoc emits `Figure`, then `CodeBlock`. (Bare ``` fences
    // don't interrupt here yet --- a separate, pre-existing divergence that
    // is not figure-specific --- so pin the info-string form.)
    assert_block_kinds(
        "![Cap.](img.jpg)\n```py\ncode\n```\n",
        &[SyntaxKind::FIGURE, SyntaxKind::CODE_BLOCK],
    );
}

#[test]
fn two_images_on_consecutive_lines_stay_a_paragraph() {
    // pandoc -f markdown: Para [Image, SoftBreak, Image]
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
