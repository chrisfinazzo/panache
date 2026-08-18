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

fn child_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
    node.children().map(|child| child.kind()).collect()
}

fn first_of(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.descendants().find(|n| n.kind() == kind)
}

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

#[test]
fn pipe_table_without_body_rows_is_a_table() {
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
    let input = "a | b\nc | d\n";
    let node = parse_blocks(input);

    assert_eq!(node.text().to_string(), input);
    assert_eq!(child_kinds(&node), vec![SyntaxKind::PARAGRAPH]);
}

#[test]
fn setext_underline_outranks_a_bodyless_pipe_table() {
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

#[test]
fn bodyless_pipe_table_caps_nested_blockquote_under_pandoc() {
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
    let input = "> > a | b\n> ---|---\n";
    let node = parse_blocks_with_config(input, &commonmark_options());

    assert_eq!(node.text().to_string(), input);
    let outer = node.children().next().unwrap();
    assert_eq!(outer.kind(), SyntaxKind::BLOCK_QUOTE);
    assert_eq!(child_kinds(&outer), vec![SyntaxKind::BLOCK_QUOTE]);
}

/// `pandoc -f markdown -t native`: BlockQuote [Table ...], the div closes,
/// and `after` is a sibling of the div. The closer is never a table row.
#[test]
fn simple_table_in_a_quoted_div_ends_at_the_div_closer() {
    let input = "::: note\n\n> A    B\n> --- ---\n> x    y\n:::\n\nafter\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        child_kinds(&node),
        vec![
            SyntaxKind::FENCED_DIV,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::PARAGRAPH
        ],
        "the div closes at `:::`, so `after` sits outside it"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table inside the quote");
    assert!(
        !table.text().to_string().contains(":::"),
        "the div closer is not a table row: {}",
        table.text()
    );
}

/// A headerless single-column table needs its closing dash line before the
/// container's lines end. `pandoc -f markdown -t native` on this document
/// has no table at all: the body is `Para [-- x]` and the trailing `--`
/// opens a table outside the div.
#[test]
fn single_column_table_needs_its_closer_before_the_div_closer() {
    let input = "::: note\n\nTerm\n\n:   body\n\n    --\n    x\n:::\n--\ny\n--\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let div = node.children().next().expect("div");
    assert_eq!(div.kind(), SyntaxKind::FENCED_DIV);
    assert!(
        first_of(&div, SyntaxKind::SIMPLE_TABLE).is_none(),
        "the closing dash line is out of reach, so `--` stays a paragraph"
    );
    assert!(
        first_of(&node, SyntaxKind::SIMPLE_TABLE).is_some(),
        "the table below the div is unaffected"
    );
}

/// The bound is gated on a div being open *outside* the container: pandoc
/// raises its div level when the opening fence is parsed, so a stray `:::`
/// with no div to close is collected as ordinary content. Here
/// `pandoc -f markdown -t native` puts the fence in the table as a row.
#[test]
fn stray_div_closer_in_a_definition_body_stays_a_table_row() {
    let input = "Term\n\n:   body\n\n    --\n    x\n:::\n--\ny\n--\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the body");
    assert!(
        table.text().to_string().contains(":::"),
        "with no div open the fence is a row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: Div \[Table\] — a table sitting
/// directly in the div keeps its footer even with the closer contiguous.
#[test]
fn simple_table_directly_in_a_div_survives_the_contiguous_closer() {
    let input = "::: note\nA    B\n--- ---\nx    y\n:::\n\nafter\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the div");
    assert!(
        !table.text().to_string().contains(":::"),
        "the div closer is not a table row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: the closer ends the list item's run,
/// whose raw ends without the blank the footer rule needs — the item
/// holds a paragraph, not a table.
#[test]
fn simple_table_in_a_div_list_item_degrades_at_the_div_closer() {
    let input = "::: warn\n- item\n\n  A    B\n  --- ---\n  x    y\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let div = node.children().next().expect("div");
    assert_eq!(div.kind(), SyntaxKind::FENCED_DIV);
    assert!(
        first_of(&div, SyntaxKind::SIMPLE_TABLE).is_none(),
        "the contiguous run end fails the footer rule: {}",
        div.text()
    );
    let item = first_of(&div, SyntaxKind::LIST_ITEM).expect("item in the div");
    assert!(
        !item.text().to_string().contains(":::"),
        "the closer stays outside the item: {}",
        item.text()
    );
}

/// `pandoc -f markdown -t native`: same for a definition body — its raw
/// ends contiguous at the closer, so the table degrades to a paragraph.
#[test]
fn simple_table_in_a_div_definition_body_degrades_at_the_div_closer() {
    let input = "::: warn\nterm\n:   body\n\n    A    B\n    --- ---\n    x    y\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let div = node.children().next().expect("div");
    assert_eq!(div.kind(), SyntaxKind::FENCED_DIV);
    assert!(
        first_of(&div, SyntaxKind::SIMPLE_TABLE).is_none(),
        "the contiguous run end fails the footer rule: {}",
        div.text()
    );
}

/// A blockquote inside the item rescues the table: pandoc reparses the
/// quote's raw with `"\n\n"` appended, so `pandoc -f markdown -t native`
/// keeps Div \[BulletList \[\[BlockQuote \[Table\]\]\]\].
#[test]
fn quoted_table_in_a_div_list_item_survives_the_div_closer() {
    let input = "::: warn\n- item\n\n  > A    B\n  > --- ---\n  > x    y\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("quoted table in the item");
    assert!(
        !table.text().to_string().contains(":::"),
        "the div closer is not a table row: {}",
        table.text()
    );
}

/// A footnote body inside the div rescues the table the same way:
/// pandoc's `noteBlock` appends a newline to its collected raw, so
/// `pandoc -f markdown -t native` keeps the Table in the note.
#[test]
fn note_body_table_in_a_div_survives_the_div_closer() {
    let input = "::: warn\n[^1]: body\n\n    A    B\n    --- ---\n    x    y\n:::\n\nx[^1]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the note body");
    assert!(
        !table.text().to_string().contains(":::"),
        "the div closer is not a table row: {}",
        table.text()
    );
}

fn footnote_definitions(node: &SyntaxNode) -> Vec<SyntaxNode> {
    node.descendants()
        .filter(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
        .collect()
}

/// `pandoc -f markdown -t native`: Note 1 holds Para + Table, Note 2
/// survives -- the marker line is never a table row.
#[test]
fn simple_table_in_a_footnote_body_ends_at_a_note_marker() {
    let input = "[^1]: body\n\n    A    B\n    --- ---\n    x    y\n[^2]: two\n\ntext[^1][^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let notes = footnote_definitions(&node);
    assert_eq!(notes.len(), 2, "the second note survives the table scan");
    let table = first_of(&notes[0], SyntaxKind::SIMPLE_TABLE).expect("table in note 1's body");
    assert!(
        !table.text().to_string().contains("[^2]"),
        "the new note marker is not a table row: {}",
        table.text()
    );
}

/// A marker AT the note's content column is note content: pandoc's fence
/// allows at most 3 spaces in the note's outer frame, so
/// `pandoc -f markdown -t native` collects this one as a table row and the
/// trailing `[^2]` reference stays literal.
#[test]
fn note_marker_at_the_content_column_stays_a_table_row() {
    let input =
        "[^1]: body\n\n    A    B\n    --- ---\n    x    y\n    [^2]: two\n\ntext[^1][^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(footnote_definitions(&node).len(), 1);
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the body");
    assert!(
        table.text().to_string().contains("[^2]"),
        "the indented marker is note content, collected as a row: {}",
        table.text()
    );
}

/// The fence lives in `noteBlock`, not `listLine`: with no footnote on the
/// stack, `pandoc -f markdown -t native` itself collects the marker as a
/// table row inside a plain list item, and the trailing `x[^2]` stays
/// literal. Matching pandoc means leaving this shape alone.
#[test]
fn note_marker_after_a_plain_list_item_table_stays_a_table_row() {
    let input = "- item\n\n  A    B\n  --- ---\n  x    y\n[^2]: two\n\nx[^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(footnote_definitions(&node).len(), 0);
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("[^2]"),
        "no note on the stack, so the marker is a row: {}",
        table.text()
    );
}

/// The fence applies transitively to blocks nested inside the note's body:
/// the note's collected raw contains their lines, so a scan running inside
/// a list item in the note stops at the marker all the same.
/// `pandoc -f markdown -t native`: Note 1 holds Para + BulletList [Para,
/// Table], Note 2 survives.
#[test]
fn note_marker_ends_a_list_item_table_inside_a_footnote_body() {
    let input = "[^1]: body\n\n    - item\n\n      A    B\n      --- ---\n      x    y\n[^2]: two\n\ntext[^1][^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let notes = footnote_definitions(&node);
    assert_eq!(notes.len(), 2, "the second note survives the table scan");
    let table = first_of(&notes[0], SyntaxKind::SIMPLE_TABLE).expect("item table in note 1");
    assert!(
        !table.text().to_string().contains("[^2]"),
        "the new note marker is not a table row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: `- next` is a sibling item, and the
/// contiguous run end fails the table's footer rule, so item 1 holds a
/// paragraph, not a table.
#[test]
fn simple_table_in_a_list_item_degrades_at_a_contiguous_sibling_marker() {
    let input = "- item\n\n  A    B\n  --- ---\n  x    y\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let list = node.children().next().expect("list");
    assert_eq!(list.kind(), SyntaxKind::LIST);
    let items: Vec<_> = list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    assert_eq!(items.len(), 2, "the sibling item survives the table scan");
    assert!(
        first_of(&items[0], SyntaxKind::SIMPLE_TABLE).is_none(),
        "the contiguous run end fails the footer rule: {}",
        items[0].text()
    );
    let para = items[0]
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::PLAIN)
        .find(|p| p.text().to_string().contains("x    y"))
        .expect("degraded paragraph in item 1");
    assert!(
        para.text().to_string().contains("--- ---"),
        "the separator line is paragraph text: {}",
        para.text()
    );
}

/// A blank line before the sibling marker satisfies the footer rule:
/// `pandoc -f markdown -t native` keeps the Table.
#[test]
fn simple_table_in_a_list_item_survives_with_a_blank_before_the_sibling() {
    let input = "- item\n\n  A    B\n  --- ---\n  x    y\n\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in item 1");
    assert!(
        !table.text().to_string().contains("next"),
        "the sibling marker is not a table row: {}",
        table.text()
    );
}

/// A blockquote between the item and the table supplies the blank:
/// pandoc reparses the quote's raw with `"\n\n"` appended, so
/// `pandoc -f markdown -t native` keeps BlockQuote \[Table\] in item 1.
#[test]
fn quoted_table_in_a_list_item_survives_a_contiguous_sibling_marker() {
    let input = "- item\n\n  > A    B\n  > --- ---\n  > x    y\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("quoted table in item 1");
    assert!(
        !table.text().to_string().contains("next"),
        "the sibling marker is not a table row: {}",
        table.text()
    );
}

/// A marker at the item's content column is nested-list content, which
/// the run collects like any other line: `pandoc -f markdown -t native`
/// makes it a table row (sliced across the column boundaries -- pandoc
/// itself splits `- ne` / `sted` here).
#[test]
fn nested_list_marker_at_the_content_column_stays_a_table_row() {
    let input = "- item\n\n  A    B\n  --- ---\n  x    y\n  - nested\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("- nested"),
        "the nested marker is collected as a row: {}",
        table.text()
    );
}

/// A marker past the 3-column tolerance but short of the content column
/// is a lazy continuation: `pandoc -f markdown -t native` collects
/// `    - lazy` under `10.  item` (content column 5) as a table row.
#[test]
fn list_marker_in_the_lazy_band_stays_a_table_row() {
    let input = "10.  item\n\n     A    B\n     --- ---\n     x    y\n    - lazy\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("- lazy"),
        "the lazy marker is collected as a row: {}",
        table.text()
    );
}

