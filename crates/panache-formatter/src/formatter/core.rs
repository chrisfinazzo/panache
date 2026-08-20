use crate::config::{Config, HorizontalRuleStyle, WrapMode};
use crate::directives::DirectiveTracker;
use crate::syntax::{DefinitionItem, FencedDiv, SyntaxKind, SyntaxNode};
use panache_parser::parser::blocks::definition_lists::try_parse_definition_marker;
use panache_parser::parser::blocks::headings::try_parse_atx_heading;
use panache_parser::parser::blocks::horizontal_rules::try_parse_horizontal_rule;
use panache_parser::parser::utils::attributes::{AttrComponent, attribute_content_spans};
use rowan::NodeOrToken;
use rowan::ast::AstNode;

use super::code_blocks;
use super::code_blocks::FormattedCodeMap;
use super::headings;
use super::inline;
use super::inline_layout;
use super::paragraphs;
use super::preserve::{preserve_lines, preserve_lines_unprefixed};
use super::smart::normalize_smart_punctuation;
use super::tables;
use super::utils::{is_block_element, is_structural_block};

pub struct Formatter {
    pub(super) output: String,
    pub(super) config: Config,
    pub(super) consecutive_blank_lines: usize,
    pub(super) fenced_div_depth: usize,
    pub(super) formatted_code: FormattedCodeMap,
    /// Stack of max marker widths for nested lists (for right-aligning markers)
    pub(super) max_marker_widths: Vec<usize>,
    /// Optional byte range to format (start, end). If None, format entire document.
    range: Option<(usize, usize)>,
    /// Track ignore directives for formatting
    pub(super) directive_tracker: DirectiveTracker,
    /// Depth of ignore region (for preserving content exactly)
    pub(super) ignore_region_start: Option<usize>,
    /// Structured rendering context for nested blockquote containers.
    blockquote_context: Option<BlockquoteContext>,
}

#[derive(Clone, Debug)]
struct BlockquoteContext {
    in_list_continuation: bool,
}

impl Formatter {
    pub fn new(
        config: Config,
        formatted_code: FormattedCodeMap,
        range: Option<(usize, usize)>,
    ) -> Self {
        Self {
            output: String::with_capacity(8192),
            config,
            consecutive_blank_lines: 0,
            fenced_div_depth: 0,
            formatted_code,
            max_marker_widths: Vec::new(),
            range,
            directive_tracker: DirectiveTracker::new(),
            ignore_region_start: None,
            blockquote_context: None,
        }
    }
    pub fn format(mut self, node: &SyntaxNode) -> String {
        self.format_node_sync(node, 0);
        self.output
    }

    fn is_in_range(&self, node: &SyntaxNode) -> bool {
        if let Some((range_start, range_end)) = self.range {
            let node_start: usize = node.text_range().start().into();
            let node_end: usize = node.text_range().end().into();

            node_start < range_end && node_end > range_start
        } else {
            true
        }
    }

    fn should_process_top_level_node(&self, node: &SyntaxNode) -> bool {
        if self.range.is_none() {
            return true;
        }

        if node.kind() == SyntaxKind::DOCUMENT {
            return true;
        }

        if is_structural_block(node.kind()) {
            return self.is_in_range(node);
        }

        false
    }

    pub(super) fn format_inline_node(&self, node: &SyntaxNode) -> String {
        inline::format_inline_node(node, &self.config)
    }

    pub(super) fn wrapped_lines_for_paragraph(
        &self,
        node: &SyntaxNode,
        width: usize,
    ) -> Vec<String> {
        inline_layout::wrapped_lines_for_paragraph(&self.config, node, width, &|n| {
            self.format_inline_node(n)
        })
    }

    pub(super) fn wrapped_lines_for_paragraph_with_widths(
        &self,
        node: &SyntaxNode,
        widths: &[usize],
    ) -> Vec<String> {
        inline_layout::wrapped_lines_for_paragraph_with_widths(&self.config, node, widths, &|n| {
            self.format_inline_node(n)
        })
    }

    pub(super) fn sentence_lines_for_paragraph(&self, node: &SyntaxNode) -> Vec<String> {
        inline_layout::sentence_lines_for_paragraph(&self.config, node, &|n| {
            self.format_inline_node(n)
        })
    }

    pub(super) fn semantic_lines_for_paragraph(&self, node: &SyntaxNode) -> Vec<String> {
        inline_layout::semantic_lines_for_paragraph(&self.config, node, &|n| {
            self.format_inline_node(n)
        })
    }

    pub(super) fn format_heading(&self, node: &SyntaxNode) -> String {
        headings::format_heading(node, &self.config)
    }

    fn contains_latex_command(&self, node: &SyntaxNode) -> bool {
        paragraphs::contains_latex_command(node)
    }

