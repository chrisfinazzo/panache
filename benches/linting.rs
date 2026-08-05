//! Linter throughput benchmark.
//!
//! The other benches only touch the linter incidentally: `cli_cache` lints
//! five-line synthetic stubs (it measures cache hit ratio), and `lsp_relint`
//! hides rule cost behind salsa memos. Neither answers "did adding a rule slow
//! linting down", so this bench times the pieces separately:
//!
//! 1. `LintIndex::build` with an empty interest set (the unconditional
//!    `preorder_with_tokens` walk) versus one that also buckets `PARAGRAPH` and
//!    `PLAIN`. Rules declare interests through `Rule::node_interests`, and the
//!    runner unions them into a single shared index, so a rule that introduces
//!    a `SyntaxKind` no other registered rule wanted pays a step change here
//!    that has nothing to do with its own `check` body.
//! 2. Full `panache::linter::lint`, the user-visible number.
//! 3. Individual rules via `Rule::check_tree`, to attribute cost to a rule.
//!
//! Run with `taskset -c 0` and compare medians across several runs; per-run
//! variance on a warm machine is around 3-5%.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use panache::linter::index::LintIndex;
use panache::linter::rules::Rule;
use panache::linter::{Diagnostic, lint};
use panache::syntax::{SyntaxKind, SyntaxNode};
use serde::Serialize;

/// Time `LintIndex::build` over a fixed interest set.
fn bench_index(tree: &SyntaxNode, want: &HashSet<SyntaxKind>, iterations: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(LintIndex::build(black_box(tree), want, false));
    }
    start.elapsed()
}

/// Time the whole registry end to end. This includes `default_registry`
/// construction, which `lint` does per call; that cost is identical on both
/// sides of an A/B comparison.
fn bench_lint(
    tree: &SyntaxNode,
    input: &str,
    config: &panache::Config,
    iterations: usize,
) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(lint(black_box(tree), black_box(input), config));
    }
    start.elapsed()
}

/// Time a single rule in isolation. `check_tree` builds a one-off index for
/// just that rule's interests, so this is the rule's own cost plus its slice of
/// the index walk, not its marginal cost inside the shared runner.
fn bench_rule(
    rule: &dyn Rule,
    tree: &SyntaxNode,
    input: &str,
    config: &panache::Config,
    iterations: usize,
) -> (Duration, usize) {
    let mut found = 0;
    let start = Instant::now();
    for _ in 0..iterations {
        let diagnostics: Vec<Diagnostic> =
            rule.check_tree(black_box(tree), black_box(input), config, None);
        found = diagnostics.len();
        black_box(&diagnostics);
    }
    (start.elapsed(), found)
}

#[derive(Debug, Serialize)]
struct RuleResult {
    rule: String,
    avg_us: f64,
    diagnostics: usize,
}

#[derive(Debug, Serialize)]
struct BenchmarkResult {
    document: String,
    size_bytes: usize,
    line_count: usize,
    iterations: usize,
    index_empty_avg_us: f64,
    index_para_plain_avg_us: f64,
    lint_avg_us: f64,
    throughput_kb_s: f64,
    rules: Vec<RuleResult>,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    schema_version: u32,
    results: Vec<BenchmarkResult>,
}

/// Rules timed individually. `stray-fenced-div-markers` is the closest
/// structural sibling of `swallowed-list-marker` (same `PARAGRAPH`/`PLAIN`
/// buckets), so it doubles as a reference point for what a paragraph-walking
/// rule costs on this corpus.
fn individual_rules() -> Vec<Box<dyn Rule>> {
    use panache::linter::rules::{stray_fenced_div_markers, swallowed_list_marker};
    vec![
        Box::new(stray_fenced_div_markers::StrayFencedDivMarkersRule),
        Box::new(swallowed_list_marker::SwallowedListMarkerRule),
    ]
}

