use super::helpers::{find_first, parse_blocks};
use crate::options::{Dialect, Extensions, Flavor, ParserOptions};
use crate::parser::Parser;
use crate::syntax::{SyntaxKind, SyntaxNode};

fn get_heading_content(node: &SyntaxNode) -> Option<String> {
    find_first(node, SyntaxKind::HEADING_CONTENT).map(|n| n.text().to_string())
}

#[test]
fn parses_simple_atx_heading() {
    let node = parse_blocks("# Heading\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Heading");
}

#[test]
fn empty_atx_heading() {
    let node = parse_blocks("# \n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "");
}

#[test]
fn parses_atx_heading_with_leading_spaces() {
    let node = parse_blocks("  # Leading spaces\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Leading spaces");
}

#[test]
fn parses_atx_heading_with_multiple_hashes() {
    let node = parse_blocks("### Subheading\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Subheading");
}

#[test]
fn parses_atx_heading_with_trailing_hashes() {
    let node = parse_blocks("### Foo Bar ###\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Foo Bar");
}

#[test]
fn does_not_parse_with_four_leading_spaces() {
    let node = parse_blocks("    # Not a heading\n");
    assert!(find_first(&node, SyntaxKind::HEADING).is_none());
}

#[test]
fn requires_blank_line_before_heading() {
    let node = parse_blocks("text\n# Heading\n");
    assert!(find_first(&node, SyntaxKind::HEADING).is_none());
}

#[test]
fn parses_heading_after_horizontal_rule_without_blank_line() {
    let node = parse_blocks("---\n# Heading\n");
    let headings: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING)
        .collect();
    assert_eq!(headings.len(), 1);
}

#[test]
fn parses_heading_after_code_block_without_blank_line() {
    let node = parse_blocks("```r\nx\n```\n# Heading\n");
    let headings: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING)
        .collect();
    assert_eq!(headings.len(), 1);
}

#[test]
fn parses_empty_atx_heading_before_table_caption_table() {
    let input = "##\n\n: Example After One Iteration\n\n| Experiment | Heads | $\\gamma$ |\n|------------|-------|----------|\n| 1          |     7 |     0.78 |\n";
    let node = parse_blocks(input);
    let blocks: Vec<_> = node
        .children()
        .filter(|n| n.kind() != SyntaxKind::BLANK_LINE)
        .collect();
    assert_eq!(blocks.first().map(|n| n.kind()), Some(SyntaxKind::HEADING));
    assert_eq!(
        blocks.get(1).map(|n| n.kind()),
        Some(SyntaxKind::PIPE_TABLE)
    );
    assert!(
        find_first(&node, SyntaxKind::DEFINITION_LIST).is_none(),
        "empty ATX heading should not be parsed as a definition-list term"
    );
}

#[test]
fn parses_heading_without_blank_line_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = "Text\n# Heading\nMore\n";
    let node = Parser::new(input, &config).parse();
    let blocks: Vec<_> = node.children().map(|node| node.kind()).collect();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        blocks,
        vec![
            SyntaxKind::PARAGRAPH,
            SyntaxKind::HEADING,
            SyntaxKind::PARAGRAPH
        ]
    );
}

#[test]
fn atx_interrupts_lazy_blockquote_line_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = "> para\n# head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn indented_atx_on_a_lazy_blockquote_line_folds_into_the_quote() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = "> para\n # head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE]
    );
    let quote = node.children().next().unwrap();
    assert_eq!(
        quote.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::PARAGRAPH, SyntaxKind::HEADING]
    );
}

#[test]
fn atx_interrupts_lazy_blockquote_line_commonmark() {
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let input = "> para\n# head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn atx_on_lazy_blockquote_line_stays_text_by_default() {
    let config = ParserOptions::default();
    let input = "> para\n# head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE]
    );
    assert!(
        node.descendants().all(|n| n.kind() != SyntaxKind::HEADING),
        "heading-shaped lazy line must stay paragraph text by default"
    );
}

