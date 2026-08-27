//! Manual TeX/PDF smoke gate for representative formatted math shapes.
//!
//! This test is ignored in ordinary runs because it invokes an external TeX
//! installation. Run it explicitly before stabilizing math formatting:
//!
//! ```text
//! cargo test -p panache-formatter --test math_tex_pdf -- --ignored
//! ```

use std::fs;
use std::path::Path;
use std::process::Command;

use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};
use panache_parser::semantic::math::SignatureScope;

fn options(context: MathContext) -> MathFormatOptions {
    MathFormatOptions {
        enabled: true,
        math_indent: 2,
        line_width: 32,
        bookdown_equation_labels: false,
        context,
        signature_scope: SignatureScope::default(),
    }
}

fn document(body: &str, context: MathContext) -> String {
    let math = match context {
        MathContext::Inline => format!("Inline math: \\({body}\\)."),
        MathContext::Display | MathContext::EnvironmentBody => {
            format!("\\[\n{body}\n\\]")
        }
    };
    format!(
        "\\documentclass{{article}}\n\\usepackage{{amsmath}}\n\\begin{{document}}\n{math}\n\\end{{document}}\n"
    )
}

fn compile_pdf(workdir: &Path, job_name: &str, source: &str) {
    let tex_path = workdir.join(format!("{job_name}.tex"));
    fs::write(&tex_path, source)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", tex_path.display()));

    let output = Command::new("pdflatex")
        .current_dir(workdir)
        .args(["-interaction=nonstopmode", "-halt-on-error"])
        .arg(
            tex_path
                .file_name()
                .expect("temporary TeX path has a file name"),
        )
        .output()
        .expect("failed to invoke `pdflatex`; enter the project development shell");
    assert!(
        output.status.success(),
        "pdflatex rejected {job_name}:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let pdf_path = workdir.join(format!("{job_name}.pdf"));
    let pdf = fs::read(&pdf_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", pdf_path.display()));
    assert!(
        pdf.starts_with(b"%PDF-") && pdf.len() > 1_000,
        "{} is not a nonempty PDF",
        pdf_path.display(),
    );
}

#[test]
#[ignore = "requires an external pdflatex installation"]
fn representative_original_and_formatted_math_compile_to_pdf() {
    let cases = [
        ("inline", MathContext::Inline, r"\frac { x_i+y^2 } { 1+z }"),
        (
            "wrapped_display",
            MathContext::Display,
            "alpha+beta+gamma+delta=epsilon+zeta+eta+theta",
        ),
        (
            "commented_environment",
            MathContext::Display,
            "\\begin{aligned}\na&=b+c \\\\\nd&=e % retain this comment\n+f\n\\end{aligned}",
        ),
        (
            "delimited_operand_environment",
            MathContext::Display,
            "\\left(x\\right)\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}",
        ),
    ];
    let temp = tempfile::tempdir().expect("failed to create temporary TeX directory");
    let mut changed = 0usize;

    for (name, context, input) in cases {
        let formatted = format_math(input, &options(context))
            .unwrap_or_else(|| panic!("{name} crossed the preservation boundary"));
        changed += usize::from(formatted != input);

        compile_pdf(
            temp.path(),
            &format!("{name}_original"),
            &document(input, context),
        );
        compile_pdf(
            temp.path(),
            &format!("{name}_formatted"),
            &document(&formatted, context),
        );
    }

    assert_eq!(
        changed,
        cases.len(),
        "every representative must exercise a formatting change"
    );
}
