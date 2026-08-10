use super::helpers::{assert_block_kinds, find_all, find_first, parse_blocks};
use crate::syntax::SyntaxKind;

#[test]
fn definition_list_allows_nested_list_after_blank_line() {
    let input = "Term\n\n:  Definition\n\n    - Bullet\n";
    let tree = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::DEFINITION_LIST]);
    assert!(
        find_first(&tree, SyntaxKind::LIST).is_some(),
        "Expected list to be nested inside definition"
    );
}

#[test]
fn definition_list_plain_starts_list_at_content_column_without_blank_line() {
    // A list marker indented to the definition's content column starts a nested
    // list inside the definition, even without a separating blank line. Matches
    // pandoc-native (`pandoc -f markdown -t native`).
    let input = "A definition list with nested items\n:   Here comes a list (or wait, is it?)\n    - A\n    - B\n";
    let tree = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::DEFINITION_LIST]);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("definition");
    assert!(
        find_first(&definition, SyntaxKind::PLAIN).is_some(),
        "definition should contain PLAIN for the leading text"
    );
    let nested_list = find_first(&definition, SyntaxKind::LIST).expect("nested list");
    assert_eq!(
        nested_list
            .children()
            .filter(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .count(),
        2,
        "nested list should contain both items"
    );
}

#[test]
fn definition_list_content_starting_with_list_marker_parses_as_list() {
    let input = "Term\n:   - One\n    - Two\n";
    let tree = parse_blocks(input);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("should find definition");

    assert!(
        find_first(&definition, SyntaxKind::LIST).is_some(),
        "definition should contain LIST when content starts with list marker"
    );

    let has_direct_plain_child = definition
        .children()
        .any(|child| child.kind() == SyntaxKind::PLAIN);
    assert!(
        !has_direct_plain_child,
        "list-only definition should not have a direct PLAIN child"
    );
}

