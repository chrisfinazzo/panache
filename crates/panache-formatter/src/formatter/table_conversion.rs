//! Explicit, content-preserving conversions between Markdown table styles.

use crate::{
    Config,
    syntax::{
        AstNode, LatexCommand, SyntaxKind, SyntaxNode, Table, TableAlignment,
        text_without_line_prefixes,
    },
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
    Pipe,
    Simple,
    Multiline,
}

impl TableStyle {
    pub fn syntax_kind(self) -> SyntaxKind {
        match self {
            Self::Pipe => SyntaxKind::PIPE_TABLE,
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
#[derive(Debug, Clone)]
struct Cell {
    pieces: Vec<String>,
    pipe_text: String,
    comparison: String,
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
        // The parser does not yet include verbatim delimiters and content in
        // the command node. Escaping or reflowing those bytes can change TeX.
        if node
            .descendants()
            .filter_map(LatexCommand::cast)
            .any(|command| {
                command
                    .text()
                    .chars()
                    .skip(1)
                    .take_while(char::is_ascii_alphabetic)
                    .eq("verb".chars())
            })
        {
            return Err(TableConversionError::InvalidOutput);
        }
        Ok(Self {
            pieces: cell_pieces(&node, CellText::Source)?,
            pipe_text: cell_pieces(&node, CellText::Pipe)?.join(" "),
            comparison: cell_pieces(&node, CellText::Comparison)?.join(" "),
        })
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

fn cell_pieces(node: &SyntaxNode, mode: CellText) -> Result<Vec<String>, TableConversionError> {
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
                        if ch == '|' && matches!(mode, CellText::Pipe) {
                            current.push('\\');
                        }
                        current.push(ch);
                    }
                }
            }
            element => {
                let text = match element {
                    NodeOrToken::Node(node) => inline_cell_text(&node, mode),
                    NodeOrToken::Token(token) => comparison_token_text(&token, mode),
                };
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
    Ok(pieces)
}

fn comparison_token_text(token: &crate::syntax::SyntaxToken, mode: CellText) -> String {
    if matches!(mode, CellText::Comparison)
        && token.kind() == SyntaxKind::ESCAPED_CHAR
        && token.text() == "\\|"
    {
        "|".to_string()
    } else {
        token.text().to_string()
    }
}

#[derive(Clone, Copy)]
enum CellText {
    Source,
    Pipe,
    Comparison,
}

fn inline_cell_text(node: &SyntaxNode, mode: CellText) -> String {
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
                if matches!(
                    token.kind(),
                    SyntaxKind::TEXT | SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE
                ) =>
            {
                let text = collapse_spaces(token.text());
                if matches!(mode, CellText::Pipe) {
                    text.replace('|', "\\|")
                } else {
                    text
                }
            }
            NodeOrToken::Node(node) => inline_cell_text(&node, mode),
            NodeOrToken::Token(token) => comparison_token_text(&token, mode),
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

#[derive(Debug)]
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
        let mut has_header = rows.first().is_some_and(|row| row.is_header());
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
        let mut rows: Vec<Vec<Cell>> = cell_texts
            .into_iter()
            .map(|row| row.iter().map(|cell| Cell::parse(cell, config)).collect())
            .collect::<Result<_, _>>()?;
        // Pandoc uses an empty pipe header as the spelling of a headerless
        // table. Keep that syntax in the CST, but compare its logical rows.
        if matches!(table, Table::Pipe(_))
            && config.dialect() == panache_parser::Dialect::Pandoc
            && has_header
            && rows[0].iter().all(|cell| cell.pieces.is_empty())
        {
            rows.remove(0);
            has_header = false;
        }
        Ok(Self {
            rows,
            alignments,
            has_header,
            caption,
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.rows.len() == other.rows.len()
            && self.rows.iter().zip(&other.rows).all(|(source, target)| {
                source.len() == target.len()
                    && source
                        .iter()
                        .zip(target)
                        .all(|(source, target)| source.comparison == target.comparison)
            })
            && self.has_header == other.has_header
            && self.caption == other.caption
            && self.alignments.len() == other.alignments.len()
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
        if target == TableStyle::Pipe {
            return Ok(self.render_pipe());
        }
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

    fn render_pipe(&self) -> String {
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.pipe_text.clone()).collect())
            .collect();
        let widths = super::tables::calculate_column_widths(&rows);
        let mut out = String::new();
        if let Some((true, caption)) = &self.caption {
            out.push_str(caption);
            out.push_str("\n\n");
        }
        let empty_header = vec![String::new(); widths.len()];
        let header = if self.has_header {
            &rows[0]
        } else {
            &empty_header
        };
        let write_row = |out: &mut String, row: &[String]| {
            out.push('|');
            for (i, cell) in row.iter().enumerate() {
                let alignment = match self.alignments[i] {
                    TableAlignment::Default | TableAlignment::Left => Alignment::Left,
                    TableAlignment::Center => Alignment::Center,
                    TableAlignment::Right => Alignment::Right,
                };
                out.push(' ');
                out.push_str(&pad_simple_cell(cell, widths[i], alignment));
                out.push_str(" |");
            }
            out.push('\n');
        };
        write_row(&mut out, header);
        out.push('|');
        for (i, &width) in widths.iter().enumerate() {
            out.push(' ');
            let (left, right) = match self.alignments[i] {
                TableAlignment::Default => (false, false),
                TableAlignment::Left => (true, false),
                TableAlignment::Center => (true, true),
                TableAlignment::Right => (false, true),
            };
            if left {
                out.push(':');
            }
            out.push_str(&"-".repeat(width - usize::from(left) - usize::from(right)));
            if right {
                out.push(':');
            }
            out.push_str(" |");
        }
        out.push('\n');
        for row in rows.iter().skip(usize::from(self.has_header)) {
            write_row(&mut out, row);
        }
        if let Some((false, caption)) = &self.caption {
            out.push('\n');
            out.push_str(caption);
            out.push('\n');
        }
        out
    }
}

/// Convert a table while preserving its content, structure, and explicit alignment.
///
/// Column widths and wrapping may change, and default alignment may become
/// left alignment. Simple tables discard explicit column widths. Conversions
/// that cannot preserve the remaining table information return an error.
/// Pipe tables join wrapped prose onto one line per row and use an empty
/// header for headerless tables under the Pandoc dialect. Pipes in prose are
/// escaped; literal content in code and math is preserved.
/// The result has no container prefixes and uses LF line endings.
pub fn convert_table(
    table: &Table,
    target: TableStyle,
    config: &Config,
    available_width: usize,
) -> Result<String, TableConversionError> {
    let enabled = match target {
        TableStyle::Pipe => config.parser_extensions.pipe_tables,
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