fn run_benchmark(doc_id: &str, input: &str, iterations: usize) -> BenchmarkResult {
    let config = panache::Config::default();
    let tree = panache::parse(input, Some(config.clone()));

    println!("\n{}", "=".repeat(60));
    println!("Benchmark: {doc_id}");
    println!("{}", "=".repeat(60));
    println!(
        "Document size: {} bytes, {} lines",
        input.len(),
        input.lines().count()
    );

    // Warmup.
    for _ in 0..5 {
        let _ = lint(&tree, input, &config);
    }

    let empty: HashSet<SyntaxKind> = HashSet::new();
    let para_plain: HashSet<SyntaxKind> = [SyntaxKind::PARAGRAPH, SyntaxKind::PLAIN]
        .into_iter()
        .collect();

    let index_empty = bench_index(&tree, &empty, iterations);
    let index_empty_avg = index_empty.as_micros() as f64 / iterations as f64;
    let index_para = bench_index(&tree, &para_plain, iterations);
    let index_para_avg = index_para.as_micros() as f64 / iterations as f64;

    println!("\nLintIndex::build:");
    println!("  no interests:        {index_empty_avg:.2}us");
    println!("  PARAGRAPH + PLAIN:   {index_para_avg:.2}us");
    println!(
        "  marginal:            {:.2}us",
        index_para_avg - index_empty_avg
    );

    let lint_time = bench_lint(&tree, input, &config, iterations);
    let lint_avg = lint_time.as_micros() as f64 / iterations as f64;
    println!("\nFull lint (all default rules, CST pre-built):");
    println!("  Total: {lint_time:?} for {iterations} iterations");
    println!(
        "  Average: {lint_avg:.2}us per iteration ({:.2}ms)",
        lint_avg / 1000.0
    );

    println!("\nIndividual rules (check_tree):");
    let mut rules = Vec::new();
    for rule in individual_rules() {
        let (elapsed, found) = bench_rule(rule.as_ref(), &tree, input, &config, iterations);
        let avg = elapsed.as_micros() as f64 / iterations as f64;
        println!(
            "  {:<28} {:>9.2}us  ({found} diagnostics)",
            rule.name(),
            avg
        );
        rules.push(RuleResult {
            rule: rule.name().to_owned(),
            avg_us: avg,
            diagnostics: found,
        });
    }

    let throughput = (input.len() as f64 / 1024.0) / (lint_avg / 1_000_000.0);
    println!("\nLint throughput: {throughput:.2} KB/s");

    BenchmarkResult {
        document: doc_id.to_owned(),
        size_bytes: input.len(),
        line_count: input.lines().count(),
        iterations,
        index_empty_avg_us: index_empty_avg,
        index_para_plain_avg_us: index_para_avg,
        lint_avg_us: lint_avg,
        throughput_kb_s: throughput,
        rules,
    }
}

/// Bench documents live under `benches/documents/` (fetched by `download.sh`);
/// tracked repo documents are addressed from the repo root so the bench still
/// has something to chew on in a fresh clone.
fn load_document(name: &str) -> Option<String> {
    fs::read_to_string(Path::new("benches/documents").join(name))
        .or_else(|_| fs::read_to_string(Path::new(name)))
        .ok()
}

fn main() {
    println!("Panache Linter Benchmarks");
    println!("=========================\n");

    let mut results = Vec::new();

    if let Ok(doc_name) = env::var("PANACHE_BENCH_DOC") {
        let iterations = env::var("PANACHE_BENCH_ITERATIONS")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(20);
        let doc = load_document(&doc_name).unwrap_or_else(|| {
            panic!("PANACHE_BENCH_DOC '{doc_name}' not found under benches/documents/ or repo root")
        });
        results.push(run_benchmark(&doc_name, &doc, iterations));
        maybe_write_json_report(results);
        return;
    }

    let cases: [(&str, usize); 3] = [
        // Tracked; survives a fresh clone without download.sh.
        ("docs/reference/linter-rules.qmd", 50),
        ("medium_quarto.qmd", 100),
        ("pandoc_manual.md", 20),
    ];

    for (name, iterations) in cases {
        match load_document(name) {
            Some(doc) => results.push(run_benchmark(name, &doc, iterations)),
            None => println!("\n[skip] {name} not found - run benches/documents/download.sh"),
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("Benchmarks complete!");
    println!("{}", "=".repeat(60));

    maybe_write_json_report(results);
}

fn maybe_write_json_report(results: Vec<BenchmarkResult>) {
    let Ok(path) = env::var("PANACHE_LINT_BENCH_OUTPUT_JSON") else {
        return;
    };

    let report = BenchmarkReport {
        schema_version: 1,
        results,
    };
    let json =
        serde_json::to_string_pretty(&report).expect("failed to serialize benchmark JSON report");
    fs::write(&path, json)
        .unwrap_or_else(|e| panic!("failed to write benchmark JSON report to '{path}': {e}"));
}
