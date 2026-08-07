use super::helpers::{
    assert_block_kinds, assert_block_kinds_for_node, find_all, find_first, parse_blocks,
    parse_blocks_gfm, parse_blocks_quarto, parse_blocks_with_config,
};
use crate::options::{Dialect, Extensions, Flavor, ParserOptions};
use crate::syntax::SyntaxKind;

fn get_code_content(node: &crate::syntax::SyntaxNode) -> Option<String> {
    find_first(node, SyntaxKind::CODE_CONTENT).map(|n| n.text().to_string())
}

fn get_code_info_node(node: &crate::syntax::SyntaxNode) -> Option<crate::syntax::SyntaxNode> {
    node.descendants()
        .find(|element| element.kind() == SyntaxKind::CODE_INFO)
}

fn get_code_info(node: &crate::syntax::SyntaxNode) -> Option<String> {
    get_code_info_node(node).map(|n| n.text().to_string())
}

#[test]
fn parses_simple_backtick_code_block() {
    let input = "```\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_simple_tilde_code_block() {
    let input = "~~~\nprint(\"hello\")\n~~~\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_code_block_with_language() {
    let input = "```python\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "python");
}

#[test]
fn parses_code_block_with_attributes() {
    let input = "```{python}\nprint(\"hello\")\n```\n";
    let node = parse_blocks_quarto(input);

    assert_block_kinds_for_node(&node, &[SyntaxKind::CODE_BLOCK], input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python}");
}

#[test]
fn parses_code_block_with_complex_attributes() {
    let input = "```{python #mycode .numberLines startFrom=\"100\"}\nprint(\"hello\")\n```\n";
    let node = parse_blocks_quarto(input);

    assert_block_kinds_for_node(&node, &[SyntaxKind::CODE_BLOCK], input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python #mycode .numberLines startFrom=\"100\"}");
}

#[test]
fn parses_multiline_code_block() {
    let input = "```python\nfor i in range(10):\n    print(i)\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "for i in range(10):\n    print(i)\n");
}

#[test]
fn code_block_can_interrupt_paragraph() {
    // Fenced code blocks with language identifiers can interrupt paragraphs
    // Bare fences (```) require a blank line to avoid ambiguity with inline code
    let input = "text\n```python\ncode\n```\n";
    let node = parse_blocks(input);

    // Should parse as paragraph followed by code block
    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );

    let code_content = get_code_content(&node).unwrap();
    assert_eq!(code_content, "code\n");
}

#[test]
fn bare_fence_without_closing_fence_does_not_interrupt_paragraph() {
    // Unclosed bare fences should not interrupt paragraphs.
    let input = "text\n```\ncode\n";
    // Use full parse to get inline parsing too
    let tree = crate::parse(input, None);

    // Should parse as single paragraph
    let paragraphs: Vec<_> = tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::PARAGRAPH)
        .collect();
    assert_eq!(paragraphs.len(), 1, "Should have one paragraph");

    let code_block = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CODE_BLOCK);
    assert!(code_block.is_none(), "Should not contain fenced code block");
}

#[test]
fn fence_with_info_without_closing_fence_is_not_code_block() {
    let input = "````markdown\n";
    let tree = crate::parse(input, None);

    let code_block = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CODE_BLOCK);
    assert!(code_block.is_none(), "Should not contain fenced code block");
}

#[test]
fn code_block_with_language_can_interrupt_paragraph() {
    // Test with language identifier
    let input = "Some text:\n```r\na <- 1\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );

    let code_content = get_code_content(&node).unwrap();
    assert_eq!(code_content, "a <- 1\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "r");
}

#[test]
fn bare_fence_after_colon_with_command_transcript_can_interrupt_paragraph() {
    let input = "Some text:\n```\n% pandoc -t plain\n```\n";
    let node = parse_blocks(input);
    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );
}

#[test]
fn bare_fence_in_list_item_with_closing_fence_can_interrupt_paragraph() {
    let input = "- one\n  ```\n  code\n  ```\n- two\n";
    let node = parse_blocks(input);
    let has_code_block = node
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CODE_BLOCK);
    assert!(
        has_code_block,
        "Expected fenced code block inside list item"
    );
}

/// The children of `root`, for the shape assertions below.
fn block_kinds(root: &crate::syntax::SyntaxNode) -> Vec<SyntaxKind> {
    root.children().map(|node| node.kind()).collect()
}

