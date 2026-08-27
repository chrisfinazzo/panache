//! Tier 1 property harness for the math formatter.
//!
//! For each `*.tex` case under
//! `crates/panache-formatter/tests/fixtures/math_corpus/`, asserts four
//! properties that need **no external oracle**:
//!
//! 1. **Idempotency.** `format_math(format_math(x)) == format_math(x)`.
//! 2. **Parser losslessness.** The structural math CST reconstructs the input
//!    byte-for-byte: `parse_math_content(x).text() == x`. (The corpus holds
//!    bare content with no host container prefixes, so `tree.text()` is the right
//!    surface — same shape as `debug format --checks losslessness`.)
//! 3. **Verbatim returns `None`.** `format_math(x, { mode: Verbatim, .. })` is
//!    `None`, so the caller falls back to its verbatim path and a mis-wired call
//!    site can never change bytes in verbatim mode.
//! 4. **Comment preservation.** Formatting retains every TeX comment byte and
//!    its source order, including comments nested inside groups, arguments, and
//!    environments.
//!
//! This is the load-bearing correctness signal; the `pulldown-latex`
//! cross-validation in `math_cross_validation.rs` (Tier 2) is a secondary
//! meaning-drift alarm layered on top.
//!
//! The `MathContext` is chosen by subdirectory (see `fixtures/math_corpus/
//! README.md`): `inline/` → `Inline`; everything else → `Display`.

use std::fs;
use std::path::PathBuf;

use panache_formatter::MathMode;
use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::syntax::{SyntaxKind, SyntaxNode};

#[path = "common/math_corpus.rs"]
mod math_corpus;
use math_corpus::{discover_cases, read_preamble, signature_scope};

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/math_corpus")
}

/// Subdirectory → layout context. `inline/` collapses whitespace on one line;
/// everything else gets the multi-line display treatment.
fn context_for(id: &str) -> MathContext {
    if id.starts_with("inline/") {
        MathContext::Inline
    } else {
        MathContext::Display
    }
}

fn opts(
    reflow: bool,
    context: MathContext,
    signature_scope: panache_parser::semantic::math::SignatureScope,
) -> MathFormatOptions {
    MathFormatOptions {
        mode: if reflow {
            MathMode::Reflow
        } else {
            MathMode::Verbatim
        },
        math_indent: 2,
        line_width: 80,
        bookdown_equation_labels: false,
        context,
        signature_scope,
    }
}

#[test]
fn corpus_satisfies_math_formatter_properties() {
    let root = corpus_root();
    let cases = discover_cases(&root);
    assert!(
        !cases.is_empty(),
        "no cases discovered under {}",
        root.display()
    );

    let mut failures: Vec<String> = Vec::new();
    for case in &cases {
        let id = case
            .strip_prefix(&root)
            .unwrap_or(case)
            .display()
            .to_string();
        let input = match fs::read_to_string(case) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("[{id}] read error: {e}"));
                continue;
            }
        };
        let preamble = match read_preamble(case) {
            Ok(preamble) => preamble,
            Err(e) => {
                failures.push(format!("[{id}] preamble read error: {e}"));
                continue;
            }
        };
        let signature_scope = signature_scope(preamble.as_deref());
        let context = context_for(&id);

        let green = parse_math_content(
            &input,
            MathParseOptions {
                bookdown_equation_labels: false,
            },
        );
        let tree_text = SyntaxNode::new_root(green).text().to_string();
        if tree_text != input {
            failures.push(format!(
                "[{id}] losslessness break ({:+} bytes):\n  input:\n{}\n  tree:\n{}",
                tree_text.len() as i64 - input.len() as i64,
                indent_block(&input),
                indent_block(&tree_text),
            ));
            continue;
        }

        if format_math(&input, &opts(false, context, signature_scope.clone())).is_some() {
            failures.push(format!(
                "[{id}] verbatim mode should return None (caller preserves content):\n  input:\n{}",
                indent_block(&input),
            ));
            continue;
        }

        if let Some(once) = format_math(&input, &opts(true, context, signature_scope.clone())) {
            let twice = format_math(&once, &opts(true, context, signature_scope));
            if twice.as_deref() != Some(once.as_str()) {
                failures.push(format!(
                    "[{id}] idempotency break:\n  pass1:\n{}\n  pass2:\n{}",
                    indent_block(&once),
                    indent_block(twice.as_deref().unwrap_or("<None>")),
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} corpus cases failed:\n\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n\n"),
        );
    }
}

#[test]
fn corpus_preserves_tex_comments() {
    let root = corpus_root();
    let cases = discover_cases(&root);
    let mut comment_cases = 0usize;
    let mut formatted_comment_cases = 0usize;

    for case in &cases {
        let id = case
            .strip_prefix(&root)
            .unwrap_or(case)
            .display()
            .to_string();
        let input =
            fs::read_to_string(case).unwrap_or_else(|error| panic!("failed to read {id}: {error}"));
        let input_comments = tex_comments(&input);
        if input_comments.is_empty() {
            continue;
        }
        comment_cases += 1;

        let preamble = read_preamble(case)
            .unwrap_or_else(|error| panic!("failed to read preamble for {id}: {error}"));
        let formatted = format_math(
            &input,
            &opts(true, context_for(&id), signature_scope(preamble.as_deref())),
        )
        .inspect(|_| {
            formatted_comment_cases += 1;
        })
        .unwrap_or_else(|| input.clone());

        assert_eq!(
            tex_comments(&formatted),
            input_comments,
            "[{id}] formatting changed TeX comment text or source order\n\ninput:\n{}\n\nformatted:\n{}",
            indent_block(&input),
            indent_block(&formatted),
        );
    }

    assert!(
        comment_cases >= 10,
        "comment-preservation gate became too narrow: only {comment_cases} corpus cases contain comments"
    );
    assert!(
        formatted_comment_cases >= 10,
        "comment-preservation gate became too narrow: only {formatted_comment_cases} comment-bearing cases exercise typed formatting"
    );
}

