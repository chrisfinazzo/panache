use super::helpers::{find_all, find_first, parse_blocks_gfm};
use crate::options::{Dialect, Extensions, Flavor, ParserOptions};
use crate::parser::Parser;
use crate::syntax::SyntaxKind;

#[test]
fn test_losslessness_basic() {
    let input = "# H1\n\n### H3\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(
        tree.text().to_string(),
        input,
        "AST must preserve exact input (lossless CST)"
    );
}

#[test]
fn test_losslessness_no_trailing_newline() {
    let input = "# Heading";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_multiple_blank_lines() {
    let input = "\n\n\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_paragraph() {
    let input = "First line\nSecond line\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_indented_code_blank_line_with_spaces() {
    let input = "    A\n        \n    B\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_div_open_with_trailing_space() {
    let input = "::: {.panel-tabset group=\"language\"} \n\n## R\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_div_open_whitespace_run_before_label() {
    // The opener emitter used to consume a single space before the label while
    // detection had trimmed the whole whitespace run, so the leftover run was
    // re-emitted as a duplicated label suffix (`:::  te` -> `::: tee`).
    for input in [
        ":::  te\nbody\n:::\n\npara\n",
        ":::: \ty\n:::\n::::\n\npara\n",
        "::::     outer\n::: inner\nbody\n:::\n::::\n",
        ":::\t{.note}\nbody\n:::\n",
    ] {
        let config = ParserOptions::default();
        let parser = Parser::new(input, &config);
        let tree = parser.parse();
        assert_eq!(tree.text().to_string(), input, "input: {input:?}");
    }
}

#[test]
fn test_losslessness_blockquote_list_continuation_lines() {
    let input = "> practical skills in:\n> \n> - Developing and integrating custom formats\n>   while reducing repetition across projects.\n> - Implementing filters to automate and streamline content\n>   transformation.\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_closing_fence_trailing_spaces() {
    let input = "````{.python}\ncity = \"Corvallis\"\n````    \n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_opening_fence_trailing_spaces() {
    let input = "```{r em-alg} \nem <- 1\n```\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_definition_first_line_trailing_spaces() {
    let input = "`repo`\n\n:   Add a link to repo:  \n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_cell_with_leading_pipe_text() {
    let input = "+--------------------------+--------------------------+\n| ``` markdown             | | Line Block             |\n| | Line Block             | |    Spaces and newlines |\n+--------------------------+--------------------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_cell_with_nbsp() {
    let input = "+--------------------------------------------+----------------+\n| `QUARTO_FIG_WIDTH` and `QUARTO_FIG_HEIGHT` | Value          |\n+--------------------------------------------+----------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_colon_definition_before_grid_table() {
    let input = "Misaligned separators in grid table \\`\\`\\`\n\n% pandoc -f markdown -t html\n:   Grid Table\n\n+-----------+---------------------------------+\n| Some text | [text]{.class1 .class2 .class3} |\n+===========+:===============================:+\n| Some text | [text]{.class1 .class2 .class3} |\n+-----------+---------------------------------+\n| Some text | [text]{.class1 .class2 .class3} |\n+-----------+---------------------------------+\n^D\n<table style=\"width:69%;\">\n<caption>Grid Table</caption>\n<colgroup>\n<col style=\"width: 25%\" />\n<col style=\"width: 44%\" />\n</colgroup>\n<tbody>\n<tr>\n<td>Some text</td>\n<td><span class=\"class1 class2 class3\">text</span></td>\n</tr>\n</tbody>\n</table> \\`\\`\\`\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_open_leading_space() {
    let input = " ```\n x\n ```\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_spanning_style_row() {
    let input = "+-----------------------------------------+-----------------------------------------+\n| Student ID                              | Name                                    |\n+:========================================+:========================================+\n| Computer Science                                                                  |\n+-----------------------------------------+-----------------------------------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_three_col_row_with_asymmetric_padding() {
    let input = "+-------------------------+---------------------------+-----------------------+\n| `scale_fill_grey()`     | `scale_colour_grey()`     | Greyscale palette     |\n+-------------------------+---------------------------+-----------------------+\n| `scale_fill_viridis_d()`| `scale_colour_viridis_d()` |  Viridis palettes    |\n+-------------------------+---------------------------+-----------------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_fenced_code_lines() {
    let input = "> ~~~ {.xml}\n> <ruby>text</ruby>\n> ~~~\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_line_block_empty_marker_line() {
    let input = "| Hello\n|\n| Goodbye\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_horizontal_rule_with_leading_spaces() {
    let input = "before\n\n  ----\n\nafter\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_atx_heading_with_attributes() {
    let input = "> ## Header attributes inside block quote {#foobar .baz key=\"val\"}\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_tex_command_attribution_line() {
    let input = "> quote line\n>\n> \\medskip\n> \\hfill---Joe Armstrong\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_wide_and_zero_width_chars() {
    let input = "+--+----+\n|魚|fish|\n+--+----+\n\n+-------+-------+\n|German |English|\n+-------+-------+\n|Auf‌lage|edition|\n+-------+-------+\n\n+-------+---------+\n|می‌خواهم|I want to|\n+-------+---------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_adjacent_tables_with_caption_between_and_following_heading() {
    let input = "| H1 | H2 |\n|----|----|\n| a  | b  |\nTable: first\n\n| J1 | J2 |\n|----|----|\n| c  | d  |\nTable: second\n\n### Exercises\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_triple_underscore_emphasis_preserves_delimiters() {
    let input = "a. ___License grant.___\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_line_with_pipe_does_not_hang() {
    // Regression: this shape previously triggered a non-progress loop by
    // misdetecting a line block from blockquote-stripped content.
    let input = "> | When dollars appear it's a sign\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_list_fenced_code_indentation() {
    let input = "> - One bullet.\n> \n>   ````\n>   ```{r, eval=TRUE}`r ''`\n>   ````\n>   ```r\n>   2 + 2\n>   ```\n>   ```\n>   ## [1] 4\n>   ```\n>   ````\n>   ```\n>   ````\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_hashpipe_block_scalar_in_list_fenced_chunk() {
    // Regression (#140): continuation metadata lines like `#| fig-alt: |`
    // must keep their original indentation in indented list contexts.
    let input = "- item\n\n    ```{r}\n    #| fig-cap: |\n    #|   A visual representation.\n    #| fig-alt: |\n    #|   Alt text.\n    plot(1:3)\n    ```\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_gfm_reference_definition_and_shortcut_link() {
    // Regression: GFM is a strict CommonMark superset, so it must recognize
    // link reference definitions and shortcut reference links. Previously
    // `gfm_defaults()` left `reference_links`/`shortcut_reference_links` off,
    // so `[argmin]: url` fell through to a paragraph where
    // `autolink_bare_uris` rewrote the bare URL into a full `[url](url)` link
    // — duplicating bytes and breaking losslessness (the formatter then
    // escaped the `[argmin]` brackets).
    let input = "[argmin]\n\n[argmin]: https://github.com/argmin-rs/argmin\n";
    let tree = parse_blocks_gfm(input);
    assert_eq!(tree.text().to_string(), input, "GFM parse must be lossless");
    assert!(
        find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).is_some(),
        "GFM should parse `[label]: url` as a REFERENCE_DEFINITION, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_multiline_table_blank_rows_and_following_captioned_simple_table() {
    let input = "Table: (\\#tab:basic-data-types) Types of variables encountered in typical data visualization scenarios.\n\n---------------------------------------------------------------------------------------------------------------------\nType of variable         Examples              Appropriate scale       Description\n------------------------ --------------------- ----------------------- ----------------------------------------------\nquantitative/numerical   1.3, 5.7, 83,         continuous              Arbitrary numerical values. These can be\ncontinuous               1.5x10^-2^                                    integers, rational numbers, or real numbers.\n \nquantitative/numerical   1, 2, 3, 4            discrete                Numbers in discrete units. These are most\ndiscrete                                                               commonly but not necessarily integers.\n                                                                       For example, the numbers 0.5, 1.0, 1.5 could\n                                                                       also be treated as discrete if intermediate\n                                                                       values cannot exist in the given dataset.\n                                                                       \nqualitative/categorical  good, fair, poor      discrete                Categories with order. These are discrete\nordered                                                                and unique categories with an order. For\n                                                                       example, \"fair\" always lies between \"good\"\n                                                                       and \"poor\". These variables are\n                                                                       also called *ordered factors*.\n\ndate or time             Jan. 5 2018, 8:03am   continuous or discrete  Specific days and/or times. Also\n                                                                       generic dates, such as July 4 or Dec. 25\n                                                                       (without year).\n\ntext                     The quick brown fox   none, or discrete       Free-form text. Can be treated\n                         jumps over the lazy                           as categorical if needed.\n                         dog.\n---------------------------------------------------------------------------------------------------------------------\n\nTable: (\\#tab:data-example) First 12 rows of a dataset listing daily temperature normals for four weather stations. Data source: NOAA.\n\n Month   Day  Location      Station ID   Temperature\n------- ----- ------------ ------------ -------------\n  Jan     1   Chicago      USW00014819        25.6\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_refdef_after_list_item_line() {
    // Found by the incremental fuzz harness. A list item's first-line text
    // lives in the item's `ListItemBuffer`, not in the green builder, so a
    // reference definition emitted on the next line lands *before* it and
    // swaps the document's bytes: `- a\n[x]: /url\n` round-tripped as
    // `- [x]: /url\na\n`. Pandoc folds the line into the item's `Plain`
    // (`Plain [Str "a", SoftBreak, Str "[x]:", Space, Str "/url"]`), the same
    // rule that already keeps a refdef from interrupting a paragraph.
    let input = "- a\n[x]: /url\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).is_none(),
        "a refdef must not interrupt buffered list-item text, got:\n{tree:#?}"
    );
}

