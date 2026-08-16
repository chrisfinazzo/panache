use lsp_types::*;

use crate::linter;
use crate::linter::Severity as PanacheSeverity;
use crate::lsp::line_index::LineIndex;

/// Convert an LSP UTF-16 position to a byte offset via the cached [`LineIndex`].
pub(crate) fn position_to_offset(index: &LineIndex, position: Position) -> Option<usize> {
    index.position_to_offset(position)
}

/// Convert a byte offset to an LSP position via the cached [`LineIndex`].
pub(crate) fn offset_to_position(index: &LineIndex, offset: usize) -> Position {
    index.offset_to_position(offset)
}

/// Convert panache Diagnostic to LSP Diagnostic
pub(crate) fn convert_diagnostic(diag: &linter::Diagnostic, index: &LineIndex) -> Diagnostic {
    let start = offset_to_position(index, diag.location.range.start().into());
    let end = offset_to_position(index, diag.location.range.end().into());

    let severity = match diag.severity {
        PanacheSeverity::Error => DiagnosticSeverity::ERROR,
        PanacheSeverity::Warning => DiagnosticSeverity::WARNING,
        PanacheSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    Diagnostic {
        range: Range { start, end },
        severity: Some(severity),
        code: Some(NumberOrString::String(diag.code.clone())),
        source: Some("panache".to_string()),
        message: if diag.notes.is_empty() {
            diag.message.clone()
        } else {
            let mut message = diag.message.clone();
            for note in &diag.notes {
                message.push('\n');
                match note.kind {
                    linter::DiagnosticNoteKind::Note => message.push_str("note: "),
                    linter::DiagnosticNoteKind::Help => message.push_str("help: "),
                }
                message.push_str(&note.message);
            }
            message
        },
        ..Default::default()
    }
}

/// Resolve a client-sent change range to a byte span of the text `index`
/// describes.
///
/// Both ends clamp to the end of the document when the line is past it. A
/// client whose version has drifted can send a range beyond the text the server
/// holds; degrading to an append is recoverable, where the obvious alternative
/// for the start -- falling back to 0 -- silently replaces the whole document.
///
/// A reversed range is normalized rather than trusted: left alone it underflows
/// the length arithmetic downstream, which is a panic in debug and a
/// `usize::MAX`-sized allocation request in release.
pub(crate) fn content_change_span(index: &LineIndex, range: Range) -> std::ops::Range<usize> {
    let start = position_to_offset(index, range.start).unwrap_or_else(|| index.len());
    let end = position_to_offset(index, range.end).unwrap_or_else(|| index.len());
    start.min(end)..start.max(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::line_index::LineIndex;

    /// Apply one content change the way `did_change` does: resolve its span
    /// against the index, then patch the index in place. The full-replace case
    /// rebuilds, having no span to resolve.
    fn apply(text: &str, change: &TextDocumentContentChangeEvent) -> String {
        let mut index = LineIndex::new(text);
        match change.range {
            Some(range) => {
                let span = content_change_span(&index, range);
                index.replace_range(span, &change.text);
            }
            None => index = LineIndex::new(&change.text),
        }
        index.text_arc().to_string()
    }

    #[test]
    fn test_convert_diagnostic_basic() {
        use crate::linter::diagnostics::{
            Diagnostic as PanacheDiagnostic, DiagnosticOrigin, Location, Severity,
        };
        use rowan::TextRange;

        let text = "# H1\n\n### H3\n";
        let index = LineIndex::new(text);

        let diag = PanacheDiagnostic {
            severity: Severity::Warning,
            location: Location {
                line: 3,
                column: 1,
                range: TextRange::new(7.into(), 14.into()),
            },
            message: "Heading level skipped from h1 to h3".to_string(),
            code: "heading-hierarchy".to_string(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&diag, &index);

        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            lsp_diag.code,
            Some(NumberOrString::String("heading-hierarchy".to_string()))
        );
        assert_eq!(lsp_diag.source, Some("panache".to_string()));
        assert!(lsp_diag.message.contains("h1 to h3"));

        // Verify range conversion
        assert_eq!(lsp_diag.range.start.line, 2); // Line 3 in text becomes line 2 (0-indexed)
    }

    #[test]
    fn test_convert_diagnostic_severity() {
        use crate::linter::diagnostics::{
            Diagnostic as PanacheDiagnostic, DiagnosticOrigin, Location, Severity,
        };
        use rowan::TextRange;

        let text = "test\n";
        let index = LineIndex::new(text);

        let error_diag = PanacheDiagnostic {
            severity: Severity::Error,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 4.into()),
            },
            message: "Error".to_string(),
            code: "test-error".to_string(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&error_diag, &index);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::ERROR));

        let info_diag = PanacheDiagnostic {
            severity: Severity::Info,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 4.into()),
            },
            message: "Info".to_string(),
            code: "test-info".to_string(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&info_diag, &index);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn test_apply_content_change_insert() {
        let text = "hello world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 6,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            }),
            range_length: None,
            text: "beautiful ".to_string(),
        };

        assert_eq!(apply(text, &change), "hello beautiful world");
    }

    #[test]
    fn test_apply_content_change_delete() {
        let text = "hello beautiful world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 5,
                },
                end: Position {
                    line: 0,
                    character: 15,
                },
            }),
            range_length: None,
            text: String::new(),
        };

        assert_eq!(apply(text, &change), "hello world");
    }

    #[test]
    fn test_apply_content_change_replace() {
        let text = "hello world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            }),
            range_length: None,
            text: "goodbye".to_string(),
        };

        assert_eq!(apply(text, &change), "goodbye world");
    }

    #[test]
    fn test_apply_content_change_full_document() {
        let text = "old content";
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "new content".to_string(),
        };

        assert_eq!(apply(text, &change), "new content");
    }

    #[test]
    fn test_apply_content_change_multiline() {
        let text = "line1\nline2\nline3";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 1,
                    character: 2,
                },
                end: Position {
                    line: 2,
                    character: 2,
                },
            }),
            range_length: None,
            text: "NEW\nLINE".to_string(),
        };

        assert_eq!(apply(text, &change), "line1\nliNEW\nLINEne3");
    }

    /// A client whose version has drifted can send a range past the end of the
    /// text the server holds. Both ends must clamp to the end of the document:
    /// clamping the *start* to 0 instead (which is what an `unwrap_or(0)` does)
    /// replaces the entire document with the change text.
    #[test]
    fn out_of_range_change_appends_rather_than_wiping_the_document() {
        let text = "line1\n";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 9,
                    character: 0,
                },
                end: Position {
                    line: 9,
                    character: 0,
                },
            }),
            range_length: None,
            text: "x".to_string(),
        };

        assert_eq!(apply(text, &change), "line1\nx");
    }

    /// A reversed range is normalized. Left alone it underflows the capacity
    /// computation: a panic in debug, and in release a `usize::MAX`-sized
    /// allocation request.
    #[test]
    fn reversed_change_range_is_normalized() {
        let text = "abcdef";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 4,
                },
                end: Position {
                    line: 0,
                    character: 2,
                },
            }),
            range_length: None,
            text: "X".to_string(),
        };

        assert_eq!(apply(text, &change), "abXef");
    }
}