#[test]
fn atx_interrupts_lazy_blockquote_list_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = "> - item\n# head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn atx_interrupts_reduced_marker_lazy_line_commonmark() {
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let input = ">> para\n> # head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE]
    );
    let outer = node.children().next().unwrap();
    assert_eq!(
        outer.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn atx_interrupts_reduced_marker_lazy_line_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = ">> para\n> # head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE]
    );
    let outer = node.children().next().unwrap();
    assert_eq!(
        outer.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn atx_on_reduced_marker_lazy_line_stays_text_by_default() {
    let config = ParserOptions::default();
    let input = ">> para\n> # head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        node.descendants().all(|n| n.kind() != SyntaxKind::HEADING),
        "heading-shaped reduced-marker lazy line must stay paragraph text by default"
    );
}

#[test]
fn atx_interrupts_reduced_marker_lazy_line_depth3() {
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let input = ">>> para\n>> # head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(outer.kind(), SyntaxKind::BLOCK_QUOTE);
    let middle = outer.children().next().unwrap();
    assert_eq!(
        middle
            .children()
            .map(|node| node.kind())
            .collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn atx_interrupts_reduced_marker_lazy_list_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = ">> - item\n> # head\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE]
    );
    let outer = node.children().next().unwrap();
    assert_eq!(
        outer.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HEADING]
    );
}

#[test]
fn setext_forms_inside_blockquote_under_commonmark() {
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let input = "> a\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().unwrap();
    assert_eq!(quote.kind(), SyntaxKind::BLOCK_QUOTE);
    assert_eq!(
        quote.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::HEADING]
    );
}

#[test]
fn setext_underline_in_deeper_blockquote_is_not_an_underline() {
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let input = "a\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::PARAGRAPH, SyntaxKind::BLOCK_QUOTE]
    );
}

#[test]
fn setext_underline_that_opens_a_blockquote_is_lazy_text_under_pandoc() {
    let config = ParserOptions::default();
    let input = "a\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::PARAGRAPH]
    );
}

#[test]
fn setext_underline_in_deeper_blockquote_is_lazy_text_under_pandoc() {
    let config = ParserOptions::default();
    let input = "> a\n> > ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().unwrap();
    assert_eq!(quote.kind(), SyntaxKind::BLOCK_QUOTE);
    assert!(
        node.descendants().all(|n| n.kind() != SyntaxKind::HEADING),
        "an underline one quote deeper than its text must not form a heading"
    );
}

#[test]
fn setext_underline_mid_paragraph_stays_text_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = "Text\nTitle\n-----\nMore\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::PARAGRAPH]
    );
    assert!(
        node.descendants().all(|n| n.kind() != SyntaxKind::HEADING),
        "setext underline after paragraph text must not form a heading"
    );
}

#[test]
fn setext_heading_at_document_start_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let input = "Title\n=====\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(node.text().to_string(), input);
    assert_eq!(
        node.children().map(|node| node.kind()).collect::<Vec<_>>(),
        vec![SyntaxKind::HEADING]
    );
}

#[test]
fn parses_heading_at_start_of_document() {
    let node = parse_blocks("# Start\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Start");
}

#[test]
fn parses_multiple_headings() {
    let node = parse_blocks("# First\n\n## Second\n");
    let mut headings = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING_CONTENT);
    assert_eq!(headings.next().unwrap().text(), "First");
    assert_eq!(headings.next().unwrap().text(), "Second");
}

#[test]
fn parses_mmd_header_identifier_in_atx_when_enabled() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = true;
    let node = Parser::new("# Heading [my id]\n", &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "[my id]");
}

#[test]
fn does_not_parse_mmd_header_identifier_in_atx_when_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = false;
    let node = Parser::new("# Heading [my id]\n", &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    assert!(
        heading
            .children()
            .all(|n| n.kind() != SyntaxKind::ATTRIBUTE),
        "mmd_header_identifiers disabled should keep [my id] in heading content"
    );
}