#[test]
fn test_refdef_after_list_still_claims_top_level_line() {
    // The guard above must stay narrow: once the list is closed the buffered
    // item content is gone, so a refdef at top level is still a refdef.
    let input = "- a\n\n[x]: /url\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).is_some(),
        "a refdef after a closed list is still a refdef, got:\n{tree:#?}"
    );
}

#[test]
fn test_refdef_after_blank_line_in_list_item() {
    // Blank-line separated: the buffer has been flushed, so the refdef is a
    // block of its own inside the item (pandoc's own spec example).
    let input = "- a\n\n  [x]: /url\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).is_some(),
        "a blank-line separated refdef inside a list item is a refdef, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_setext_underline_after_blockquote_line() {
    // Found by the incremental fuzz harness. Under the Pandoc dialect a
    // setext underline claims the preceding line *as raw text*, blockquote
    // marker included: `> a\n---\n` is `Header 2 [Str ">", Space, Str "a"]`
    // with no blockquote at all. `parse_line` opened a BLOCK_QUOTE off the
    // raw marker count anyway and then re-dispatched the stripped content,
    // where setext ran a second time and re-emitted the marker — so the
    // 10-byte input produced a 12-byte CST (`> > a\n---\nb\n`).
    let input = "> a\n---\nb\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::BLOCK_QUOTE).is_none(),
        "pandoc gives a top-level setext heading here, not a blockquote, got:\n{tree:#?}"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "expected a top-level setext HEADING, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_setext_underline_after_nested_blockquote_line() {
    // Same rule at depth 2: `Header 2 [Str ">", Space, Str ">", Space, Str "a"]`.
    let input = "> > a\n---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::BLOCK_QUOTE).is_none(),
        "expected no blockquote, got:\n{tree:#?}"
    );
}