    pub(super) fn is_grid_table_continuation_paragraph(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::PARAGRAPH {
            return false;
        }
        let text = node.text().to_string();
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
            .collect();
        if lines.len() < 2 {
            return false;
        }
        lines.iter().all(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('|') || trimmed.starts_with('+')
        }) && lines.iter().any(|line| line.contains("+-"))
            && lines.iter().any(|line| line.trim_start().starts_with('|'))
    }

    fn is_grid_table_caption_definition_list(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::DEFINITION_LIST {
            return false;
        }
        if !node
            .text()
            .to_string()
            .lines()
            .any(|line| line.trim_start().starts_with(':'))
        {
            return false;
        }
        if let Some(prev) = node.prev_sibling() {
            return prev.kind() == SyntaxKind::GRID_TABLE
                || self.is_grid_table_continuation_paragraph(&prev);
        }
        false
    }

    fn horizontal_rule_text(&self, available_width: usize) -> String {
        match self.config.horizontal_rule_style {
            HorizontalRuleStyle::LineWidth => "-".repeat(available_width.max(3)),
            HorizontalRuleStyle::Compact => "---".to_string(),
        }
    }

    fn starts_with_list_marker(text: &str) -> bool {
        text.starts_with("- ")
            || text.starts_with("* ")
            || text.starts_with("+ ")
            || text.starts_with("(@")
            || {
                let mut chars = text.chars().peekable();
                let mut saw_digit = false;
                while let Some(ch) = chars.peek().copied() {
                    if ch.is_ascii_digit() {
                        saw_digit = true;
                        chars.next();
                    } else {
                        break;
                    }
                }
                saw_digit && matches!(chars.peek().copied(), Some('.') | Some(')'))
            }
    }

    fn paragraph_starts_with_atx_heading_candidate(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::PARAGRAPH {
            return false;
        }
        let text = node.text().to_string();
        let first_line = text.lines().next().unwrap_or_default();
        let trimmed = first_line.trim_start_matches([' ', '\t']);
        let leading_hashes = trimmed.chars().take_while(|&c| c == '#').count();
        (1..=6).contains(&leading_hashes)
            && trimmed[leading_hashes..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
    }

    pub(super) fn leading_atx_heading_with_remainder(
        &self,
        node: &SyntaxNode,
    ) -> Option<(String, String)> {
        if !matches!(node.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH) {
            return None;
        }

        let text = node.text().to_string();
        let mut lines = text.lines();
        let first_line = lines.next()?.trim_start_matches([' ', '\t']);
        try_parse_atx_heading(first_line)?;

        let remainder = lines
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ");

        if remainder.is_empty() {
            return None;
        }

        Some((first_line.trim_end().to_string(), remainder))
    }

    pub(super) fn wrap_text_for_indent(&self, text: &str, indent: usize) -> Vec<String> {
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let width = self.config.line_width.saturating_sub(indent);
        match wrap_mode {
            WrapMode::Preserve | WrapMode::Semantic => vec![text.to_string()],
            WrapMode::Reflow | WrapMode::Sentence => {
                inline_layout::wrap_text_first_fit(text, width)
            }
        }
    }

    fn format_code_block(&mut self, node: &SyntaxNode) {
        code_blocks::format_code_block(node, &self.config, &self.formatted_code, &mut self.output);
    }

    pub(super) fn format_code_block_to_string(&mut self, node: &SyntaxNode) -> String {
        let saved_output = self.output.clone();
        self.output.clear();
        self.format_code_block(node);
        let formatted = self.output.clone();
        self.output = saved_output;
        formatted
    }

    fn strip_leading_columns(line: &str, columns: usize) -> String {
        let mut cols = 0usize;
        let mut idx = 0usize;

        for (byte_idx, ch) in line.char_indices() {
            if cols >= columns {
                idx = byte_idx;
                break;
            }

            match ch {
                ' ' => {
                    cols += 1;
                    idx = byte_idx + ch.len_utf8();
                }
                '\t' => {
                    cols += 4 - (cols % 4);
                    idx = byte_idx + ch.len_utf8();
                }
                _ => {
                    idx = byte_idx;
                    break;
                }
            }
        }

        if cols >= columns {
            line[idx..].to_string()
        } else if line.chars().all(|c| matches!(c, ' ' | '\t')) {
            String::new()
        } else {
            line[idx..].to_string()
        }
    }

    fn format_container_code_block(
        &mut self,
        node: &SyntaxNode,
        first_line_prefix: &str,
        continuation_indent: usize,
        trim_first_line_start: bool,
        strip_content_columns: Option<usize>,
        indent_blank_content_lines: bool,
    ) {
        let formatted = self.format_code_block_to_string(node);

        let mut lines = formatted.lines();
        if let Some(first_line) = lines.next() {
            self.output.push_str(first_line_prefix);
            if trim_first_line_start {
                self.output.push_str(first_line.trim_start());
            } else {
                self.output.push_str(first_line);
            }
            self.output.push('\n');
        }

        let mut remaining: Vec<&str> = lines.collect();
        if remaining.is_empty() {
            return;
        }

        let closing = remaining.pop().unwrap();

        let continuation_prefix = " ".repeat(continuation_indent);
        for line in remaining {
            if line.trim().is_empty() && !indent_blank_content_lines {
                self.output.push('\n');
                continue;
            }

            self.output.push_str(&continuation_prefix);
            match strip_content_columns {
                Some(cols) => self
                    .output
                    .push_str(&Self::strip_leading_columns(line, cols)),
                None => self.output.push_str(line),
            }
            self.output.push('\n');
        }

        self.output.push_str(&continuation_prefix);
        match strip_content_columns {
            Some(cols) => self
                .output
                .push_str(&Self::strip_leading_columns(closing, cols)),
            None => self.output.push_str(closing),
        }
        self.output.push('\n');
    }

    /// Format a code block that is a continuation of a definition or list item.
    /// Adds indentation prefix to each line of the fenced code block.
    pub(super) fn format_indented_code_block(&mut self, node: &SyntaxNode, indent: usize) {
        let is_fenced = node
            .children()
            .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN);
        let in_list_item = node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::LIST_ITEM);
        let code_text = node.text().to_string();
        let should_preserve_raw_indented = !is_fenced
            && in_list_item
            && (code_text.contains("```")
                || code_text.contains("<details")
                || code_text.contains("</details>"));
        if should_preserve_raw_indented {
            self.output.push_str(&code_text);
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }

        let indent_str = " ".repeat(indent);

        let strip_content_columns = (!is_fenced)
            .then(|| Self::container_content_offset(node))
            .filter(|cols| *cols > 0);
        self.format_container_code_block(
            node,
            &indent_str,
            indent,
            false,
            strip_content_columns,
            false,
        );

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    /// Visual column at which the content of `node`'s enclosing container
    /// starts *in the source*.
    ///
    /// The parser leaves that offset on every body line of an indented code
    /// block, so it is what has to come off before the block is re-prefixed at
    /// its formatted indent. Mirrors `definition_content_offset` and
    /// `list_item_content_offset` in the pandoc-native projector
    /// (`panache_parser::to_pandoc_ast`); the two must agree on what a code
    /// block's payload is.
    fn container_content_offset(node: &SyntaxNode) -> usize {
        let Some(parent) = node.parent() else {
            return 0;
        };
        match parent.kind() {
            SyntaxKind::DEFINITION => {
                Self::marker_content_offset(&parent, SyntaxKind::DEFINITION_MARKER)
            }
            SyntaxKind::LIST_ITEM => {
                let parent_ws = match parent.prev_sibling_or_token() {
                    Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::WHITESPACE => {
                        Self::advance_col(0, t.text())
                    }
                    _ => 0,
                };
                parent_ws + Self::marker_content_offset(&parent, SyntaxKind::LIST_MARKER)
            }
            _ => 0,
        }
    }

    fn marker_content_offset(container: &SyntaxNode, marker: SyntaxKind) -> usize {
        let mut col = 0usize;
        let mut saw_marker = false;
        for el in container.children_with_tokens() {
            match el {
                NodeOrToken::Token(t) if t.kind() == marker => {
                    col = Self::advance_col(col, t.text());
                    saw_marker = true;
                }
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                    col = Self::advance_col(col, t.text());
                    if saw_marker {
                        return col;
                    }
                }
                _ if saw_marker => return col,
                _ => {}
            }
        }
        col
    }

    fn advance_col(start: usize, s: &str) -> usize {
        let mut col = start;
        for c in s.chars() {
            if c == '\t' {
                col = (col / 4 + 1) * 4;
            } else {
                col += 1;
            }
        }
        col
    }

    fn code_block_leading_indent(node: &SyntaxNode) -> String {
        node.children_with_tokens()
            .take_while(
                |item| matches!(item, NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE),
            )
            .filter_map(|item| match item {
                NodeOrToken::Token(t) => Some(t.text().to_string()),
                _ => None,
            })
            .collect::<String>()
    }

    /// Format `node` into a scratch buffer instead of `self.output`.
    ///
    /// Callers that re-prefix the result line by line (blockquote children,
    /// most of all) need the rendering in isolation. `width_reduction` shrinks
    /// `line_width` for the duration so wrapped content still fits once the
    /// prefix is put back in front of it.
    fn render_to_buffer(
        &mut self,
        node: &SyntaxNode,
        indent: usize,
        width_reduction: usize,
    ) -> String {
        let saved_output = std::mem::take(&mut self.output);
        let saved_line_width = self.config.line_width;
        self.config.line_width = saved_line_width.saturating_sub(width_reduction);

        self.format_node_sync(node, indent);

        self.config.line_width = saved_line_width;
        std::mem::replace(&mut self.output, saved_output)
    }

    fn append_blockquote_prefixed_block(
        &mut self,
        rendered: &str,
        content_prefix: &str,
        blank_prefix: &str,
        leading_indent: Option<&str>,
    ) {
        for line in rendered.lines() {
            if line.is_empty() {
                self.output.push_str(blank_prefix);
            } else {
                self.output.push_str(content_prefix);
                if let Some(indent) = leading_indent
                    && !indent.is_empty()
                {
                    self.output.push_str(indent);
                }
                self.output.push_str(line);
            }
            self.output.push('\n');
        }
    }

    /// Re-prefix a rendered child that may itself contain a nested blockquote.
    ///
    /// A nested `BLOCK_QUOTE` derives its prefix from its own ancestor depth,
    /// so its lines arrive already carrying `> ` for every enclosing quote and
    /// only need the base indent. Unlike
    /// [`Self::append_blockquote_prefixed_block`], which is for content that
    /// can legitimately start a line with `>` (code, raw HTML), this trusts
    /// that a leading `> ` means "already quoted".
    fn append_blockquote_prefixed_nested_block(
        &mut self,
        rendered: &str,
        base_indent: &str,
        content_prefix: &str,
        blank_prefix: &str,
    ) {
        for line in rendered.lines() {
            if line.is_empty() {
                self.output.push_str(blank_prefix);
            } else if line.starts_with("> ") {
                self.output.push_str(base_indent);
                self.output.push_str(line);
            } else {
                self.output.push_str(content_prefix);
                self.output.push_str(line);
            }
            self.output.push('\n');
        }
    }

    fn append_blockquote_prefixed_list_output(
        &mut self,
        list_output: &str,
        base_indent: &str,
        content_prefix: &str,
        blank_prefix: &str,
    ) -> bool {
        let mut in_list_item_continuation = false;
        for line in list_output.lines() {
            let trimmed_line = line.trim_start();
            let starts_with_list_marker = Self::starts_with_list_marker(trimmed_line)
                && !tables::is_partial_grid_separator(trimmed_line);
            if trimmed_line.is_empty() {
                self.output.push_str(blank_prefix);
                in_list_item_continuation = false;
            } else if line.starts_with("> ") {
                let trimmed_rest = line.trim_start_matches("> ").trim_start();
                if trimmed_rest.is_empty() {
                    self.output.push_str(blank_prefix);
                    in_list_item_continuation = false;
                    self.output.push('\n');
                    continue;
                }
                self.output.push_str(base_indent);
                self.output.push_str(line);
                in_list_item_continuation = Self::starts_with_list_marker(trimmed_rest)
                    && !tables::is_partial_grid_separator(trimmed_rest);
            } else {
                self.output.push_str(content_prefix);
                self.output.push_str(line);
                in_list_item_continuation = starts_with_list_marker
                    || (in_list_item_continuation && line.starts_with(char::is_whitespace));
            }
            self.output.push('\n');
        }

        in_list_item_continuation
    }

    /// Smart punctuation turns `—`→`---` and `–`→`--`. When a paragraph's
    /// whole content normalizes to dashes, the emitted line re-parses as a
    /// thematic break (or setext underline) — a semantic + idempotency break
    /// (one pandoc itself shares). When that happens, re-emit the paragraph
    /// with smart off so the lossless unicode dash is preserved. The smart-off
    /// rendering is adopted only when it actually clears the marker, so a
    /// paragraph that genuinely contains a `***`/`___`/`- - -` line (not
    /// produced by smart) is left untouched.
    fn guard_dash_block_marker(&mut self, start: usize, node: &SyntaxNode, indent: usize) {
        if !self.config.formatter_extensions.smart
            || !Self::produces_dash_block_marker(&self.output[start..])
        {
            return;
        }

        let original = self.output[start..].to_string();
        self.output.truncate(start);

        let mut cfg = self.config.clone();
        cfg.formatter_extensions.smart = false;
        let saved = std::mem::replace(&mut self.config, cfg);
        self.format_node_sync(node, indent);
        self.config = saved;

        if Self::produces_dash_block_marker(&self.output[start..]) {
            self.output.truncate(start);
            self.output.push_str(&original);
        }
    }

    /// A paragraph whose first emitted line is a `:` or `~` definition marker
    /// re-parses as a `DEFINITION` as soon as the block above it is a single
    /// line: the parser promotes that line to the `TERM`. Reflow manufactures
    /// exactly that situation out of a multi-line paragraph, so escape the
    /// marker to keep `format(format(x)) == format(x)`. `\: def` is
    /// `Para [":", Space, "def"]` for pandoc and panache alike.
    ///
    /// Reads the emitted text rather than the source, because the reparse sees
    /// the emitted text — a paragraph that is still multi-line after wrapping
    /// cannot supply a term and is left alone.
    ///
    /// `content_indent` is the content-container indent the calling arm
    /// prepends to every line (a footnote body's four columns, a list item's
    /// hanging column). The parser strips it before measuring the marker's own
    /// 0-3 space allowance, so the guard must too, or a marker at a content
    /// column of 4 reads as indented code and is never guarded.
    pub(super) fn guard_definition_marker_start(&mut self, start: usize, content_indent: usize) {
        if !self.config.parser_extensions.definition_lists {
            return;
        }
        let Some(first) = self.output[start..].lines().next() else {
            return;
        };
        let container_indent = first
            .bytes()
            .take_while(|byte| *byte == b' ')
            .count()
            .min(content_indent);
        let prefix_len = container_indent + Self::block_prefix_len(&first[container_indent..]);
        let Some((marker, ..)) = try_parse_definition_marker(&first[prefix_len..]) else {
            return;
        };
        if !Self::preceding_block_is_one_line(&self.output[..start]) {
            return;
        }
        let marker = first[prefix_len..]
            .find(marker)
            .expect("marker parsed above")
            .saturating_add(prefix_len);
        self.output.insert(start + marker, '\\');
    }

    fn block_prefix_len(line: &str) -> usize {
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut saw_marker = false;
        loop {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'>' {
                i += 1;
                if i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
                saw_marker = true;
                continue;
            }
            return if saw_marker { start } else { 0 };
        }
    }

    fn preceding_block_is_one_line(before: &str) -> bool {
        let is_blank = |line: &str| line[Self::block_prefix_len(line)..].trim().is_empty();
        let ends_the_block_above = |line: &str| {
            is_blank(line)
                || inline_layout::looks_like_div_fence_line(
                    line[Self::block_prefix_len(line)..].trim_start(),
                )
        };
        let mut lines = before.lines().rev().skip_while(|l| is_blank(l));
        lines.next().is_some() && lines.next().is_none_or(ends_the_block_above)
    }

    fn produces_dash_block_marker(text: &str) -> bool {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if try_parse_horizontal_rule(line).is_some() {
                return true;
            }
            if i > 0 && trimmed.chars().all(|c| c == '-') && !lines[i - 1].trim().is_empty() {
                return true;
            }
        }
        false
    }

    pub(super) fn format_node_sync(&mut self, node: &SyntaxNode, indent: usize) {
        if self.directive_tracker.is_formatting_ignored()
            && node.kind() != SyntaxKind::DOCUMENT
            && node.kind() != SyntaxKind::COMMENT
            && node.kind() != SyntaxKind::HTML_BLOCK
            && node.kind() != SyntaxKind::HTML_BLOCK_RAW
            && node.kind() != SyntaxKind::HTML_BLOCK_DIV
        {
            let text = node.text().to_string();
            self.output.push_str(&text);
            for directive in crate::directives::collect_inline_directives(node) {
                self.directive_tracker.process_directive(&directive);
            }
            return;
        }

        if matches!(node.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN) {
            let inline_directives = crate::directives::collect_inline_directives(node);
            if !inline_directives.is_empty() {
                let affects_formatting = inline_directives.iter().any(|d| match d {
                    crate::directives::Directive::Start(kind)
                    | crate::directives::Directive::End(kind) => kind.affects_formatting(),
                });
                if affects_formatting {
                    let text = node.text().to_string();
                    self.output.push_str(&text);
                    if !text.ends_with('\n') {
                        self.output.push('\n');
                    }
                    for directive in inline_directives {
                        self.directive_tracker.process_directive(&directive);
                    }
                    return;
                }
                for directive in inline_directives {
                    self.directive_tracker.process_directive(&directive);
                }
            }
        }

        if node.kind() != SyntaxKind::BLANK_LINE {
            self.consecutive_blank_lines = 0;
        }

        let line_width = self.config.line_width;

        match node.kind() {
            SyntaxKind::DOCUMENT => {
                for el in node.children_with_tokens() {
                    match el {
                        rowan::NodeOrToken::Node(n) => {
                            if self.should_process_top_level_node(&n) {
                                self.format_node_sync(&n, indent);
                            }
                        }
                        rowan::NodeOrToken::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {}
                            SyntaxKind::NEWLINE => {}
                            SyntaxKind::BLANK_LINE => {
                                if !self.output.is_empty() {
                                    self.output.push('\n');
                                }
                            }
                            SyntaxKind::ESCAPED_CHAR => {
                                self.output.push_str(t.text());
                            }
                            SyntaxKind::NONBREAKING_SPACE => {
                                self.output.push_str(r"\ ");
                            }
                            SyntaxKind::IMAGE_LINK_START
                            | SyntaxKind::LINK_START
                            | SyntaxKind::LATEX_COMMAND => {
                                self.output.push_str(t.text());
                            }
                            _ => self.output.push_str(t.text()),
                        },
                    }
                }
            }

            SyntaxKind::HEADING => {
                log::trace!("Formatting heading");
                if let Some(prev) = node.prev_sibling()
                    && is_block_element(prev.kind())
                    && !self.output.is_empty()
                    && self.output.ends_with('\n')
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }

                self.output.push_str(&" ".repeat(indent));
                self.output
                    .push_str(&headings::format_heading(node, &self.config));
                self.output.push('\n');

                if let Some(next) = node.next_sibling()
                    && (is_block_element(next.kind()) || next.kind() == SyntaxKind::HEADING)
                    && !(self.config.formatter_extensions.blank_before_header
                        && self.paragraph_starts_with_atx_heading_candidate(&next))
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::HORIZONTAL_RULE => {
                if !self.output.is_empty()
                    && self.output.ends_with('\n')
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }

                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(
                    &self.horizontal_rule_text(self.config.line_width.saturating_sub(indent)),
                );
                self.output.push('\n');

                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                    self.consecutive_blank_lines = 1;
                }
            }

            SyntaxKind::REFERENCE_DEFINITION => {
                let text = node.text().to_string();
                self.output.push_str(text.trim_end());
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }

                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && next.kind() != SyntaxKind::REFERENCE_DEFINITION
                    && next.kind() != SyntaxKind::FOOTNOTE_DEFINITION
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::ADMONITION => {
                let mut marker = String::new();
                let mut type_str = String::new();
                let mut title_str = String::new();
                let mut body = Vec::new();

                for element in node.children_with_tokens() {
                    match element {
                        NodeOrToken::Token(token) => match token.kind() {
                            SyntaxKind::ADMONITION_MARKER => marker.push_str(token.text()),
                            SyntaxKind::ADMONITION_TYPE => type_str.push_str(token.text()),
                            SyntaxKind::ADMONITION_TITLE => title_str.push_str(token.text()),
                            _ => {}
                        },
                        NodeOrToken::Node(child) => body.push(child),
                    }
                }

                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(marker.trim());
                let normalized_type = type_str.split_whitespace().collect::<Vec<_>>().join(" ");
                if !normalized_type.is_empty() {
                    self.output.push(' ');
                    self.output.push_str(&normalized_type);
                }
                if !title_str.trim().is_empty() {
                    self.output.push(' ');
                    self.output.push_str(title_str.trim());
                }
                self.output.push('\n');

                let child_indent = indent + 4;
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

                let leading = body
                    .iter()
                    .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
                    .count();
                let trailing = body
                    .iter()
                    .rev()
                    .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
                    .count();
                let end = body.len().saturating_sub(trailing).max(leading);

                let mut prev_blank = false;
                for child in &body[leading..end] {
                    match child.kind() {
                        SyntaxKind::BLANK_LINE => {
                            if !prev_blank {
                                self.output.push('\n');
                                prev_blank = true;
                            }
                            continue;
                        }
                        SyntaxKind::PARAGRAPH => {
                            let para_start = self.output.len();
                            let available_width =
                                self.config.line_width.saturating_sub(child_indent);
                            let lines = match wrap_mode {
                                WrapMode::Preserve => preserve_lines(
                                    child,
                                    self.config.formatter_extensions.escaped_line_breaks,
                                )
                                .iter()
                                .map(|line| {
                                    normalize_smart_punctuation(
                                        line.trim_start_matches([' ', '\t']),
                                        self.config.formatter_extensions.smart,
                                        self.config.formatter_extensions.smart_quotes,
                                    )
                                    .to_string()
                                })
                                .collect(),
                                WrapMode::Reflow => {
                                    self.wrapped_lines_for_paragraph(child, available_width)
                                }
                                WrapMode::Sentence => self.sentence_lines_for_paragraph(child),
                                WrapMode::Semantic => self.semantic_lines_for_paragraph(child),
                            };
                            for line in lines {
                                self.output.push_str(&" ".repeat(child_indent));
                                self.output.push_str(line.trim_start_matches([' ', '\t']));
                                self.output.push('\n');
                            }
                            self.guard_definition_marker_start(para_start, child_indent);
                        }
                        SyntaxKind::CODE_BLOCK => {
                            self.format_indented_code_block(child, child_indent);
                        }
                        _ => {
                            self.format_node_sync(child, child_indent);
                        }
                    }
                    prev_blank = false;
                }

                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.consecutive_blank_lines = 0;
            }

            SyntaxKind::FOOTNOTE_DEFINITION => {
                let mut marker = String::new();
                let mut child_blocks = Vec::new();

                for element in node.children_with_tokens() {
                    match element {
                        NodeOrToken::Token(token)
                            if matches!(
                                token.kind(),
                                SyntaxKind::FOOTNOTE_REFERENCE
                                    | SyntaxKind::FOOTNOTE_LABEL_START
                                    | SyntaxKind::FOOTNOTE_LABEL_ID
                                    | SyntaxKind::FOOTNOTE_LABEL_END
                                    | SyntaxKind::FOOTNOTE_LABEL_COLON
                            ) =>
                        {
                            marker.push_str(token.text());
                        }
                        NodeOrToken::Node(child) => {
                            child_blocks.push(child);
                        }
                        _ => {}
                    }
                }

                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(marker.trim_end());

                let child_indent = indent + 4;
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let mut first = true;
                let mut pending_blank_lines = 0usize;

                for child in &child_blocks {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        pending_blank_lines = pending_blank_lines.saturating_add(1);
                        continue;
                    }

                    if !first && pending_blank_lines > 0 && !self.output.ends_with("\n\n") {
                        self.output.push('\n');
                    }
                    pending_blank_lines = 0;

                    if first {
                        first = false;
                        if child.kind() == SyntaxKind::PARAGRAPH {
                            let marker_len = marker.len();
                            let first_line_space = self
                                .config
                                .line_width
                                .saturating_sub(indent + marker_len + 1);

                            let available_width =
                                self.config.line_width.saturating_sub(child_indent);
                            let widths = [first_line_space, available_width];
                            let lines = match wrap_mode {
                                WrapMode::Preserve => preserve_lines(
                                    child,
                                    self.config.formatter_extensions.escaped_line_breaks,
                                )
                                .iter()
                                .map(|line| {
                                    normalize_smart_punctuation(
                                        line,
                                        self.config.formatter_extensions.smart,
                                        self.config.formatter_extensions.smart_quotes,
                                    )
                                    .to_string()
                                })
                                .collect(),
                                WrapMode::Reflow => {
                                    self.wrapped_lines_for_paragraph_with_widths(child, &widths)
                                }
                                WrapMode::Sentence => self.sentence_lines_for_paragraph(child),
                                WrapMode::Semantic => self.semantic_lines_for_paragraph(child),
                            };

                            if !lines.is_empty() {
                                self.output.push(' ');
                                self.output
                                    .push_str(lines[0].trim_start_matches([' ', '\t']));
                                self.output.push('\n');
                                for line in lines.iter().skip(1) {
                                    self.output.push_str(&" ".repeat(child_indent));
                                    self.output.push_str(line.trim_start_matches([' ', '\t']));
                                    self.output.push('\n');
                                }
                                continue;
                            }
                        } else if matches!(
                            child.kind(),
                            SyntaxKind::DEFINITION_LIST
                                | SyntaxKind::HTML_BLOCK
                                | SyntaxKind::HTML_BLOCK_RAW
                                | SyntaxKind::HTML_BLOCK_DIV
                        ) {
                            self.output.push(' ');
                            self.format_node_sync(child, child_indent);
                            continue;
                        }

                        self.output.push('\n');
                    }

                    match child.kind() {
                        SyntaxKind::PARAGRAPH => {
                            let para_start = self.output.len();
                            let available_width =
                                self.config.line_width.saturating_sub(child_indent);

                            match wrap_mode {
                                WrapMode::Preserve => {
                                    let escaped =
                                        self.config.formatter_extensions.escaped_line_breaks;
                                    for line in preserve_lines(child, escaped) {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(
                                            normalize_smart_punctuation(
                                                line.trim_start_matches([' ', '\t']),
                                                self.config.formatter_extensions.smart,
                                                self.config.formatter_extensions.smart_quotes,
                                            )
                                            .as_ref(),
                                        );
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Reflow => {
                                    let lines =
                                        self.wrapped_lines_for_paragraph(child, available_width);
                                    for line in lines {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(line.trim_start_matches([' ', '\t']));
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Sentence | WrapMode::Semantic => {
                                    let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                                        self.semantic_lines_for_paragraph(child)
                                    } else {
                                        self.sentence_lines_for_paragraph(child)
                                    };
                                    for line in lines {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(line.trim_start_matches([' ', '\t']));
                                        self.output.push('\n');
                                    }
                                }
                            }
                            self.guard_definition_marker_start(para_start, child_indent);
                        }
                        SyntaxKind::BLANK_LINE => {
                            self.output.push('\n');
                        }
                        SyntaxKind::CODE_BLOCK => {
                            let mut code_lines = Vec::new();
                            for code_child in child.children() {
                                if code_child.kind() == SyntaxKind::CODE_CONTENT {
                                    let mut line_content = String::new();
                                    for token in code_child.children_with_tokens() {
                                        if let NodeOrToken::Token(t) = token {
                                            match t.kind() {
                                                SyntaxKind::WHITESPACE => {}
                                                SyntaxKind::TEXT => {
                                                    line_content.push_str(t.text());
                                                }
                                                SyntaxKind::NEWLINE => {
                                                    code_lines.push(line_content.clone());
                                                    line_content.clear();
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    if !line_content.is_empty() {
                                        code_lines.push(line_content);
                                    }
                                }
                            }

                            while code_lines.last().is_some_and(|l| l.is_empty()) {
                                code_lines.pop();
                            }

                            self.output.push_str(&" ".repeat(child_indent));
                            self.output.push_str("```\n");
                            for line in code_lines {
                                if !line.is_empty() {
                                    self.output.push_str(&" ".repeat(child_indent));
                                    self.output.push_str(&line);
                                }
                                self.output.push('\n');
                            }
                            self.output.push_str(&" ".repeat(child_indent));
                            self.output.push_str("```\n");
                        }
                        _ => {
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node_sync(child, child_indent);
                            let formatted = self.output.clone();
                            self.output = saved_output;
                            self.output.push_str(&formatted);
                        }
                    }
                }

                if child_blocks.is_empty() {
                    self.output.push('\n');
                }

                if let Some(next) = node.next_sibling() {
                    let next_kind = next.kind();
                    if next_kind == SyntaxKind::FOOTNOTE_DEFINITION
                        && !self.output.ends_with("\n\n")
                    {
                        self.output.push('\n');
                    }
                }
            }

            SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_RAW | SyntaxKind::HTML_BLOCK_DIV => {
                self.format_html_block(node)
            }
            SyntaxKind::COMMENT => self.format_comment(node),
            SyntaxKind::LATEX_COMMAND => self.format_latex_command(node),
            SyntaxKind::TEX_BLOCK => self.format_tex_block(node),

            SyntaxKind::BLOCK_QUOTE => {
                log::trace!("Formatting blockquote");
                let depth = node
                    .ancestors()
                    .take_while(|ancestor| ancestor.kind() != SyntaxKind::LIST_ITEM)
                    .filter(|ancestor| ancestor.kind() == SyntaxKind::BLOCK_QUOTE)
                    .count()
                    .max(1);

                let base_indent = " ".repeat(indent);
                let content_prefix = format!("{}{}", base_indent, "> ".repeat(depth)); // includes trailing space
                let blank_prefix = content_prefix.trim_end().to_string(); // no trailing space

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let blockquote_children: Vec<_> = node.children().collect();
                let saved_blockquote_context = self.blockquote_context.clone();
                self.blockquote_context = Some(BlockquoteContext {
                    in_list_continuation: false,
                });

                for child in &blockquote_children {
                    match child.kind() {
                        SyntaxKind::BLOCK_QUOTE_MARKER => continue,

                        SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN => {
                            let para_start = self.output.len();
                            match wrap_mode {
                                WrapMode::Preserve => {
                                    let escaped =
                                        self.config.formatter_extensions.escaped_line_breaks;
                                    for line in preserve_lines_unprefixed(child, escaped) {
                                        if line.is_empty() {
                                            self.output.push_str(content_prefix.trim_end());
                                        } else {
                                            self.output.push_str(&content_prefix);
                                            self.output.push_str(&line);
                                        }
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Reflow => {
                                    let width =
                                        self.config.line_width.saturating_sub(content_prefix.len());
                                    let lines = self.wrapped_lines_for_paragraph(child, width);
                                    for line in lines {
                                        self.output.push_str(&content_prefix);
                                        self.output.push_str(&line);
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Sentence | WrapMode::Semantic => {
                                    let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                                        self.semantic_lines_for_paragraph(child)
                                    } else {
                                        self.sentence_lines_for_paragraph(child)
                                    };
                                    for line in lines {
                                        self.output.push_str(&content_prefix);
                                        self.output.push_str(&line);
                                        self.output.push('\n');
                                    }
                                }
                            }
                            self.guard_definition_marker_start(para_start, 0);
                        }
                        SyntaxKind::ALERT => {
                            let marker = child
                                .children_with_tokens()
                                .filter_map(|item| item.into_token())
                                .find(|tok| tok.kind() == SyntaxKind::ALERT_MARKER)
                                .map(|tok| tok.text().to_string())
                                .unwrap_or_else(|| "[!NOTE]".to_string());

                            self.output.push_str(&content_prefix);
                            self.output.push_str(&marker);
                            self.output.push('\n');

                            for alert_child in child.children() {
                                match alert_child.kind() {
                                    SyntaxKind::PARAGRAPH => match wrap_mode {
                                        WrapMode::Preserve => {
                                            let escaped = self
                                                .config
                                                .formatter_extensions
                                                .escaped_line_breaks;
                                            for line in preserve_lines(&alert_child, escaped) {
                                                if line.is_empty() {
                                                    self.output.push_str(content_prefix.trim_end());
                                                } else {
                                                    self.output.push_str(&content_prefix);
                                                    self.output.push_str(&line);
                                                }
                                                self.output.push('\n');
                                            }
                                        }
                                        WrapMode::Reflow => {
                                            let width = self
                                                .config
                                                .line_width
                                                .saturating_sub(content_prefix.len());
                                            for line in self
                                                .wrapped_lines_for_paragraph(&alert_child, width)
                                            {
                                                self.output.push_str(&content_prefix);
                                                self.output.push_str(&line);
                                                self.output.push('\n');
                                            }
                                        }
                                        WrapMode::Sentence | WrapMode::Semantic => {
                                            let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                                                self.semantic_lines_for_paragraph(&alert_child)
                                            } else {
                                                self.sentence_lines_for_paragraph(&alert_child)
                                            };
                                            for line in lines {
                                                self.output.push_str(&content_prefix);
                                                self.output.push_str(&line);
                                                self.output.push('\n');
                                            }
                                        }
                                    },
                                    SyntaxKind::BLANK_LINE => {
                                        self.output.push_str(&blank_prefix);
                                        self.output.push('\n');
                                    }
                                    _ => {
                                        let rendered = self.render_to_buffer(
                                            &alert_child,
                                            indent,
                                            content_prefix.len(),
                                        );
                                        self.append_blockquote_prefixed_nested_block(
                                            &rendered,
                                            &base_indent,
                                            &content_prefix,
                                            &blank_prefix,
                                        );
                                    }
                                }
                            }
                        }
                        SyntaxKind::BLANK_LINE => {
                            self.output.push_str(&blank_prefix);
                            self.output.push('\n');
                        }
                        SyntaxKind::HORIZONTAL_RULE => {
                            self.output.push_str(&content_prefix);
                            let available_width =
                                self.config.line_width.saturating_sub(content_prefix.len());
                            self.output
                                .push_str(&self.horizontal_rule_text(available_width));
                            self.output.push('\n');
                        }
                        SyntaxKind::HEADING => {
                            let heading_text = self.format_heading(child);
                            for line in heading_text.lines() {
                                self.output.push_str(&content_prefix);
                                self.output.push_str(line);
                                self.output.push('\n');
                            }
                            if let Some(next) = child.next_sibling()
                                && next.kind() != SyntaxKind::BLANK_LINE
                                && is_block_element(next.kind())
                            {
                                self.output.push_str(&blank_prefix);
                                self.output.push('\n');
                            }
                        }
                        SyntaxKind::LIST => {
                            let list_output = self.render_to_buffer(child, 0, content_prefix.len());

                            let ends_in_list_continuation = self
                                .append_blockquote_prefixed_list_output(
                                    &list_output,
                                    &base_indent,
                                    &content_prefix,
                                    &blank_prefix,
                                );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = ends_in_list_continuation;
                            }
                        }
                        SyntaxKind::CODE_BLOCK => {
                            let code_block_leading_indent = Self::code_block_leading_indent(child);
                            let code_output = self.render_to_buffer(child, indent, 0);

                            self.append_blockquote_prefixed_block(
                                &code_output,
                                &content_prefix,
                                &blank_prefix,
                                Some(&code_block_leading_indent),
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        SyntaxKind::HTML_BLOCK
                        | SyntaxKind::HTML_BLOCK_RAW
                        | SyntaxKind::HTML_BLOCK_DIV => {
                            let html_output = self.render_to_buffer(child, indent, 0);

                            self.append_blockquote_prefixed_block(
                                &html_output,
                                &content_prefix,
                                &blank_prefix,
                                None,
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        SyntaxKind::TEX_BLOCK => {
                            let tex_output = self.render_to_buffer(child, indent, 0);

                            self.append_blockquote_prefixed_block(
                                &tex_output,
                                &content_prefix,
                                &blank_prefix,
                                None,
                            );
                        }
                        SyntaxKind::PIPE_TABLE
                        | SyntaxKind::GRID_TABLE
                        | SyntaxKind::SIMPLE_TABLE
                        | SyntaxKind::MULTILINE_TABLE => {
                            let table_output = self.render_to_buffer(child, 0, 0);

                            let min_indent = table_output
                                .lines()
                                .filter(|line| !line.trim().is_empty())
                                .map(|line| line.len() - line.trim_start().len())
                                .min()
                                .unwrap_or(0);
                            let dedented: String = table_output
                                .lines()
                                .map(|line| {
                                    if line.trim().is_empty() {
                                        String::new()
                                    } else {
                                        line[min_indent..].to_string()
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            self.append_blockquote_prefixed_block(
                                &dedented,
                                &content_prefix,
                                &blank_prefix,
                                None,
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        SyntaxKind::LINE_BLOCK => {
                            let line_block_output = self.render_to_buffer(child, 0, 0);

                            self.append_blockquote_prefixed_block(
                                &line_block_output,
                                &content_prefix,
                                &blank_prefix,
                                None,
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        SyntaxKind::DEFINITION_LIST => {
                            let def_output = self.render_to_buffer(child, indent, 0);

                            self.append_blockquote_prefixed_block(
                                &def_output,
                                &content_prefix,
                                &blank_prefix,
                                None,
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        SyntaxKind::BLOCK_QUOTE => {
                            self.format_node_sync(child, indent);
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        _ => {
                            let rendered = self.render_to_buffer(child, 0, content_prefix.len());

                            self.append_blockquote_prefixed_nested_block(
                                &rendered,
                                &base_indent,
                                &content_prefix,
                                &blank_prefix,
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                    }
                }
                self.blockquote_context = saved_blockquote_context;
            }

            SyntaxKind::PARAGRAPH => {
                if indent == 0
                    && node
                        .prev_sibling()
                        .is_some_and(|prev| prev.kind() == SyntaxKind::LIST)
                    && self.output.ends_with('\n')
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }

                let para_start = self.output.len();
                let text = node.text().to_string();
                log::trace!("Formatting paragraph, text length: {}", text.len());
                let paragraph_indent = " ".repeat(indent);

                if self.is_grid_table_continuation_paragraph(node) {
                    if indent > 0 {
                        for (i, line) in text.lines().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(&paragraph_indent);
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    line.trim_start(),
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    } else {
                        self.output.push_str(
                            normalize_smart_punctuation(
                                &text,
                                self.config.formatter_extensions.smart,
                                self.config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                if self.config.formatter_extensions.bookdown_references
                    && paragraphs::is_bookdown_text_reference(node)
                {
                    if indent > 0 {
                        for (i, line) in text.lines().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(&paragraph_indent);
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    line.trim_start(),
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    } else {
                        self.output.push_str(
                            normalize_smart_punctuation(
                                &text,
                                self.config.formatter_extensions.smart,
                                self.config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let preserve_newlines_for_latex =
                    self.fenced_div_depth > 0 && self.contains_latex_command(node);
                if preserve_newlines_for_latex && self.fenced_div_depth > 0 {
                    if indent > 0 {
                        for (i, line) in text.lines().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(&paragraph_indent);
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    line.trim_start(),
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    } else {
                        self.output.push_str(
                            normalize_smart_punctuation(
                                &text,
                                self.config.formatter_extensions.smart,
                                self.config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }
                log::trace!(
                    "Paragraph wrap mode: {:?}, line_width: {}",
                    wrap_mode,
                    line_width
                );
                match wrap_mode {
                    WrapMode::Preserve => {
                        log::trace!("Preserving paragraph line breaks");
                        let escaped = self.config.formatter_extensions.escaped_line_breaks;
                        for (i, line) in preserve_lines(node, escaped).iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            if indent > 0 {
                                self.output.push_str(&paragraph_indent);
                            }
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    if indent > 0 { line.trim_start() } else { line },
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Reflow => {
                        log::trace!("Reflowing paragraph to {} width", line_width);
                        let lines = self.wrapped_lines_for_paragraph(node, line_width);

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            if indent > 0 {
                                self.output.push_str(&paragraph_indent);
                            }
                            self.output.push_str(line);
                        }
                    }
                    WrapMode::Sentence | WrapMode::Semantic => {
                        let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                            log::trace!("Wrapping paragraph by semantic line breaks");
                            self.semantic_lines_for_paragraph(node)
                        } else {
                            log::trace!("Wrapping paragraph by sentence");
                            self.sentence_lines_for_paragraph(node)
                        };

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            if indent > 0 {
                                self.output.push_str(&paragraph_indent);
                            }
                            self.output.push_str(line);
                        }
                    }
                }

                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }

                self.guard_dash_block_marker(para_start, node, indent);
                self.guard_definition_marker_start(para_start, indent);
            }

            SyntaxKind::FIGURE => {
                log::trace!("Formatting figure");
                let text = self.format_inline_node(node);
                let trimmed = text.trim();
                if indent > 0 && !self.output.ends_with(":   ") {
                    self.output.push_str(&" ".repeat(indent));
                }
                self.output.push_str(trimmed);
                self.output.push('\n');
            }

            SyntaxKind::PLAIN => {
                let text = node.text().to_string();
                log::trace!("Formatting Plain block, text length: {}", text.len());

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let needs_indent = indent > 0
                    && (self.output.ends_with('\n') || self.output.is_empty())
                    && !self.output.ends_with(":   ");
                match wrap_mode {
                    WrapMode::Preserve => {
                        let escaped = self.config.formatter_extensions.escaped_line_breaks;
                        for (i, line) in preserve_lines(node, escaped).iter().enumerate() {
                            if needs_indent {
                                self.output.push_str(&" ".repeat(indent));
                            } else if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    if needs_indent {
                                        line.trim_start()
                                    } else {
                                        line
                                    },
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                            if needs_indent {
                                self.output.push('\n');
                            }
                        }
                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Reflow => {
                        log::trace!("Reflowing Plain block to {} width", line_width);
                        let in_definition = self.output.ends_with(":   ");
                        let preserve_ambiguous_definition_emphasis =
                            in_definition && text.contains(r"\|*") && text.contains(".*");
                        let lines = if in_definition {
                            if preserve_ambiguous_definition_emphasis {
                                text.lines().map(ToString::to_string).collect()
                            } else {
                                let marker_len = ":   ".len();
                                let marker_indent = indent.saturating_sub(4);
                                let first_line_space =
                                    line_width.saturating_sub(marker_indent + marker_len);
                                let continuation_width = line_width.saturating_sub(indent);
                                let widths = [first_line_space, continuation_width];
                                self.wrapped_lines_for_paragraph_with_widths(node, &widths)
                            }
                        } else {
                            self.wrapped_lines_for_paragraph(node, line_width)
                        };

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                                self.output.push_str(&" ".repeat(indent));
                            } else if needs_indent {
                                self.output.push_str(&" ".repeat(indent));
                            }
                            let rendered = if i > 0 && indent > 0 {
                                line.trim_start()
                            } else {
                                line.as_str()
                            };
                            self.output.push_str(rendered);
                        }

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Sentence | WrapMode::Semantic => {
                        let in_definition = self.output.ends_with(":   ");
                        let preserve_ambiguous_definition_emphasis =
                            in_definition && text.contains(r"\|*") && text.contains(".*");
                        let lines = if preserve_ambiguous_definition_emphasis {
                            text.lines().map(ToString::to_string).collect()
                        } else if matches!(wrap_mode, WrapMode::Semantic) {
                            log::trace!("Wrapping Plain block by semantic line breaks");
                            self.semantic_lines_for_paragraph(node)
                        } else {
                            log::trace!("Wrapping Plain block by sentence");
                            self.sentence_lines_for_paragraph(node)
                        };

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                                self.output.push_str(&" ".repeat(indent));
                            } else if needs_indent {
                                self.output.push_str(&" ".repeat(indent));
                            }
                            let rendered = if i > 0 && indent > 0 {
                                line.trim_start()
                            } else {
                                line.as_str()
                            };
                            self.output.push_str(rendered);
                        }

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                }
            }

            SyntaxKind::LIST => {
                self.format_list(node, indent);
            }

            SyntaxKind::DEFINITION_LIST => {
                if self.is_grid_table_caption_definition_list(node) {
                    self.output.push_str(&node.text().to_string());
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                let mut saw_item = false;
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        continue;
                    }
                    if child.kind() == SyntaxKind::DEFINITION_ITEM {
                        if saw_item && !self.output.ends_with("\n\n") {
                            self.output.push('\n');
                        }
                        saw_item = true;
                    }
                    self.format_node_sync(&child, indent);
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LINE_BLOCK => {
                log::trace!("Formatting line block");
                if !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }

                let line_indent = " ".repeat(indent);

                let mut rendered: Vec<String> = Vec::new();
                for child in node.children() {
                    if child.kind() != SyntaxKind::LINE_BLOCK_LINE {
                        continue;
                    }
                    let mut content = String::new();
                    let mut has_marker = false;
                    let mut past_prefix = false;
                    for elem in child.children_with_tokens() {
                        let kind = elem.kind();
                        if !past_prefix
                            && matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::LINE_PREFIX)
                        {
                            continue;
                        }
                        past_prefix = true;
                        match kind {
                            SyntaxKind::LINE_BLOCK_MARKER => {
                                has_marker = true;
                                continue;
                            }
                            SyntaxKind::NEWLINE => break,
                            _ => match &elem {
                                NodeOrToken::Token(t) => content.push_str(t.text()),
                                NodeOrToken::Node(n) => content.push_str(&n.text().to_string()),
                            },
                        }
                    }
                    match rendered.last_mut() {
                        Some(previous) if !has_marker => {
                            let continuation = content.trim();
                            if !continuation.is_empty() {
                                if !previous.is_empty() {
                                    previous.push(' ');
                                }
                                previous.push_str(continuation);
                            }
                        }
                        _ => rendered.push(content.trim_end().to_string()),
                    }
                }

                for line in &rendered {
                    self.output.push_str(&line_indent);
                    if line.trim().is_empty() {
                        self.output.push('|');
                    } else {
                        self.output.push_str("| ");
                        self.output.push_str(line);
                    }
                    self.output.push('\n');
                }

                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::DEFINITION_ITEM => {
                let is_compact_by_structure = DefinitionItem::cast(node.clone())
                    .map(|item| item.is_compact())
                    .unwrap_or(true);
                let mut has_blank_between_term_and_first_definition = false;
                let mut seen_term = false;
                let mut seen_definition = false;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::TERM => {
                            seen_term = true;
                        }
                        SyntaxKind::BLANK_LINE => {
                            if seen_term && !seen_definition {
                                has_blank_between_term_and_first_definition = true;
                            }
                        }
                        SyntaxKind::DEFINITION => {
                            seen_definition = true;
                        }
                        _ => {}
                    }
                }

                let is_compact =
                    is_compact_by_structure && !has_blank_between_term_and_first_definition;
                let mut saw_term = false;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::BLANK_LINE => {}
                        SyntaxKind::TERM => {
                            self.format_node_sync(&child, indent);
                            saw_term = true;
                        }
                        SyntaxKind::DEFINITION => {
                            if saw_term {
                                if is_compact {
                                    if !self.output.ends_with('\n') {
                                        self.output.push('\n');
                                    }
                                } else if !self.output.ends_with("\n\n") {
                                    self.output.push('\n');
                                }
                            } else if !self.output.is_empty() && !self.output.ends_with('\n') {
                                self.output.push('\n');
                            }
                            self.format_node_sync(&child, indent);
                        }
                        _ => self.format_node_sync(&child, indent),
                    }
                }
            }

            SyntaxKind::TERM => {
                if indent > 0 && (self.output.is_empty() || self.output.ends_with('\n')) {
                    self.output.push_str(&" ".repeat(indent));
                }
                for child in node.children_with_tokens() {
                    match child {
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                            self.output.push_str(tok.text());
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                            self.output.push('\n');
                        }
                        NodeOrToken::Node(n) => {
                            self.format_node_sync(&n, indent);
                        }
                        _ => {}
                    }
                }
            }

            SyntaxKind::DEFINITION => {
                let def_indent = indent + 4;
                let saved_wrap = Self::reflow_would_promote_a_definition_term(node)
                    .then(|| self.config.wrap.replace(WrapMode::Preserve));
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

                if indent > 0 {
                    self.output.push_str(&" ".repeat(indent));
                }
                self.output.push_str(":   ");

                let children: Vec<_> = node.children_with_tokens().collect();
                let mut first_para_idx = None;

                let mut text_idx = None;
                for (i, child) in children.iter().enumerate() {
                    if let NodeOrToken::Token(tok) = child
                        && tok.kind() == SyntaxKind::TEXT
                    {
                        text_idx = Some(i);
                    }
                }

                if let Some(tidx) = text_idx {
                    for (i, child) in children.iter().enumerate().skip(tidx + 1) {
                        if let NodeOrToken::Node(n) = child {
                            match n.kind() {
                                SyntaxKind::PARAGRAPH => {
                                    first_para_idx = Some(i);
                                    break;
                                }
                                SyntaxKind::BLANK_LINE => {
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                for (i, child) in children.iter().enumerate() {
                    match child {
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                            self.output.push_str(tok.text());
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                            let bare_marker_pull_up = self.output.ends_with(":   ")
                                && children.get(i + 1).is_some_and(|next| match next {
                                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::PLAIN => {
                                        let first_line = n
                                            .text()
                                            .to_string()
                                            .lines()
                                            .next()
                                            .unwrap_or_default()
                                            .trim_start_matches([' ', '\t'])
                                            .to_string();
                                        try_parse_atx_heading(&first_line).is_none()
                                    }
                                    NodeOrToken::Node(n) => is_block_element(n.kind()),
                                    _ => false,
                                });
                            if first_para_idx.is_some_and(|idx| i + 1 == idx) {
                                self.output.push(' ');
                            } else if !bare_marker_pull_up {
                                if self.output.ends_with(":   ") {
                                    self.output.truncate(self.output.len() - 3);
                                }
                                self.output.push('\n');
                            }
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DEFINITION_MARKER => {}
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::WHITESPACE => {}
                        NodeOrToken::Node(n) => match n.kind() {
                            SyntaxKind::CODE_BLOCK => {
                                if self.output.ends_with(":   ") {
                                    self.format_container_code_block(
                                        n,
                                        "",
                                        def_indent,
                                        true,
                                        Some(Self::container_content_offset(n)),
                                        false,
                                    );
                                } else {
                                    if !self.output.ends_with("\n\n") {
                                        self.output.push('\n');
                                    }
                                    self.format_indented_code_block(n, def_indent);
                                }
                            }
                            SyntaxKind::HEADING => {
                                self.output.push_str(&self.format_heading(n));
                                self.output.push('\n');

                                let has_following_blocks =
                                    children.iter().skip(i + 1).any(|sib| match sib {
                                        NodeOrToken::Node(sn) => {
                                            sn.kind() != SyntaxKind::BLANK_LINE
                                        }
                                        _ => false,
                                    });
                                let next_is_blank_line = children.get(i + 1).is_some_and(|sib| {
                                    matches!(
                                        sib,
                                        NodeOrToken::Node(sn) if sn.kind() == SyntaxKind::BLANK_LINE
                                    )
                                });
                                if has_following_blocks && !next_is_blank_line {
                                    self.output.push('\n');
                                }
                            }
                            SyntaxKind::PLAIN => {
                                if let Some((heading_line, remainder)) =
                                    self.leading_atx_heading_with_remainder(n)
                                {
                                    self.output.push_str(&heading_line);
                                    self.output.push('\n');
                                    self.output.push('\n');
                                    for line in self.wrap_text_for_indent(&remainder, def_indent) {
                                        self.output.push_str(&" ".repeat(def_indent));
                                        self.output.push_str(line.trim_start());
                                        self.output.push('\n');
                                    }
                                } else {
                                    self.format_node_sync(n, def_indent);
                                }
                            }
                            SyntaxKind::PARAGRAPH => {
                                if first_para_idx == Some(i) {
                                    let marker_len = ":   ".len();
                                    let first_line_space =
                                        self.config.line_width.saturating_sub(indent + marker_len);
                                    let available_width =
                                        self.config.line_width.saturating_sub(def_indent);
                                    let widths = [first_line_space, available_width];

                                    let lines = match wrap_mode {
                                        WrapMode::Preserve => preserve_lines(
                                            n,
                                            self.config.formatter_extensions.escaped_line_breaks,
                                        ),
                                        WrapMode::Reflow => {
                                            self.wrapped_lines_for_paragraph_with_widths(n, &widths)
                                        }
                                        WrapMode::Sentence => self.sentence_lines_for_paragraph(n),
                                        WrapMode::Semantic => self.semantic_lines_for_paragraph(n),
                                    };

                                    if !lines.is_empty() {
                                        self.output.push_str(&lines[0]);
                                        self.output.push('\n');
                                        for line in lines.iter().skip(1) {
                                            self.output.push_str(&" ".repeat(def_indent));
                                            self.output.push_str(line.trim_start());
                                            self.output.push('\n');
                                        }
                                    }
                                } else {
                                    if !self.output.ends_with("\n\n") {
                                        self.output.push('\n');
                                    }
                                    self.format_list_continuation_paragraph(n, def_indent);
                                }
                            }
                            SyntaxKind::BLANK_LINE => {
                                let is_before_first_para =
                                    first_para_idx.is_some_and(|idx| i < idx);

                                if !is_before_first_para {
                                    self.output.push('\n');
                                }
                            }
                            SyntaxKind::LIST => {
                                let start = self.output.len();
                                self.format_node_sync(n, def_indent);

                                if self.output[..start].ends_with(":   ")
                                    && self.output[start..].starts_with(&" ".repeat(def_indent))
                                {
                                    self.output.drain(start..start + def_indent);
                                }
                            }
                            SyntaxKind::BLOCK_QUOTE => {
                                if self.output.ends_with(":   ") {
                                    let mut pieces: Vec<String> = Vec::new();
                                    let block_text = n.text().to_string();
                                    for line in block_text.lines() {
                                        let trimmed = line.trim_start();
                                        let content = if let Some(rest) = trimmed.strip_prefix('>')
                                        {
                                            rest.trim_start()
                                        } else {
                                            trimmed
                                        };
                                        if !content.is_empty() {
                                            pieces.push(content.to_string());
                                        }
                                    }

                                    self.output.push_str("> ");
                                    self.output.push_str(&pieces.join(" "));
                                    self.output.push('\n');

                                    if let Some(next_non_blank) = node
                                        .children()
                                        .skip(i + 1)
                                        .find(|sibling| sibling.kind() != SyntaxKind::BLANK_LINE)
                                        && is_block_element(next_non_blank.kind())
                                        && !self.output.ends_with("\n\n")
                                    {
                                        self.output.push('\n');
                                    }
                                } else {
                                    self.format_node_sync(n, def_indent);
                                }
                            }
                            _ => {
                                self.format_node_sync(n, def_indent);
                            }
                        },
                        _ => {}
                    }
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                if let Some(saved) = saved_wrap {
                    self.config.wrap = saved;
                }
            }

            SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::MULTILINE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE => self.format_table(node, indent),

            SyntaxKind::INLINE_MATH => self.format_inline_math(node),

            SyntaxKind::LIST_ITEM => {
                self.format_list_item(node, indent);
            }

            SyntaxKind::FENCED_DIV => {
                let Some(fenced_div) = FencedDiv::cast(node.clone()) else {
                    self.output.push_str(&node.text().to_string());
                    return;
                };

                let opening_has_trailing_inline_text =
                    fenced_div.opening_fence().is_some_and(|open| {
                        let mut saw_info = false;
                        for child in open.syntax().children_with_tokens() {
                            match child {
                                rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::DIV_INFO => {
                                    saw_info = true;
                                }
                                rowan::NodeOrToken::Token(t)
                                    if saw_info && t.kind() == SyntaxKind::TEXT =>
                                {
                                    let trimmed = t.text().trim();
                                    if !trimmed.is_empty() && !trimmed.chars().all(|c| c == ':') {
                                        return true;
                                    }
                                }
                                _ => {}
                            }
                        }
                        false
                    });
                if opening_has_trailing_inline_text {
                    self.output.push_str(&node.text().to_string());
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                let has_close = fenced_div.has_closing_fence();
                let has_content = fenced_div
                    .body_blocks()
                    .any(|child| child.kind() != SyntaxKind::BLANK_LINE);
                let leading_blank_lines = fenced_div
                    .body_blocks()
                    .take_while(|child| child.kind() == SyntaxKind::BLANK_LINE)
                    .count();

                if !has_close && !has_content {
                    if let Some(open) = fenced_div.opening_fence() {
                        self.output
                            .push_str(open.syntax().text().to_string().trim_end_matches('\n'));
                    } else {
                        self.output
                            .push_str(node.text().to_string().trim_end_matches('\n'));
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                let source_opening_colons = fenced_div
                    .opening_fence()
                    .map(|open| {
                        open.syntax()
                            .text()
                            .to_string()
                            .trim_start()
                            .chars()
                            .take_while(|&c| c == ':')
                            .count()
                    })
                    .unwrap_or(3)
                    .max(3);
                let in_list_item = node
                    .ancestors()
                    .any(|ancestor| ancestor.kind() == SyntaxKind::LIST_ITEM);
                let depth_encoded_colons = 3 + (self.fenced_div_depth * 2);
                let opening_colons = if in_list_item {
                    source_opening_colons
                } else {
                    depth_encoded_colons
                };
                let colons = ":".repeat(opening_colons);

                let attributes = fenced_div.info_text();
                if !has_close && !has_content {
                    self.output.push_str(&" ".repeat(indent));
                    if let Some(attrs) = &attributes
                        && !attrs.is_empty()
                    {
                        self.output.push_str(&colons);
                        self.output.push(' ');
                        self.output.push_str(attrs);
                        self.output.push('\n');
                        return;
                    }
                } else {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(&colons);
                    if let Some(attrs) = &attributes
                        && !attrs.is_empty()
                    {
                        self.output.push(' ');
                        self.output.push_str(attrs);
                    }
                    self.output.push('\n');
                }

                self.fenced_div_depth += 1;

                let content_children: Vec<_> = node
                    .children()
                    .filter(|child| {
                        !matches!(
                            child.kind(),
                            SyntaxKind::DIV_FENCE_OPEN
                                | SyntaxKind::DIV_INFO
                                | SyntaxKind::DIV_FENCE_CLOSE
                        )
                    })
                    .collect();

                let trailing_blank_lines = content_children
                    .iter()
                    .rev()
                    .take_while(|child| child.kind() == SyntaxKind::BLANK_LINE)
                    .count();
                let first_non_blank_kind = content_children
                    .iter()
                    .find(|child| child.kind() != SyntaxKind::BLANK_LINE)
                    .map(|child| child.kind());
                let start = leading_blank_lines;
                let end = content_children.len().saturating_sub(trailing_blank_lines);
                let end = end.max(start);

                let mut prev_was_blank = false;
                for (idx, child) in content_children[start..end].iter().enumerate() {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        if idx < leading_blank_lines
                            && matches!(
                                first_non_blank_kind,
                                Some(SyntaxKind::LIST | SyntaxKind::LIST_ITEM)
                            )
                        {
                            continue;
                        }
                        if !prev_was_blank {
                            self.output.push('\n');
                            prev_was_blank = true;
                        }
                        continue;
                    }
                    prev_was_blank = false;
                    if child.kind() == SyntaxKind::CODE_BLOCK && indent > 0 {
                        self.format_indented_code_block(child, indent);
                        if let Some(next) = content_children[start..end].get(idx + 1)
                            && ((next.kind() == SyntaxKind::PARAGRAPH
                                && next.text().to_string().trim_start().starts_with(":::"))
                                || (next.kind() == SyntaxKind::PLAIN
                                    && next.text().to_string().trim_start().starts_with(":::"))
                                || next.kind() == SyntaxKind::FENCED_DIV)
                            && !self.output.ends_with("\n\n")
                        {
                            self.output.push('\n');
                        }
                    } else {
                        self.format_node_sync(child, indent);
                    }
                }

                self.fenced_div_depth -= 1;

                let last_non_blank_kind = content_children
                    .iter()
                    .rev()
                    .find(|c| c.kind() != SyntaxKind::BLANK_LINE)
                    .map(|c| c.kind());
                if last_non_blank_kind == Some(SyntaxKind::HORIZONTAL_RULE)
                    && self.output.ends_with('\n')
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }

                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(&":".repeat(opening_colons));
                self.output.push('\n');

                self.consecutive_blank_lines = 0;

                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    let needs_separator = if in_list_item {
                        matches!(
                            next.kind(),
                            SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN | SyntaxKind::LIST
                        )
                    } else {
                        true
                    };
                    if needs_separator {
                        self.output.push('\n');
                        self.consecutive_blank_lines = 1;
                    }
                }
            }

            SyntaxKind::INLINE_MATH_MARKER => {
                self.output.push_str(node.text().to_string().trim());
            }

            SyntaxKind::DISPLAY_MATH => self.format_display_math(node),

            SyntaxKind::CODE_BLOCK => {
                log::trace!("Formatting code block");

                if let Some(prev_sibling) = node.prev_sibling()
                    && prev_sibling.kind() == SyntaxKind::PARAGRAPH
                    && !self.output.ends_with("\n\n")
                    && !self.output.ends_with("\n \n")
                {
                    self.output.push('\n');
                }

                self.format_code_block(node);
            }

            SyntaxKind::YAML_METADATA
            | SyntaxKind::PANDOC_TITLE_BLOCK
            | SyntaxKind::MMD_TITLE_BLOCK => {
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                    self.consecutive_blank_lines = 1;
                }
            }

            SyntaxKind::BLANK_LINE => {
                if self.output.is_empty() {
                    return;
                }
                if self.consecutive_blank_lines < 1 {
                    self.output.push('\n');
                    self.consecutive_blank_lines += 1;
                }
            }

            SyntaxKind::EMPHASIS => {
                self.output.push('*');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::EMPHASIS_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('*');
            }

            SyntaxKind::STRONG => {
                self.output.push_str("**");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::STRONG_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("**");
            }

            SyntaxKind::STRIKEOUT => {
                self.output.push_str("~~");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::STRIKEOUT_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("~~");
            }

            SyntaxKind::SUPERSCRIPT => {
                self.output.push('^');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::SUPERSCRIPT_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('^');
            }

            SyntaxKind::SUBSCRIPT => {
                self.output.push('~');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::SUBSCRIPT_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('~');
            }

            SyntaxKind::MYST_DIRECTIVE => {
                let mut open_text: Option<String> = None;
                let mut close_text: Option<String> = None;
                let mut options = Vec::new();
                let mut body = Vec::new();
                let mut verbatim_body: Option<SyntaxNode> = None;
                for element in node.children_with_tokens() {
                    if let NodeOrToken::Node(child) = element {
                        match child.kind() {
                            SyntaxKind::MYST_DIRECTIVE_OPEN => {
                                open_text = Some(child.text().to_string());
                            }
                            SyntaxKind::MYST_DIRECTIVE_CLOSE => {
                                close_text = Some(child.text().to_string());
                            }
                            SyntaxKind::MYST_DIRECTIVE_OPTION => options.push(child),
                            SyntaxKind::MYST_DIRECTIVE_BODY => verbatim_body = Some(child),
                            _ => body.push(child),
                        }
                    }
                }

                if let Some(open) = &open_text {
                    self.output.push_str(open.trim_end_matches('\n'));
                    self.output.push('\n');
                }

                for option in &options {
                    self.output.push_str(&format_directive_option(option));
                    self.output.push('\n');
                }

                if let Some(body_node) = verbatim_body {
                    let body_text = code_blocks::extract_myst_directive_parts(node)
                        .and_then(|(language, body)| {
                            self.formatted_code.get(&(language, body)).cloned()
                        })
                        .unwrap_or_else(|| body_node.text().to_string());
                    self.output.push_str(body_text.trim_end_matches('\n'));
                    self.output.push('\n');
                    if let Some(close) = &close_text {
                        self.output.push_str(close.trim_end_matches('\n'));
                        self.output.push('\n');
                    }
                    self.consecutive_blank_lines = 0;
                    return;
                }

                let leading = body
                    .iter()
                    .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
                    .count();
                let trailing = body
                    .iter()
                    .rev()
                    .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
                    .count();
                let end = body.len().saturating_sub(trailing).max(leading);

                if !options.is_empty() && leading < end {
                    self.output.push('\n');
                }

                let mut prev_blank = false;
                for child in &body[leading..end] {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        if !prev_blank {
                            self.output.push('\n');
                            prev_blank = true;
                        }
                        continue;
                    }
                    self.format_node_sync(child, indent);
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    prev_blank = false;
                }

                if let Some(close) = &close_text {
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    self.output.push_str(close.trim_end_matches('\n'));
                    self.output.push('\n');
                }
                self.consecutive_blank_lines = 0;
            }

            SyntaxKind::MYST_TARGET
            | SyntaxKind::MYST_COMMENT
            | SyntaxKind::MYST_BLOCK_BREAK
            | SyntaxKind::SVELTE_BLOCK => {
                self.output
                    .push_str(node.text().to_string().trim_end_matches('\n'));
                self.output.push('\n');
                self.consecutive_blank_lines = 0;
            }

            _ => {
                self.output.push_str(&node.text().to_string());
            }
        }
    }
}

/// Render a `MYST_DIRECTIVE_OPTION` node in canonical form: `:name: value`, or
/// `:name:` when the option has no value. A single space follows the closing
/// colon so the output re-parses to the same option CST (idempotency).
fn format_directive_option(node: &SyntaxNode) -> String {
    let mut name = String::new();
    let mut value = String::new();
    for element in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::MYST_DIRECTIVE_OPTION_NAME => name = token.text().to_string(),
                SyntaxKind::MYST_DIRECTIVE_OPTION_VALUE => value = token.text().to_string(),
                _ => {}
            }
        }
    }
    if value.is_empty() {
        format!(":{name}:")
    } else {
        format!(":{name}: {value}")
    }
}

/// Rewrite a Pandoc `{...}` attribute block in canonical form: id first, then
/// classes in source order, then key/value pairs with quoted values.
///
/// Components are rendered from their *source* slices rather than from derived
/// semantics, so a bare `-` (pandoc's shorthand for `.unnumbered`) survives as
/// `-` instead of being expanded — the shorthand is the preferred spelling in
/// non-English documents, and expanding it would rewrite the author's source.
pub(super) fn normalize_attribute_text(attr_text: &str) -> String {
    let Some(inner) = attr_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
    else {
        return attr_text.to_string();
    };
    let Some(spans) = attribute_content_spans(inner) else {
        return attr_text.to_string();
    };

    let mut id = String::new();
    let mut classes: Vec<&str> = Vec::new();
    let mut key_values: Vec<String> = Vec::new();
    for comp in &spans.components {
        match comp {
            AttrComponent::Id(r) => id = inner[r.clone()].to_string(),
            AttrComponent::Class(r) | AttrComponent::Unnumbered(r) => {
                classes.push(&inner[r.clone()])
            }
            AttrComponent::KeyValue { key, value, .. } => {
                let value = strip_attr_value_quotes(&inner[value.clone()]);
                key_values.push(format!(
                    "{}=\"{}\"",
                    &inner[key.clone()],
                    value.replace('"', "\\\"")
                ));
            }
        }
    }

    let mut out = String::from("{");
    if !id.is_empty() {
        out.push_str(&id);
    }
    for part in classes.into_iter().map(str::to_string).chain(key_values) {
        if out.len() > 1 {
            out.push(' ');
        }
        out.push_str(&part);
    }
    out.push('}');
    out
}

fn strip_attr_value_quotes(raw: &str) -> &str {
    match raw.as_bytes().first() {
        Some(&q @ (b'"' | b'\'')) => {
            let inner = &raw[1..];
            inner.strip_suffix(q as char).unwrap_or(inner)
        }
        _ => raw,
    }
}

/// Render a `SPAN_ATTRIBUTES` node, collapsing interior whitespace runs to a
/// single space. Reads the node's `.text()` rather than its children, so it is
/// independent of whether the body is structured into `ATTR_*` tokens. This
/// reproduces the historical span normalization (preserve token order, single-
/// space separation) byte-for-byte.
pub(super) fn normalize_span_attributes(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    let inner = text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(text.as_str());
    let joined = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{{{joined}}}")
}