#[test]
fn parses_mmd_header_identifier_in_setext_when_enabled() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = true;
    let node = Parser::new("Heading [setext id]\n---\n", &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "[setext id]");
}

#[test]
fn atx_heading_immediately_after_yaml_frontmatter() {
    let input = "---\ntitle: Test\n---\n# Heading\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();
    assert!(
        find_first(&node, SyntaxKind::HEADING).is_some(),
        "heading directly after YAML frontmatter should be parsed as a heading"
    );
}

#[test]
fn atx_heading_with_id_immediately_after_yaml_frontmatter() {
    let input = "---\ntitle: Test\n---\n# One {#one}\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();
    assert!(
        find_first(&node, SyntaxKind::HEADING).is_some(),
        "heading with ID directly after YAML frontmatter should be parsed as a heading"
    );
}

#[test]
fn atx_closing_hashes_keep_a_preceding_brace_block_as_content() {
    let input = "# foo {#id} #\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    assert!(
        heading
            .children()
            .all(|n| n.kind() != SyntaxKind::ATTRIBUTE),
        "closing `#` run must cancel the trailing attribute block"
    );
    assert_eq!(get_heading_content(&node).unwrap(), "foo {#id}");
    assert_eq!(node.text().to_string(), input);
}

#[test]
fn atx_attribute_block_after_closing_hashes_is_an_attribute() {
    let input = "# foo # {#id}\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "{#id}");
    assert_eq!(get_heading_content(&node).unwrap(), "foo");
    assert_eq!(node.text().to_string(), input);
}

#[test]
fn parses_mmd_header_identifier_before_atx_closing_hashes() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = true;
    let input = "## Title [my id] ###\n";
    let node = Parser::new(input, &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "[my id]");
    assert_eq!(node.text().to_string(), input);
}

fn commonmark_options() -> ParserOptions {
    ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    }
}

fn child_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
    node.children().map(|child| child.kind()).collect()
}

#[test]
fn setext_underline_caps_nested_blockquote_depth_under_pandoc() {
    let config = ParserOptions::default();
    let input = "> > a\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(child_kinds(&node), vec![SyntaxKind::BLOCK_QUOTE]);
    let quote = node.children().next().unwrap();
    assert_eq!(child_kinds(&quote), vec![SyntaxKind::HEADING]);
    assert!(
        quote
            .descendants()
            .all(|n| n.kind() != SyntaxKind::BLOCK_QUOTE || n == quote),
        "the underline's single marker caps the quote at depth 1"
    );
    assert_eq!(
        get_heading_content(&node).as_deref(),
        Some("> a"),
        "the surplus marker is literal heading text"
    );
}

#[test]
fn setext_equals_underline_caps_nested_blockquote_depth_under_pandoc() {
    let config = ParserOptions::default();
    let input = "> > a\n> ===\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().unwrap();
    assert_eq!(child_kinds(&quote), vec![SyntaxKind::HEADING]);
    assert_eq!(get_heading_content(&node).as_deref(), Some("> a"));
}

#[test]
fn setext_underline_caps_three_blockquotes_to_one_under_pandoc() {
    let config = ParserOptions::default();
    let input = "> > > a\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().unwrap();
    assert_eq!(child_kinds(&quote), vec![SyntaxKind::HEADING]);
    assert_eq!(get_heading_content(&node).as_deref(), Some("> > a"));
}

#[test]
fn capped_blockquote_keeps_following_line_in_the_same_quote() {
    let config = ParserOptions::default();
    let input = "> > a\n> ---\n> b\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().unwrap();
    assert_eq!(
        child_kinds(&quote),
        vec![SyntaxKind::HEADING, SyntaxKind::PARAGRAPH]
    );
}

#[test]
fn setext_underline_caps_depth_from_inside_an_open_blockquote() {
    let config = ParserOptions::default();
    let input = "> a\n>\n> > b\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().unwrap();
    assert_eq!(
        child_kinds(&quote),
        vec![
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::HEADING
        ]
    );
    assert_eq!(get_heading_content(&node).as_deref(), Some("> b"));
}