#[test]
fn test_commonmark_setext_underline_does_not_cross_a_blockquote() {
    // CommonMark keeps the container boundary: `> a\n---\n` is
    // `BlockQuote [Para "a"], HorizontalRule` (verified with
    // `pandoc -f commonmark`), so the setext parser must decline the quoted
    // line under this dialect and let the blockquote parser take it.
    let input = "> a\n---\nb\n";
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::BLOCK_QUOTE).is_some(),
        "expected a BLOCK_QUOTE under CommonMark, got:\n{tree:#?}"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_none(),
        "expected no setext HEADING under CommonMark, got:\n{tree:#?}"
    );
}

#[test]
fn test_commonmark_setext_underline_does_not_cross_a_nested_blockquote() {
    // Same at depth 2: the underline sits outside both quotes.
    let input = "> > a\n---\n";
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::BLOCK_QUOTE).is_some(),
        "expected a BLOCK_QUOTE under CommonMark, got:\n{tree:#?}"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_none(),
        "expected no setext HEADING under CommonMark, got:\n{tree:#?}"
    );
}

#[test]
fn test_blockquote_survives_a_thematic_break_below_it() {
    // The guard must stay narrow: with a blank line between them the `---`
    // is a thematic break and the blockquote is a real blockquote.
    let input = "> a\n\n---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::BLOCK_QUOTE).is_some(),
        "expected a BLOCK_QUOTE, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_setext_heading_inside_blockquote() {
    // `SetextHeadingParser::parse_prepared` emitted from `lines.raw_at(..)`
    // while detection used the stripped lines, so the container prefix was
    // written twice: once by `parse_line`'s marker emission and again inside
    // the heading's own text. Pandoc: `BlockQuote [Header 2 [Str "a"]]`.
    let input = "> a\n> ---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::BLOCK_QUOTE).is_some(),
        "expected a BLOCK_QUOTE wrapping the heading, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_line_block_in_list_item_with_lazy_pipe_line() {
    // Found by the incremental fuzz harness. The line-block classifier peeked
    // with the column-blind strip while the emitter used the whitespace-bounded
    // one, so on the lazy line ` b |` the peek consumed the `b` and read the
    // trailing `|` as a line-block marker. Emission then found no marker and hit
    // `expect("marker presence verified upstream")`. Both walks are
    // whitespace-bounded now.
    let input = "- x\n\n  | a\n b |\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_marker_line_table_delimiter_in_quoted_nested_list() {
    // The delimiter line of a marker-line table nested list > quote > list used
    // to reach the same `expect("marker presence verified upstream")` panic: the
    // inner item is already closed when `  >   - | -` dispatches, so the
    // delimiter is never buffered and `- | -` opens a list whose content is a
    // line block, whose `| ` marker the peek and the emitter disagreed about.
    // Pinned here because the fix landed for a different input.
    let input = "- > - a | b\n  >   - | -\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_div_open_after_list_item_line() {
    // Same trap as the refdef case: a `:::` opener detected as `Yes` while a
    // list item's content is still buffered would emit the div *before* the
    // buffered text. Pandoc folds the line into the item's `Plain`
    // (`Str "a", SoftBreak, Str "|", SoftBreak, Str ":::", Space, Str "note"`).
    let input = "1. a\n|\n::: note\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::FENCED_DIV).is_none(),
        "a `:::` opener must not interrupt buffered list-item text, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_setext_underline_caps_nested_blockquote() {
    // The capped depth leaves the surplus `>` in the heading's text, so the
    // marker bytes are re-emitted by the setext parser rather than by the
    // blockquote path. Losslessness pins that hand-off.
    let input = "> > a\n> ---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_capped_blockquote_markers_without_spaces() {
    let input = ">> a\n>---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_capped_blockquote_three_levels_with_trailing_line() {
    let input = "> > > a\n> ---\n> b\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_capped_blockquote_inside_open_quote() {
    let input = "> a\n>\n> > b\n> ---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
}

