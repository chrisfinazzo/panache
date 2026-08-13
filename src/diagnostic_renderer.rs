use std::path::Path;

use crate::cli::MessageFormat;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use panache::linter::{Diagnostic, DiagnosticNoteKind, DiagnosticOrigin, Severity};

pub(crate) fn print_diagnostics(
    diagnostics: &[Diagnostic],
    file: Option<&Path>,
    source: Option<&str>,
    use_color: bool,
    message_format: MessageFormat,
    show_summary: bool,
) {
    let file_name = file.and_then(Path::to_str).unwrap_or("<stdin>");
    let renderer = if use_color {
        Renderer::styled()
    } else {
        Renderer::plain()
    };
    // Scanned once for the whole file. Both of the things a snippet needs from
    // the source used to be recomputed per diagnostic, which made rendering
    // O(findings x file length) — 817 ms against 18 ms of analysis on a
    // 12 000-line file with 1500 findings.
    let index = source.map(SourceIndex::new);

    for diag in diagnostics {
        if matches!(message_format, MessageFormat::Short) {
            println!(
                "{}:{}:{}: {}[{}]: {}",
                file_name,
                diag.location.line,
                diag.location.column,
                severity_name(&diag.severity),
                diag.code,
                diag.message,
            );
            continue;
        }

        if let Some(source) = source {
            print_source_snippet(
                diag,
                file_name,
                source,
                index.as_ref().expect("built whenever source is present"),
                &renderer,
                diag.fix.as_ref(),
            );
        } else {
            println!(
                "{}[{}]: {}",
                severity_name(&diag.severity),
                diag.code,
                diag.message
            );
            println!(
                "  --> {}:{}:{}",
                file_name, diag.location.line, diag.location.column
            );
        }

        if let Some(fix) = &diag.fix
            && (source.is_none() || fix.edits.is_empty())
        {
            print_subdiag("help", &fix.message);
        }
        for note in &diag.notes {
            let kind = match note.kind {
                DiagnosticNoteKind::Note => "note",
                DiagnosticNoteKind::Help => "help",
            };
            print_subdiag(kind, &note.message);
        }

        if diag.origin == DiagnosticOrigin::BuiltIn {
            print_subdiag(
                "note",
                &format!(
                    "configure this rule in panache.toml with [lint.rules] {} = false",
                    diag.code
                ),
            );
            print_subdiag(
                "help",
                &format!(
                    "for further information visit https://panache.bz/reference/linter-rules.html#{}",
                    diag.code
                ),
            );
        }
    }

    if show_summary {
        println!("\nFound {} issue(s)", diagnostics.len());
    }
}

/// The per-file facts a snippet is built from, scanned once instead of per
/// diagnostic.
///
/// Both were O(file) *per finding* before: `annotate-snippets` rebuilds a source
/// map over whatever text it is handed, and it used to be handed the whole file;
/// and `heading-hierarchy`'s context annotation walked from byte 0 looking for
/// the preceding heading. Two independent quadratics over the same loop.
struct SourceIndex {
    /// Byte offset of the start of each line.
    line_starts: Vec<usize>,
    /// Length of the source, so [`SourceIndex::line_start`] past the last line
    /// clamps to the end and `line_start(n)..line_start(n + 1)` is always a
    /// valid slice range.
    len: usize,
    /// Every ATX heading, as `(its `#` run, the end of the line carrying it)`,
    /// ascending, so [`SourceIndex::previous_heading`] is a binary search rather
    /// than a walk from byte 0. The line end is carried because that — not the
    /// `#` run's own end — is what the scan this replaced compared against.
    headings: Vec<(std::ops::Range<usize>, usize)>,
}

