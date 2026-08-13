//! Blocks opened by a non-bare footnote marker line's own text.
//!
//! Pandoc's `noteBlock` collects a note's raw lines and reparses them from
//! scratch, so the text after `[^1]: ` sits at a fresh block context and can
//! open a table, thematic break, list, blockquote, or fenced code block. The
//! collected raw keeps the column the marker's trailing space occupied, which
//! defeats margin-anchored constructs: an ATX heading (`[^1]: # h`), a line
//! block, or a fenced div stays lazy paragraph text — even with
//! `blank_before_header` disabled (all verified against
//! `pandoc -f markdown -t native`).

use super::helpers::{find_first, parse_blocks};
use crate::syntax::SyntaxKind;

fn footnote_body(input: &str) -> crate::syntax::SyntaxNode {
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    find_first(&tree, SyntaxKind::FOOTNOTE_DEFINITION).expect("should find footnote definition")
}

#[test]
fn marker_line_table_opens_block() {
    // Pandoc: `Note [Table …]` — the separator line makes the marker line a
    // header row.
    let input = "x[^1]\n\n[^1]: a | b\n    ---|---\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::PIPE_TABLE).is_some(),
        "marker-line `a | b` + separator should open a pipe table, got: {body:#?}"
    );
}

#[test]
fn marker_line_hr_opens_block() {
    // Pandoc: `Note [HorizontalRule]`.
    let input = "x[^1]\n\n[^1]: ***\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::HORIZONTAL_RULE).is_some(),
        "marker-line `***` should open a thematic break, got: {body:#?}"
    );
}

#[test]
fn marker_line_list_opens_block() {
    // Pandoc: `Note [BulletList [[Plain [Str "li"]]]]`.
    let input = "x[^1]\n\n[^1]: - li\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::LIST).is_some(),
        "marker-line `- li` should open a list, got: {body:#?}"
    );
}

#[test]
fn marker_line_list_takes_lazy_continuation() {
    // Pandoc collects the unindented follow-up into the note's raw
    // (`rawLine` takes any non-blank, non-marker line), so it lazily
    // continues the list item.
    let input = "x[^1]\n\n[^1]: - li\nlazy\n";
    let body = footnote_body(input);
    let list = find_first(&body, SyntaxKind::LIST).expect("list should open");
    assert!(
        list.text().to_string().contains("lazy"),
        "unindented follow-up line should lazily continue the list item, got: {body:#?}"
    );
}

#[test]
fn marker_line_blockquote_opens_block() {
    // Pandoc: `Note [BlockQuote [Para [Str "q"]]]`.
    let input = "x[^1]\n\n[^1]: > q\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::BLOCK_QUOTE).is_some(),
        "marker-line `> q` should open a blockquote, got: {body:#?}"
    );
}

#[test]
fn bare_marker_body_blockquote_opens_block() {
    // Pandoc: `Note [BlockQuote [Para [Str "q"]]]` — the body of a bare
    // marker is reparsed from scratch, so its first line is quote-startable
    // even though the marker line above is not blank. Companion of the
    // marker-line case below; without it the two formatting styles ping-pong
    // (`[^1]: > q` ⇄ `[^1]:` + indented quote) and idempotency breaks.
    let input = "x[^1]\n\n[^1]:\n    > q\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::BLOCK_QUOTE).is_some(),
        "bare-marker body `> q` should open a blockquote, got: {body:#?}"
    );
}

#[test]
fn marker_line_fence_opens_block() {
    // Pandoc: `Note [CodeBlock ("",[],[]) "code"]`.
    let input = "x[^1]\n\n[^1]: ```\n    code\n    ```\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::CODE_BLOCK).is_some(),
        "marker-line fence should open a code block, got: {body:#?}"
    );
}

#[test]
fn marker_line_atx_heading_stays_lazy() {
    // Pandoc: `Note [Para [Str "#", Space, Str "h"]]` — the marker's trailing
    // space indents the collected raw one column, and pandoc's ATX headers
    // are margin-anchored.
    let input = "x[^1]\n\n[^1]: # h\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::HEADING).is_none(),
        "marker-line `# h` must stay paragraph text, got: {body:#?}"
    );
    assert!(find_first(&body, SyntaxKind::PARAGRAPH).is_some());
}

#[test]
fn marker_line_setext_heading_opens_block() {
    // Pandoc: `Note [Header 1 ("h",[],[]) [Str "h"]]` — setext text tolerates
    // the one-column indent, unlike ATX.
    let input = "x[^1]\n\n[^1]: h\n    ===\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::HEADING).is_some(),
        "marker-line setext text + underline should open a heading, got: {body:#?}"
    );
}

#[test]
fn marker_line_line_block_stays_lazy() {
    // Pandoc keeps `Note [Para [Str "|", …]]` — line blocks are
    // margin-anchored like ATX headings.
    let input = "x[^1]\n\n[^1]: | line\n    | block\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::LINE_BLOCK).is_none(),
        "marker-line `| line` must stay paragraph text, got: {body:#?}"
    );
}

#[test]
fn marker_line_fenced_div_stays_lazy() {
    // Pandoc keeps `Note [Para [Str ":::", …]]` — fenced divs are
    // margin-anchored.
    let input = "x[^1]\n\n[^1]: ::: note\n    inner\n    :::\n";
    let body = footnote_body(input);
    assert!(
        find_first(&body, SyntaxKind::FENCED_DIV).is_none(),
        "marker-line `::: note` must stay paragraph text, got: {body:#?}"
    );
}

#[test]
fn marker_line_table_in_quoted_footnote_stays_lossless() {
    // The same dispatch inside a blockquote: continuation lines carry both
    // the `> ` marker and the note indent.
    let input = "x[^1]\n\n> [^1]: a | b\n>     ---|---\n";
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}