#[test]
fn setext_underline_caps_to_its_own_depth_not_to_one() {
    let config = ParserOptions::default();
    let input = "> a\n>\n> > > b\n> > ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(
        child_kinds(&outer),
        vec![
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::BLOCK_QUOTE
        ]
    );
    let inner = outer
        .children()
        .find(|n| n.kind() == SyntaxKind::BLOCK_QUOTE)
        .unwrap();
    assert_eq!(child_kinds(&inner), vec![SyntaxKind::HEADING]);
    assert_eq!(get_heading_content(&node).as_deref(), Some("> b"));
}

#[test]
fn capped_blockquote_handles_markers_without_trailing_spaces() {
    let config = ParserOptions::default();
    for input in ["> > a\n>---\n", ">> a\n> ---\n"] {
        let node = Parser::new(input, &config).parse();
        assert_eq!(
            node.text().to_string(),
            input,
            "parser must remain lossless for {input:?}"
        );
        let quote = node.children().next().unwrap();
        assert_eq!(
            child_kinds(&quote),
            vec![SyntaxKind::HEADING],
            "unexpected shape for {input:?}"
        );
        assert_eq!(get_heading_content(&node).as_deref(), Some("> a"));
    }
}

#[test]
fn underline_matching_full_depth_keeps_both_quotes_under_pandoc() {
    let config = ParserOptions::default();
    let input = "> > a\n> > ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(child_kinds(&outer), vec![SyntaxKind::BLOCK_QUOTE]);
    let inner = outer.children().next().unwrap();
    assert_eq!(child_kinds(&inner), vec![SyntaxKind::HEADING]);
    assert_eq!(get_heading_content(&node).as_deref(), Some("a"));
}

#[test]
fn spaced_thematic_break_does_not_cap_blockquote_depth() {
    let config = ParserOptions::default();
    let input = "> > a\n> - - -\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(child_kinds(&outer), vec![SyntaxKind::BLOCK_QUOTE]);
    assert!(node.descendants().all(|n| n.kind() != SyntaxKind::HEADING));
}

#[test]
fn definition_marker_below_blockquote_rank_does_not_cap_depth() {
    let config = ParserOptions::default();
    let input = "> > a\n: b\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(outer.kind(), SyntaxKind::BLOCK_QUOTE);
    assert_eq!(child_kinds(&outer), vec![SyntaxKind::BLOCK_QUOTE]);
}

#[test]
fn blank_line_before_underline_does_not_cap_depth() {
    let config = ParserOptions::default();
    let input = "> > a\n\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        child_kinds(&node),
        vec![
            SyntaxKind::BLOCK_QUOTE,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::BLOCK_QUOTE
        ]
    );
}

#[test]
fn commonmark_nested_underline_still_closes_the_inner_quote() {
    let config = commonmark_options();
    let input = "> > a\n> ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(
        child_kinds(&outer),
        vec![SyntaxKind::BLOCK_QUOTE, SyntaxKind::HORIZONTAL_RULE]
    );
}

#[test]
fn commonmark_nested_underline_at_full_depth_is_a_heading() {
    let config = commonmark_options();
    let input = "> > a\n> > ---\n";
    let node = Parser::new(input, &config).parse();

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let outer = node.children().next().unwrap();
    assert_eq!(child_kinds(&outer), vec![SyntaxKind::BLOCK_QUOTE]);
    let inner = outer.children().next().unwrap();
    assert_eq!(child_kinds(&inner), vec![SyntaxKind::HEADING]);
    assert_eq!(get_heading_content(&node).as_deref(), Some("a"));
}

fn heading_content_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
    find_first(node, SyntaxKind::HEADING_CONTENT)
        .map(|n| n.children_with_tokens().map(|el| el.kind()).collect())
        .unwrap_or_default()
}

#[test]
fn atx_heading_trailing_backslash_is_a_hard_break_in_pandoc() {
    let input = "# foo\\\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(
        heading_content_kinds(&node),
        vec![SyntaxKind::TEXT, SyntaxKind::HARD_LINE_BREAK]
    );
}

