use rowan::TextRange;

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AstNode, PipeTable, SyntaxKind};

/// A pipe table's delimiter row owns its column count: pandoc reads the count
/// off that row and truncates every other row to it, so cells past it never
/// reach the rendered output.
///
/// There is no unambiguous repair --- the author either mistyped the delimiter
/// row or left junk at the end of a data row --- so this rule ships no fix. The
/// formatter leaves such a table byte-for-byte as written for the same reason.
pub struct TableColumnCountRule;

impl Rule for TableColumnCountRule {
    fn name(&self) -> &str {
        "table-column-count"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "table-column-count",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("table-column-count")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::PIPE_TABLE]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for table in cx
            .nodes(SyntaxKind::PIPE_TABLE)
            .iter()
            .cloned()
            .filter_map(PipeTable::cast)
        {
            let Some(columns) = table.column_count() else {
                continue;
            };

            for row in table.cell_rows() {
                let cells: Vec<_> = row
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
                    .collect();
                if cells.len() <= columns {
                    continue;
                }

                // Point at the surplus cells only, not the whole row.
                let start = cells[columns].text_range().start();
                let end = cells[cells.len() - 1].text_range().end();
                let surplus = cells.len() - columns;
                let location = Location::from_range(TextRange::new(start, end), input);

                diagnostics.push(Diagnostic::warning(
                    location,
                    "table-column-count",
                    format!(
                        "This row has {} cells but the delimiter row declares {} column{}; \
                         the {} extra cell{} {} dropped when the table is rendered",
                        cells.len(),
                        columns,
                        if columns == 1 { "" } else { "s" },
                        surplus,
                        if surplus == 1 { "" } else { "s" },
                        if surplus == 1 { "is" } else { "are" },
                    ),
                ));
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        TableColumnCountRule.check_tree(&tree, input, &config, None)
    }

    /// `pandoc -f markdown -t native` on this input is a two-column `Table`:
    /// `c` and `3` never appear in the output.
    #[test]
    fn flags_every_row_with_surplus_cells() {
        let diagnostics = parse_and_lint("a | b | c\n---|---\n1 | 2 | 3\n");

        assert_eq!(diagnostics.len(), 2, "header and body row both overflow");
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.code, "table-column-count");
            assert!(
                diagnostic.message.contains("declares 2 columns"),
                "message names the delimiter's count: {}",
                diagnostic.message
            );
        }
    }

    /// The span covers the surplus cell, not the whole row.
    #[test]
    fn span_covers_only_the_surplus_cells() {
        let input = "a | b | c\n---|---\n";
        let diagnostics = parse_and_lint(input);

        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(
            &input[usize::from(range.start())..usize::from(range.end())],
            "c",
            "only the surplus cell is flagged"
        );
    }

    /// A row *short* of the delimiter's count is padded by pandoc, so nothing
    /// is lost and nothing is flagged.
    #[test]
    fn does_not_flag_short_rows() {
        let diagnostics = parse_and_lint("a | b\n---|---|---\n1 | 2 | 3\n");
        assert!(
            diagnostics.is_empty(),
            "short rows are padded, not truncated: {diagnostics:?}"
        );
    }

    /// A table whose rows all match the delimiter row is silent.
    #[test]
    fn does_not_flag_a_matched_table() {
        let diagnostics = parse_and_lint("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    }
}
