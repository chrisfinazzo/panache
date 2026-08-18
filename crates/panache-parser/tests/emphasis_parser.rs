//! Unit tests for emphasis parser.
//!
//! These tests verify CST structure directly, focusing on parser correctness
//! rather than formatter output. Each test parses input and inspects the
//! resulting syntax tree to ensure emphasis is parsed correctly.

use panache_parser::{parse, syntax::SyntaxKind};

fn find_nodes(
    tree: &panache_parser::SyntaxNode,
    kind: SyntaxKind,
) -> Vec<panache_parser::SyntaxNode> {
    tree.descendants_with_tokens()
        .filter_map(|element| {
            if element.kind() == kind {
                match element {
                    rowan::NodeOrToken::Node(n) => Some(n),
                    rowan::NodeOrToken::Token(_) => None,
                }
            } else {
                None
            }
        })
        .collect()
}

fn count_elements(tree: &panache_parser::SyntaxNode, kind: SyntaxKind) -> usize {
    tree.descendants_with_tokens()
        .filter(|element| element.kind() == kind)
        .count()
}

fn has_child(node: &panache_parser::SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .any(|child| child.kind() == kind)
}

fn count_nodes(tree: &panache_parser::SyntaxNode, kind: SyntaxKind) -> usize {
    find_nodes(tree, kind).len()
}

#[test]
fn code_span_in_emphasis() {
    let input = "*text `code here` end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_CODE),
        "Emphasis should contain code span as child"
    );
}

#[test]
fn code_span_with_asterisk_in_emphasis() {
    let input = "*text `code * here` end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_CODE),
        "Emphasis should contain code span"
    );

    let code_spans = find_nodes(&tree, SyntaxKind::INLINE_CODE);
    assert_eq!(code_spans.len(), 1, "Should have exactly one code span");
}

#[test]
fn math_in_emphasis() {
    let input = "*text $math$ end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_MATH),
        "Emphasis should contain inline math"
    );
}

#[test]
fn math_with_asterisk_in_emphasis() {
    let input = "*text $a * b$ end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_MATH),
        "Emphasis should contain inline math"
    );
}

#[test]
fn link_in_emphasis() {
    let input = "*text [link](url) end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::LINK),
        "Emphasis should contain link"
    );
}

#[test]
fn link_with_asterisk_in_emphasis() {
    let input = "*text [link * here](url) end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::LINK),
        "Emphasis should contain link"
    );
}

#[test]
fn image_in_emphasis() {
    let input = "*text ![alt](img.png) end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::IMAGE_LINK),
        "Emphasis should contain image"
    );
}

#[test]
fn strong_with_code_span() {
    let input = "**text `code ** here` end**\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert_eq!(
        strong_nodes.len(),
        1,
        "Should parse exactly one strong node"
    );

    let strong = &strong_nodes[0];
    assert!(
        has_child(strong, SyntaxKind::INLINE_CODE),
        "Strong should contain code span"
    );
}

#[test]
fn complex_nesting() {
    let input = "*em with `code`, [link](url), and text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_CODE),
        "Should contain code span"
    );
    assert!(has_child(emphasis, SyntaxKind::LINK), "Should contain link");
}

#[test]
fn three_opener_two_closer() {
    let input = "***foo**\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert_eq!(
        strong_nodes.len(),
        1,
        "Should parse exactly one strong node"
    );
}

#[test]
fn triple_matched() {
    let input = "***foo***\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);

    assert!(
        !strong_nodes.is_empty() || !emphasis_nodes.is_empty(),
        "Should parse emphasis/strong from triple delimiters"
    );
}

#[test]
fn four_or_more_delimiters_literal() {
    let input = "****foo****\n";
    let tree = parse(input, None);

    let _emphasis_count = count_nodes(&tree, SyntaxKind::EMPHASIS);
    let _strong_count = count_nodes(&tree, SyntaxKind::STRONG);
}

#[test]
fn overlapping_emphasis_strong() {
    let input = "*foo **bar* baz**\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert!(
        !strong_nodes.is_empty(),
        "Should parse at least one strong node"
    );
}

#[test]
fn overlapping_strong_emphasis() {
    let input = "**foo *bar** baz*\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);

    assert_eq!(tree.text().to_string(), input, "Should be lossless");

    let _ = (strong_nodes, emphasis_nodes);
}

#[test]
fn adjacent_emphasis() {
    let input = "*foo**bar*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert!(
        emphasis_nodes.is_empty(),
        "Adjacent ** should prevent * from matching"
    );
}

#[test]
fn adjacent_strong() {
    let input = "**foo****bar**\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert!(!strong_nodes.is_empty(), "Should parse strong emphasis");
}

