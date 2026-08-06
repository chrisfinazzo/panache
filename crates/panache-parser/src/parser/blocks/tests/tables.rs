use super::helpers::{parse_blocks, parse_blocks_gfm, parse_blocks_with_config};
use crate::options::{Dialect, Extensions, Flavor, ParserOptions};
use crate::syntax::{SyntaxKind, SyntaxNode};

fn commonmark_options() -> ParserOptions {
    ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    }
}

/// Kinds of `node`'s direct children.
fn child_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
    node.children().map(|child| child.kind()).collect()
}

fn first_of(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.descendants().find(|n| n.kind() == kind)
}

/// Text of every `TABLE_CELL` in the table's header row.
fn header_cells(node: &SyntaxNode) -> Vec<String> {
    first_of(node, SyntaxKind::TABLE_HEADER)
        .map(|header| {
            header
                .children()
                .filter(|n| n.kind() == SyntaxKind::TABLE_CELL)
                .map(|n| n.text().to_string())
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Bodyless pipe tables
//
// pandoc's `pipeTable` reads the rows after the delimiter with `many`, so a
// header plus a delimiter row and nothing else is a complete table.
// ---------------------------------------------------------------------------

#[test]
fn pipe_table_without_body_rows_is_a_table() {
    // pandoc -f markdown on `a | b\n---|---`: a Table whose head carries
    // `a` and `b` and whose body is empty.
    let input = "a | b\n---|---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(child_kinds(&node), vec![SyntaxKind::PIPE_TABLE]);
    assert_eq!(header_cells(&node), vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn bodyless_pipe_table_still_needs_a_delimiter_row() {
    // Without a delimiter row there is no table at all — the two lines stay
    // one paragraph, as they were before bodyless tables were recognized.
    let input = "a | b\nc | d\n";
    let node = parse_blocks(input);

    assert_eq!(node.text().to_string(), input);
    assert_eq!(child_kinds(&node), vec![SyntaxKind::PARAGRAPH]);
}

#[test]
fn setext_underline_outranks_a_bodyless_pipe_table() {
    // pandoc -f markdown on `a | b\n---`: Header 2 [Str "a", Space, Str "|",
    // Space, Str "b"]. `setextHeader` runs before `table` in the reader
    // order, and the registry mirrors it.
    let input = "a | b\n---\n";
    let node = parse_blocks(input);

    assert_eq!(node.text().to_string(), input);
    assert_eq!(child_kinds(&node), vec![SyntaxKind::HEADING]);
}

#[test]
fn bodyless_pipe_table_is_recognized_under_gfm() {
    let input = "a | b\n---|---\n";
    let node = parse_blocks_gfm(input);

    assert_eq!(node.text().to_string(), input);
    assert_eq!(child_kinds(&node), vec![SyntaxKind::PIPE_TABLE]);
}

// ---------------------------------------------------------------------------
// Bodyless pipe tables as a blockquote depth cap
//
// `blockQuote` strips exactly one `>` per line and re-parses the rest, and
// `table` runs before it in pandoc's reader order — so a delimiter row
// carrying fewer markers than its header line caps the quote and the surplus
// `>` becomes literal cell text. See `Parser::blockquote_depth_cap`.
// ---------------------------------------------------------------------------

#[test]
fn bodyless_pipe_table_caps_nested_blockquote_under_pandoc() {
    // pandoc -f markdown on `> > a | b\n> ---|---`: BlockQuote [Table …]
    // with `> a` as the first header cell.
    let input = "> > a | b\n> ---|---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(child_kinds(&node), vec![SyntaxKind::BLOCK_QUOTE]);
    let quote = node.children().next().unwrap();
    assert_eq!(child_kinds(&quote), vec![SyntaxKind::PIPE_TABLE]);
    assert!(
        quote
            .descendants()
            .all(|n| n.kind() != SyntaxKind::BLOCK_QUOTE || n == quote),
        "the delimiter row's single marker caps the quote at depth 1"
    );
    assert_eq!(
        header_cells(&node),
        vec!["> a".to_string(), "b".to_string()],
        "the surplus marker is literal cell text"
    );
}

#[test]
fn bodyless_pipe_table_does_not_cap_under_commonmark() {
    // CommonMark has no pipe tables, so nothing outranks `blockQuote` here
    // and both markers open real containers — matching
    // `pandoc -f commonmark`: BlockQuote [BlockQuote [Para …]].
    let input = "> > a | b\n> ---|---\n";
    let node = parse_blocks_with_config(input, &commonmark_options());

    assert_eq!(node.text().to_string(), input);
    let outer = node.children().next().unwrap();
    assert_eq!(outer.kind(), SyntaxKind::BLOCK_QUOTE);
    assert_eq!(child_kinds(&outer), vec![SyntaxKind::BLOCK_QUOTE]);
}