/// The same shape with the marker inside the tolerance: `pandoc -f
/// markdown -t native` ends the ordered item's run and opens a sibling
/// BulletList after the list — and the contiguous run end fails the
/// table's footer rule, so the item holds a paragraph, not a table.
#[test]
fn list_marker_within_nonindent_tolerance_ends_the_run() {
    let input = "10.  item\n\n     A    B\n     --- ---\n     x    y\n   - sib\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::SIMPLE_TABLE).is_none(),
        "the contiguous run end fails the footer rule"
    );
    let lists: Vec<_> = node
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST)
        .collect();
    assert_eq!(
        lists.len(),
        2,
        "the bullet opens its own list after the ordered one"
    );
    assert!(
        !lists[0].text().to_string().contains("sib"),
        "the marker is not item content: {}",
        lists[0].text()
    );
}

/// `pandoc -f markdown -t native` on `- <div>` + blank + quoted simple
/// table + `  </div>`: `Div [BlockQuote [Table]]` — the quote's run
/// stops and the closer belongs to the div. Before this fence the quoted
/// table sliced the closer into `</di` / `v>` cells.
#[test]
fn quoted_table_in_a_list_item_div_ends_at_the_html_closer() {
    let input = "- <div>\n\n  > col1  col2\n  > ----- -----\n  > a     b\n  </div>\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = first_of(&node, SyntaxKind::BLOCK_QUOTE).expect("quote in the item");
    assert!(
        !quote.text().to_string().contains("</div>"),
        "the closer is not quote content: {}",
        quote.text()
    );
}