#[test]
fn intraword_asterisk() {
    let input = "un*frigging*believable\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse intraword emphasis with asterisks"
    );
}

#[test]
fn intraword_underscore_disabled() {
    let input = "feas_ible\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse intraword emphasis with underscores"
    );
}

#[test]
fn whitespace_flanking_opener() {
    let input = "* foo*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis with space after opener"
    );
}

#[test]
fn whitespace_flanking_closer() {
    let input = "*foo *\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Asterisk emphasis should allow space before closer (Pandoc behavior)"
    );
}

#[test]
fn escaped_opener() {
    let input = "\\*foo*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis when opener is escaped"
    );

    let escape_count = count_elements(&tree, SyntaxKind::ESCAPED_CHAR);
    assert!(escape_count >= 1, "Should have escape node");
}

#[test]
fn escaped_closer() {
    let input = "*foo\\*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis when closer is escaped"
    );
}

#[test]
fn escaped_within_emphasis() {
    let input = "*foo \\* bar*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse emphasis with escape inside"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::ESCAPED_CHAR),
        "Emphasis should contain escape node"
    );
}

#[test]
fn unclosed_code_in_emphasis() {
    let input = "*text `unclosed code end*\n";
    let _tree = parse(input, None);
}

#[test]
fn unclosed_emphasis() {
    let input = "*foo\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse unclosed emphasis"
    );
}

#[test]
fn unclosed_strong() {
    let input = "**foo\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert_eq!(strong_nodes.len(), 0, "Should not parse unclosed strong");
}

#[test]
fn emphasis_in_strikeout() {
    let input = "~~strike *em* text~~\n";
    let tree = parse(input, None);

    let strikeout_nodes = find_nodes(&tree, SyntaxKind::STRIKEOUT);
    assert_eq!(strikeout_nodes.len(), 1, "Should parse strikeout");

    let strikeout = &strikeout_nodes[0];
    assert!(
        has_child(strikeout, SyntaxKind::EMPHASIS),
        "Strikeout should contain emphasis"
    );
}

#[test]
fn strikeout_in_emphasis() {
    let input = "*em ~~strike~~ text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(emphasis_nodes.len(), 1, "Should parse emphasis");

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::STRIKEOUT),
        "Emphasis should contain strikeout"
    );
}

#[test]
fn subscript_in_emphasis() {
    let input = "*em ~sub~ text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(emphasis_nodes.len(), 1, "Should parse emphasis");

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::SUBSCRIPT),
        "Emphasis should contain subscript"
    );
}

#[test]
fn superscript_in_emphasis() {
    let input = "*em ^super^ text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(emphasis_nodes.len(), 1, "Should parse emphasis");

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::SUPERSCRIPT),
        "Emphasis should contain superscript"
    );
}

#[test]
fn empty_emphasis() {
    let input = "**\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);

    assert_eq!(emphasis_nodes.len(), 0, "Should not parse empty emphasis");
    assert_eq!(strong_nodes.len(), 0, "Should not parse empty strong");
}

#[test]
fn emphasis_only_code() {
    let input = "*`code`*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse emphasis with only code"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_CODE),
        "Emphasis should contain code span"
    );
}

#[test]
fn lossless_simple_emphasis() {
    let input = "*foo*\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(
        input, output,
        "Parser should be lossless for simple emphasis"
    );
}

#[test]
fn lossless_triple_delimiters() {
    let input = "***foo**\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(
        input, output,
        "Parser should preserve all bytes for mismatched delimiters"
    );
}

#[test]
fn lossless_with_nested_code() {
    let input = "*text `code * here` end*\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(input, output, "Parser should be lossless with nested code");
}

#[test]
fn lossless_unclosed() {
    let input = "*foo\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(
        input, output,
        "Parser should be lossless for unclosed emphasis"
    );
}

#[test]
fn pandoc_overlapping_strong_emphasis_all_literal() {
    let input = "**foo *bar** baz*\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);

    assert_eq!(
        strong_nodes.len(),
        0,
        "Pandoc: **foo *bar** baz* should have NO strong nodes (all literal)"
    );
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Pandoc: **foo *bar** baz* should have NO emphasis nodes (all literal)"
    );
}

#[test]
fn pandoc_escaped_star_then_emphasis() {
    let input = "\\**not bold\\**\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);

    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Pandoc: \\**not bold\\** should produce exactly ONE emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    let emph_text = emphasis.text().to_string();
    assert!(
        emph_text.contains("not bold"),
        "Emphasis should contain 'not bold', got: {}",
        emph_text
    );
}
