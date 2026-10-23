use std::path::Path;

use crate::cli::MessageFormat;
use panache::linter::render::DiagnosticRenderer;
use panache::linter::{Diagnostic, DiagnosticOrigin};

pub(crate) fn print_diagnostics(
    diagnostics: &[Diagnostic],
    file: Option<&Path>,
    source: Option<&str>,
    use_color: bool,
    message_format: MessageFormat,
    show_summary: bool,
) {
    let file_name = file.and_then(Path::to_str).unwrap_or("<stdin>");
    let renderer = DiagnosticRenderer::new(file_name, source, use_color);
    let rule_metadata = panache::linter::builtin_rule_metadata();

    for diag in diagnostics {
        if matches!(message_format, MessageFormat::Short) {
            print!("{}", renderer.render_short(diag));
            continue;
        }

        print!("{}", renderer.render(diag));

        if diag.origin == DiagnosticOrigin::BuiltIn {
            if let Some(rule) = rule_metadata
                .iter()
                .find(|rule| rule.codes.iter().any(|code| code.code == diag.code))
            {
                print_subdiag(
                    "note",
                    &format!(
                        "configure this rule in panache.toml with [lint] ignore = [\"{}\"]",
                        rule.name
                    ),
                );
            }
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

fn print_subdiag(kind: &str, message: &str) {
    println!("  = {kind}: {message}");
}