/// The closer at column 0 ends the quoted run the same way (`pandoc -f
/// markdown -t native` gives `Div [BlockQuote [Table]]` with one body
/// row).
#[test]
fn html_closer_at_column_zero_ends_the_quoted_run() {
    let input = "- <div>\n\n  > col1  col2\n  > ----- -----\n  > a     b\n</div>\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = first_of(&node, SyntaxKind::BLOCK_QUOTE).expect("quote in the item");
    assert!(
        !quote.text().to_string().contains("</div>"),
        "the closer is not quote content: {}",
        quote.text()
    );
}

/// The ordering gate: for the item's *own* table the fence does not
/// apply — `pandoc -f markdown -t native` collects the closer as a
/// table row (empty cells after HTML stripping; panache slices it as
/// raw inline, the pre-existing width-wobble family in TODO.md).
#[test]
fn item_level_table_collects_the_html_closer_as_a_row() {
    let input = "- <div>\n\n  col1  col2\n  ----- -----\n  a     b\n  </div>\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("</div>"),
        "the item's own table keeps the closer as a row: {}",
        table.text()
    );
}

/// A different tag's closer is no fence: `pandoc -f markdown -t native`
/// collects `  </span>` as a table row sliced into `</span` / `>`
/// cells, and the div stays unclosed.
#[test]
fn wrong_tag_closer_stays_a_quoted_table_row() {
    let input = "- <div>\n\n  > col1  col2\n  > ----- -----\n  > a     b\n  </span>\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("</span>"),
        "the wrong-tag closer is collected as a row: {}",
        table.text()
    );
}

/// The quote gobble consults the same fence for its lazy fold: with a
/// quoted paragraph instead of a table, `pandoc -f markdown -t native`
/// gives `Div [BlockQuote [Para]]` — the closer at the content column
/// ends the quote — while one extra space of indent keeps the line lazy
/// quote content.
#[test]
fn lazy_quote_fold_stops_at_the_html_closer() {
    let stops = "- <div>\n\n  > a\n  </div>\n";
    let node = parse_blocks(stops);
    assert_eq!(
        node.text().to_string(),
        stops,
        "parser must remain lossless"
    );
    let quote = first_of(&node, SyntaxKind::BLOCK_QUOTE).expect("quote in the item");
    assert!(
        !quote.text().to_string().contains("</div>"),
        "the closer is not quote content: {}",
        quote.text()
    );

    let lazy = "- <div>\n\n  > a\n   </div>\n";
    let node = parse_blocks(lazy);
    assert_eq!(node.text().to_string(), lazy, "parser must remain lossless");
    let quote = first_of(&node, SyntaxKind::BLOCK_QUOTE).expect("quote in the item");
    assert!(
        quote.text().to_string().contains("</div>"),
        "one space past the content column stays lazy quote content: {}",
        quote.text()
    );
}

/// The closer ends a *nested* item's run with no blockquote to supply
/// the footer's blank, so the table degrades: `pandoc -f markdown -t
/// native` puts a Para, not a Table, in the nested item. (Contrast the
/// quoted shape above, where the quote's reparse appends the blank and
/// the Table survives.)
#[test]
fn nested_item_table_degrades_at_the_html_closer() {
    let input = "- <div>\n\n  - sub\n\n    A    B\n    --- ---\n    x    y\n  </div>\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::SIMPLE_TABLE).is_none(),
        "the contiguous run end fails the footer rule"
    );
    let para = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::PLAIN)
        .find(|p| p.text().to_string().contains("x    y"))
        .expect("degraded paragraph in the nested item");
    assert!(
        !para.text().to_string().contains("</div>"),
        "the closer is not paragraph content: {}",
        para.text()
    );
}

/// `pandoc -f markdown -t native`: `- e | f` is a sibling item, never a
/// pipe-table row.
#[test]
fn pipe_table_in_a_list_item_ends_at_a_sibling_marker() {
    let input = "- x\n\n  a | b\n  ---|---\n  c | d\n- e | f\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let list = node.children().next().expect("list");
    assert_eq!(list.kind(), SyntaxKind::LIST);
    let items: Vec<_> = list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    assert_eq!(items.len(), 2, "the sibling item survives the table scan");
    let table = first_of(&items[0], SyntaxKind::PIPE_TABLE).expect("table in item 1");
    assert!(
        !table.text().to_string().contains("e | f"),
        "the sibling marker is not a table row: {}",
        table.text()
    );
}

/// A marker at the item's content column is nested-list content, which
/// the run collects like any other line: `pandoc -f markdown -t native`
/// makes `  - e | f` a table row (`- e` / `f` cells).
#[test]
fn nested_list_marker_with_pipes_stays_a_pipe_table_row() {
    let input = "- x\n\n  a | b\n  ---|---\n  c | d\n  - e | f\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::PIPE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("- e | f"),
        "the nested marker is collected as a row: {}",
        table.text()
    );
}