/// Parse under the Pandoc dialect and assert byte-identity plus "no HEADING
/// anywhere in the tree".
///
/// Shared by the setext-after-setext cases below: the underline lookback in
/// `SetextHeadingParser::detect_prepared` is purely textual, so it used to fire
/// on lines the parser had actually absorbed as paragraph text, emitting the
/// HEADING *before* those still-buffered bytes and reordering the CST.
fn assert_pandoc_lossless_without_heading(input: &str) {
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_none(),
        "pandoc keeps this whole run as one paragraph, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_setext_underline_after_multiline_paragraph_underline() {
    // `pandoc -f markdown -t native` gives a single
    // `Para [a, SoftBreak, b, SoftBreak, ---, SoftBreak, c, SoftBreak, ---]`:
    // setext content must be a single line, so the first `---` is paragraph
    // text, and setext never interrupts a paragraph under Pandoc. The textual
    // lookback saw `b\n---` and let the second underline through as a
    // "consecutive setext heading" while `a\nb\n---` was still buffered.
    assert_pandoc_lossless_without_heading("a\nb\n---\nc\n---\n");
}

#[test]
fn test_losslessness_setext_equals_underline_after_multiline_paragraph() {
    // Same shape with `=` underlines; pandoc again gives one `Para`.
    assert_pandoc_lossless_without_heading("a\nb\n===\nc\n===\n");
}

#[test]
fn test_losslessness_setext_pair_after_unterminated_fence() {
    // An unterminated fence is paragraph text under Pandoc, so the lookback
    // matched `x\n---` inside that text. pandoc gives one `Para` starting with
    // `Str "```"`.
    assert_pandoc_lossless_without_heading("```\nx\n---\ny\n---\n");
}

