//! Block and inline code concatenation for external linter invocation.
//!
//! Explicit source mappings keep diagnostics and fixes aligned even when inline
//! framing shifts later snippets away from their original line numbers.

use crate::config::Flavor;
use crate::utils::CodeSnippet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetKind {
    Block,
    /// A displayed fence or a chunk with evaluation disabled.
    DisplayBlock,
    Inline,
}

/// Mapping information for a source snippet in the concatenated file.
#[derive(Debug, Clone)]
pub struct BlockMapping {
    pub kind: SnippetKind,
    /// Byte offset range in the concatenated file
    pub concatenated_range: std::ops::Range<usize>,
    /// Byte offset range in the original document
    pub original_range: std::ops::Range<usize>,
    /// Starting line number in the original document.
    pub start_line: usize,
    /// Per content line: the line's start offset in the concatenated file
    /// paired with the offset of its first content byte in the original
    /// document. Block content is dedented (container prefixes stripped),
    /// so offsets must map line by line rather than by a single block-start
    /// delta; empty means the content is byte-identical to the original.
    pub line_offsets: Vec<(usize, usize)>,
}

/// Result of concatenating code blocks with mapping information.
#[derive(Debug, Clone)]
pub struct ConcatenatedBlocks {
    /// The concatenated content
    pub content: String,
    /// Mapping information for each block
    pub mappings: Vec<BlockMapping>,
}

/// Keep Quarto examples independent while sharing executable document state.
pub fn concatenate_for_lint(blocks: &[CodeSnippet], flavor: Flavor) -> Vec<ConcatenatedBlocks> {
    if flavor != Flavor::Quarto {
        return if blocks.is_empty() {
            Vec::new()
        } else {
            vec![concatenate_with_blanks_and_mapping(blocks)]
        };
    }

    let mut groups = Vec::new();
    let executable = concatenate_snippets(
        blocks
            .iter()
            .filter(|block| block.kind != SnippetKind::DisplayBlock),
    );
    if !executable.mappings.is_empty() {
        groups.push(executable);
    }
    groups.extend(
        blocks
            .iter()
            .filter(|block| block.kind == SnippetKind::DisplayBlock)
            .map(|block| concatenate_with_blanks_and_mapping(std::slice::from_ref(block))),
    );
    groups
}

/// Concatenate code blocks with blank line preservation and return mapping info.
///
/// Pads to original line numbers where possible. Inline framing may shift later
/// snippets; mappings, rather than line-number equality, locate source content.
pub fn concatenate_with_blanks_and_mapping(blocks: &[CodeSnippet]) -> ConcatenatedBlocks {
    concatenate_snippets(blocks.iter())
}

fn concatenate_snippets<'a>(
    blocks: impl Iterator<Item = &'a CodeSnippet> + Clone,
) -> ConcatenatedBlocks {
    let mut content = String::new();
    let mut mappings = Vec::new();
    let mut current_line = 1;
    let mut binding_index = 0;
    // Reserve a namespace once instead of scanning every snippet per expression.
    let mut binding_prefix = "panache_inline_".to_string();
    while blocks
        .clone()
        .any(|snippet| snippet.content.contains(&binding_prefix))
    {
        binding_prefix.push('_');
    }

    for block in blocks {
        // Add blank lines to reach the block's start line
        while current_line < block.start_line {
            content.push('\n');
            current_line += 1;
        }

        if block.kind == SnippetKind::Inline {
            let binding = format!("{binding_prefix}{binding_index}");
            binding_index += 1;
            let framing = match block.language.as_str() {
                "r" => format!("{binding} <- {{\n"),
                "python" => format!("{binding} = (\n"),
                "julia" => format!("{binding} = begin\n"),
                _ => unreachable!("parser only recognizes supported inline runtimes"),
            };
            content.push_str(&framing);
            current_line += 1;
        }

        // Only expression bytes receive source mappings, never generated framing.
        let concat_start = content.len();

        // Add the block content
        content.push_str(&block.content);

        // Track the end of this block in the concatenated file
        let concat_end = content.len();

        // Pair each content line's concatenated start with the original
        // offset of its first content byte (past the container prefix).
        let mut line_offsets = Vec::with_capacity(block.line_starts.len());
        let mut line_start = concat_start;
        for (idx, line) in block.content.split_inclusive('\n').enumerate() {
            if let Some(&original) = block.line_starts.get(idx) {
                line_offsets.push((line_start, original));
            }
            line_start += line.len();
        }

        mappings.push(BlockMapping {
            kind: block.kind,
            concatenated_range: concat_start..concat_end,
            original_range: block.original_range.clone(),
            start_line: block.start_line,
            line_offsets,
        });

        current_line += block.content.bytes().filter(|&byte| byte == b'\n').count();

        if !block.content.ends_with('\n') {
            content.push('\n');
            current_line += 1;
        }
        if block.kind == SnippetKind::Inline {
            content.push_str(match block.language.as_str() {
                "r" => "}\n",
                "python" => ")\n",
                "julia" => "end\n",
                _ => unreachable!(),
            });
            current_line += 1;
        }
    }

    ConcatenatedBlocks { content, mappings }
}

