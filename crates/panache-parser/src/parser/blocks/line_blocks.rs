use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::{count_blockquote_markers, strip_n_blockquote_markers};
use super::container_prefix::{
    StrippedLines, advance_emitted_marker_columns, bq_outer_of_list, strip_list_indent,
};
use crate::parser::utils::container_stack::{byte_index_at_column, leading_indent};
use crate::parser::utils::helpers::strip_newline;
use crate::parser::utils::inline_emission;

/// Try to parse the start of a line block.
/// Returns Some(()) if this line starts a line block (| followed by space or end of line).
pub fn try_parse_line_block_start(line: &str) -> Option<()> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("| ") || trimmed == "|" {
        Some(())
    } else {
        None
    }
}

/// Parse a complete line block starting at current position.
/// Returns the new position after the line block.
///
/// The container geometry is carried by the `StrippedLines` window: the
/// dispatch line (`window.pos()`) keeps its bespoke `emit_open_line_prefixes`
/// emitter — `list_marker_consumed_on_line_0` selects between a silent
/// column-advance through the upstream-emitted list marker
/// (`advance_emitted_marker_columns`) and a whitespace-only strip with
/// WHITESPACE emission (`strip_list_indent`).
/// Continuation lines route through the window's `strip_at` (peek) and
/// `emit_prefix_at` (re-emit prefix tokens), so the list-content-indent is
/// always whitespace and blank lines aren't eaten by the column-advance.
pub(crate) fn parse_line_block(
    window: &StrippedLines<'_, '_>,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> usize {
    let lines = window.raw();
    let start_pos = window.pos();
    let prefix = window.prefix();
    let bq_depth = prefix.bq_depth();
    let list_content_col = prefix.list_content_col();
    let list_marker_consumed_on_line_0 = prefix.list_marker_consumed_on_line_0;
    let bq_outer = bq_outer_of_list(prefix);
    let content_indent = prefix.content_indent();

    log::trace!("Parsing line block at line {}", start_pos + 1);

    builder.start_node(SyntaxKind::LINE_BLOCK.into());

    let mut pos = start_pos;
    let mut first_line = true;

    while pos < lines.len() {
        let raw_line = lines[pos];

        let kind = if first_line {
            LineKind::Marker
        } else {
            let peek = window.peek_prefix_at(pos);
            let lazy_in_quote = bq_depth > 0 && count_blockquote_markers(raw_line).0 < bq_depth;
            if !lazy_in_quote && continues_previous_line(peek, raw_line, list_content_col) {
                LineKind::Continuation
            } else if parse_line_block_line_marker(peek).is_some() {
                LineKind::Marker
            } else {
                break;
            }
        };

        builder.start_node(SyntaxKind::LINE_BLOCK_LINE.into());

        let stripped = if first_line {
            emit_open_line_prefixes(
                builder,
                raw_line,
                bq_depth,
                list_content_col,
                list_marker_consumed_on_line_0,
                bq_outer,
                content_indent,
            )
        } else {
            window.emit_prefix_at(builder, pos)
        };

        match kind {
            LineKind::Marker => {
                let content_start = parse_line_block_line_marker(stripped)
                    .expect("marker presence verified upstream");
                builder.token(
                    SyntaxKind::LINE_BLOCK_MARKER.into(),
                    &stripped[..content_start],
                );
                let content = &stripped[content_start..];
                let (content_without_newline, newline_str) = strip_newline(content);
                if !content_without_newline.is_empty() {
                    inline_emission::emit_inlines(builder, content_without_newline, config, false);
                }
                if !newline_str.is_empty() {
                    builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
            }
            LineKind::Continuation => {
                let (line_without_newline, newline_str) = strip_newline(stripped);
                if !line_without_newline.is_empty() {
                    inline_emission::emit_inlines(builder, line_without_newline, config, false);
                }
                if !newline_str.is_empty() {
                    builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
            }
        }

        builder.finish_node(); // LineBlockLine
        pos += 1;
        first_line = false;
    }

    builder.finish_node(); // LineBlock

    log::trace!("Parsed line block: lines {}-{}", start_pos + 1, pos);

    pos
}

enum LineKind {
    Marker,
    Continuation,
}

/// Does this line continue the line above rather than open a line of its own?
///
/// Pandoc's rule is "the line starts with a space"
/// (`many (try (char ' ' >> anyLine))` in `lineBlockLine`), applied to the
/// text left after the enclosing containers have taken their prefix. `peek`
/// is that text, so it decides the ordinary cases.
///
/// The rule looks at nothing but that leading space: a line that *also*
/// carries a `| ` marker still folds in, because the gobble runs before the
/// next `lineBlockLine` gets a chance. `| a\n  | b` is one line reading
/// `a | b`, and the caller checks this predicate before the marker.
///
/// The list case needs the raw line too. Pandoc gobbles a list item's
/// continuation indent all-or-nothing (`optional (gobbleSpaces n)`): a lazy
/// line indented *less* than the content column keeps every leading space it
/// had, so it still reads as a continuation. Panache's strip is greedy and
/// eats what it can, so `" b |"` inside a two-column item reaches us as
/// `"b |"` — recover the pre-strip shape from `raw_line`. A line indented
/// *exactly* to the content column is not a continuation: there the gobble
/// succeeds and consumes all of it.
///
/// A whitespace-only line satisfies the rule and folds in as an empty
/// continuation, which is also what pandoc does — `| a\n  \n| b` is one line
/// block of two lines, not two line blocks.
fn continues_previous_line(peek: &str, raw_line: &str, list_content_col: usize) -> bool {
    if peek.starts_with(' ') {
        return true;
    }
    list_content_col > 0
        && raw_line.starts_with([' ', '\t'])
        && leading_indent(raw_line).0 < list_content_col
}

/// Strip and emit the active container prefix on the dispatch line (line 0).
/// Mirrors `prepare_fence_open_line` in `code_blocks.rs` minus the final
/// `strip_leading_spaces` step — line blocks treat any leading spaces
/// before `|` as part of `LINE_BLOCK_MARKER`, so we must not strip them.
fn emit_open_line_prefixes<'a>(
    builder: &mut GreenNodeBuilder<'static>,
    source_line: &'a str,
    bq_depth: usize,
    list_content_col: usize,
    list_marker_consumed_on_line_0: bool,
    bq_outer: bool,
    content_indent: usize,
) -> &'a str {
    let mut s: &'a str = source_line;
    let mut pending_ws_start: Option<usize> = None;
    let suppress_list = list_marker_consumed_on_line_0;

    let flush_ws = |builder: &mut GreenNodeBuilder<'static>,
                    pending: &mut Option<usize>,
                    current_offset: usize| {
        if let Some(start) = *pending
            && current_offset > start
        {
            builder.token(
                SyntaxKind::LINE_PREFIX.into(),
                &source_line[start..current_offset],
            );
        }
        *pending = None;
    };

    let do_strip_list = |s: &mut &'a str, pending: &mut Option<usize>| {
        if list_content_col == 0 {
            return;
        }
        let stripped = if suppress_list {
            advance_emitted_marker_columns(s, list_content_col)
        } else {
            strip_list_indent(s, list_content_col)
        };
        let consumed = s.len() - stripped.len();
        if consumed > 0 {
            let start = source_line.len() - s.len();
            if !suppress_list && pending.is_none() {
                *pending = Some(start);
            }
            *s = stripped;
        }
    };

    let do_strip_bq =
        |builder: &mut GreenNodeBuilder<'static>, s: &mut &'a str, pending: &mut Option<usize>| {
            if bq_depth == 0 {
                return;
            }
            let current_offset = source_line.len() - s.len();
            flush_ws(builder, pending, current_offset);
            *s = strip_n_blockquote_markers(s, bq_depth);
        };

    if bq_outer {
        do_strip_bq(builder, &mut s, &mut pending_ws_start);
        do_strip_list(&mut s, &mut pending_ws_start);
    } else {
        do_strip_list(&mut s, &mut pending_ws_start);
        do_strip_bq(builder, &mut s, &mut pending_ws_start);
    }

    if content_indent > 0 {
        let indent_bytes = byte_index_at_column(s, content_indent);
        if s.len() >= indent_bytes && indent_bytes > 0 {
            let start = source_line.len() - s.len();
            if pending_ws_start.is_none() {
                pending_ws_start = Some(start);
            }
            s = &s[indent_bytes..];
        }
    }

    let final_offset = source_line.len() - s.len();
    flush_ws(builder, &mut pending_ws_start, final_offset);
    s
}