#[test]
fn test_losslessness_setext_underline_after_multiline_paragraph_in_blockquote() {
    // `BlockQuote [Para [...]]` per pandoc -- the quoted paragraph buffers the
    // same way, so the reorder happened inside the blockquote.
    assert_pandoc_lossless_without_heading("> a\n> b\n> ---\n> c\n> ---\n");
}

#[test]
fn test_losslessness_setext_underline_after_multiline_list_item_content() {
    // `BulletList [[Plain [...]]]` per pandoc. Here the open buffer is the list
    // item's, not a paragraph's -- `is_paragraph_open` alone does not see it.
    assert_pandoc_lossless_without_heading("- a\n  b\n  ---\n  c\n  ---\n");
}

#[test]
fn test_losslessness_setext_underline_in_quoted_list_item() {
    // `BlockQuote [BulletList [[Header 2 "Foo"]]]` per pandoc (both
    // dialects). The underline line's `>` sits in the item buffer as a
    // structural marker segment; the fold has to see past it and re-emit
    // it between the heading's text and its underline, byte-for-byte.
    let input = "> - Foo\n>   ---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert_eq!(
        find_all(&tree, SyntaxKind::HEADING).len(),
        1,
        "expected a setext heading inside the quoted list item, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_setext_underline_in_nested_quoted_list_item() {
    // `BlockQuote [BlockQuote [BulletList [[Header 2 "Foo"]]]]` per
    // pandoc. One marker segment per depth level is buffered; all of them
    // must be re-emitted in push order.
    let input = "> > - Foo\n> >   ---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert_eq!(
        find_all(&tree, SyntaxKind::HEADING).len(),
        1,
        "expected a setext heading inside the nested quoted list item, got:\n{tree:#?}"
    );
}

#[test]
fn test_consecutive_setext_headings_in_list_item_still_form_headings() {
    // One of the live uses of the `follows_setext_heading` escape (see also
    // `test_consecutive_top_level_setext_headings_still_form_headings` and the
    // `setext_headings` fixture's metadata-lookalike tail): inside a list item
    // `has_blank_before` is false even once the item's buffer has been flushed,
    // so without the escape the second heading would not form. pandoc gives
    // `BulletList [[Header 2 "a", Header 2 "c"]]`.
    let input = "- a\n  ---\n  c\n  ---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert_eq!(
        find_all(&tree, SyntaxKind::HEADING).len(),
        2,
        "expected two setext headings inside the list item, got:\n{tree:#?}"
    );
}

#[test]
fn test_losslessness_tables_in_blockquote_in_footnote() {
    // The footnote body's content indent precedes the blockquote marker on
    // the table's dispatch line, so the core's blockquote opener already
    // emitted those bytes as WHITESPACE. `emit_or_dispatch_tail` used to
    // re-emit them as LINE_PREFIX, duplicating the indent
    // (`    > A    B` round-tripped as `    >     A    B`). All four table
    // types share the helper, so all four are pinned.
    for input in [
        // simple
        "[^1]: body\n\n    > A    B\n    > --- ---\n    > x    y\n",
        // pipe
        "[^1]: body\n\n    > | A | B |\n    > |---|---|\n    > | x | y |\n",
        // grid
        "[^1]: body\n\n    > +---+---+\n    > | A | B |\n    > +===+===+\n    > | x | y |\n    > +---+---+\n",
        // multiline
        "[^1]: body\n\n    > ----- -----\n    > A     B\n    > ----- -----\n    > x     y\n    >\n    > ----- -----\n",
    ] {
        let config = ParserOptions::default();
        let tree = Parser::new(input, &config).parse();
        assert_eq!(tree.text().to_string(), input, "input: {input:?}");
    }
}

#[test]
fn test_consecutive_top_level_setext_headings_still_form_headings() {
    // pandoc gives three `Header 2`s with no intervening blank lines.
    let input = "a\n---\nc\n---\ne\n---\n";
    let config = ParserOptions::default();
    let tree = Parser::new(input, &config).parse();
    assert_eq!(tree.text().to_string(), input);
    assert_eq!(
        find_all(&tree, SyntaxKind::HEADING).len(),
        3,
        "expected three consecutive setext headings, got:\n{tree:#?}"
    );
}