#[test]
fn definition_marker_without_content_preserves_newline_losslessly() {
    let input = "Input\n:   \n\n````markdown\n";
    let tree = parse_blocks(input);

    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn definition_content_can_start_with_atx_heading() {
    let input = "Term\n: # Header\n";
    let tree = parse_blocks(input);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("should find definition");

    assert!(
        find_first(&definition, SyntaxKind::HEADING).is_some(),
        "definition should contain HEADING"
    );
    assert!(
        find_first(&definition, SyntaxKind::PLAIN).is_none(),
        "heading-only definition should not be parsed as PLAIN"
    );
}

#[test]
fn definition_list_continues_across_blank_lines_with_additional_definitions() {
    let input = "Term\n: Def\n\n: Def\n";
    let tree = parse_blocks(input);

    let definition_lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(
        definition_lists.len(),
        1,
        "should remain one definition list"
    );

    let definition_items = find_all(&tree, SyntaxKind::DEFINITION_ITEM);
    assert_eq!(
        definition_items.len(),
        1,
        "should remain one definition item"
    );

    let definitions = find_all(&tree, SyntaxKind::DEFINITION);
    assert_eq!(
        definitions.len(),
        2,
        "should have two definitions for one term"
    );
}

#[test]
fn definition_marker_after_blank_line_does_not_create_orphan_item() {
    let input = "Term\n: Def\n\n: Def\n";
    let tree = parse_blocks(input);

    let definition_item = find_first(&tree, SyntaxKind::DEFINITION_ITEM).expect("definition item");
    let term_count = definition_item
        .children()
        .filter(|child| child.kind() == SyntaxKind::TERM)
        .count();
    assert_eq!(
        term_count, 1,
        "definition item should keep exactly one term"
    );
}

#[test]
fn definition_marker_after_list_definition_closes_nested_list() {
    let input = "Orange\n:   - a\n    - b\n:   Also a color\n";
    let tree = parse_blocks(input);

    let definition_item = find_first(&tree, SyntaxKind::DEFINITION_ITEM).expect("definition item");
    let definitions = definition_item
        .children()
        .filter(|child| child.kind() == SyntaxKind::DEFINITION)
        .count();
    assert_eq!(
        definitions, 2,
        "marker after list definition should create a sibling definition"
    );

    let nested_definition_item = definition_item
        .descendants()
        .any(|node| node.kind() == SyntaxKind::DEFINITION_ITEM && node != definition_item);
    assert!(
        !nested_definition_item,
        "list content should not capture a nested DEFINITION_ITEM"
    );
}

#[test]
fn dedented_list_after_blank_line_does_not_continue_definition_list() {
    let input = "Term\n\n:   - List\n    - a\n\n- b\n";
    let tree = parse_blocks(input);

    assert_block_kinds(
        input,
        &[
            SyntaxKind::DEFINITION_LIST,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::LIST,
        ],
    );

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("definition");
    let nested_list = find_first(&definition, SyntaxKind::LIST).expect("nested list");
    assert_eq!(
        nested_list
            .children()
            .filter(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .count(),
        2,
        "definition list should only contain the indented items"
    );

    assert_eq!(
        find_all(&tree, SyntaxKind::LIST).len(),
        2,
        "expected one nested list and one top-level list"
    );
}

#[test]
fn orphan_colon_marker_with_content_is_paragraph() {
    // A `:` marker with no preceding term is not a definition list; pandoc
    // treats the whole line as a paragraph (`Para [Str ":", Space, Str "foo"]`).
    let input = ":   foo\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "orphan `:` marker should not open a definition list"
    );
    assert_block_kinds(input, &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn orphan_tilde_marker_with_content_is_paragraph() {
    let input = "~   foo\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "orphan `~` marker should not open a definition list"
    );
    assert_block_kinds(input, &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn orphan_bare_marker_with_body_next_line_is_paragraph() {
    // Bare marker with the body on the next line, no term above: pandoc yields
    // `Para [Str ":", SoftBreak, Str "foo"]`.
    let input = ":\n    foo\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "bare orphan `:` marker should not open a definition list"
    );
}

#[test]
fn colon_marker_line_becomes_term_when_next_line_is_marker() {
    // `: foo` / `: bar` has no explicit term, but pandoc makes the first line
    // the term (literal `: foo`) and the second the definition (`bar`).
    let input = ":   foo\n:   bar\n";
    let tree = parse_blocks(input);

    let definition_list =
        find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("should be a definition list");
    let term = find_first(&definition_list, SyntaxKind::TERM).expect("term");
    assert_eq!(
        term.text().to_string().trim_end(),
        ":   foo",
        "the marker-shaped first line should be the literal term"
    );
    let definition = find_first(&definition_list, SyntaxKind::DEFINITION).expect("definition");
    assert!(
        definition.text().to_string().contains("bar"),
        "second marker line supplies the definition body"
    );
}

#[test]
fn colon_table_caption_before_table_is_not_definition_list() {
    let input = "Here's a table with a reference:\n\n: (\\#tab:mytable) A table with a reference.\n\n| A   | B   | C   |\n| --- | --- | --- |\n| 1   | 2   | 3   |\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "colon table caption before a table should not be parsed as DEFINITION_LIST"
    );
    assert!(
        find_first(&tree, SyntaxKind::PIPE_TABLE).is_some(),
        "expected PIPE_TABLE to be parsed for colon caption + table"
    );
    assert!(
        find_first(&tree, SyntaxKind::TABLE_CAPTION).is_some(),
        "expected TABLE_CAPTION node for colon caption"
    );
}

/// Assert the whole document is a single definition list nested `depth`
/// blockquotes deep, with `term` / `definition` as its one item.
fn assert_quoted_definition_list(input: &str, depth: usize, term: &str, definition: &str) {
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "parse must be lossless");

    assert_block_kinds(input, &[SyntaxKind::BLOCK_QUOTE]);
    assert_eq!(
        find_all(&tree, SyntaxKind::BLOCK_QUOTE).len(),
        depth,
        "definition list should stay {depth} blockquotes deep"
    );

    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    assert_eq!(
        find_first(&list, SyntaxKind::TERM)
            .expect("term")
            .text()
            .to_string()
            .trim_end(),
        term
    );
    assert!(
        find_first(&list, SyntaxKind::DEFINITION)
            .expect("definition body")
            .text()
            .to_string()
            .contains(definition),
        "definition body should carry {definition:?}"
    );
}

#[test]
fn definition_list_in_blockquote_keeps_its_body() {
    // The term look-ahead runs on container-stripped lines, so it sees `: b`
    // through the `> ` prefix. Pandoc: `BlockQuote [DefinitionList [(a, [[Plain
    // b]])]]`.
    assert_quoted_definition_list("> a\n> : b\n", 1, "a", "b");
}

#[test]
fn definition_list_in_nested_blockquote_keeps_its_body() {
    assert_quoted_definition_list("> > a\n> > : b\n", 2, "a", "b");
}

#[test]
fn lazy_definition_marker_stays_inside_blockquote() {
    // Pandoc folds lazy lines into the blockquote's raw content before parsing
    // blocks, so the unquoted `: b` is the term's definition rather than a
    // top-level paragraph.
    assert_quoted_definition_list("> a\n: b\n", 1, "a", "b");
}

#[test]
fn lazy_definition_marker_stays_inside_nested_blockquote() {
    assert_quoted_definition_list("> > a\n: b\n", 2, "a", "b");
}

#[test]
fn lazy_definition_marker_reduced_depth_stays_inside_blockquote() {
    // One `>` under a depth-2 quote is still lazy: the marker belongs to the
    // inner definition list, not to the outer quote.
    assert_quoted_definition_list("> > a\n> : b\n", 2, "a", "b");
}

#[test]
fn lazy_definition_markers_add_further_definitions() {
    let input = "> > a\n: b\n: c\n";
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "parse must be lossless");

    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    let definitions = find_all(&list, SyntaxKind::DEFINITION);
    assert_eq!(definitions.len(), 2, "both lazy markers open a definition");
    assert!(definitions[0].text().to_string().contains('b'));
    assert!(definitions[1].text().to_string().contains('c'));
}