#[test]
fn equivalent_trivia_perturbations_converge() {
    struct Case {
        name: &'static str,
        context: MathContext,
        variants: &'static [&'static str],
    }

    let cases = [
        Case {
            name: "inline operators and scripts",
            context: MathContext::Inline,
            variants: &["x_i+y^2=z", " x_i  +  y^2 = z ", "x_i\n+ y^2\n= z"],
        },
        Case {
            name: "signature-proven math arguments",
            context: MathContext::Inline,
            variants: &[
                r"\frac{a+b}{c+d}",
                r"\frac{ a + b }{ c + d }",
                "\\frac{\n a+b\n}{\n c+d\n}",
            ],
        },
        Case {
            name: "paired delimiters",
            context: MathContext::Display,
            variants: &[
                r"\left(x+y\right)_i",
                r"\left ( x + y \right ) _i",
                "\\left(\n x+y\n\\right)_i",
            ],
        },
        Case {
            name: "aligned environment",
            context: MathContext::Display,
            variants: &[
                "\\begin{aligned}\na&=b+c\\\\\nd&=e+f\n\\end{aligned}",
                "\\begin{aligned}\n  a & = b + c \\\\\n  d & = e + f\n\\end{aligned}",
                "\\begin{aligned} a&=b+c \\\\ d&=e+f \\end{aligned}",
            ],
        },
    ];

    for case in cases {
        let mut variants = case.variants.iter();
        let first = variants
            .next()
            .expect("every convergence case has a baseline");
        let expected = format_math(first, &opts(true, case.context, signature_scope(None)))
            .unwrap_or_else(|| panic!("{} baseline crossed the preservation boundary", case.name));

        for variant in variants {
            let actual = format_math(variant, &opts(true, case.context, signature_scope(None)))
                .unwrap_or_else(|| {
                    panic!(
                        "{} trivia variant crossed the preservation boundary: {variant:?}",
                        case.name
                    )
                });
            assert_eq!(
                actual, expected,
                "{} did not converge after trivia perturbation\nvariant: {variant:?}",
                case.name
            );
        }
    }
}

#[test]
fn corpus_layout_trivia_perturbations_converge() {
    let root = corpus_root();
    let mut exercised = 0usize;
    let mut failures = Vec::new();

    for case in discover_cases(&root) {
        let id = case
            .strip_prefix(&root)
            .unwrap_or(&case)
            .display()
            .to_string();
        if !is_layout_trivia_candidate(&id) {
            continue;
        }

        let input = fs::read_to_string(&case)
            .unwrap_or_else(|error| panic!("failed to read {id}: {error}"));
        if !tex_comments(&input).is_empty() {
            continue;
        }
        let preamble = read_preamble(&case)
            .unwrap_or_else(|error| panic!("failed to read preamble for {id}: {error}"));
        let options = opts(true, context_for(&id), signature_scope(preamble.as_deref()));
        let Some(expected) = format_math(&input, &options) else {
            continue;
        };
        let perturbed = perturb_layout_trivia(&input);
        if perturbed == input {
            continue;
        }
        exercised += 1;

        match format_math(&perturbed, &options) {
            Some(actual) if actual == expected => {}
            Some(actual) => failures.push(format!(
                "[{id}] did not converge\n  input:\n{}\n  perturbed:\n{}\n  expected:\n{}\n  actual:\n{}",
                indent_block(&input),
                indent_block(&perturbed),
                indent_block(&expected),
                indent_block(&actual),
            )),
            None => failures.push(format!(
                "[{id}] perturbation crossed the preservation boundary\n  input:\n{}\n  perturbed:\n{}",
                indent_block(&input),
                indent_block(&perturbed),
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "{} corpus trivia convergence failure(s):\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
    assert!(
        exercised >= 40,
        "corpus trivia-convergence gate became too narrow: only {exercised} typed cases were perturbed"
    );
}

fn is_layout_trivia_candidate(id: &str) -> bool {
    [
        "display/",
        "environments/",
        "groups/",
        "inline/",
        "operators/",
        "scripts/",
    ]
    .iter()
    .any(|prefix| id.starts_with(prefix))
}

fn perturb_layout_trivia(source: &str) -> String {
    let green = parse_math_content(source, MathParseOptions::default());
    let tokens = SyntaxNode::new_root(green)
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .collect::<Vec<_>>();
    let token_count = tokens.len();
    tokens
        .into_iter()
        .enumerate()
        .map(|(index, token)| match token.kind() {
            SyntaxKind::MATH_SPACE => "   ".to_owned(),
            SyntaxKind::MATH_NEWLINE if index + 1 < token_count => {
                format!("{}   ", token.text())
            }
            _ => token.text().to_owned(),
        })
        .collect()
}

fn tex_comments(source: &str) -> Vec<String> {
    let green = parse_math_content(source, MathParseOptions::default());
    SyntaxNode::new_root(green)
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::MATH_COMMENT)
        .map(|token| token.text().to_owned())
        .collect()
}

fn indent_block(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