/// A marker past the 3-column tolerance but short of the content column
/// is a lazy continuation: `pandoc -f markdown -t native` collects
/// `    - e | f` under `10.  item` (content column 5) as a table row.
#[test]
fn list_marker_in_the_lazy_band_stays_a_pipe_table_row() {
    let input = "10.  item\n\n     a | b\n     ---|---\n     c | d\n    - e | f\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::PIPE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("- e | f"),
        "the lazy marker is collected as a row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: `[^2]: e | f` opens the second note;
/// the first note's table must not claim it as a row.
#[test]
fn pipe_table_in_a_footnote_body_ends_at_a_note_marker() {
    let input = "[^1]: x\n\n    a | b\n    ---|---\n    c | d\n[^2]: e | f\n\nx[^1][^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::PIPE_TABLE).expect("table in the note body");
    assert!(
        !table.text().to_string().contains("[^2]"),
        "the new note marker is not a table row: {}",
        table.text()
    );
    let notes: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
        .collect();
    assert_eq!(notes.len(), 2, "the second note survives the table scan");
}

/// `pandoc -f markdown -t native`: `- next` is a sibling item after the
/// grid table, never a row.
#[test]
fn grid_table_in_a_list_item_ends_at_a_sibling_marker() {
    let input = "- x\n\n  +---+---+\n  | a | b |\n  +---+---+\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let list = node.children().next().expect("list");
    assert_eq!(list.kind(), SyntaxKind::LIST);
    let items: Vec<_> = list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    assert_eq!(items.len(), 2, "the sibling item survives the table scan");
    let table = first_of(&items[0], SyntaxKind::GRID_TABLE).expect("table in item 1");
    assert!(
        !table.text().to_string().contains("next"),
        "the sibling marker is not a table row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: the closing `:::` ends the div, never
/// a grid row.
#[test]
fn grid_table_in_a_fenced_div_ends_at_the_closer() {
    let input = "::: note\n+---+---+\n| a | b |\n+---+---+\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::GRID_TABLE).expect("table in the div");
    assert!(
        !table.text().to_string().contains(":::"),
        "the div closer is not a table row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: one Table, first-column cell RowSpan 2.
#[test]
fn rowspan_partial_separator_stays_in_the_grid_table() {
    let input = "+---+---+\n| c | d |\n+   +---+\n| e | f |\n+---+---+\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        child_kinds(&node),
        vec![SyntaxKind::GRID_TABLE],
        "the partial separator must not end the table"
    );
}

/// `pandoc -f markdown -t native`: one Table, last-column cell RowSpan 2.
/// The continuing column's right edge may show `|` instead of `+`.
#[test]
fn rowspan_partial_separator_may_end_with_a_pipe() {
    let input = "+---+---+\n| a | b |\n+---+   |\n| c |   |\n+---+---+\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        child_kinds(&node),
        vec![SyntaxKind::GRID_TABLE],
        "the pipe-edged partial separator must not end the table"
    );
}

/// `pandoc -f markdown -t native`: BlockQuote [Table ...] — one table.
#[test]
fn rowspan_partial_separator_in_a_blockquote_stays_in_the_table() {
    let input = "> +---+---+\n> | c | d |\n> +   +---+\n> | e | f |\n> +---+---+\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = node.children().next().expect("blockquote");
    assert_eq!(quote.kind(), SyntaxKind::BLOCK_QUOTE);
    let table = first_of(&quote, SyntaxKind::GRID_TABLE).expect("table in the quote");
    assert!(
        table.text().to_string().contains("| e | f |"),
        "the table keeps its tail rows: {}",
        table.text()
    );
    assert!(
        first_of(&quote, SyntaxKind::LIST).is_none(),
        "the partial separator must not open a list"
    );
}

/// `pandoc -f markdown -t native`: BulletList [[Table ...]] — one table.
#[test]
fn rowspan_partial_separator_in_a_list_item_stays_in_the_table() {
    let input = "- +---+---+\n  | c | d |\n  +   +---+\n  | e | f |\n  +---+---+\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let item = first_of(&node, SyntaxKind::LIST_ITEM).expect("list item");
    let table = first_of(&item, SyntaxKind::GRID_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("| e | f |"),
        "the table keeps its tail rows: {}",
        table.text()
    );
    assert!(
        first_of(&item, SyntaxKind::LIST).is_none(),
        "the partial separator must not open a nested list"
    );
}

/// `pandoc -f markdown -t native`: a *dedented* partial separator is a
/// sibling `+` list item, not a table row — the item's line run ends
/// within the list-start tolerance, and the table stays truncated. Pins
/// that the container terminator keeps priority over the partial-
/// separator classification.
#[test]
fn dedented_partial_separator_still_ends_the_list_item() {
    let input = "- +---+---+\n  | a | b |\n+   +---+\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let list = node.children().next().expect("list");
    assert_eq!(list.kind(), SyntaxKind::LIST);
    let items: Vec<_> = list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    assert_eq!(items.len(), 2, "the dedented line opens a sibling item");
}

/// `pandoc -f markdown -t native`: the `:::` closes the div; the
/// headerless table finds no closer inside it and degrades to
/// HorizontalRule + Para (the trailing separator sits outside the div).
#[test]
fn multiline_table_in_a_fenced_div_ends_at_the_closer() {
    let input = "::: note\n----- -----\nc     d\n:::\n\n----- -----\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::MULTILINE_TABLE).is_none()
            && first_of(&node, SyntaxKind::SIMPLE_TABLE).is_none(),
        "no table crosses the div closer"
    );
    let div = first_of(&node, SyntaxKind::FENCED_DIV).expect("div");
    assert!(
        div.text().to_string().ends_with(":::\n"),
        "the div closes at its own fence: {}",
        div.text()
    );
}

/// `pandoc -f markdown -t native`: the run ends at `- next`, so the
/// full-width shape finds no closer; the truncated run reparses as a
/// single-column table closed by the column separator, with `x y` left
/// over as a paragraph.
#[test]
fn multiline_table_in_a_list_item_ends_at_a_sibling_marker() {
    let input =
        "- x\n\n  -----------\n  a     b\n  ----- -----\n  x     y\n- next\n  -----------\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let list = node.children().next().expect("list");
    let items: Vec<_> = list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    assert_eq!(items.len(), 2, "the sibling item survives the table scan");
    assert!(
        !items[0].text().to_string().contains("next"),
        "the sibling marker stays outside item 1: {}",
        items[0].text()
    );
    let table = first_of(&items[0], SyntaxKind::SIMPLE_TABLE).expect("truncated table in item 1");
    assert!(
        table.text().to_string().contains("a     b") && !table.text().to_string().contains("x"),
        "the truncated run reparses as the single-column table: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: a raw blank ends the blockquote (only
/// a `>`-marked blank is an interior row separator), so the table cannot
/// close in the next quote; both quotes hold HorizontalRule (+ Para).
#[test]
fn multiline_table_does_not_cross_a_blockquote_blank() {
    let input = "> ----- -----\n> a     b\n\n> ----- -----\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::MULTILINE_TABLE).is_none()
            && first_of(&node, SyntaxKind::SIMPLE_TABLE).is_none(),
        "no table crosses the raw blank"
    );
    let quotes: Vec<_> = node
        .children()
        .filter(|n| n.kind() == SyntaxKind::BLOCK_QUOTE)
        .collect();
    assert_eq!(quotes.len(), 2, "the raw blank splits the quotes");
}

/// `pandoc -f markdown -t native`: with no div open, `:::` is an
/// ordinary row, not a boundary. (The shape lands on the headerless
/// simple-table path — the multiline reading wants blank-separated rows
/// — but the cell structure matches pandoc's either way.)
#[test]
fn unmatched_div_fence_stays_a_multiline_row() {
    let input = "----- -----\na     b\n:::\n----- -----\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table");
    assert!(
        table.text().to_string().contains(":::"),
        "the unmatched fence is collected as a row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: same at the single-column shape — the
/// old scope guard rejected the reinterpretation on any fence, but an
/// unmatched `:::` is just a row there too.
#[test]
fn unmatched_div_fence_stays_a_single_column_row() {
    let input = "p\n\n----\na\n\n:::\n\nb\n----\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::MULTILINE_TABLE).expect("single-column table");
    assert!(
        table.text().to_string().contains(":::"),
        "the unmatched fence is collected as a row: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: a `::: nested` opener inside an open
/// div is a row, not a boundary — only closers end the run.
#[test]
fn nested_div_opener_stays_a_single_column_row() {
    let input = "::: note\n----\na\n\n::: nested\n\nb\n----\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::MULTILINE_TABLE).expect("single-column table");
    assert!(
        table.text().to_string().contains("::: nested"),
        "the nested opener is collected as a row: {}",
        table.text()
    );
    assert!(
        !table.text().to_string().ends_with(":::\n"),
        "the div's own closer stays outside the table: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: a sibling `- --- ---` is a list
/// start, so it cannot serve as the closing column separator peeked
/// after a blank; the headerless shape degrades to HorizontalRule +
/// Para.
#[test]
fn sibling_marker_is_no_multiline_closer() {
    let input = "- x\n\n  --- ---\n  a   b\n\n- --- ---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::MULTILINE_TABLE).is_none()
            && first_of(&node, SyntaxKind::SIMPLE_TABLE).is_none(),
        "the sibling marker is not a closing separator"
    );
}

/// `pandoc -f markdown -t native`: `[^2]: z` opens the second note; the
/// caption is `cap` alone.
#[test]
fn caption_after_table_ends_at_a_note_marker() {
    let input =
        "[^1]: q\n\n    --- ---\n    a   b\n    --- ---\n\n    : cap\n[^2]: z\n\nx[^1][^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    assert!(
        !caption.text().to_string().contains("[^2]"),
        "the new note marker is not caption content: {}",
        caption.text()
    );
    let notes = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
        .count();
    assert_eq!(notes, 2, "the second note survives the caption scan");
}

/// `pandoc -f markdown -t native`: `- next` is a sibling item. (Pandoc
/// additionally drops the caption on this contiguous shape — documented
/// divergence, see the section comment.)
#[test]
fn caption_after_table_ends_at_a_sibling_marker() {
    let input = "- q\n\n  --- ---\n  a   b\n  --- ---\n\n  : cap\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let list = node.children().next().expect("list");
    let items = list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .count();
    assert_eq!(items, 2, "the sibling item survives the caption scan");
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    assert!(
        !caption.text().to_string().contains("next"),
        "the sibling marker is not caption content: {}",
        caption.text()
    );
}

/// `pandoc -f markdown -t native`: with no div open, a `:::` line is
/// caption content (`cap` SoftBreak `:::`), not a boundary.
#[test]
fn unmatched_fence_stays_caption_content() {
    let input = "--- ---\na   b\n--- ---\n\n: cap\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    assert!(
        caption.text().to_string().contains(":::"),
        "the unmatched fence is collected as caption content: {}",
        caption.text()
    );
}

/// `pandoc -f markdown -t native`: with the div open its closer still
/// ends the caption run, and the div closes.
#[test]
fn div_closer_still_ends_the_caption() {
    let input = "::: note\n--- ---\na   b\n--- ---\n\n: cap\n:::\n\nafter\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    assert!(
        !caption.text().to_string().contains(":::"),
        "the open div's closer is not caption content: {}",
        caption.text()
    );
    let div = first_of(&node, SyntaxKind::FENCED_DIV).expect("div");
    assert!(
        !div.text().to_string().contains("after"),
        "the div closes at its fence: {}",
        div.text()
    );
}

/// `pandoc -f markdown -t native`: a `::: nested` opener inside an open
/// div is caption content; the run ends at the first bare `:::` closer
/// (caption = `cap ::: nested z`).
#[test]
fn nested_opener_stays_caption_content() {
    let input = "::: note\n--- ---\na   b\n--- ---\n\n: cap\n::: nested\nz\n:::\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    let text = caption.text().to_string();
    assert!(
        text.contains("::: nested") && text.contains("z"),
        "the nested opener is caption content: {text}",
    );
    assert!(
        !text.trim_end().ends_with(":::"),
        "the caption stops at the first bare closer: {text}",
    );
}

/// `pandoc -f markdown -t native`: Div \[Table\] — the closer directly
/// before `:::` completes the table.
#[test]
fn table_closer_directly_before_div_closer_completes_the_table() {
    let input = "::: note\n--- ---\nx   y\n--- ---\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the div");
    assert!(
        !table.text().to_string().contains(":::"),
        "the div closer stays outside the table: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: two items, the first holding the
/// table.
#[test]
fn table_closer_directly_before_sibling_marker_completes_the_table() {
    let input = "- q\n\n  --- ---\n  x   y\n  --- ---\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in item 1");
    assert!(
        !table.text().to_string().contains("next"),
        "the sibling marker stays outside the table: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: two notes, the first holding the
/// table.
#[test]
fn table_closer_directly_before_note_marker_completes_the_table() {
    let input = "[^1]: q\n\n    --- ---\n    x   y\n    --- ---\n[^2]: z\n\na[^1][^2]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in note 1");
    assert!(
        !table.text().to_string().contains("[^2]"),
        "the new note marker stays outside the table: {}",
        table.text()
    );
}

/// `pandoc -f markdown -t native`: the unmatched fence is caption
/// content (caption `cap ::: mid`). Regression pin for the losslessness
/// break: the caption bytes were dropped and the table emitted twice.
#[test]
fn backward_caption_scan_collects_an_unmatched_fence() {
    let input = ": cap\n:::\nmid\n\n--- ---\nx   y\n--- ---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    let text = caption.text().to_string();
    assert!(
        text.contains(":::") && text.contains("mid"),
        "the fence and its tail are caption content: {text}",
    );
}

/// `pandoc -f markdown -t native`: `: cap` directly under the div's
/// opening fence captions the table (the fence is structure, not a
/// paragraph the colon line could define).
#[test]
fn caption_directly_under_a_div_opener_captions_the_table() {
    let input = "::: note\n: cap\n\n--- ---\nx   y\n--- ---\n:::\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    assert!(
        caption.text().to_string().contains("cap"),
        "the caption attaches: {}",
        caption.text()
    );
}

/// `pandoc -f markdown -t native`: `: cap` directly under a *closed*
/// div's `:::` captions the table below it.
#[test]
fn caption_directly_under_a_closed_div_captions_the_table() {
    let input = "::: note\nz\n:::\n: cap\n\n--- ---\nx   y\n--- ---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let caption = first_of(&node, SyntaxKind::TABLE_CAPTION).expect("caption");
    assert!(
        caption.text().to_string().contains("cap"),
        "the caption attaches: {}",
        caption.text()
    );
}

/// `pandoc -f markdown -t native`: `BulletList [[Table …]]`. The
/// marker-line lift used to gate on a leading `|`/`+`/`:` byte, so the
/// pipe-less header fell through to the item's `Plain`.
#[test]
fn leading_pipeless_pipe_table_on_a_list_item_marker_line_lifts() {
    let input = "- a | b\n  ---|---\n  c | d\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let item = first_of(&node, SyntaxKind::LIST_ITEM).expect("list item");
    assert_eq!(
        child_kinds(&item)
            .into_iter()
            .filter(|k| *k == SyntaxKind::PIPE_TABLE)
            .count(),
        1,
        "the table is a direct item child, not `Plain` text: {:?}",
        child_kinds(&item)
    );
    assert_eq!(header_cells(&item), vec!["a", "b"]);
}

/// The same shape with the table opening on the item's first
/// continuation line: `pandoc -f markdown -t native` is
/// `BulletList [[Table …]]` there too.
#[test]
fn leading_pipeless_pipe_table_under_a_bare_list_marker_lifts() {
    let input = "-\n  a | b\n  ---|---\n  c | d\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let item = first_of(&node, SyntaxKind::LIST_ITEM).expect("list item");
    assert!(
        first_of(&item, SyntaxKind::PIPE_TABLE).is_some(),
        "the table is lifted out of the item's buffered text: {:?}",
        child_kinds(&item)
    );
}

/// A bare `[^1]:` marker line makes the next line the first line of the
/// note's reparsed body, so pandoc's `blank_before_header` /
/// indented-code rules measure from there. `pandoc -f markdown -t
/// native`: `Note [Table …]`.
#[test]
fn pipe_table_opening_a_bare_marker_footnote_body_is_a_table() {
    let input = "[^1]:\n    a | b\n    ---|---\n    c | d\n\nx[^1]\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let note = first_of(&node, SyntaxKind::FOOTNOTE_DEFINITION).expect("note");
    assert!(
        first_of(&note, SyntaxKind::PIPE_TABLE).is_some(),
        "the note body opens with a table: {:?}",
        child_kinds(&note)
    );
}

/// Same rule, other constructs: an ATX heading and an indented code
/// block open a bare-marker note body. `pandoc -f markdown -t native`
/// gives `Note [Header …]` and `Note [CodeBlock …]`.
#[test]
fn bare_marker_footnote_body_starts_a_fresh_block_context() {
    let heading = parse_blocks("[^1]:\n    # head\n\nx[^1]\n");
    assert!(
        first_of(&heading, SyntaxKind::HEADING).is_some(),
        "`# head` is a heading, not lazy paragraph text"
    );

    let code = parse_blocks("[^1]:\n        code\n\nx[^1]\n");
    assert!(
        first_of(&code, SyntaxKind::CODE_BLOCK).is_some(),
        "8 columns is 4 past the note's content column: indented code"
    );
}

/// The non-bare form is unchanged: the marker line's text opens a
/// paragraph, so `# head` under it is lazy continuation
/// (`pandoc -f markdown -t native`: `Note [Para [y, SoftBreak, #, head]]`).
#[test]
fn footnote_body_after_marker_line_text_keeps_lazy_continuation() {
    let node = parse_blocks("[^1]: y\n    # head\n\nx[^1]\n");
    assert!(
        first_of(&node, SyntaxKind::HEADING).is_none(),
        "the heading cannot interrupt the marker line's paragraph"
    );
}

/// `pandoc -f markdown -t native` on `[^1]: a | b` + a 4-column-indented
/// delimiter and row: `Note [Table …]`. The pipe table cannot start on the
/// marker line because its delimiter row is indented past
/// `nonindentSpaces`, so the note claims the line — and the note's own
/// body reparse (which sees the delimiter dedented to its content column)
/// then reads the body as the table. See the marker-line dispatch in
/// `handle_footnote_open_effect` and the `footnote_marker_line` tests.
#[test]
fn indented_delimiter_leaves_the_note_marker_to_the_note() {
    let input = "x[^1]\n\n[^1]: a | b\n    --- | ---\n    c | d\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let note = first_of(&node, SyntaxKind::FOOTNOTE_DEFINITION).expect("footnote definition");
    assert!(
        note.text().to_string().contains("--- | ---"),
        "the whole body stays inside the note: {}",
        note.text()
    );
    let table = first_of(&node, SyntaxKind::PIPE_TABLE).expect("note body should be a table");
    assert!(
        table
            .ancestors()
            .any(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION),
        "the table belongs to the note, not the top level"
    );
    assert!(
        !table.text().to_string().contains("[^1]:"),
        "no table claims the marker line as its header row"
    );
}

/// `pandoc -f markdown -t native` on `a | b` + `    --- | ---`: a single
/// `Para`, because `pipeBreak`'s `nonindentSpaces` fails on the 4-column
/// delimiter row.
#[test]
fn pipe_delimiter_past_nonindent_spaces_is_not_a_table() {
    let input = "a | b\n    --- | ---\n    c | d\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "the indented delimiter row cannot open a table"
    );
}

/// A tab is 4 columns, so it fails the same bound
/// (`pandoc -f markdown -t native`: a `Para`).
#[test]
fn tab_indented_pipe_delimiter_is_not_a_table() {
    let node = parse_blocks("a | b\n\t--- | ---\n");
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "a tab is 4 columns: past `nonindentSpaces`"
    );
}

/// Three columns is still inside the tolerance
/// (`pandoc -f markdown -t native`: a `Table`).
#[test]
fn pipe_delimiter_within_nonindent_spaces_stays_a_table() {
    let input = "a | b\n   --- | ---\n   c | d\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_some(),
        "3 columns is within `nonindentSpaces`"
    );
}

/// The delimiter bound is Pandoc's alone. GFM grows its table out of an
/// open paragraph, and a paragraph's continuation lines carry no indent
/// bound: `pandoc -f gfm -t native` on the same input reads the `Table`.
#[test]
fn gfm_keeps_a_table_with_an_indented_pipe_delimiter() {
    let input = "a | b\n    --- | ---\n    c | d\n";
    let node = parse_blocks_gfm(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_some(),
        "GFM has no `pipeBreak` indent bound"
    );
}

/// The header row's bound is not dialect-specific: 4 columns is an
/// indented code block under GFM too (`pandoc -f gfm -t native`:
/// `CodeBlock`).
#[test]
fn gfm_pipe_header_past_nonindent_spaces_is_indented_code() {
    let node = parse_blocks_gfm("x\n\n    a | b\n    --- | ---\n");
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "an indented header opens indented code, not a table"
    );
    assert!(
        first_of(&node, SyntaxKind::CODE_BLOCK).is_some(),
        "the indented block is code"
    );
}