/// Concatenate snippets with inline framing and blank-line padding.
pub fn concatenate_with_blanks(blocks: &[CodeSnippet]) -> String {
    concatenate_with_blanks_and_mapping(blocks).content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};
    use crate::parse;
    use crate::utils::{CodeSnippet, collect_code_snippets, offset_to_line};

    fn inline_input(input: &str) -> ConcatenatedBlocks {
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_snippets(&tree, input);
        concatenate_with_blanks_and_mapping(&blocks["r"])
    }

    #[test]
    fn quarto_lint_groups_follow_cell_options_and_preserve_execution_order() {
        let cases = [
            ("r", false),
            ("{.r}", false),
            ("{r}", true),
            (
                "{r}\n#| label: example\n#| eval: false # Display only.",
                false,
            ),
            ("{r, eval=FALSE}", false),
            ("{r, eval=TRUE}\n#| eval: false", false),
            ("{r, eval=FALSE}\n#| eval: true", true),
            ("{r}\n#| echo: false\n#| include: false", true),
            ("{r}\n#| eval: true", true),
            ("{r}\n#| eval: 'false'", true),
            ("{r}\n#| eval: !expr condition", true),
            ("{r}\n#| eval: !expr false", true),
            ("{r, eval=FALSE}\n#| eval: !expr false", true),
        ];
        for (fence, executes) in cases {
            let input = format!("```{{r}}\nbefore\n```\n\n```{fence}\nexample\n```\n\n`r after`\n");
            let config = Config {
                flavor: Flavor::Quarto,
                extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
                ..Default::default()
            };
            let tree = parse(&input, Some(config));
            let snippets = collect_code_snippets(&tree, &input);
            let groups = concatenate_for_lint(&snippets["r"], Flavor::Quarto);
            assert_eq!(groups.len(), if executes { 1 } else { 2 }, "{fence}");
            assert!(
                groups[0].content.find("before").unwrap()
                    < groups[0].content.find("after").unwrap()
            );
            assert_eq!(groups[0].content.contains("example"), executes, "{fence}");
            for group in &groups {
                assert!(!group.content.contains("#|"), "{fence}");
            }
        }
    }

    #[test]
    fn displayed_fences_are_independent_only_under_quarto() {
        let input = "```r\nfirst\n```\n\n```r\nsecond\n```\n";
        let snippets = collect_code_snippets(&parse(input, None), input);
        let quarto = concatenate_for_lint(&snippets["r"], Flavor::Quarto);
        assert_eq!(quarto.len(), 2);
        assert_eq!(quarto[0].content.trim(), "first");
        assert_eq!(quarto[1].content.trim(), "second");
        assert_eq!(
            concatenate_for_lint(&snippets["r"], Flavor::Pandoc).len(),
            1
        );
    }

    #[test]
    fn inline_execution_maps_same_line_and_later_block() {
        let input = "é `r first` and `r second`.\r\n\r\n> ```{r}\r\n> third\r\n> ```\r\n";
        let result = inline_input(input);
        assert_eq!(result.mappings.len(), 3);
        for word in ["first", "second", "third"] {
            let offset = result.content.find(word).unwrap();
            assert_eq!(
                super::super::external_linters::map_concatenated_edit_to_original(
                    &result.content,
                    offset,
                    offset + word.len(),
                    "value",
                    &result.mappings,
                ),
                Some((
                    input.find(word).unwrap(),
                    input.find(word).unwrap() + word.len()
                ))
            );
        }
    }

    #[test]
    fn inline_execution_maps_multiline_expression_edits() {
        use super::super::external_linters::map_concatenated_edit_to_original;
        let input = "> Value `r (x +\r\n>   y)`.\r\n";
        let result = inline_input(input);
        let start = result.content.find("y)").unwrap();
        let original = input.find("y)").unwrap();
        assert_eq!(
            map_concatenated_edit_to_original(
                &result.content,
                start,
                start + 1,
                "z",
                &result.mappings
            ),
            Some((original, original + 1))
        );
        let mapping = &result.mappings[0];
        assert_eq!(
            map_concatenated_edit_to_original(
                &result.content,
                mapping.concatenated_range.end,
                mapping.concatenated_range.end,
                " + 1",
                &result.mappings
            ),
            Some((mapping.original_range.end, mapping.original_range.end))
        );
        assert!(
            map_concatenated_edit_to_original(
                &result.content,
                mapping.concatenated_range.start,
                mapping.concatenated_range.end,
                "z",
                &result.mappings
            )
            .is_none()
        );
    }

    #[test]
    fn inline_execution_rejects_fixes_that_change_markdown_structure() {
        let input = "`r x` and `r y`\n";
        let result = inline_input(input);
        let range = result.mappings[0].concatenated_range.clone();
        for replacement in ["", " ", "a\nb", "a\rb", "`x`"] {
            assert!(
                super::super::external_linters::map_concatenated_edit_to_original(
                    &result.content,
                    range.start,
                    range.end,
                    replacement,
                    &result.mappings,
                )
                .is_none(),
                "{replacement:?}"
            );
        }
        assert!(
            super::super::external_linters::map_concatenated_edit_to_original(
                &result.content,
                range.start,
                result.mappings[1].concatenated_range.end,
                "x",
                &result.mappings,
            )
            .is_none()
        );
    }

    #[test]
    fn inline_execution_skips_generated_diagnostics() {
        let input = "`r x`\n";
        let result = inline_input(input);
        let start = result.content.find("panache_inline_").unwrap();
        let output = serde_json::json!([{
            "rule": "unused-binding", "severity": "Warning",
            "range": {"start": start, "end": start + 16},
            "message": {"name": "unused-binding", "body": "synthetic"}
        }]);
        let diagnostics = super::super::external_linters::parse_linter_output(
            "arity",
            &output.to_string(),
            &result.content,
            input,
            Some(&result.mappings),
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn inline_execution_rejects_combined_fixes_that_empty_an_expression() {
        let input = "`r x+y`\n";
        let result = inline_input(input);
        let output = serde_json::json!([{
            "code": "example", "message": "example", "filename": "input.py",
            "location": {"row": 2, "column": 1}, "end_location": {"row": 2, "column": 4},
            "fix": {"message": "remove both", "applicability": "unsafe", "edits": [
                {"content": "", "location": {"row": 2, "column": 1}, "end_location": {"row": 2, "column": 2}},
                {"content": "", "location": {"row": 2, "column": 2}, "end_location": {"row": 2, "column": 4}}
            ]}
        }]);
        let diagnostics = super::super::external_linters::parse_linter_output(
            "ruff",
            &output.to_string(),
            &result.content,
            input,
            Some(&result.mappings),
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn inline_execution_binding_names_do_not_shadow_document_code() {
        let input = "```r\npanache_inline_0 <- 1\n```\n\n`r panache_inline_0`\n";
        let result = inline_input(input);
        assert!(result.content.contains("panache_inline__0 <- {"));
        assert!(!result.content.contains("panache_inline_0 <- {"));
    }

    #[test]
    fn test_collect_single_r_block() {
        let input = r#"# Test

```r
x <- 1
y <- 2
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_snippets(&tree, input);

        assert_eq!(blocks.len(), 1);
        assert!(blocks.contains_key("r"));

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 1);
        assert_eq!(r_blocks[0].language, "r");
        assert_eq!(r_blocks[0].content, "x <- 1\ny <- 2\n");
        assert_eq!(r_blocks[0].start_line, 4); // Content starts on line 4, not fence line 3
    }

    #[test]
    fn test_collect_multiple_blocks_same_language() {
        let input = r#"```r
x <- 1
```

Text in between.

```r
y <- 2
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_snippets(&tree, input);

        assert_eq!(blocks.len(), 1);
        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 2);
        assert_eq!(r_blocks[0].start_line, 2); // Content on line 2, fence on line 1
        assert_eq!(r_blocks[1].start_line, 8); // Content on line 8, fence on line 7
    }

    #[test]
    fn test_collect_multiple_languages() {
        let input = r#"```python
print("hello")
```

```r
print("hello")
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_snippets(&tree, input);

        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key("python"));
        assert!(blocks.contains_key("r"));
    }

    #[test]
    fn test_concatenate_with_blanks_single_block() {
        let blocks = vec![CodeSnippet {
            kind: crate::linter::code_block_collector::SnippetKind::Block,
            language: "r".to_string(),
            content: "x <- 1\n".to_string(),
            start_line: 5,
            original_range: 100..107, // Dummy range for test
            line_starts: vec![100],
        }];

        let result = concatenate_with_blanks(&blocks);

        // Should have 4 blank lines (lines 1-4), then content at line 5
        let expected = "\n\n\n\nx <- 1\n";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_concatenate_with_blanks_multiple_blocks() {
        let blocks = vec![
            CodeSnippet {
                kind: crate::linter::code_block_collector::SnippetKind::Block,
                language: "r".to_string(),
                content: "x <- 1\n".to_string(),
                start_line: 2,
                original_range: 50..57,
                line_starts: vec![50],
            },
            CodeSnippet {
                kind: crate::linter::code_block_collector::SnippetKind::Block,
                language: "r".to_string(),
                content: "y <- 2\n".to_string(),
                start_line: 6,
                original_range: 150..157,
                line_starts: vec![150],
            },
        ];

        let result = concatenate_with_blanks(&blocks);

        // Line 1: blank
        // Line 2: x <- 1
        // Lines 3-5: blank
        // Line 6: y <- 2
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], ""); // Line 1 (blank)
        assert_eq!(lines[1], "x <- 1"); // Line 2
        assert_eq!(lines[2], ""); // Line 3
        assert_eq!(lines[3], ""); // Line 4
        assert_eq!(lines[4], ""); // Line 5
        assert_eq!(lines[5], "y <- 2"); // Line 6
    }

    #[test]
    fn test_concatenate_preserves_line_numbers() {
        let blocks = vec![
            CodeSnippet {
                kind: crate::linter::code_block_collector::SnippetKind::Block,
                language: "r".to_string(),
                content: "a <- 1\n".to_string(),
                start_line: 10,
                original_range: 200..207,
                line_starts: vec![200],
            },
            CodeSnippet {
                kind: crate::linter::code_block_collector::SnippetKind::Block,
                language: "r".to_string(),
                content: "b <- 2\n".to_string(),
                start_line: 20,
                original_range: 400..407,
                line_starts: vec![400],
            },
        ];

        let result = concatenate_with_blanks(&blocks);

        // Count total lines
        let line_count = result.lines().count();
        assert_eq!(line_count, 20);

        // Check that line 10 has "a <- 1"
        let line_10 = result.lines().nth(9).unwrap(); // 0-indexed
        assert_eq!(line_10, "a <- 1");

        // Check that line 20 has "b <- 2"
        let line_20 = result.lines().nth(19).unwrap();
        assert_eq!(line_20, "b <- 2");
    }

    #[test]
    fn test_offset_to_line() {
        let input = "line1\nline2\nline3\n";

        assert_eq!(offset_to_line(input, 0), 1); // Start of file
        assert_eq!(offset_to_line(input, 5), 1); // Before first \n
        assert_eq!(offset_to_line(input, 6), 2); // Start of line 2
        assert_eq!(offset_to_line(input, 12), 3); // Start of line 3
    }

    #[test]
    fn test_collect_blockquoted_block_dedents_prefix() {
        let input = "> ```python\n> x=1\n> y=2\n> ```\n";
        let tree = parse(input, None);
        let blocks = collect_code_snippets(&tree, input);

        let py_blocks = &blocks["python"];
        assert_eq!(py_blocks.len(), 1);
        assert_eq!(
            py_blocks[0].content, "x=1\ny=2\n",
            "container prefix bytes must not reach external tools"
        );
        assert_eq!(py_blocks[0].start_line, 2);
        assert_eq!(
            py_blocks[0].line_starts,
            vec![input.find("x=1").unwrap(), input.find("y=2").unwrap()]
        );
    }

    #[test]
    fn test_collect_list_item_block_dedents_indent() {
        let input = "- item\n\n  ```python\n  x=1\n  ```\n";
        let tree = parse(input, None);
        let blocks = collect_code_snippets(&tree, input);

        let py_blocks = &blocks["python"];
        assert_eq!(py_blocks.len(), 1);
        assert_eq!(py_blocks[0].content, "x=1\n");
        assert_eq!(py_blocks[0].line_starts, vec![input.find("x=1").unwrap()]);
    }

    #[test]
    fn test_mapping_maps_dedented_offsets_back_through_prefix() {
        let input = "> ```python\n> x=1\n> y=2\n> ```\n";
        let tree = parse(input, None);
        let blocks = collect_code_snippets(&tree, input);
        let result = concatenate_with_blanks_and_mapping(&blocks["python"]);

        // Line numbers are preserved: content starts on document line 2.
        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines[1], "x=1");
        assert_eq!(lines[2], "y=2");

        // A tool offset inside the dedented view maps back past the `> `
        // prefix of its own line.
        let concat_y = result.content.find("y=2").unwrap();
        assert_eq!(
            crate::linter::external_linters::map_concatenated_offset_to_original(
                concat_y,
                &result.mappings
            ),
            Some(input.find("y=2").unwrap())
        );
        let concat_1 = result.content.find('1').unwrap();
        assert_eq!(
            crate::linter::external_linters::map_concatenated_offset_to_original(
                concat_1,
                &result.mappings
            ),
            Some(input.find('1').unwrap())
        );
    }

    #[test]
    fn test_quarto_style_braces() {
        // Quarto uses {r} instead of just r
        let input = r#"```{r}
x <- 1
```
"#;

        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_snippets(&tree, input);

        assert_eq!(blocks.len(), 1);
        assert!(blocks.contains_key("r"), "Should extract 'r' from '{{r}}'");

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 1);
        assert_eq!(r_blocks[0].language, "r");
        assert_eq!(r_blocks[0].content, "x <- 1\n");
    }

    #[test]
    fn test_quarto_style_braces_with_options() {
        // Quarto supports {r label, echo=FALSE}
        let input = r#"```{r my-label, echo=FALSE}
x <- 1
```
"#;

        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_snippets(&tree, input);

        assert_eq!(blocks.len(), 1);
        assert!(
            blocks.contains_key("r"),
            "Should extract 'r' from '{{r my-label, echo=FALSE}}'"
        );

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 1);
        assert_eq!(r_blocks[0].language, "r");
    }

    #[test]
    fn test_quarto_display_class_language_normalized() {
        let input = "```{.bash filename=\"Terminal\"}\necho hi\n```\n";
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_snippets(&tree, input);

        assert!(blocks.contains_key("bash"));
        let bash_blocks = &blocks["bash"];
        assert_eq!(bash_blocks.len(), 1);
        assert_eq!(bash_blocks[0].language, "bash");
    }

    #[test]
    fn test_quarto_various_syntaxes() {
        let input = r#"```{r}
a <- 1
```

```{python}
b = 2
```

```{r chunk-label}
c <- 3
```

```{r chunk2, echo=FALSE}
d <- 4
```
"#;

        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_snippets(&tree, input);

        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key("r"));
        assert!(blocks.contains_key("python"));

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 3, "Should find all three R blocks");

        let py_blocks = &blocks["python"];
        assert_eq!(py_blocks.len(), 1);
    }

    #[test]
    fn test_concatenate_with_mapping() {
        let blocks = vec![
            CodeSnippet {
                kind: crate::linter::code_block_collector::SnippetKind::Block,
                language: "r".to_string(),
                content: "x <- 1\n".to_string(),
                start_line: 2,
                original_range: 10..17, // Hypothetical original positions
                line_starts: vec![10],
            },
            CodeSnippet {
                kind: crate::linter::code_block_collector::SnippetKind::Block,
                language: "r".to_string(),
                content: "y <- 2\n".to_string(),
                start_line: 6,
                original_range: 50..57,
                line_starts: vec![50],
            },
        ];

        let result = concatenate_with_blanks_and_mapping(&blocks);

        // Check content is correct
        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[1], "x <- 1"); // Line 2
        assert_eq!(lines[5], "y <- 2"); // Line 6

        // Check mappings
        assert_eq!(result.mappings.len(), 2);

        // First block mapping
        assert_eq!(result.mappings[0].start_line, 2);
        assert_eq!(result.mappings[0].original_range, 10..17);
        // In concatenated: "\n" (line 1) + "x <- 1\n" = offset 1 to 8
        assert_eq!(result.mappings[0].concatenated_range.start, 1);
        assert_eq!(result.mappings[0].concatenated_range.end, 8);

        // Second block mapping
        assert_eq!(result.mappings[1].start_line, 6);
        assert_eq!(result.mappings[1].original_range, 50..57);
        // In concatenated: 8 (after first) + "\n\n\n" (lines 3-5) = 11, then "y <- 2\n" = 11 to 18
        assert_eq!(result.mappings[1].concatenated_range.start, 11);
        assert_eq!(result.mappings[1].concatenated_range.end, 18);
    }
}