/// pandoc: `[ BulletList [ [ Plain [ Str "a" ] ] ], CodeBlock ("",["rust"],[]) "c" ]`.
///
/// `rawListItem` stops collecting at a line that opens a fenced code block
/// below the item's content column, so the block is the list's *sibling*, not
/// its child. Only a fence does this: a heading or thematic break at the same
/// column is still item content (see below).
#[test]
fn under_indented_fence_closes_the_list_item_pandoc() {
    let input = "- a\n```rust\nc\n```\n";
    let node = parse_blocks(input);

    assert_eq!(node.text().to_string(), input, "parser must be lossless");
    assert_eq!(
        block_kinds(&node),
        vec![SyntaxKind::LIST, SyntaxKind::CODE_BLOCK]
    );
    assert_eq!(get_code_content(&node).unwrap(), "c\n");
}

/// The boundary: at the item's content column the fence *is* item content, so
/// the code block stays inside and the `Plain` is promoted to `Para` (0514).
#[test]
fn fence_at_the_item_content_column_stays_in_the_item() {
    let input = "- a\n  ```rust\n  c\n  ```\n";
    let node = parse_blocks(input);

    assert_eq!(node.text().to_string(), input, "parser must be lossless");
    assert_eq!(block_kinds(&node), vec![SyntaxKind::LIST]);
    assert!(
        find_first(&node, SyntaxKind::CODE_BLOCK).is_some(),
        "the fence at the content column is item content"
    );
}

/// Only a *complete* fence ends the item — pandoc's `codeBlockFenced` needs its
/// closer, so an unterminated one is lazy item text.
#[test]
fn under_indented_fence_without_a_closer_stays_lazy_item_text() {
    let input = "- a\n```rust\nc\n";
    let node = parse_blocks(input);

    assert_eq!(node.text().to_string(), input, "parser must be lossless");
    assert_eq!(block_kinds(&node), vec![SyntaxKind::LIST]);
    assert!(
        find_first(&node, SyntaxKind::CODE_BLOCK).is_none(),
        "an unclosed fence is not a fence"
    );
}

/// The boundary is the item's content column, so an ordered marker moves it:
/// `1. a` puts content at 3, and indent 2 is still under-indented.
#[test]
fn ordered_item_content_column_sets_the_fence_boundary() {
    let outside = parse_blocks("1. a\n  ```r\n  c\n  ```\n");
    assert_eq!(
        block_kinds(&outside),
        vec![SyntaxKind::LIST, SyntaxKind::CODE_BLOCK]
    );

    let inside = parse_blocks("1. a\n   ```r\n   c\n   ```\n");
    assert_eq!(block_kinds(&inside), vec![SyntaxKind::LIST]);
}

/// Neither a heading nor a thematic break ends a pandoc list item — they are
/// lazy item text, which is why the close is gated on the fenced-code parser
/// rather than on `!OpenList` the way CommonMark's is.
#[test]
fn under_indented_heading_and_rule_stay_lazy_item_text() {
    for input in ["- a\n# h\n", "- a\n***\n"] {
        let node = parse_blocks(input);
        assert_eq!(node.text().to_string(), input, "parser must be lossless");
        assert_eq!(block_kinds(&node), vec![SyntaxKind::LIST], "for {input:?}");
    }
}

#[test]
fn adjacent_bare_fences_with_command_transcripts_parse_as_two_code_blocks() {
    let input = "```\n% one\n```\n```\n% two\n```\n";
    let node = parse_blocks(input);
    let code_blocks = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CODE_BLOCK)
        .count();
    assert_eq!(code_blocks, 2);
}

#[test]
fn parses_code_block_at_start_of_document() {
    let input = "```\ncode\n```\n";

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);
}

#[test]
fn parses_code_block_after_blank_line() {
    let input = "text\n\n```\ncode\n```\n";
    let node = parse_blocks(input);

    let blocks: Vec<_> = node
        .descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::CODE_BLOCK))
        .collect();

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind(), SyntaxKind::PARAGRAPH);
    assert_eq!(blocks[1].kind(), SyntaxKind::CODE_BLOCK);
}

#[test]
fn requires_at_least_three_fence_chars() {
    let input = "``\ncode\n``\n";
    let node = parse_blocks(input);

    // Should not parse as code block
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_none());
}