/// The header row carries the same bound: with both lines indented 4
/// columns `pandoc -f markdown -t native` gives a `CodeBlock`, and with
/// only the header indented it gives `CodeBlock` + `Para`.
#[test]
fn pipe_header_past_nonindent_spaces_is_indented_code() {
    let both = parse_blocks("x\n\n    a | b\n    --- | ---\n");
    assert!(
        first_of(&both, SyntaxKind::PIPE_TABLE).is_none(),
        "an indented header opens indented code, not a table"
    );
    assert!(
        first_of(&both, SyntaxKind::CODE_BLOCK).is_some(),
        "the indented block is code"
    );

    let header_only = parse_blocks("x\n\n    a | b\n--- | ---\n");
    assert!(
        first_of(&header_only, SyntaxKind::PIPE_TABLE).is_none(),
        "the delimiter row cannot reach back into indented code"
    );
}

/// The bound is measured in the container's frame, which is pandoc's: a
/// blockquote body is reparsed with its `> ` peeled, so `>     ---|---`
/// is 4 columns there (`pandoc -f markdown -t native`:
/// `BlockQuote [Para …]`).
#[test]
fn indented_pipe_delimiter_in_a_blockquote_is_not_a_table() {
    let input = "> a | b\n>     --- | ---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "the delimiter row is 4 columns inside the quote"
    );
}

