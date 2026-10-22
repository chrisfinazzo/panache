use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext, line_col_to_offset,
    map_concatenated_edit_to_original, map_diagnostic_range,
};
use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, DiagnosticOrigin};

#[derive(Debug, Deserialize)]
struct JarlOutput {
    diagnostics: Vec<JarlDiagnostic>,
    #[allow(dead_code)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JarlDiagnostic {
    message: JarlMessage,
    #[allow(dead_code)]
    filename: String,
    range: [usize; 2],
    location: JarlLocation,
    #[serde(default)]
    fix: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JarlMessage {
    name: String,
    body: String,
    #[allow(dead_code)]
    suggestion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JarlLocation {
    row: usize,
    column: usize,
}

#[derive(Deserialize)]
struct JarlFixPayload {
    content: Option<String>,
    start: Option<usize>,
    end: Option<usize>,
    range: Option<[usize; 2]>,
    edits: Option<Vec<JarlEdit>>,
    // Required in every supported release. Accepting extra metadata does not
    // make omission of this fix-application guard permission to apply a fix.
    to_skip: bool,
    safety: Option<JarlFixSafety>,
}

struct JarlFix {
    edits: Vec<JarlEdit>,
    to_skip: bool,
    safety: JarlFixSafety,
}

impl<'de> Deserialize<'de> for JarlFix {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let payload = JarlFixPayload::deserialize(deserializer)?;
        // Jarl 0.5/0.6 diagnostic.rs keeps safety on the rule, not in Fix;
        // output_format.rs serializes Fix unchanged. Missing safety cannot prove
        // a fix safe, so legacy shapes require the user's unsafe-fix opt-in.
        // https://github.com/etiennebacher/jarl/blob/0.6.0/crates/jarl-core/src/diagnostic.rs
        let (edits, safety) = match (
            payload.content,
            payload.start,
            payload.end,
            payload.range,
            payload.edits,
        ) {
            (Some(content), Some(start), Some(end), None, None) => (
                vec![JarlEdit {
                    range: [start, end],
                    content,
                }],
                JarlFixSafety::Unsafe,
            ),
            (Some(content), None, None, Some(range), None) => {
                (vec![JarlEdit { range, content }], JarlFixSafety::Unsafe)
            }
            (None, None, None, None, Some(edits)) => (
                edits,
                payload
                    .safety
                    .ok_or_else(|| serde::de::Error::missing_field("safety"))?,
            ),
            _ => {
                return Err(serde::de::Error::custom(
                    "expected exactly one supported Jarl fix shape: content/start/end, content/range, or edits/safety",
                ));
            }
        };
        Ok(Self {
            edits,
            to_skip: payload.to_skip,
            safety,
        })
    }
}

#[derive(Debug, Deserialize)]
struct JarlEdit {
    range: [usize; 2],
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum JarlFixSafety {
    Safe,
    Unsafe,
    None,
}

pub(crate) struct JarlParser;

fn diagnostic_excerpt(text: &str) -> String {
    const MAX_CHARS: usize = 256;
    match text.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

impl ExternalLinterParser for JarlParser {
    const NAME: &'static str = "jarl";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        use crate::linter::diagnostics::{Edit, Fix};

        let output: JarlOutput = serde_json::from_str(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid jarl JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for jarl_diag in output.diagnostics {
            let line = jarl_diag.location.row;
            let column = jarl_diag.location.column + 1;
            let range_len = jarl_diag.range[1].saturating_sub(jarl_diag.range[0]);
            let Some(start_offset) = line_col_to_offset(ctx.linted_input, line, column) else {
                continue;
            };
            let Some(location) =
                map_diagnostic_range(ctx, start_offset, start_offset.saturating_add(range_len))
            else {
                continue;
            };

            let parsed_fix = jarl_diag.fix.as_ref().map(JarlFix::deserialize);
            let mut fix_note = None;
            let fix = if let (Some(mappings), Some(Ok(jarl_fix))) = (ctx.mappings, &parsed_fix) {
                if !jarl_fix.to_skip
                    && !matches!(jarl_fix.safety, JarlFixSafety::None)
                    && !jarl_fix.edits.is_empty()
                {
                    let edits: Option<Vec<Edit>> = jarl_fix
                        .edits
                        .iter()
                        .map(|edit| {
                            let (start, end) = map_concatenated_edit_to_original(
                                ctx.linted_input,
                                edit.range[0],
                                edit.range[1],
                                &edit.content,
                                mappings,
                            )?;
                            Some(Edit {
                                range: TextRange::new(
                                    u32::try_from(start).ok()?.into(),
                                    u32::try_from(end).ok()?.into(),
                                ),
                                replacement: edit.content.clone(),
                            })
                        })
                        .collect();
                    if edits.is_none() {
                        fix_note = Some(
                            "Jarl fix could not be mapped to the document; diagnostic retained without a fix.",
                        );
                    }
                    edits.and_then(|mut edits| {
                        edits.sort_by_key(|edit| (edit.range.start(), edit.range.end()));
                        let overlapping_edits = edits.windows(2).any(|pair| {
                            pair[0].range.end() > pair[1].range.start()
                                || (pair[0].range.is_empty()
                                    && pair[1].range.is_empty()
                                    && pair[0].range.start() == pair[1].range.start())
                        });
                        if overlapping_edits {
                            fix_note =
                                Some("Overlapping Jarl edits; diagnostic retained without a fix.");
                            return None;
                        }
                        let message = format!("Apply suggested fix: {}", jarl_diag.message.name);
                        match jarl_fix.safety {
                            JarlFixSafety::Safe => Some(Fix::safe(message, edits)),
                            JarlFixSafety::Unsafe => Some(Fix::unsafe_fix(message, edits)),
                            JarlFixSafety::None => None,
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            };

            let mut diagnostic =
                Diagnostic::warning(location, jarl_diag.message.name, jarl_diag.message.body)
                    .with_origin(DiagnosticOrigin::External);
            if let (Some(Err(error)), Some(value)) = (&parsed_fix, &jarl_diag.fix) {
                let error = diagnostic_excerpt(&error.to_string());
                let value = diagnostic_excerpt(&value.to_string());
                diagnostic = diagnostic.with_note(
                    DiagnosticNoteKind::Note,
                    format!(
                        "Unsupported Jarl fix JSON shape; diagnostic retained without a fix. {error}; fix: {value}"
                    ),
                );
            }
            if let Some(suggestion) = jarl_diag.message.suggestion.as_ref()
                && !suggestion.trim().is_empty()
            {
                diagnostic = diagnostic.with_note(DiagnosticNoteKind::Help, suggestion.clone());
            }
            if let Some(note) = fix_note {
                diagnostic = diagnostic.with_note(DiagnosticNoteKind::Note, note);
            }
            diagnostics.push(if let Some(fix) = fix {
                diagnostic.with_fix(fix)
            } else {
                diagnostic
            });
        }
        Ok(diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::external_linters::ParseContext;

    #[test]
    fn maps_jarl_suggestion_to_help_note() {
        let ctx = ParseContext {
            output: r#"{"diagnostics":[{"message":{"name":"any_is_na","body":"Prefer anyNA()","suggestion":"Use anyNA(x) instead"},"filename":"x.R","range":[0,5],"location":{"row":1,"column":0},"fix":{"edits":[],"to_skip":true,"safety":"none"}}],"errors":[]}"#,
            linted_input: "any(is.na(x))\n",
            original_input: "any(is.na(x))\n",
            mappings: None,
        };
        let diagnostics = JarlParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].notes.len(), 1);
        assert_eq!(diagnostics[0].notes[0].kind, DiagnosticNoteKind::Help);
        assert_eq!(diagnostics[0].notes[0].message, "Use anyNA(x) instead");
    }

    #[test]
    fn parses_real_jarl_release_captures() {
        use crate::linter::code_block_collector::{BlockMapping, SnippetKind};
        use crate::linter::diagnostics::FixSafety;

        let code = include_str!("../../../tests/fixtures/jarl/probe.R");
        let document = format!("```r\n{code}```\n");
        let mappings = [BlockMapping {
            kind: SnippetKind::Block,
            concatenated_range: 0..code.len(),
            original_range: 5..5 + code.len(),
            start_line: 2,
            line_offsets: Vec::new(),
        }];
        for (output, safety) in [
            (
                include_str!("../../../tests/fixtures/jarl/jarl-0.5.0.json"),
                FixSafety::Unsafe,
            ),
            (
                include_str!("../../../tests/fixtures/jarl/jarl-0.6.0.json"),
                FixSafety::Unsafe,
            ),
            (
                include_str!("../../../tests/fixtures/jarl/jarl-0.7.1.json"),
                FixSafety::Safe,
            ),
        ] {
            let diagnostics = JarlParser::parse(&ParseContext {
                output,
                linted_input: code,
                original_input: &document,
                mappings: Some(&mappings),
            })
            .unwrap();
            assert_eq!(diagnostics.len(), 1);
            let diagnostic = &diagnostics[0];
            assert_eq!(diagnostic.code, "any_is_na");
            assert_eq!(diagnostic.message, "`any(is.na(...))` is inefficient.");
            assert_eq!(diagnostic.location.line, 3);
            assert_eq!(diagnostic.location.column, 1);
            let fix = diagnostic.fix.as_ref().unwrap();
            assert_eq!(fix.safety, safety);
            assert_eq!(fix.edits.len(), 1);
            let edit = &fix.edits[0];
            assert_eq!(
                &document[usize::from(edit.range.start())..usize::from(edit.range.end())],
                "any(is.na(x))"
            );
            assert_eq!(edit.replacement, "anyNA(x)");
            assert!(
                !diagnostic
                    .notes
                    .iter()
                    .any(|note| note.message.contains("Jarl fix JSON shape"))
            );
        }
    }

    #[test]
    fn additive_fix_metadata_preserves_edits() {
        use crate::linter::code_block_collector::{BlockMapping, SnippetKind};
        use serde_json::json;

        let code = include_str!("../../../tests/fixtures/jarl/probe.R");
        let mappings = [BlockMapping {
            kind: SnippetKind::Block,
            concatenated_range: 0..code.len(),
            original_range: 0..code.len(),
            start_line: 1,
            line_offsets: Vec::new(),
        }];
        for (capture, pointer) in [
            (
                include_str!("../../../tests/fixtures/jarl/jarl-0.5.0.json"),
                "/diagnostics/0/fix",
            ),
            (
                include_str!("../../../tests/fixtures/jarl/jarl-0.7.1.json"),
                "/diagnostics/0/fix/edits/0",
            ),
        ] {
            let mut output: serde_json::Value = serde_json::from_str(capture).unwrap();
            output
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("description".to_owned(), json!("Additional metadata"));
            let output = output.to_string();
            let diagnostics = JarlParser::parse(&ParseContext {
                output: &output,
                linted_input: code,
                original_input: code,
                mappings: Some(&mappings),
            })
            .unwrap();
            assert_eq!(diagnostics.len(), 1);
            let diagnostic = &diagnostics[0];
            assert_eq!(diagnostic.code, "any_is_na");
            assert!(
                !diagnostic
                    .notes
                    .iter()
                    .any(|note| note.message.contains("Jarl fix JSON shape")),
                "{pointer}: {:?}",
                diagnostic.notes
            );
            let fix = diagnostic
                .fix
                .as_ref()
                .expect("additive metadata must not discard the fix");
            assert_eq!(fix.edits.len(), 1);
            let edit = &fix.edits[0];
            assert_eq!(
                &code[usize::from(edit.range.start())..usize::from(edit.range.end())],
                "any(is.na(x))"
            );
            assert_eq!(edit.replacement, "anyNA(x)");
        }
    }

    #[test]
    fn unsupported_fix_notes_bound_external_text() {
        let mut output: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/jarl/jarl-0.7.1.json"))
                .unwrap();
        output["diagnostics"][0]["fix"]["safety"] =
            serde_json::Value::String(format!("{}private_tail_marker", "\u{e9}".repeat(2048)));
        let output = output.to_string();
        let code = include_str!("../../../tests/fixtures/jarl/probe.R");
        let diagnostics = JarlParser::parse(&ParseContext {
            output: &output,
            linted_input: code,
            original_input: code,
            mappings: None,
        })
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none());
        let note = diagnostics[0]
            .notes
            .iter()
            .find(|note| note.message.starts_with("Unsupported Jarl fix JSON shape"))
            .unwrap();
        let (error, fix) = note
            .message
            .strip_prefix("Unsupported Jarl fix JSON shape; diagnostic retained without a fix. ")
            .unwrap()
            .split_once("; fix: ")
            .unwrap();
        for excerpt in [error, fix] {
            assert_eq!(excerpt.chars().take(256).count(), 256);
            assert_eq!(excerpt.chars().skip(256).collect::<String>(), "...");
        }
        assert!(!note.message.contains("private_tail_marker"));
    }

    #[test]
    fn absent_and_unknown_fixes_retain_diagnostics() {
        use crate::linter::code_block_collector::{BlockMapping, SnippetKind};
        use serde_json::json;
        let capture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/jarl/jarl-0.7.1.json"))
                .unwrap();
        for (fix, expected_json) in [
            (None, None),
            (Some(json!(null)), None),
            (
                Some(
                    json!({"edits": [{"range": [14, 27], "content": "anyNA(x)"}], "safety": "safe"}),
                ),
                Some(r#"{"edits":[{"content":"anyNA(x)","range":[14,27]}],"safety":"safe"}"#),
            ),
            (
                Some(json!({"replacement": "anyNA(x)", "span": [14, 27]})),
                Some(r#"{"replacement":"anyNA(x)","span":[14,27]}"#),
            ),
            (
                Some(
                    json!({"edits": [{"range": [14, 27], "content": "anyNA(x)"}], "to_skip": false, "safety": "future"}),
                ),
                Some(
                    r#"{"edits":[{"content":"anyNA(x)","range":[14,27]}],"safety":"future","to_skip":false}"#,
                ),
            ),
            (
                Some(
                    json!({"content": "anyNA(x)", "start": 14, "end": 27, "range": [14, 27], "to_skip": false}),
                ),
                Some(
                    r#"{"content":"anyNA(x)","end":27,"range":[14,27],"start":14,"to_skip":false}"#,
                ),
            ),
            (
                Some(
                    json!({"edits": [{"range": [14, 27], "content": "anyNA(x)"}], "to_skip": false, "safety": "safe", "start": 14, "end": 27}),
                ),
                Some(
                    r#"{"edits":[{"content":"anyNA(x)","range":[14,27]}],"end":27,"safety":"safe","start":14,"to_skip":false}"#,
                ),
            ),
            (
                Some(
                    json!({"edits": [{"range": [14, 27], "content": "anyNA(x)"}], "to_skip": false, "safety": "safe", "range": [14, 27]}),
                ),
                Some(
                    r#"{"edits":[{"content":"anyNA(x)","range":[14,27]}],"range":[14,27],"safety":"safe","to_skip":false}"#,
                ),
            ),
            (
                Some(
                    json!({"edits": [{"range": [14], "content": "anyNA(x)"}], "to_skip": false, "safety": "safe"}),
                ),
                Some(
                    r#"{"edits":[{"content":"anyNA(x)","range":[14]}],"safety":"safe","to_skip":false}"#,
                ),
            ),
        ] {
            let mut output = capture.clone();
            if let Some(value) = &fix {
                output["diagnostics"][0]["fix"] = value.clone();
            } else {
                output["diagnostics"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("fix");
            }
            output["diagnostics"]
                .as_array_mut()
                .unwrap()
                .push(capture["diagnostics"][0].clone());
            let output = output.to_string();
            let code = include_str!("../../../tests/fixtures/jarl/probe.R");
            let mappings = [BlockMapping {
                kind: SnippetKind::Block,
                concatenated_range: 0..code.len(),
                original_range: 0..code.len(),
                start_line: 1,
                line_offsets: Vec::new(),
            }];
            let diagnostics = JarlParser::parse(&ParseContext {
                output: &output,
                linted_input: code,
                original_input: code,
                mappings: Some(&mappings),
            })
            .unwrap();
            assert_eq!(diagnostics.len(), 2);
            assert_eq!(diagnostics[0].code, "any_is_na");
            for diagnostic in &diagnostics {
                assert_eq!(diagnostic.location.line, 2);
                assert_eq!(diagnostic.location.column, 1);
                let range = diagnostic.location.range;
                assert_eq!(
                    &code[usize::from(range.start())..usize::from(range.end())],
                    "any(is.na(x))"
                );
            }
            assert!(diagnostics[0].fix.is_none());
            assert!(diagnostics[1].fix.is_some());
            let warnings: Vec<_> = diagnostics[0]
                .notes
                .iter()
                .filter(|note| note.message.contains("Jarl fix JSON shape"))
                .collect();
            if let Some(expected_json) = expected_json {
                assert_eq!(warnings.len(), 1);
                assert_eq!(warnings[0].kind, DiagnosticNoteKind::Note);
                assert!(warnings[0].message.starts_with(
                    "Unsupported Jarl fix JSON shape; diagnostic retained without a fix."
                ));
                let cause = warnings[0]
                    .message
                    .strip_prefix(
                        "Unsupported Jarl fix JSON shape; diagnostic retained without a fix.",
                    )
                    .unwrap()
                    .split_once("; fix: ")
                    .unwrap()
                    .0;
                assert!(!cause.trim().is_empty(), "missing decoder cause");
                let actual_json = warnings[0].message.split_once("; fix: ").unwrap().1;
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(actual_json).unwrap(),
                    serde_json::from_str::<serde_json::Value>(expected_json).unwrap(),
                );
            } else {
                assert!(warnings.is_empty());
            }
            assert!(
                !diagnostics[1]
                    .notes
                    .iter()
                    .any(|note| note.message.contains("Jarl fix JSON shape"))
            );
        }
    }

    #[test]
    fn overlapping_edits_are_rejected_atomically() {
        use crate::linter::code_block_collector::{BlockMapping, SnippetKind};
        use serde_json::json;
        let code = "x = 1\ny = 2\n";
        let mappings = [BlockMapping {
            kind: SnippetKind::Block,
            concatenated_range: 0..code.len(),
            original_range: 0..code.len(),
            start_line: 1,
            line_offsets: Vec::new(),
        }];
        for (ranges, overlaps) in [
            ([[2, 4], [3, 5]], true),
            ([[3, 5], [2, 4]], true),
            ([[2, 5], [3, 4]], true),
            ([[2, 2], [2, 2]], true),
            ([[2, 2], [2, 4]], false),
            ([[2, 4], [2, 2]], false),
            ([[2, 3], [3, 4]], false),
        ] {
            let output = json!({"diagnostics": [{
                "message": {"name": "assignment", "body": "Use <-", "suggestion": null},
                "filename": "x.R", "range": [2, 3], "location": {"row": 1, "column": 2},
                "fix": {"edits": [
                    {"range": ranges[0], "content": "<-"},
                    {"range": ranges[1], "content": "<-"}
                ], "to_skip": false, "safety": "safe"}
            }], "errors": []})
            .to_string();
            let diagnostics = JarlParser::parse(&ParseContext {
                output: &output,
                linted_input: code,
                original_input: code,
                mappings: Some(&mappings),
            })
            .unwrap();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].code, "assignment");
            assert_eq!(diagnostics[0].fix.is_none(), overlaps, "{ranges:?}");
            assert_eq!(
                diagnostics[0]
                    .notes
                    .iter()
                    .any(|note| note.kind == DiagnosticNoteKind::Note
                        && note.message
                            == "Overlapping Jarl edits; diagnostic retained without a fix."),
                overlaps,
            );
        }
    }

    #[test]
    fn maps_jarl_edits_atomically_and_preserves_safety() {
        use crate::linter::code_block_collector::{BlockMapping, SnippetKind};
        use crate::linter::diagnostics::FixSafety;
        use serde_json::json;

        let code = "x = 1\ny = 2\n";
        let document = format!("```r\n{code}```\n");
        let mappings = [BlockMapping {
            kind: SnippetKind::Block,
            concatenated_range: 0..code.len(),
            original_range: 5..5 + code.len(),
            start_line: 2,
            line_offsets: Vec::new(),
        }];
        let edits = json!([
            {"range": [2, 3], "content": "<-"},
            {"range": [8, 9], "content": "<-"}
        ]);
        for (safety, skipped, replacements, expected, unmappable) in [
            ("safe", false, edits.clone(), Some(FixSafety::Safe), false),
            (
                "unsafe",
                false,
                edits.clone(),
                Some(FixSafety::Unsafe),
                false,
            ),
            ("none", false, edits.clone(), None, false),
            (
                "none",
                false,
                json!([{"range": [30, 31], "content": "<-"}]),
                None,
                false,
            ),
            ("safe", true, edits.clone(), None, false),
            ("safe", false, json!([]), None, false),
            (
                "safe",
                false,
                json!([
                    {"range": [2, 3], "content": "<-"},
                    {"range": [30, 31], "content": "<-"}
                ]),
                None,
                true,
            ),
        ] {
            let output = json!({
                "diagnostics": [{
                    "message": {"name": "assignment", "body": "Use <-", "suggestion": null},
                    "filename": "x.R", "range": [2, 3],
                    "location": {"row": 1, "column": 2},
                    "fix": {"edits": replacements, "to_skip": skipped, "safety": safety}
                }],
                "errors": []
            })
            .to_string();
            let context = ParseContext {
                output: &output,
                linted_input: code,
                original_input: &document,
                mappings: Some(&mappings),
            };
            let diagnostics = JarlParser::parse(&context).unwrap();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].location.line, 2);
            assert_eq!(diagnostics[0].location.column, 3);
            assert_eq!(diagnostics[0].fix.as_ref().map(|fix| fix.safety), expected);
            assert_eq!(diagnostics[0].notes.iter().any(|note|
                note.kind == DiagnosticNoteKind::Note && note.message == "Jarl fix could not be mapped to the document; diagnostic retained without a fix."
            ), unmappable);
            if let Some(fix) = &diagnostics[0].fix {
                assert_eq!(fix.edits.len(), 2);
                for (edit, offset) in fix.edits.iter().zip([7, 13]) {
                    assert_eq!(usize::from(edit.range.start()), offset);
                    assert_eq!(&document[offset..usize::from(edit.range.end())], "=");
                    assert_eq!(edit.replacement, "<-");
                }
            }
        }
    }
}
