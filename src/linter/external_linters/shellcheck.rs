use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext, line_col_to_offset,
    map_concatenated_edit_to_original, map_tool_range,
};
use crate::linter::diagnostics::{Diagnostic, DiagnosticOrigin};

#[derive(Debug, Deserialize)]
struct ShellcheckDiagnostic {
    code: i64,
    level: String,
    message: String,
    line: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    column: usize,
    #[serde(rename = "endColumn")]
    end_column: usize,
    #[serde(default)]
    fix: Option<ShellcheckFix>,
}

#[derive(Debug, Deserialize)]
struct ShellcheckFix {
    replacements: Vec<ShellcheckReplacement>,
}

#[derive(Debug, Deserialize)]
struct ShellcheckReplacement {
    line: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    column: usize,
    #[serde(rename = "endColumn")]
    end_column: usize,
    replacement: String,
    #[serde(default)]
    #[serde(rename = "insertionPoint")]
    insertion_point: Option<String>,
}

pub(crate) struct ShellcheckParser;

impl ExternalLinterParser for ShellcheckParser {
    const NAME: &'static str = "shellcheck";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        use crate::linter::diagnostics::{Edit, Fix};

        let output: Vec<ShellcheckDiagnostic> = serde_json::from_str(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid shellcheck JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for sc_diag in output {
            let Some(location) = map_tool_range(
                ctx,
                sc_diag.line,
                sc_diag.column,
                sc_diag.end_line,
                sc_diag.end_column,
            ) else {
                continue;
            };

            let fix = if let (Some(mappings), Some(fix)) = (ctx.mappings, sc_diag.fix.as_ref()) {
                let mut edits: Vec<(usize, Edit)> = Vec::new();
                for replacement in &fix.replacements {
                    let start =
                        line_col_to_offset(ctx.linted_input, replacement.line, replacement.column);
                    let end = line_col_to_offset(
                        ctx.linted_input,
                        replacement.end_line,
                        replacement.end_column,
                    );
                    let (Some(mut start), Some(mut end)) = (start, end) else {
                        edits.clear();
                        break;
                    };

                    if matches!(replacement.insertion_point.as_deref(), Some("afterEnd")) {
                        start = end;
                    } else if matches!(replacement.insertion_point.as_deref(), Some("beforeStart"))
                    {
                        end = start;
                    }

                    let Some((mapped_start, mapped_end)) = map_concatenated_edit_to_original(
                        ctx.linted_input,
                        start,
                        end,
                        &replacement.replacement,
                        mappings,
                    ) else {
                        edits.clear();
                        break;
                    };

                    edits.push((
                        mapped_start,
                        Edit {
                            range: TextRange::new(
                                (mapped_start as u32).into(),
                                (mapped_end as u32).into(),
                            ),
                            replacement: replacement.replacement.clone(),
                        },
                    ));
                }

                if edits.is_empty() {
                    None
                } else {
                    edits.sort_by_key(|(start, _)| *start);
                    Some(Fix::safe(
                        format!("Apply ShellCheck fix for SC{}", sc_diag.code),
                        edits.into_iter().map(|(_, e)| e).collect(),
                    ))
                }
            } else {
                None
            };

            let code = format!("SC{}", sc_diag.code);
            let diagnostic = match sc_diag.level.as_str() {
                "error" => Diagnostic::error(location, code, sc_diag.message),
                "warning" => Diagnostic::warning(location, code, sc_diag.message),
                _ => Diagnostic::info(location, code, sc_diag.message),
            }
            .with_origin(DiagnosticOrigin::External);
            diagnostics.push(if let Some(fix) = fix {
                diagnostic.with_fix(fix)
            } else {
                diagnostic
            });
        }
        Ok(diagnostics)
    }
}