/// `pandoc -f markdown -t native` reads one, two, and three dashes per cell
/// as the same `Table`.
#[test]
fn short_pipe_delimiter_rows_are_tables() {
    for delim in [
        "-|-", "--|--", "---|---", "- | -", "-- | --", ":-|-:", "--|--:",
    ] {
        let input = format!("a | b\n{delim}\nc | d\n");
        let node = parse_blocks(&input);

        assert_eq!(
            node.text().to_string(),
            input,
            "parser must remain lossless on {delim:?}"
        );
        assert!(
            first_of(&node, SyntaxKind::PIPE_TABLE).is_some(),
            "{delim:?} is a delimiter row"
        );
    }
}

/// `pandoc -f markdown -t native` on `- a | b` + `  - | -`:
/// `BulletList [[Table …]]`. Before the gate the `- ` opened a nested bullet
/// whose content was a line block (`| -`).
#[test]
fn marker_shaped_delimiter_row_completes_the_item_table() {
    let input = "- a | b\n  - | -\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let item = first_of(&node, SyntaxKind::LIST_ITEM).expect("list item");
    let table = first_of(&item, SyntaxKind::PIPE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("- | -"),
        "the marker-shaped line is the delimiter row: {}",
        table.text()
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        1,
        "no nested list opens on the delimiter row"
    );
}

