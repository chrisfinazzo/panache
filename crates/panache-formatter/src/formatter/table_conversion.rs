//! Explicit, content-preserving conversions between Markdown table styles.

use crate::{
    Config,
    syntax::{AstNode, SyntaxKind, SyntaxNode, Table, TableAlignment, text_without_line_prefixes},
};
use panache_parser::parser::inlines::core::parse_inline_text_recursive;
use rowan::{GreenNodeBuilder, NodeOrToken};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    inline::collapse_spaces,
    tables::{Alignment, pad_simple_cell},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStyle {
    Simple,
    Multiline,
}

impl TableStyle {
    pub fn syntax_kind(self) -> SyntaxKind {
        match self {
            Self::Simple => SyntaxKind::SIMPLE_TABLE,
            Self::Multiline => SyntaxKind::MULTILINE_TABLE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableConversionError {
    UnsupportedSource,
    DisabledExtension,
    RaggedRows,
    MissingBody,
    HardLineBreak,
    MultilineLiteral,
    EastAsianLineBreak,
    Alignment,
    ContainerLayout,
    InvalidOutput,
}

impl std::fmt::Display for TableConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedSource => "Grid table conversion is not supported yet",
            Self::DisabledExtension => "The destination table extension is disabled",
            Self::RaggedRows => "Rows do not all have the declared number of columns",
            Self::MissingBody => "The table has no body rows",
            Self::HardLineBreak => "A cell contains a hard line break",
            Self::MultilineLiteral => "A cell contains an inline construct spanning multiple lines",
            Self::EastAsianLineBreak => "Conversion of East Asian line breaks is not supported yet",
            Self::Alignment => "The destination cannot preserve an explicit column alignment",
            Self::ContainerLayout => {
                "The destination cannot preserve this table's container layout"
            }
            Self::InvalidOutput => {
                "The conversion cannot preserve this table's structure and content"
            }
        })
    }
}

impl std::error::Error for TableConversionError {}

/// Breaks are permitted only between pieces, never inside an inline construct.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cell {
    pieces: Vec<String>,
}

impl Cell {
    fn parse(text: &str, config: &Config) -> Result<Self, TableConversionError> {
        // With this extension, replacing a soft break between wide characters
        // with a space changes content. Preserve the source until the converter
        // can represent that distinction in its inline pieces.
        if config.parser_extensions.east_asian_line_breaks
            && text.lines().zip(text.lines().skip(1)).any(|(left, right)| {
                left.trim_end()
                    .chars()
                    .next_back()
                    .and_then(UnicodeWidthChar::width)
                    == Some(2)
                    && right
                        .trim_start()
                        .chars()
                        .next()
                        .and_then(UnicodeWidthChar::width)
                        == Some(2)
            })
        {
            return Err(TableConversionError::EastAsianLineBreak);
        }
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::TABLE_CELL.into());
        parse_inline_text_recursive(&mut builder, text, &config.parser_options(), false);
        builder.finish_node();
        let node = SyntaxNode::new_root(builder.finish());
        if node
            .descendants_with_tokens()
            .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK)
        {
            return Err(TableConversionError::HardLineBreak);
        }
        let mut pieces = Vec::new();
        let mut current = String::new();
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::TEXT | SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE
                    ) =>
                {
                    for ch in token.text().chars() {
                        if ch.is_ascii_whitespace() {
                            if !current.is_empty() {
                                pieces.push(std::mem::take(&mut current));
                            }
                        } else {
                            current.push(ch);
                        }
                    }
                }
                NodeOrToken::Node(node) => {
                    let text = normalize_prose(&node);
                    if text.contains(['\n', '\r']) {
                        return Err(TableConversionError::MultilineLiteral);
                    }
                    current.push_str(&text);
                }
                element => {
                    let text = element.to_string();
                    if text.contains(['\n', '\r']) {
                        return Err(TableConversionError::MultilineLiteral);
                    }
                    current.push_str(&text);
                }
            }
        }
        if !current.is_empty() {
            pieces.push(current);
        }
        Ok(Self { pieces })
    }

    fn text(&self) -> String {
        self.pieces.join(" ")
    }

    fn minimum_width(&self) -> usize {
        self.pieces.iter().map(|s| s.width()).max().unwrap_or(0)
    }

    fn wrap(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for piece in &self.pieces {
            if !line.is_empty() && line.width() + 1 + piece.width() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(piece);
        }
        if !line.is_empty() || lines.is_empty() {
            lines.push(line);
        }
        lines
    }
}