#[test]
fn closing_fence_must_have_at_least_same_length() {
    let input = "````\ncode\n```\n";
    let node = parse_blocks(input);

    // Without a valid closing fence, this should stay paragraph content.
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_none());
}

#[test]
fn closing_fence_can_be_longer() {
    let input = "```\ncode\n`````\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n");
}

#[test]
fn mixed_fence_chars_dont_close() {
    let input = "```\ncode\n~~~\n";
    let node = parse_blocks(input);

    // Without a matching closing fence, this should stay paragraph content.
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_none());
}

#[test]
fn empty_code_block() {
    let input = "```\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    // Should have no content node for empty blocks
    assert!(get_code_content(&node).is_none());
}

#[test]
fn code_block_with_leading_spaces() {
    let input = "  ```python\n  print(\"hello\")\n  ```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  print(\"hello\")\n");
}

#[test]
fn definition_list_inline_fence_parses_as_code_block() {
    let input = "Term\n: ```r\n  a <- 1\n  ```\n";
    let node = parse_blocks_quarto(input);

    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK);
    assert!(
        code_block.is_some(),
        "Expected code block inside definition list"
    );

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  a <- 1\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "r");
}

#[test]
fn executable_chunk_embeds_hashpipe_label_as_yaml() {
    let input = "```{r}\n#| label: foobar\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);

    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let info = get_code_info(&code_block).expect("expected code info");
    assert_eq!(info, "{r}");

    // Hashpipe options are now embedded YAML structure, not CHUNK_OPTION.
    assert!(
        code_block
            .descendants()
            .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP),
        "expected the hashpipe preamble to embed a YAML block map"
    );
    assert!(
        !code_block
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CHUNK_OPTION),
        "hashpipe options should no longer emit CHUNK_OPTION nodes"
    );

    // The label key and value survive as YAML scalar-text leaves.
    let scalar_texts: Vec<String> = code_block
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::YAML_SCALAR_TEXT)
        .map(|t| t.text().to_string())
        .collect();
    assert!(scalar_texts.iter().any(|t| t == "label"));
    assert!(scalar_texts.iter().any(|t| t == "foobar"));
}

#[test]
fn executable_chunk_keeps_non_hashpipe_lines_in_code_content() {
    let input = "```{r}\n#| label: foobar\na <- 1\n# comment\n```\n";
    let node = parse_blocks_quarto(input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "#| label: foobar\na <- 1\n# comment\n");
}

#[test]
fn executable_chunk_multiline_hashpipe_continuation_is_not_top_level_text() {
    let input = "```{r}\n#| fig-cap: \"A multiline caption\n#|  that spans multiple lines and demonstrates\n#|  wrapping.\"\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|  ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "multiline hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_block_scalar_hashpipe_continuation_is_not_top_level_text() {
    let input = "```{r}\n#| fig-cap: |\n#|   A caption\n#|   spanning some lines\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|   ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "block-scalar hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_folded_block_scalar_hashpipe_continuation_is_not_top_level_text() {
    let input =
        "```{r}\n#| fig-cap: >-\n#|   A folded caption\n#|   spanning some lines\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|   ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "folded block-scalar hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_indented_hashpipe_value_continuation_is_not_top_level_text() {
    let input = "```{r}\n#| list:\n#|   - a\n#|   - b\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|   - ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "indented hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_emits_hashpipe_yaml_preamble_node() {
    let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");
    assert_eq!(
        preamble.text().to_string(),
        "#| echo: false\n#| fig-cap: |\n#|   A caption\n"
    );
}

#[test]
fn executable_chunk_emits_hashpipe_yaml_content_node() {
    let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");
    let preamble_content = preamble
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_CONTENT)
        .expect("expected hashpipe preamble content node");
    assert_eq!(
        preamble_content.text().to_string(),
        "#| echo: false\n#| fig-cap: |\n#|   A caption\n"
    );
}

#[test]
fn executable_chunk_carries_hashpipe_prefix_as_yaml_line_prefix() {
    let input = "```{r}\n#| echo: false\nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");

    // The `#|` marker is now carried as YAML_LINE_PREFIX trivia inside the
    // embedded YAML (marker plus its one trailing space), not a
    // HASHPIPE_PREFIX token.
    let prefix_tokens: Vec<_> = preamble
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::YAML_LINE_PREFIX)
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(prefix_tokens, vec!["#| ".to_string()]);
    assert!(
        !preamble
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|t| t.kind() == SyntaxKind::HASHPIPE_PREFIX),
        "hashpipe `#|` should no longer emit a HASHPIPE_PREFIX token"
    );
}