/// The lift reaches through a quote that itself opened on a list marker
/// line. `pandoc 3.10.2 -f markdown -t native` on `- > - a | b` +
/// `  >   - | -`: `BulletList [[BlockQuote [BulletList [[Table …]]]]]`.
///
/// `- > - x` has its own recursion in `finish_list_item_with_optional_nested`
/// rather than going through `BqDispatch`, and that path used to frame the
/// inner item in raw-line columns while every continuation line is measured
/// after the quote prefix is stripped. The item was closed as "dedented" on
/// the delimiter row, so the lift's probe never ran and the row opened a
/// nested list whose content was a line block --- which the pandoc-AST
/// projector then dropped entirely.
#[test]
fn marker_shaped_delimiter_row_completes_a_quoted_nested_item_table() {
    let input = "- > - a | b\n  >   - | -\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let quote = first_of(&node, SyntaxKind::BLOCK_QUOTE).expect("block quote");
    let table = first_of(&quote, SyntaxKind::PIPE_TABLE).expect("table inside the quote");
    assert!(
        table.text().to_string().contains("- | -"),
        "the marker-shaped line is the delimiter row: {}",
        table.text()
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        2,
        "only the outer and quoted lists open, none on the delimiter row"
    );
    assert!(
        first_of(&node, SyntaxKind::LINE_BLOCK).is_none(),
        "the delimiter row is not a line block: {node:#?}"
    );
}

/// Same shape one quote deeper. `pandoc 3.10.2 -f markdown -t native` on
/// `- > > - a | b` + `  > >   - | -`:
/// `BulletList [[BlockQuote [BlockQuote [BulletList [[Table …]]]]]]`.
#[test]
fn marker_shaped_delimiter_row_completes_a_twice_quoted_nested_item_table() {
    let input = "- > > - a | b\n  > >   - | -\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::BLOCK_QUOTE)
            .count(),
        2,
        "both quote markers open a quote: {node:#?}"
    );
    let table = first_of(&node, SyntaxKind::PIPE_TABLE).expect("table inside the quotes");
    assert!(
        table.text().to_string().contains("- | -"),
        "the marker-shaped line is the delimiter row: {}",
        table.text()
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        2,
        "only the outer and quoted lists open, none on the delimiter row"
    );
}

/// The table keeps growing past its delimiter row, and a following sibling
/// marker still ends the item (`pandoc -f markdown -t native`: a two-item
/// `BulletList` whose first item is the `Table`).
#[test]
fn marker_shaped_delimiter_row_keeps_its_body_rows() {
    let input = "- a | b\n  - | -\n  c | d\n- next\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::PIPE_TABLE).expect("table in item 1");
    assert!(
        table.text().to_string().contains("c | d"),
        "the body row joins the table: {}",
        table.text()
    );
    assert!(
        !table.text().to_string().contains("next"),
        "the sibling marker is not a row: {}",
        table.text()
    );
}

