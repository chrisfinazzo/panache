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
    // pandoc -f markdown-blank_before_header: BlockQuote [Para "para"],
    // Header 1 "head" — the heading ends the quote instead of being
    // swallowed as lazy continuation text (issue #428).
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
fn atx_interrupts_lazy_blockquote_line_commonmark() {
    // pandoc -f commonmark agrees: an ATX heading is never paragraph
    // continuation text, so the lazy line ends the quote (CommonMark §5.1).
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
    // Default pandoc (blank_before_header on) keeps the heading-shaped line
    // as lazy paragraph text: BlockQuote [Para "para SoftBreak # head"].
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
    // pandoc -f markdown-blank_before_header: BlockQuote [BulletList],
    // Header 1 "head" — same interruption for a list item's lazy line.
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
    // pandoc -f commonmark: BlockQuote [BlockQuote [Para "para"], Header 1] —
    // a `> # head` line under an open depth-2 quote is not lazy continuation;
    // the inner quote closes and the heading forms in the outer quote
    // (issue #429).
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
    // pandoc -f markdown-blank_before_header agrees with commonmark on the
    // reduced-marker shape: BlockQuote [BlockQuote [Para], Header 1].
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
    // Default pandoc (blank_before_header on) keeps the reduced-marker
    // heading-shaped line as lazy paragraph text, same as the zero-marker
    // form: BlockQuote [BlockQuote [Para "para SoftBreak # head"]].
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
    // pandoc -f commonmark on `>>> para\n>> # head`: the heading forms at
    // the two-marker level — BlockQuote [BlockQuote [BlockQuote [Para],
    // Header 1]].
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
    // pandoc -f markdown-blank_before_header: BlockQuote [BlockQuote
    // [BulletList], Header 1] — the reduced-marker heading line also ends a
    // list item's lazy continuation (issue #429).
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
    // pandoc -f commonmark on `> a\n> ---`: BlockQuote [Header 2 "a"].
    // The underline shares the text line's container, so it underlines
    // rather than closing the quote as a thematic break.
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
    // pandoc -f commonmark on `a\n> ---`: Para "a", BlockQuote
    // [HorizontalRule]. The underline opens a quote the text line is not
    // in, so the containers differ and no heading forms.
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
    // pandoc -f markdown on `a\n> ---`: one Para, `[Str "a", SoftBreak,
    // Str ">", Space, Str "---"]`. Pandoc reads a marker run on the *text*
    // line as literal text (`> foo\n---` is a top-level H2 including the
    // `>`), but the underline still has to land in the text line's
    // container — here it does not, so the line stays lazy paragraph text.
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
    // pandoc -f markdown on `> a\n> > ---`: BlockQuote [Para [Str "a",
    // SoftBreak, Str ">", Space, Str "---"]] — the underline opens a quote
    // deeper than the text line's, so no heading forms there either.
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
    // Pandoc never forms a setext heading mid-paragraph, even with
    // `blank_before_header` disabled: `markdown-blank_before_header` keeps
    // `Text\nTitle\n-----\nMore` a single Para. Only ATX interrupts.
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
    // Pandoc allows a heading directly after YAML frontmatter without a blank line.
    let input = "---\ntitle: Test\n---\n# Heading\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();
    assert!(
        find_first(&node, SyntaxKind::HEADING).is_some(),
        "heading directly after YAML frontmatter should be parsed as a heading"
    );
}

#[test]
fn atx_heading_with_id_immediately_after_yaml_frontmatter() {
    // Heading IDs must be extractable when heading follows YAML directly.
    let input = "---\ntitle: Test\n---\n# One {#one}\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();
    assert!(
        find_first(&node, SyntaxKind::HEADING).is_some(),
        "heading with ID directly after YAML frontmatter should be parsed as a heading"
    );
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
