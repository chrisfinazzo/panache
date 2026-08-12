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

// ---------------------------------------------------------------------------
// Container line runs end at a fenced-div closer
//
// pandoc collects a list item's / definition body's / blockquote's raw lines
// before parsing them as blocks, and that collection runs under
// `notFollowedByDivCloser`. So a bare `:::` inside an open div ends the run:
// the fence belongs to the div, and no table scan started inside the
// container may reach past it. The container-frame bound cannot see this --
// a closer at column 0 carries no indent to compare.

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

// ---------------------------------------------------------------------------
// Container line runs end at a new note marker
//
// pandoc collects a footnote's raw lines in `noteBlock`, whose `rawLine`
// stops at a new note marker reached by `skipNonindentSpaces >> noteMarker`
// -- at most 3 spaces in the note's *outer* frame. The fence applies to
// everything nested inside the note (its lines are part of the note's
// collected raw), but lives in `noteBlock` only: a plain list item's
// `listLine` has no such guard, and pandoc collects a stray marker there
// as ordinary content.

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

// ---------------------------------------------------------------------------
// Container line runs end at a new list start
//
// pandoc collects a list item's continuation lines with
// `listContinuationLine = notFollowedBy blankline >> notFollowedBy'
// listStart >> ...`, where `listStart` tolerates `nonindentSpaces` (at most
// 3 columns) in the frame the list was parsed in. So a marker line within
// that tolerance ends the item's run; one at the item's content column is
// nested-list content; and one in between is a lazy continuation. Known,
// deliberate divergence: when the terminator is contiguous with a headered
// simple table, pandoc's collected raw ends without the blank line the
// table's footer rule demands, so pandoc degrades the table to a
// paragraph; panache treats the terminator like a blank line and keeps the
// table (lossless and idempotent; see TODO.md).

/// `pandoc -f markdown -t native`: `- next` is a sibling item, never a
/// table row (before this fence it was sliced across the table's column
/// boundaries, reformatting to `- ne   xt`).
#[test]
fn simple_table_in_a_list_item_ends_at_a_sibling_marker() {
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
    let table = first_of(&items[0], SyntaxKind::SIMPLE_TABLE).expect("table in item 1");
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
/// BulletList after the list.
#[test]
fn list_marker_within_nonindent_tolerance_ends_the_run() {
    let input = "10.  item\n\n     A    B\n     --- ---\n     x    y\n   - sib\n";
    let node = parse_blocks(input);

    assert_eq!(
        node.text().to_string(),
        input,
        "parser must remain lossless"
    );
    let table = first_of(&node, SyntaxKind::SIMPLE_TABLE).expect("table in the item");
    assert!(
        !table.text().to_string().contains("sib"),
        "the marker is not a table row: {}",
        table.text()
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
}

// ---------------------------------------------------------------------------
// Container line runs end at an HTML closer (markdown-in-html)
//
// Inside markdown-in-html, pandoc guards its line collectors with
// `notFollowedByHtmlCloser`: a line opening with the close form of the open
// tag ends a collected run. The open tag lives in a list item's buffer
// (markdown-in-html is not a stack transition here), and the fence is
// ordering-gated at that item — its own run is not fenced (pandoc itself
// slices the closer into the item's own table), only containers pushed
// above it are. The guard's `htmlTag` skips no whitespace, so the closer
// fires at up to the item's content column of indent and not one more.
// Known, deliberate divergence: pandoc wraps the whole item body in a
// `Div` (the markdown-in-html structural lift); panache keeps the open
// and close tags as raw siblings — see the allowlist rationale for
// pandoc-conformance 0464 and TODO.md.

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

// ---------------------------------------------------------------------------
// The pipe-table row scan ends at container line-run terminators
//
// The row loop used to stop only on a blank line or a line without `|`,
// which let a container-ending line *containing* a pipe — a sibling list
// start `- e | f`, a new note marker `[^2]: e | f` — be swallowed as a
// data row. pandoc's line-run collection (`listContinuationLine`,
// `noteBlock`) ends the run before the table parser ever sees the line.
// Div closers and HTML closers carry no `|`, so the old gate already
// stopped there by accident; these pins cover the terminators the gate
// missed.

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

// ---------------------------------------------------------------------------
// The grid-table scan ends at container line-run terminators
//
// Unlike the pipe scan, no terminator can shape-match a grid line: content
// rows must open with `|`, and separator segments admit no spaces, so a
// sibling list start, note marker, div closer, or HTML closer always
// failed the grid-line checks and stopped the scan anyway. The scan still
// consults `ends_container_lines` so the stop is an invariant, not a
// coincidence of the current shapes; these pins document the behavior.

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