fn normalize_prose(node: &SyntaxNode) -> String {
    if !matches!(
        node.kind(),
        SyntaxKind::EMPHASIS
            | SyntaxKind::STRONG
            | SyntaxKind::STRIKEOUT
            | SyntaxKind::LINK
            | SyntaxKind::LINK_TEXT
            | SyntaxKind::IMAGE_LINK
            | SyntaxKind::IMAGE_ALT
    ) {
        return node.text().to_string();
    }
    node.children_with_tokens()
        .map(|element| match element {
            NodeOrToken::Token(token)
                if matches!(token.kind(), SyntaxKind::TEXT | SyntaxKind::NEWLINE) =>
            {
                collapse_spaces(token.text())
            }
            NodeOrToken::Node(node) => normalize_prose(&node),
            element => element.to_string(),
        })
        .collect()
}

/// Shared with ordinary multiline formatting so a subsequent format cannot
/// split an inline construct that conversion deliberately kept intact.
pub(super) fn reflow_inline_cell(
    lines: &[String],
    width: usize,
    config: &Config,
) -> Option<Vec<String>> {
    Cell::parse(&lines.join("\n"), config)
        .ok()
        .map(|cell| cell.wrap(width))
}

#[derive(Debug, PartialEq, Eq)]
struct LogicalTable {
    rows: Vec<Vec<Cell>>,
    alignments: Vec<TableAlignment>,
    has_header: bool,
    caption: Option<(bool, String)>,
}

/// Separator positions are display columns, not byte or character offsets.
fn column_slice(text: &str, start: usize, end: usize) -> &str {
    let mut width = 0;
    let mut first = None;
    let mut last = text.len();
    for (offset, ch) in text.char_indices() {
        let char_width = ch.width().unwrap_or(0);
        if first.is_none() && width >= start && (char_width > 0 || offset == 0) {
            first = Some(offset);
        }
        if width >= end && char_width > 0 {
            last = offset;
            break;
        }
        width += char_width;
    }
    &text[first.unwrap_or(last)..last]
}

fn columns(separator: &SyntaxNode) -> Vec<(usize, usize)> {
    let raw = text_without_line_prefixes(separator);
    let mut result = Vec::new();
    let mut start = None;
    for (offset, ch) in raw.char_indices() {
        if ch == '-' {
            start.get_or_insert(offset);
        } else if let Some(start) = start.take() {
            result.push((start, offset));
        }
    }
    if let Some(start) = start {
        result.push((start, raw.len()));
    }
    result
}

fn alignment(text: &str, width: usize) -> TableAlignment {
    let text = text.trim_end_matches([' ', '\t', '\r']);
    if text.is_empty() {
        return TableAlignment::Default;
    }
    match (text.starts_with([' ', '\t']), text.width() < width) {
        (false, false) => TableAlignment::Default,
        (false, true) => TableAlignment::Left,
        (true, false) => TableAlignment::Right,
        (true, true) => TableAlignment::Center,
    }
}