#[test]
fn executable_chunk_hashpipe_trivia_preserves_whitespace_and_crlf() {
    let input = "```{r}\r\n#|    echo: false   \r\nx <- 1\r\n```\r\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");
    assert_eq!(preamble.text().to_string(), "#|    echo: false   \r\n");
}

#[test]
fn executable_chunk_hashpipe_continuation_preserves_trailing_space() {
    let input = "```{r}\n#| fig-subcap:\n#|   - \"Histogram of `price`s\"\n#|   - \"Histogram of `area`s\" \nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    assert_eq!(node.text().to_string(), input);
}

#[test]
fn executable_chunk_hashpipe_preamble_captures_non_option_prefixed_lines() {
    let input = "```{r}\n#| fig-subcap: - \"A\"\n#|   - \"B\"\nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");
    assert_eq!(
        preamble.text().to_string(),
        "#| fig-subcap: - \"A\"\n#|   - \"B\"\n"
    );
}

#[test]
fn executable_chunk_hashpipe_multiline_scalar_in_list_does_not_gain_indent() {
    let input = "-   Press Ctrl + `.` to open tool.\n\n    ```{r}\n    #| fig-cap: >\n    #|   Go to File/Function in RStudio.\n    #| fig-alt: >\n    #|   Screenshot of the \"Go to File/Function\" tool in the\n    #|   RStudio IDE. It is a text box with the cursor in it.\n    knitr::include_graphics(\"images/file-finder.png\", dpi = 220)\n    ```\n";
    let node = parse_blocks_quarto(input);
    assert_eq!(node.text().to_string(), input);
}

#[test]
fn executable_code_respects_extension_guard() {
    let input = "```{r}\na <- 1\n```\n";
    let mut config = ParserOptions::default();
    config.extensions.executable_code = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "executable_code disabled should prevent executable chunk parsing"
    );

    config.extensions.executable_code = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "executable_code enabled should allow executable chunk parsing"
    );
}

#[test]
fn display_code_block_keeps_hashpipe_line_as_plain_text() {
    let input = "```r\n#| label: foobar\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);

    let has_chunk_option = node
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CHUNK_OPTION);
    assert!(
        !has_chunk_option,
        "display-only code blocks should not parse hashpipe as chunk options"
    );
}

#[test]
fn backtick_fenced_code_blocks_respect_extension_guard() {
    let input = "```r\na <- 1\n```\n";
    let mut config = ParserOptions::default();
    config.extensions.backtick_code_blocks = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "backtick_code_blocks disabled should prevent backtick fenced code parsing"
    );

    config.extensions.backtick_code_blocks = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "backtick_code_blocks enabled should allow backtick fenced code parsing"
    );
}

#[test]
fn tilde_fenced_code_blocks_respect_extension_guard() {
    let input = "~~~r\na <- 1\n~~~\n";
    let mut config = ParserOptions::default();
    config.extensions.fenced_code_blocks = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "fenced_code_blocks disabled should prevent tilde fenced code parsing"
    );

    config.extensions.fenced_code_blocks = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "fenced_code_blocks enabled should allow tilde fenced code parsing"
    );
}

#[test]
fn gfm_defaults_allow_backtick_and_tilde_fenced_code_blocks() {
    let backtick = parse_blocks_gfm("```r\na <- 1\n```\n");
    assert!(
        find_first(&backtick, SyntaxKind::CODE_BLOCK).is_some(),
        "gfm defaults should allow backtick fenced code blocks"
    );

    let tilde = parse_blocks_gfm("~~~r\na <- 1\n~~~\n");
    assert!(
        find_first(&tilde, SyntaxKind::CODE_BLOCK).is_some(),
        "gfm defaults should allow tilde fenced code blocks"
    );
}

#[test]
fn fenced_code_attributes_respect_extension_guard() {
    let input = "```{python}\na <- 1\n```\n";
    let mut config = ParserOptions {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    config.extensions.fenced_code_attributes = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "fenced_code_attributes disabled should prevent brace-info fenced code parsing"
    );

    config.extensions.fenced_code_attributes = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "fenced_code_attributes enabled should allow brace-info fenced code parsing"
    );
}

