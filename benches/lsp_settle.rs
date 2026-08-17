//! What one settle publish costs per open document.
//!
//! The live model re-lints **every** open document on each quiescent settle
//! (`benches/lsp_relint.rs` is the harness that established it is affordable),
//! so the per-document publish runs N times per settle while only one of those
//! documents has actually changed. `lsp_relint` measures the model --- all N
//! documents, two shapes, seven counts --- and reports the A/B ratio that
//! decision needed. What it does not do is isolate the one cell that repeats N
//! times: the publish over an *unchanged* document, where every salsa memo hits
//! and what is left is the handler's own work.
//!
//! That cell is what this bench times, at N = 1, so its number multiplies by the
//! open-document count.
//!
//! # The path under measurement
//!
//! `handlers::diagnostics::compute_publishes` (`src/lsp/handlers/diagnostics.rs`)
//! for one document, built-in linters only:
//!
//! 1. `line_index` --- the document's index, from the shared cache the write
//!    phase patched (`src/lsp/line_index.rs`).
//! 2. `built_in_lint_plan` --- the whole parse-and-lint, and a salsa memo hit
//!    whenever the document has not changed since the last publish.
//! 3. `convert_diagnostic` per diagnostic, plus the project-graph accumulated
//!    diagnostics grouped by path and converted against each target's index.
//!
//! # Rows
//!
//! | row | what runs | what it proves |
//! | --- | --------- | -------------- |
//! | `warm` | a publish over an unchanged document | the per-document floor a settle pays N times over, and that it stays **size-independent** |
//! | `after_edit` | a 1-char edit (untimed) then the publish | the denominator: what the *changed* document costs, which is a parse and a lint |
//!
//! `warm` is the row with a contract to keep: every memo hits, so anything that
//! scales with document size there is the handler re-doing work salsa already
//! did. `after_edit` is not a target to optimize here --- it is the parse, and
//! `benches/lsp_incremental.rs` owns that.
//!
//! # Results
//!
//! Median microseconds per publish. AMD Ryzen 9 7900, rustc 1.94.1, release.
//!
//! Baseline, when this bench was written --- the handler took the shared index
//! for its own diagnostics, then built a *second* index over a fresh `String`
//! copy of the same document to map the by-path group covering that same
//! document:
//!
//! ```text
//!               small (2.5 KB)   medium (28 KB)   large (112 KB)
//! warm                    1.43             6.71            22.89
//! after_edit             46.90           162.34           553.08
//! ```
//!
//! The `warm` row is the whole finding: it tracks document size, on the one
//! path where nothing about the document changed. 22.89 us of it on the large
//! document, all of it a `String` copy and an index rebuild, and all of it paid
//! once per open document per settle.
//!
//! After threading the cached `Arc<LineIndex>` through the by-path loop (the
//! copy goes with it --- the external linters now read the index's own `Arc`):
//!
//! ```text
//!               small (2.5 KB)   medium (28 KB)   large (112 KB)
//! warm                    1.05             0.90             1.29
//! after_edit             48.91           147.65           482.72
//! ```
//!
//! `warm` is now flat across a 44x size range, which is the property to hold:
//! publishing an unchanged document costs the same whatever its size.
//! `after_edit` is unchanged --- the 22 us sits inside a parse two orders of
//! magnitude larger, which is exactly why this row exists and why the finding
//! needed the `warm` row to be visible at all.
//!
//! Run: `cargo bench --bench lsp_settle` (honors `PANACHE_LSP_SETTLE_BENCH_ITERS`).

use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use lsp_types::{Position, Range, TextDocumentContentChangeEvent};
use panache::lsp::LspTester;

/// Document sizes to sweep. The span matters more than the absolute sizes: the
/// `warm` row's claim is that it does not track them.
const SIZES: [(&str, usize); 3] = [("small", 45), ("medium", 500), ("large", 1995)];

/// A document with a heading-hierarchy violation (h1 -> h3) so every publish has
/// at least one diagnostic to convert.
fn doc_body(filler_lines: usize) -> String {
    let mut out = String::with_capacity(filler_lines * 48 + 128);
    out.push_str("# Title\n\n### Skipped heading\n\n");
    for i in 0..filler_lines {
        out.push_str(&format!(
            "Paragraph {i:04} alpha beta gamma delta epsilon zeta eta.\n"
        ));
    }
    out
}

fn median(samples: &[Duration]) -> f64 {
    let mut us: Vec<f64> = samples
        .iter()
        .map(|d| d.as_nanos() as f64 / 1000.0)
        .collect();
    us.sort_by(f64::total_cmp);
    if us.is_empty() {
        return 0.0;
    }
    if us.len().is_multiple_of(2) {
        (us[us.len() / 2 - 1] + us[us.len() / 2]) / 2.0
    } else {
        us[us.len() / 2]
    }
}

/// One keystroke-shaped edit inside the filler, alternating so the text really
/// changes every iteration and salsa sees a fresh revision.
fn keystroke(iteration: usize) -> Vec<TextDocumentContentChangeEvent> {
    let at = Position {
        line: 4,
        character: 10,
    };
    vec![TextDocumentContentChangeEvent {
        range: Some(Range {
            start: at,
            end: Position {
                character: at.character + 1,
                ..at
            },
        }),
        range_length: None,
        text: (if iteration.is_multiple_of(2) {
            "x"
        } else {
            "y"
        })
        .to_owned(),
    }]
}

fn bench_size(dir: &Path, label: &str, filler_lines: usize, iters: usize) {
    let path = dir.join(format!("doc_{label}.qmd"));
    let body = doc_body(filler_lines);
    fs::write(&path, &body).expect("write doc");
    let uri = format!("file://{}", path.display());

    let mut tester = LspTester::new();
    tester.initialize(&format!("file://{}", dir.display()));
    tester.open_document(&uri, &body, "quarto");
    // Settle once, so the lint memos and the line index are primed exactly as
    // they are when a settle fires in a live session.
    tester.pump(Duration::from_secs(30));

    for _ in 0..3 {
        black_box(tester.relint_all_open_documents());
    }

    let mut warm = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        black_box(tester.relint_all_open_documents());
        warm.push(start.elapsed());
    }

    let edit_iters = (iters / 4).max(8);
    let mut after_edit = Vec::with_capacity(edit_iters);
    for iteration in 0..edit_iters {
        tester.edit_document(&uri, keystroke(iteration));
        let start = Instant::now();
        black_box(tester.relint_all_open_documents());
        after_edit.push(start.elapsed());
    }

    println!(
        "{label:<8} {:>8} B | warm {:>9.2} | after_edit {:>9.2}",
        body.len(),
        median(&warm),
        median(&after_edit),
    );
}

fn main() {
    let iters = env::var("PANACHE_LSP_SETTLE_BENCH_ITERS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(200);

    let dir = env::temp_dir().join(format!("panache_lsp_settle_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp fixture dir");
    fs::write(
        dir.join("panache.toml"),
        "flavor = \"quarto\"\ncache = false\n",
    )
    .expect("write panache.toml");

    println!("LSP settle publish, one open document (median microseconds)");
    println!("===========================================================");

    for (label, filler_lines) in SIZES {
        bench_size(&dir, label, filler_lines, iters);
    }

    let _ = fs::remove_dir_all(&dir);
}