impl LogicalTable {
    fn read(table: &Table, config: &Config) -> Result<Self, TableConversionError> {
        if matches!(table, Table::Grid(_)) {
            return Err(TableConversionError::UnsupportedSource);
        }
        let rows = table.rows();
        let has_header = rows.first().is_some_and(|row| row.is_header());
        if rows.len() <= usize::from(has_header) {
            return Err(TableConversionError::MissingBody);
        }
        let caption = table.caption().map(|caption| {
            let before =
                caption.syntax().text_range().start() < rows[0].syntax().text_range().start();
            (
                before,
                text_without_line_prefixes(caption.syntax())
                    .replace("\r\n", "\n")
                    .trim_end_matches('\n')
                    .to_string(),
            )
        });
        let (cell_texts, alignments) = if let Table::Pipe(pipe) = table {
            let count = pipe
                .column_count()
                .ok_or(TableConversionError::RaggedRows)?;
            let mut result = Vec::new();
            for row in rows {
                let cells: Vec<_> = row
                    .cells()
                    .map(|cell| cell.syntax().text().to_string())
                    .collect();
                if cells.len() != count {
                    return Err(TableConversionError::RaggedRows);
                }
                result.push(cells);
            }
            (result, pipe.alignments())
        } else {
            let skip = usize::from(matches!(table, Table::Multiline(_)) && has_header);
            let separator = table
                .syntax()
                .children()
                .filter(|node| node.kind() == SyntaxKind::TABLE_SEPARATOR)
                .nth(skip)
                .ok_or(TableConversionError::InvalidOutput)?;
            let columns = columns(&separator);
            if columns.is_empty() {
                return Err(TableConversionError::InvalidOutput);
            }
            let reference = text_without_line_prefixes(rows[0].syntax());
            let reference = reference.lines().next().unwrap_or("");
            let alignments = columns
                .iter()
                .enumerate()
                .map(|(i, &(start, end))| {
                    alignment(
                        column_slice(
                            reference,
                            start,
                            columns.get(i + 1).map_or(usize::MAX, |c| c.0),
                        ),
                        end - start,
                    )
                })
                .collect();
            let mut result = Vec::new();
            for row in rows {
                let raw = text_without_line_prefixes(row.syntax());
                let mut cells = vec![Vec::new(); columns.len()];
                for line in raw.lines() {
                    for (i, &(start, _)) in columns.iter().enumerate() {
                        let end = columns.get(i + 1).map_or(usize::MAX, |c| c.0);
                        cells[i].push(
                            column_slice(line, start, end)
                                .trim_matches([' ', '\t', '\r'])
                                .to_string(),
                        );
                    }
                }
                result.push(cells.into_iter().map(|lines| lines.join("\n")).collect());
            }
            (result, alignments)
        };
        let rows = cell_texts
            .into_iter()
            .map(|row| row.iter().map(|cell| Cell::parse(cell, config)).collect())
            .collect::<Result<_, _>>()?;
        Ok(Self {
            rows,
            alignments,
            has_header,
            caption,
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.rows == other.rows
            && self.has_header == other.has_header
            && self.caption == other.caption
            && self
                .alignments
                .iter()
                .zip(&other.alignments)
                .all(|(&source, &target)| {
                    source == target
                        || (source == TableAlignment::Default && target == TableAlignment::Left)
                })
    }

    fn render(
        &self,
        target: TableStyle,
        available_width: usize,
    ) -> Result<String, TableConversionError> {
        let cols = self.alignments.len();
        let mut widths = vec![2; cols];
        let mut minimum = vec![2; cols];
        for (row_idx, row) in self.rows.iter().enumerate() {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.text().width() + 2);
                let min = if self.has_header && row_idx == 0 {
                    cell.text().width()
                } else {
                    cell.minimum_width()
                };
                minimum[i] = minimum[i].max(min + 2);
            }
        }
        if target == TableStyle::Multiline {
            let mut total = widths.iter().sum::<usize>() + cols.saturating_sub(1);
            while total > available_width {
                let Some(i) = (0..cols)
                    .filter(|&i| widths[i] > minimum[i])
                    .max_by_key(|&i| (widths[i], std::cmp::Reverse(i)))
                else {
                    break;
                };
                widths[i] -= 1;
                total -= 1;
            }
        }
        for (i, cell) in self.rows[0].iter().enumerate() {
            if cell.pieces.is_empty() && !matches!(self.alignments[i], TableAlignment::Default) {
                return Err(TableConversionError::Alignment);
            }
        }
        let separator = widths
            .iter()
            .map(|&width| "-".repeat(width))
            .collect::<Vec<_>>()
            .join(" ");
        let border = "-".repeat(separator.len());
        let mut out = String::new();
        if let Some((true, caption)) = &self.caption {
            out.push_str(caption.trim_end_matches('\n'));
            out.push_str("\n\n");
        }
        if target == TableStyle::Multiline || !self.has_header {
            out.push_str(if self.has_header { &border } else { &separator });
            out.push('\n');
        }
        for (row_idx, row) in self.rows.iter().enumerate() {
            let is_header = self.has_header && row_idx == 0;
            let cells: Vec<_> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    if target == TableStyle::Simple || is_header {
                        vec![cell.text()]
                    } else {
                        cell.wrap(widths[i].saturating_sub(2))
                    }
                })
                .collect();
            for line_idx in 0..cells.iter().map(Vec::len).max().unwrap_or(1) {
                let line = cells
                    .iter()
                    .enumerate()
                    .map(|(i, lines)| {
                        let align = match self.alignments[i] {
                            TableAlignment::Default | TableAlignment::Left => Alignment::Left,
                            TableAlignment::Center => Alignment::Center,
                            TableAlignment::Right => Alignment::Right,
                        };
                        pad_simple_cell(
                            lines.get(line_idx).map_or("", String::as_str),
                            widths[i],
                            align,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(line.trim_end());
                out.push('\n');
            }
            if is_header {
                out.push_str(&separator);
                out.push('\n');
            } else if target == TableStyle::Multiline {
                // The final blank also disambiguates a short, single-row table.
                out.push('\n');
            }
        }
        out.push_str(if target == TableStyle::Multiline && self.has_header {
            &border
        } else {
            &separator
        });
        out.push('\n');
        if let Some((false, caption)) = &self.caption {
            out.push('\n');
            out.push_str(caption.trim_end_matches('\n'));
            out.push('\n');
        }
        Ok(out)
    }
}

/// Convert a table while preserving its content, structure, and explicit alignment.
///
/// Column widths and wrapping may change, and default alignment may become
/// left alignment. Simple tables discard explicit column widths. Conversions
/// that cannot preserve the remaining table information return an error.
/// The result has no container prefixes and uses LF line endings.
pub fn convert_table(
    table: &Table,
    target: TableStyle,
    config: &Config,
    available_width: usize,
) -> Result<String, TableConversionError> {
    let enabled = match target {
        TableStyle::Simple => config.parser_extensions.simple_tables,
        TableStyle::Multiline => config.parser_extensions.multiline_tables,
    };
    if !enabled {
        return Err(TableConversionError::DisabledExtension);
    }
    let model = LogicalTable::read(table, config)?;
    let output = model.render(target, available_width)?;
    let parsed = crate::parser::parse(&output, Some(config.parser_options()));
    let children: Vec<_> = parsed
        .children()
        .filter(|node| node.kind() != SyntaxKind::BLANK_LINE)
        .collect();
    if children.len() != 1 || children[0].kind() != target.syntax_kind() {
        return Err(TableConversionError::InvalidOutput);
    }
    let candidate = Table::cast(children[0].clone()).ok_or(TableConversionError::InvalidOutput)?;
    if !model.matches(&LogicalTable::read(&candidate, config)?) {
        return Err(TableConversionError::InvalidOutput);
    }
    Ok(output)
}

/// Check content, structure, and explicit alignment after restoring container prefixes.
///
/// Source column widths and wrapping are excluded from the conversion contract.
pub fn equivalent_tables(source: &Table, candidate: &Table, config: &Config) -> bool {
    match (
        LogicalTable::read(source, config),
        LogicalTable::read(candidate, config),
    ) {
        (Ok(source), Ok(candidate)) => source.matches(&candidate),
        _ => false,
    }
}