#[test]
fn atx_heading_lone_trailing_backslash_is_a_hard_break_in_pandoc() {
    let input = "# \\\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(
        heading_content_kinds(&node),
        vec![SyntaxKind::HARD_LINE_BREAK]
    );
}

#[test]
fn atx_heading_escaped_backslash_is_not_a_hard_break() {
    let input = "# foo\\\\\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(
        heading_content_kinds(&node),
        vec![SyntaxKind::TEXT, SyntaxKind::ESCAPED_CHAR]
    );
}

#[test]
fn atx_heading_odd_backslash_run_ends_in_a_hard_break() {
    let input = "# foo\\\\\\\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(
        heading_content_kinds(&node),
        vec![
            SyntaxKind::TEXT,
            SyntaxKind::ESCAPED_CHAR,
            SyntaxKind::HARD_LINE_BREAK
        ]
    );
}

#[test]
fn atx_heading_backslash_space_stays_a_nonbreaking_space() {
    let input = "# foo\\ \n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(
        heading_content_kinds(&node),
        vec![SyntaxKind::TEXT, SyntaxKind::NONBREAKING_SPACE]
    );
}

#[test]
fn atx_heading_backslash_before_attributes_is_not_a_hard_break() {
    let input = "# foo\\ {#id}\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert!(!heading_content_kinds(&node).contains(&SyntaxKind::HARD_LINE_BREAK));
}

/// An escaped space is content, not the gap in front of a trailing attribute
/// block. `pandoc -f markdown` reads `# foo\ {#id}` as
/// `Header 1 (id) [Str "foo\160"]` --- a nonbreaking space, with the attribute
/// stripped separately (pandoc needs no gap at all: `# foo{#id}` carries the
/// attribute too). Trimming it would strand the backslash in the content.
#[test]
fn atx_heading_escaped_space_before_attributes_stays_content() {
    let input = "# foo\\ {#id}\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(get_heading_content(&node).as_deref(), Some("foo\\ "));
    assert_eq!(
        heading_content_kinds(&node),
        vec![SyntaxKind::TEXT, SyntaxKind::NONBREAKING_SPACE]
    );
}

/// Only the escaped whitespace character is content; the rest of the run is
/// still the gap. Pandoc reads `# foo\  {#id}` as `[Str "foo\160"]`.
#[test]
fn atx_heading_escaped_space_keeps_only_the_character_it_escapes() {
    let input = "# foo\\  {#id}\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(get_heading_content(&node).as_deref(), Some("foo\\ "));
}

/// An *escaped backslash* does not escape the space after it, so the gap is
/// ordinary: pandoc reads `# baz\\ {#id}` as `[Str "baz\\"]`.
#[test]
fn atx_heading_escaped_backslash_before_attributes_leaves_the_gap_alone() {
    let input = "# baz\\\\ {#id}\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(get_heading_content(&node).as_deref(), Some("baz\\\\"));
}

/// Odd runs escape, even runs don't, all the way up: pandoc reads
/// `# foo\\\ {#id}` as `[Str "foo\\\160"]`.
#[test]
fn atx_heading_odd_backslash_run_escapes_the_gap() {
    let input = "# foo\\\\\\ {#id}\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(get_heading_content(&node).as_deref(), Some("foo\\\\\\ "));
}

#[test]
fn setext_heading_trailing_backslash_is_a_hard_break_in_pandoc() {
    let input = "foo\\\n---\n";
    let node = parse_blocks(input);
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(
        heading_content_kinds(&node),
        vec![SyntaxKind::TEXT, SyntaxKind::HARD_LINE_BREAK]
    );
}

#[test]
fn atx_heading_trailing_backslash_stays_literal_in_commonmark() {
    let input = "# foo\\\n";
    let node = Parser::new(input, &commonmark_options()).parse();
    assert_eq!(node.text().to_string(), input, "parser must stay lossless");
    assert_eq!(heading_content_kinds(&node), vec![SyntaxKind::TEXT]);
}