#[test]
fn trailing_marker_line_outside_the_item_is_not_a_definition_marker() {
    // `ContainerPrefix::strip` advances the item's content column with
    // `advance_columns`, which counts any character as a column. Inside a
    // two-column item that turns `"c :"` into `":"`, so the term lookahead
    // used to see a bare marker and promote the line above it. Pandoc reads
    // `BulletList [[Plain [a, SoftBreak, b]]]` + `Para [c, Space, ":"]`.
    for input in [
        "- a\nb\n\nc :\n",
        "- a\n  b\n\nc :\n",
        "- a\nb\n\nc ~\n",
        "- a\n\n  b\n\nc :\n",
    ] {
        let tree = parse_blocks(input);
        assert!(
            find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
            "a marker line outside the list item must not open a definition list: {input:?}"
        );
        assert_block_kinds(
            input,
            &[
                SyntaxKind::LIST,
                SyntaxKind::BLANK_LINE,
                SyntaxKind::PARAGRAPH,
            ],
        );
    }
}

#[test]
fn trailing_marker_line_outside_the_item_is_inert_in_a_blockquote() {
    let input = "> - a\n> b\n>\n> c :\n";
    let tree = parse_blocks(input);
    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "pandoc reads this as BlockQuote [BulletList, Para]"
    );
}

#[test]
fn ordered_and_tab_variants_already_agree_with_pandoc() {
    // Controls for the case above: an ordered marker gives content_col 3 (the
    // faked slice is just the newline) and a tab overshoots column 2, so
    // neither ever reached the bad path. They must stay put.
    for input in ["1. a\nb\n\nc :\n", "- a\nb\n\nc\t:\n"] {
        assert!(
            find_first(&parse_blocks(input), SyntaxKind::DEFINITION_LIST).is_none(),
            "{input:?}"
        );
    }
}

#[test]
fn definition_marker_at_the_item_content_column_still_opens_a_definition() {
    // The gate must only reject lines that fall short of the content column.
    let input = "- a\n\n  b\n\n  : def\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    let term = find_first(&list, SyntaxKind::TERM).expect("term");
    assert_eq!(term.text().to_string().trim(), "b");
}