impl SourceIndex {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        let mut headings = Vec::new();
        let mut line_start = 0usize;
        for line in source.split('\n') {
            // A trailing `\r` on CRLF input is deliberately left on: it keeps
            // `line.len()` byte-exact for the offsets, and cannot affect the
            // heading test, since `trim_start` and the `#` count both work from
            // the front and `indent` is a difference of two lengths that each
            // carry it.
            let trimmed = line.trim_start();
            let hashes = trimmed.chars().take_while(|c| *c == '#').count();
            if hashes > 0 {
                let indent = line.len() - trimmed.len();
                let run = (line_start + indent)..(line_start + indent + hashes);
                headings.push((run, line_start + line.len()));
            }
            // Only a real `\n` starts a new line. `split` yields a final empty
            // segment after a trailing newline, and pushing for that one would
            // duplicate the last offset and shift every `line_of` past it.
            let next = line_start + line.len() + 1;
            if next <= source.len() {
                line_starts.push(next);
            }
            line_start = next;
        }
        Self {
            line_starts,
            len: source.len(),
            headings,
        }
    }

    /// 0-indexed line containing `offset`.
    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        }
    }

    fn line_start(&self, line: usize) -> usize {
        self.line_starts.get(line).copied().unwrap_or(self.len)
    }

    /// The `#` run of the last heading sitting on a line that *ends* before
    /// `offset` — the boundary the linear scan this replaced used, kept exactly
    /// so a heading on the offset's own line is excluded however long that line
    /// is.
    fn previous_heading(&self, offset: usize) -> Option<std::ops::Range<usize>> {
        let n = self
            .headings
            .partition_point(|(_, line_end)| *line_end < offset);
        self.headings[..n].last().map(|(run, _)| run.clone())
    }
}

fn print_source_snippet(
    diag: &Diagnostic,
    file_name: &str,
    source: &str,
    index: &SourceIndex,
    renderer: &Renderer,
    fix: Option<&panache::linter::Fix>,
) {
    let start: usize = diag.location.range.start().into();
    let end: usize = diag.location.range.end().into();
    let end = end.max(start.saturating_add(1)).min(source.len());

    let primary_span = if let Some(fix) = fix
        && let Some(edit) = fix.edits.first()
    {
        let edit_start: usize = edit.range.start().into();
        let edit_end: usize = edit.range.end().into();
        edit_start..edit_end.max(edit_start.saturating_add(1)).min(source.len())
    } else {
        start..end
    };
    let context_span = (diag.code == "heading-hierarchy")
        .then(|| index.previous_heading(start))
        .flatten();

    // Slice the snippet to the lines its annotations touch, plus a line of
    // padding each side that folding drops. `annotate-snippets` builds a source
    // map of whatever it is given, so handing it the whole file cost O(file) per
    // finding. `line_start` anchors the gutter to absolute line numbers, and the
    // window spans the *context* annotation too — a previous heading can sit
    // arbitrarily far back, and an annotation outside the slice would panic.
    let lo = context_span
        .as_ref()
        .map_or(primary_span.start, |c| c.start.min(primary_span.start));
    let hi = context_span
        .as_ref()
        .map_or(primary_span.end, |c| c.end.max(primary_span.end));
    let first_line = index.line_of(lo).saturating_sub(1);
    let from = index.line_start(first_line);
    let to = index.line_start(index.line_of(hi) + 2);
    let rebase = |span: std::ops::Range<usize>| {
        span.start.saturating_sub(from)..span.end.saturating_sub(from).min(to - from)
    };

    let primary = if let Some(fix) = fix
        && !fix.edits.is_empty()
    {
        AnnotationKind::Primary
            .span(rebase(primary_span))
            .label(format!("help: {}", fix.message))
    } else {
        AnnotationKind::Primary.span(rebase(primary_span))
    };

    let snippet = Snippet::source(&source[from..to])
        .line_start(first_line + 1)
        .path(file_name)
        .annotation(primary);

    let snippet = if let Some(context_span) = context_span {
        snippet.annotation(
            AnnotationKind::Context
                .span(rebase(context_span))
                .label("previous heading is here"),
        )
    } else {
        snippet
    };

    let title = format!("[{}] {}", diag.code, diag.message);
    let report = &[severity_level(&diag.severity)
        .primary_title(&title)
        .element(snippet)];
    println!("{}", renderer.render(report));
}