/// Parse a line block marker and return the index where content starts.
/// Returns Some(index) if the line starts with "| " or just "|", None otherwise.
fn parse_line_block_line_marker(line: &str) -> Option<usize> {
    let trimmed_start = line.len() - line.trim_start().len();
    let after_indent = &line[trimmed_start..];

    if after_indent.starts_with("| ") {
        Some(trimmed_start + 2) // Skip "| "
    } else if after_indent == "|" || after_indent == "|\n" {
        Some(trimmed_start + 1) // Just "|", no space
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::container_prefix::ContainerPrefix;
    use super::*;

    #[test]
    fn test_try_parse_line_block_start() {
        assert!(try_parse_line_block_start("| Some text").is_some());
        assert!(try_parse_line_block_start("| ").is_some());
        assert!(try_parse_line_block_start("|").is_some()); // Empty line block
        assert!(try_parse_line_block_start("  | Some text").is_some());

        assert!(try_parse_line_block_start("|No space").is_none());
        assert!(try_parse_line_block_start("Regular text").is_none());
        assert!(try_parse_line_block_start("").is_none());
    }

    #[test]
    fn test_parse_line_block_marker() {
        assert_eq!(parse_line_block_line_marker("| Some text"), Some(2));
        assert_eq!(parse_line_block_line_marker("| "), Some(2));
        assert_eq!(parse_line_block_line_marker("|"), Some(1)); // Empty line block
        assert_eq!(parse_line_block_line_marker("  | Indented"), Some(4));

        assert_eq!(parse_line_block_line_marker("|No space"), None);
        assert_eq!(parse_line_block_line_marker("Regular"), None);
    }

    #[test]
    fn test_simple_line_block() {
        let input = vec!["| Line one", "| Line two", "| Line three"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_with_continuation() {
        let input = vec![
            "| This is a long line",
            "  that continues here",
            "| Second line",
        ];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_with_indentation() {
        let input = vec!["| First line", "|    Indented line", "| Back to normal"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn continuation_needs_leading_space_at_top_level() {
        assert!(continues_previous_line("  b", "  b", 0));
        assert!(!continues_previous_line("b", "b", 0));
        assert!(!continues_previous_line("\n", "\n", 0));
        assert!(continues_previous_line("   \n", "   \n", 0));
    }

    #[test]
    fn indented_marker_line_continues_the_line_above() {
        assert!(continues_previous_line("  | b", "  | b", 0));
        assert!(continues_previous_line(" | b", " | b", 0));
        assert!(continues_previous_line("  |\n", "  |\n", 0));
        assert!(continues_previous_line("  |b", "  |b", 0));
        assert!(!continues_previous_line("| b", "| b", 0));
    }

    #[test]
    fn indented_marker_folds_in_at_top_level() {
        let input = vec!["| a", "  | b", "| c"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
        let node = crate::syntax::SyntaxNode::new_root(builder.finish());
        let markers = node
            .descendants_with_tokens()
            .filter(|e| e.kind() == SyntaxKind::LINE_BLOCK_MARKER)
            .count();
        assert_eq!(markers, 2);
    }

    #[test]
    fn under_indented_lazy_line_continues_inside_a_list_item() {
        assert!(continues_previous_line("b |", " b |", 2));
        assert!(continues_previous_line(" b |", "   b |", 2));
        assert!(!continues_previous_line("b |", "  b |", 2));
        assert!(!continues_previous_line("b |", "b |", 2));
    }

    #[test]
    fn test_line_block_stops_at_non_line_block() {
        let input = vec!["| Line one", "| Line two", "Regular paragraph"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 2); // Should stop before "Regular paragraph"
    }
}