/// The gate is bounded to the marker line, which is where pandoc's reparse
/// puts the table: with a line of prose first, the table cannot interrupt
/// the open paragraph and `  - | -` really is a nested bullet
/// (`pandoc -f markdown -t native`: `Plain [x, SoftBreak, a | b]` then a
/// nested `BulletList`).
#[test]
fn delimiter_row_under_a_paragraph_line_stays_a_nested_marker() {
    let input = "- x\n  a | b\n  - | -\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "no table interrupts the item's paragraph"
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        2,
        "the nested list opens"
    );
}

#[test]
fn non_delimiter_marker_line_still_opens_a_nested_list() {
    let node = parse_blocks("- a | b\n  + | +\n");
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "`+ | +` is no delimiter row"
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        2,
        "the nested list opens"
    );
}

/// GFM has no reparse: `cmark-gfm` (via `pandoc -f gfm -t native`) opens the
/// nested list, since its table extension grows out of a paragraph rather
/// than out of the item's collected lines.
#[test]
fn gfm_keeps_the_nested_list_on_a_marker_shaped_delimiter_row() {
    let node = parse_blocks_gfm("- a | b\n  - | -\n");
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "GFM opens the nested list here"
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        2,
        "the nested list opens"
    );
}

/// An escaped `\|` is literal text, not a cell boundary:
/// `pandoc -f markdown -t native` on `a \| b` + `---|---` is a `Para`,
/// while the same delimiter under `a \| b | c` is a `Table` whose first
/// cell holds the literal pipe.
#[test]
fn escaped_pipe_alone_does_not_open_a_header_row() {
    let input = "a \\| b\n---|---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
        "the only pipe in the header is escaped"
    );

    let with_real_pipe = parse_blocks("a \\| b | c\n---|---\n");
    assert!(
        first_of(&with_real_pipe, SyntaxKind::PIPE_TABLE).is_some(),
        "an unescaped pipe still opens the table"
    );
    assert_eq!(
        header_cells(&with_real_pipe),
        vec!["a \\| b".to_string(), "c".to_string()],
        "the escaped pipe stays inside the first cell"
    );
}

/// `pandoc -f markdown -t native` on `a | b | c` + `---|---`: a two-column
/// `Table`, with the surplus header cell and the surplus body cell dropped.
#[test]
fn pipe_table_columns_come_from_the_delimiter_row() {
    let input = "a | b | c\n---|---\n1 | 2 | 3\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_some(),
        "the surplus header cell does not stop the table"
    );

    let native = crate::to_pandoc_ast(&node);
    assert_eq!(
        native.matches("AlignDefault , ColWidthDefault").count(),
        2,
        "the delimiter row's two cells are the column count: {native}"
    );
    assert!(
        !native.contains("Str \"c\""),
        "the surplus header cell is dropped: {native}"
    );
    assert!(
        !native.contains("Str \"3\""),
        "the surplus body cell is dropped: {native}"
    );
}

#[test]
fn a_longer_delimiter_row_pads_the_header() {
    let input = "a | b\n---|---|---\n1 | 2 | 3\n";
    let node = parse_blocks(input);

    let native = crate::to_pandoc_ast(&node);
    assert_eq!(
        native.matches("AlignDefault , ColWidthDefault").count(),
        3,
        "the delimiter row's three cells are the column count: {native}"
    );
    assert!(
        native.contains("Str \"3\""),
        "the body row fills the third column: {native}"
    );
}

/// A single-cell delimiter row is a single-column table, however wide the
/// header is (`pandoc -f markdown -t native` on `| a | b |` + `- |`).
#[test]
fn a_one_cell_delimiter_row_is_a_one_column_table() {
    let input = "| a | b |\n- |\n| 1 | 2 |\n";
    let node = parse_blocks(input);

    let native = crate::to_pandoc_ast(&node);
    assert_eq!(
        native.matches("AlignDefault , ColWidthDefault").count(),
        1,
        "the delimiter row has one cell: {native}"
    );
}

/// Pandoc puts no ceiling on the header's surplus: seven header cells over
/// a two-cell delimiter row is still a two-column `Table`, where panache
/// used to cut off at twice the delimiter's count and read a `Para`.
#[test]
fn header_surplus_has_no_upper_bound() {
    let input = "a|b|c|d|e|f|g\n---|---\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_some(),
        "no ceiling on the surplus"
    );
}

/// GFM requires the counts to match exactly; a mismatch is not a table at
/// all (`pandoc -f gfm -t html` renders one `<p>`), in either direction.
#[test]
fn gfm_requires_an_exact_column_match() {
    for input in [
        "a|b|c\n---|---\n1|2|3\n",
        "a|b\n---|---|---\n1|2|3\n",
        "a|b|c|d|e|f|g\n---|---\n",
    ] {
        let node = parse_blocks_gfm(input);

        assert_eq!(
            node.text().to_string(),
            input,
            "parser must remain lossless on {input:?}"
        );
        assert!(
            first_of(&node, SyntaxKind::PIPE_TABLE).is_none(),
            "gfm refuses a mismatched delimiter row: {input:?}"
        );
    }
}

#[test]
fn gfm_still_reads_a_matched_pipe_table() {
    let node = parse_blocks_gfm("a|b\n---|---\n1|2\n");
    assert!(
        first_of(&node, SyntaxKind::PIPE_TABLE).is_some(),
        "a matched delimiter row is a gfm table"
    );
}

/// A marker-shaped delimiter row narrower than its header is still the
/// delimiter row. `pandoc -f markdown -t native` on `- | a | b |` + `  - |`:
/// `BulletList [[Table …]]`, one column wide, holding only `a`. This needed
/// the column-exact form while the formatter re-emitted a column count of its
/// own; it now leaves a surplus-cell table verbatim, so the shape round-trips.
#[test]
fn marker_shaped_delimiter_row_may_be_narrower_than_its_header() {
    let input = "- | a | b |\n  - |\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let item = first_of(&node, SyntaxKind::LIST_ITEM).expect("list item");
    let table = first_of(&item, SyntaxKind::PIPE_TABLE).expect("table in the item");
    assert!(
        table.text().to_string().contains("- |"),
        "the marker-shaped line is the delimiter row: {}",
        table.text()
    );
    assert_eq!(
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST)
            .count(),
        1,
        "no nested list opens on the delimiter row"
    );

    let native = crate::to_pandoc_ast(&node);
    assert_eq!(
        native.matches("AlignDefault , ColWidthDefault").count(),
        1,
        "the delimiter row declares one column: {native}"
    );
}