fn severity_level(severity: &Severity) -> Level<'static> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
        Severity::Info => Level::INFO,
    }
}

fn severity_name(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn print_subdiag(kind: &str, message: &str) {
    println!("  = {kind}: {message}");
}

#[cfg(test)]
mod tests {
    use super::{SourceIndex, severity_name};
    use panache::linter::{Diagnostic, DiagnosticOrigin, Location, Severity};
    use rowan::TextRange;

    /// The linear scan `SourceIndex::previous_heading` replaced: walk from byte
    /// 0, remembering the last heading on a line that ends before `before_offset`.
    /// Kept here as the reference the index is checked against.
    fn scanned_previous_heading(
        source: &str,
        before_offset: usize,
    ) -> Option<std::ops::Range<usize>> {
        let mut line_start = 0usize;
        let mut prev_heading = None;
        for line in source.lines() {
            let line_end = line_start + line.len();
            if line_end >= before_offset {
                break;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                let indent = line.len() - trimmed.len();
                let hashes = trimmed.chars().take_while(|c| *c == '#').count();
                if hashes > 0 {
                    prev_heading = Some((line_start + indent)..(line_start + indent + hashes));
                }
            }
            line_start = line_end + 1;
        }
        prev_heading
    }

    #[test]
    fn previous_heading_matches_the_scan_it_replaced() {
        let sources = [
            "# A\n\n### C\n\ntext\n",
            "no headings at all\njust prose\n",
            "  ## indented\n\n#### deeper\n",
            "# A\n## B\n### C\n#### D\n",
            "# only one\n",
            "",
            "#\n\n##\n",
            // A heading line longer than the offset it is compared against: the
            // case where the line end and the `#` run's end disagree.
            "# a very long heading line indeed with lots of trailing words\n\n## next\n",
            "text\n# trailing heading with no newline",
        ];
        for source in sources {
            let index = SourceIndex::new(source);
            for offset in 0..=source.len() {
                assert_eq!(
                    index.previous_heading(offset),
                    scanned_previous_heading(source, offset),
                    "offset {offset} of {source:?}",
                );
            }
        }
    }

    #[test]
    fn line_starts_cover_every_offset() {
        // `line_of`/`line_start` are what slice the snippet window, so a wrong
        // boundary would move the gutter or panic on a non-char-boundary slice.
        for source in [
            "a\nbb\nccc\n",
            "no trailing newline",
            "",
            "\n\n\n",
            "x\r\ny\r\n",
        ] {
            let index = SourceIndex::new(source);
            for offset in 0..=source.len() {
                let line = index.line_of(offset);
                let start = index.line_start(line);
                assert!(start <= offset, "line {line} starts after {offset}");
                assert!(source.is_char_boundary(start), "{start} splits a char");
                assert_eq!(
                    line,
                    source[..offset].matches('\n').count(),
                    "wrong line for offset {offset} of {source:?}",
                );
            }
        }
    }

    #[test]
    fn built_in_diagnostics_show_panache_guidance() {
        let diag = Diagnostic {
            severity: Severity::Warning,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 1.into()),
            },
            message: "msg".to_string(),
            code: "heading-hierarchy".to_string(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        };
        assert_eq!(diag.origin, DiagnosticOrigin::BuiltIn);
        assert_eq!(severity_name(&diag.severity), "warning");
    }

    #[test]
    fn external_diagnostics_can_be_marked_explicitly() {
        let diag = Diagnostic::warning(
            Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 1.into()),
            },
            "SA5009",
            "msg",
        )
        .with_origin(DiagnosticOrigin::External);
        assert_eq!(diag.origin, DiagnosticOrigin::External);
    }
}