#[test]
fn raw_attribute_respects_extension_guard_for_fenced_code() {
    let input = "```{=html}\n<div>raw</div>\n```\n";
    let mut config = ParserOptions::default();
    config.extensions.raw_attribute = false;
    config.extensions.fenced_code_attributes = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "raw_attribute disabled should prevent raw-attribute fenced code parsing"
    );

    config.extensions.raw_attribute = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "raw_attribute enabled should allow raw-attribute fenced code parsing"
    );
}

#[test]
fn tex_math_gfm_parses_math_fence_as_display_math() {
    let input = "``` math\nx + y\n```\n";
    let mut config = ParserOptions::default();
    config.extensions.tex_math_gfm = true;

    let tree = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&tree, SyntaxKind::DISPLAY_MATH).is_some(),
        "tex_math_gfm enabled should parse ``` math fences as display math"
    );
}

#[test]
fn standalone_dollar_math_delimiters_do_not_split_into_tex_block() {
    let input = "And so now our between group sum of squares is obtained by summing these\n\"weighted squared deviations\" over all three groups in the study:\n$$\n\\begin{aligned} SS_b & = 1.14 + 0.18 + 2.16 \\\\ &= 3.48 \\end{aligned}\n$$\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "display math delimited by standalone $$ lines should stay paragraph-inline, not TEX_BLOCK"
    );
}

fn single_backslash_math_options() -> ParserOptions {
    let mut config = ParserOptions::default();
    config.extensions.tex_math_single_backslash = true;
    config
}

#[test]
fn bracket_display_math_does_not_split_into_tex_block() {
    let input = "Before\n\n\\[\nN(A)=\\operatorname{span}\n\\left\\{\n\\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix},\n\\begin{bmatrix}0\\\\0\\\\1\\end{bmatrix}\n\\right\\}\n\\]\n\nAfter\n\n# Heading\n\nText\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "display math delimited by `\\[` and `\\]` lines should stay paragraph-inline, not TEX_BLOCK"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a heading after bracket-delimited display math should remain a HEADING"
    );
}

#[test]
fn bracket_display_math_delimiters_with_content_do_not_split_into_tex_block() {
    // Pandoc does not require `\[`/`\]` to sit on their own lines.
    let input = "Before\n\n\\[ N(A)=\n\\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n\\]\n\nAfter\n\n# Heading\n\nText\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "an opener with trailing content (`\\[ N(A)=`) should still hold the paragraph together"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a heading after bracket-delimited display math should remain a HEADING"
    );
}

#[test]
fn bracket_display_math_closer_with_content_releases_paragraph() {
    // A closer sharing its line with math content must clear the open state so
    // following blocks can interrupt the paragraph again.
    let input = "Before\n\n\\[\nE = mc^2 \\]\n``` python\nx = 1\n```\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::CODE_BLOCK).is_some(),
        "a fence after a `... \\]` closer line should interrupt the paragraph"
    );
}

#[test]
fn bracket_delimiters_inside_dollar_math_do_not_latch() {
    // A `\[` line inside open `$$` math is dollar-math content; it must not
    // leave bracket state latched once the dollars close.
    let input = "$$\n\\[\n$$\n``` python\nx = 1\n```\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::CODE_BLOCK).is_some(),
        "a fence after closed `$$` math should interrupt the paragraph even if the math contained `\\[`"
    );
}

#[test]
fn bracket_delimiters_ignored_without_tex_math_extension() {
    // Without `tex_math_single_backslash` (Pandoc and Quarto defaults), `\[`
    // is not a display-math delimiter; pandoc splits the environment into a
    // raw TeX block, and so do we.
    let input = "Before\n\n\\[\n\\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n\\]\n\nAfter\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_some(),
        "without the extension, `\\begin{{...}}` after a `\\[` line still starts a TEX_BLOCK (pandoc parity)"
    );
}

#[test]
fn commonmark_escaped_bracket_line_does_not_hold_paragraph_open() {
    // In CommonMark `\[` is just an escaped bracket; a heading must still
    // interrupt the paragraph.
    let input = "text\n\\[\n# Heading\n";
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let tree = parse_blocks_with_config(input, &config);

    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "an escaped `\\[` line in CommonMark must not suppress paragraph interruption"
    );
}

#[test]
fn double_backslash_bracket_display_math_does_not_split_into_tex_block() {
    let input = "Before\n\n\\\\[\n\\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n\\\\]\n\nAfter\n\n# Heading\n\nText\n";
    let mut config = ParserOptions::default();
    config.extensions.tex_math_double_backslash = true;
    let tree = parse_blocks_with_config(input, &config);

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "display math delimited by `\\\\[` and `\\\\]` lines should stay paragraph-inline, not TEX_BLOCK"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a heading after double-backslash display math should remain a HEADING"
    );
}

#[test]
fn list_item_bracket_display_math_does_not_split_into_tex_block() {
    let input = "- item\n\n  \\[\n  \\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n  \\]\n\n# Heading after\n\ntext\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "bracket display math inside a list item should stay inline, not TEX_BLOCK"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a heading after list-item display math should remain a HEADING"
    );
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
    assert!(
        find_first(&item, SyntaxKind::DISPLAY_MATH).is_some(),
        "the bracket region should parse as DISPLAY_MATH inside the LIST_ITEM"
    );
}

#[test]
fn list_item_dollar_display_math_does_not_split_into_tex_block() {
    let input = "- item\n\n  $$\n  \\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n  $$\n\n# Heading after\n\ntext\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "`$$` display math inside a list item should stay inline, not TEX_BLOCK"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a heading after list-item `$$` math should remain a HEADING"
    );
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
    assert!(
        find_first(&item, SyntaxKind::DISPLAY_MATH).is_some(),
        "the `$$` region should parse as DISPLAY_MATH inside the LIST_ITEM"
    );
}

#[test]
fn list_item_marker_line_bracket_opener_does_not_split_into_tex_block() {
    // The opener sits on the marker line itself, exercising the buffer seed
    // in `lists.rs` rather than the continuation path.
    let input = "- \\[\n  \\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n  \\]\n\n# Heading after\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::TEX_BLOCK).is_none(),
        "a marker-line `\\[` opener should hold the item together, not TEX_BLOCK"
    );
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a heading after the list should remain a HEADING"
    );
}

#[test]
fn blank_line_inside_list_item_bracket_math_releases_state() {
    // A blank line flushes the item's buffered chunk and must reset the open
    // math state; the unclosed `\[` degrades to literal text and the heading
    // still interrupts.
    let input = "- item\n\n  \\[\n  x\n\n# Heading\n";
    let tree = parse_blocks_with_config(input, &single_backslash_math_options());

    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "a blank line must release the open bracket state so the heading survives"
    );
}

#[test]
fn sibling_list_marker_interrupts_open_dollar_math_in_list_item() {
    // Pandoc splits items before scanning math: `- $$` / `- next` is two
    // items with literal dollars, not one item swallowing the sibling.
    let input = "- $$\n- next\n$$\n";
    let tree = parse_blocks(input);

    let items = find_all(&tree, SyntaxKind::LIST_ITEM);
    assert_eq!(
        items.len(),
        2,
        "a sibling list marker must interrupt open `$$` math in the previous item"
    );
}

#[test]
fn commonmark_list_item_bracket_line_does_not_hold_item_open() {
    // In CommonMark `\[` is just an escaped bracket; a heading must still
    // interrupt the list item content.
    let input = "- text\n  \\[\n# Heading\n";
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..Default::default()
    };
    let tree = parse_blocks_with_config(input, &config);

    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_some(),
        "an escaped `\\[` line in a CommonMark list item must not suppress interruption"
    );
}

// Indented code block tests

#[test]
fn parses_indented_code_block() {
    let input = "
    code line 1
    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
    let code_blocks = find_all(&tree, SyntaxKind::CODE_BLOCK);
    let code = &code_blocks[0];
    let text = code.text().to_string();
    assert!(text.contains("code line 1"));
    assert!(text.contains("code line 2"));
}

#[test]
fn indented_code_block_with_blank_line() {
    let input = "
    code line 1

    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_requires_blank_line_before() {
    let input = "paragraph
    not code";
    let tree = parse_blocks(input);

    // Should be a single paragraph, not a code block
    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 0);
    assert_eq!(find_all(&tree, SyntaxKind::PARAGRAPH).len(), 1);
}

#[test]
fn indented_code_with_tab() {
    let input = "
\tcode with tab";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_with_list_marker() {
    let input = "
    * one
    * two";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_in_blockquote() {
    let input = ">
>     code in blockquote";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::BLOCK_QUOTE).len(), 1);
    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}
