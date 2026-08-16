//! What one keystroke costs the LSP main loop *before* any parse runs.
//!
//! `benches/lsp_incremental.rs` says it outright (its module doc, "Applying the
//! client's changes to the text buffer is *not* timed"): every existing bench
//! precomputes the post-change text and times parse work only. `lsp_relint`
//! does the same, keeping its `dirty_one` edit outside the timed region. So the
//! one phase that runs synchronously on the main thread, blocking every
//! subsequent notification and request, is the one phase nothing measures.
//!
//! This bench is the mirror image of that exclusion: it times exactly the
//! region those two skip, and does *not* time the parse except in the
//! end-to-end row that exists to give the others a denominator.
//!
//! # The path under measurement
//!
//! `documents::did_change` (`src/lsp/documents.rs`), in order:
//!
//! 1. `load_config_notifying` --- an ancestor-directory walk, a `panache.toml`
//!    read, and two TOML parses, per keystroke.
//! 2. `content_or_empty(..).to_string()` --- a full copy of the document out of
//!    salsa.
//! 3. `apply_content_change` per change (`src/lsp/conversions.rs`) --- a fresh
//!    `String` per change, and a from-scratch `LineIndex::new` per ranged
//!    change.
//! 4. `update_file_text` --- `Arc::from(String)`, another full copy
//!    (`src/salsa.rs`).
//! 5. `intern_config` --- a linear scan comparing whole `Config` values.
//! 6. `arm_settle` --- stamps a deadline. No parse, no dispatch.
//!
//! # Rows
//!
//! | row | what runs | what it proves |
//! | --- | --------- | -------------- |
//! | `config` | one config resolution + intern | that resolving config stays **size-independent**, and how much a keystroke would pay if it were ever put back on this path |
//! | `keystroke` | a whole `didChange`, one 1-char ranged edit | the headline: what a keystroke costs the main loop |
//! | `batch4` | a whole `didChange`, four scattered 1-char edits | the per-change fan-out --- N copies **and** N index builds |
//! | `end_to_end` | `keystroke` plus the parse it schedules | the machine-relative denominator |
//!
//! The `config` row calls the same two functions `did_change` used to call
//! before config resolution came off the keystroke path. It is kept because it
//! is the only thing that would notice the cost coming back, and because a
//! reload still pays it on every save and every watcher event.
//!
//! There is deliberately **no** no-op full-replace row. Its notification
//! carries the whole document, so the timed region would be dominated by the
//! harness's own `String` clone rather than by the server; and what a staleness
//! guard buys there is avoided *invalidation*, which a wall-clock row cannot
//! see. That belongs in a salsa exec-log test, and lives in one.
//!
//! Each timed iteration alternates inserting and deleting one character, so the
//! text genuinely changes every round: salsa sees a fresh revision and no
//! iteration is a memoized no-op. The edit site is ~80% through the document
//! and off the line start, so the splice's tail memmove is real.
//!
//! # Contracts
//!
//! Absolute microseconds are machine-dependent, so every gate check is a
//! **ratio between two numbers from the same run** --- scaling across document
//! sizes, and each row's share of the end-to-end keystroke. Ratios on a
//! sub-microsecond baseline measure noise, so each is waived below
//! [`MIN_ABSOLUTE_US`]. The thresholds below were filled in from a measured run
//! and are meant to be ratcheted down as the costs come off the path, not left
//! as a high-water mark.
//!
//! # Results
//!
//! Median microseconds per iteration. AMD Ryzen 9 7900, rustc 1.94.1, release,
//! `experimental.incrementalParsing` off.
//!
//! Baseline, when this bench was written --- `did_change` resolved config from
//! disk on every keystroke:
//!
//! ```text
//!                            small (756 B)   medium (29 KB)   large (293 KB)
//! config load + intern             54.40            53.74            55.39
//! didChange, 1-char edit           54.47            73.69           282.27
//! didChange, 4 changes             57.87           130.23           907.50
//! didChange + parse               165.49           809.34        11 803.31
//! ```
//!
//! The config load is a flat ~54 us whatever the document, so on a small one it
//! *was* the write phase: 54.40 of 54.47 us, over 0.06 us of actual text work.
//!
//! After taking config resolution off the keystroke path:
//!
//! ```text
//!                            small (756 B)   medium (29 KB)   large (293 KB)
//! config load + intern             52.67            53.06            52.65
//! didChange, 1-char edit            0.94            19.31           215.27
//! didChange, 4 changes              3.03            73.48           902.05
//! didChange + parse                84.68           697.55        10 606.75
//! ```
//!
//! A keystroke in a small document costs 58x less than it did. What is left is
//! the text work, and it is linear and not small: 215 us of copying and
//! rescanning a 293 KB document, on the thread that has to accept the next
//! keystroke, and 4x that for a four-change notification.
//!
//! Run-to-run spread on the large rows is wide (the `keystroke` row has been
//! seen anywhere from 215 to 400 us on an otherwise idle machine), which is why
//! every check below is a ratio taken within one run rather than a comparison
//! against a recorded number.
//!
//! # Known optimism
//!
//! `GlobalState::document_map_mut` is `Arc::make_mut`, which clones the whole
//! document map when a worker still holds a snapshot of it. No snapshot is live
//! during these rows, so the bench sees the cheap branch; production may not.
//!
//! In the other direction, each iteration clones the notification params (a URI
//! and a one-character change) inside the timed region, because the handler
//! consumes them. That is a couple of hundred bytes, constant across document
//! sizes, and so distorts no ratio this bench asserts on.
//!
//! # Running
//!
//! ```sh
//! bash benches/documents/download.sh     # once; the corpus is gitignored
//! cargo bench --bench lsp_write_phase    # measure
//! task bench:write-phase-gate            # measure and enforce the contracts
//! ```
//!
//! Knobs: `PANACHE_LSP_WRITE_BENCH_ASSERT=1` (enforce),
//! `PANACHE_LSP_WRITE_BENCH_ITERATIONS` (scale every row's iteration count),
//! `PANACHE_LSP_WRITE_BENCH_OUTPUT_JSON` (write the report).

use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use lsp_types::{
    DidChangeTextDocumentParams, Position, Range, TextDocumentContentChangeEvent, Uri,
    VersionedTextDocumentIdentifier,
};
use panache::lsp::LspTester;
use serde::Serialize;

/// Blocks per row: each is timed as a unit and the reported number is the
/// median block's per-iteration cost. A single mean over one batch (the shape
/// `salsa_keystroke` uses) hides the variance that a shared machine injects.
const BLOCKS: usize = 20;

/// Ratio checks are waived below this. Sized to sit above the noise floor of a
/// single `Instant::now()` pair amortized over a block, so a row that is
/// genuinely microscopic is not failed for the shape of its rounding.
const MIN_ABSOLUTE_US: f64 = 2.0;

/// Config resolution reads the document's *path*, never its bytes, so its cost
/// must not track document size. Checked across a 400x byte range, which is why
/// the slack is small: this is an invariant, not a measurement.
const CONFIG_SCALING_MAX: f64 = 1.5;

/// The write phase copies and scans the document, so it is linear in its size
/// by construction. Checked over the medium -> large step, a 10x byte ratio;
/// the slack covers cache effects that make the large document worse than
/// proportional.
///
/// Measured at ~14x against a 10x byte ratio: the per-byte cost degrades with
/// size, because a full copy and a from-scratch `LineIndex` build (a hash entry
/// per non-ASCII character) both fall out of cache on a 293 KB document. The
/// ceiling therefore allows 2x superlinearity; ratchet it once the write phase
/// splices once and patches its index instead.
const KEYSTROKE_SCALING_MAX: f64 = 20.0;

/// The write phase's ceiling as a share of the end-to-end keystroke. The write
/// phase runs on the main loop and the parse does not, so this is the number
/// that decides whether typing stays responsive. Measured at 1.1% / 2.8% / 2.0%
/// (small / medium / large); the headroom is wide because the denominator is a
/// parse, and making the parser faster must not fail this gate.
const KEYSTROKE_SHARE_MAX: f64 = 0.15;

/// Four changes in one notification against one: `did_change` loops
/// `apply_content_change` per change, so today each change pays its own full
/// copy *and* its own `LineIndex::new`. A per-notification splice collapses
/// this toward 1.0.
///
/// Measured at 3.2x on the large document. Slack again covers the config load
/// leaving the path, which lifts the ratio (~3.8x) before the splice fix drops
/// it; ratchet hard once that lands.
const BATCH_FANOUT_MAX: f64 = 5.0;

/// The mode the thresholds above were calibrated against. Incremental parsing
/// changes what the end-to-end row measures, and `PANACHE_INCREMENTAL_PARSING`
/// can flip it out from under a gate run, so assert mode refuses to compare
/// against contracts calibrated for the other mode.
const CALIBRATED_INCREMENTAL_PARSING: bool = false;

/// Documents this bench needs, checked before anything runs. They are
/// gitignored and fetched by `benches/documents/download.sh`; without this
/// check a run on a fresh checkout would pass by measuring one size.
const REQUIRED_DOCUMENTS: &[&str] = &["large_authoring.qmd", "pandoc_manual.md"];

/// How deep below the workspace root the document sits. A document in the root
/// makes `find_in_tree`'s ancestor walk degenerate and the config row
/// meaningless; real projects nest.
const DOCUMENT_DEPTH: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    Config,
    Keystroke,
    Batch4,
    EndToEnd,
}

/// Every row in measurement order. Adding a variant without adding it here is
/// a compile error at [`Row::contract`]'s exhaustive match, which is the point:
/// a row cannot exist without saying what it claims.
const ROWS: [Row; 4] = [Row::Config, Row::Keystroke, Row::Batch4, Row::EndToEnd];

struct Contract {
    /// Ceiling on the growth from the medium to the large document (a 10x byte
    /// ratio). `None` waives the check.
    max_scaling: Option<f64>,
    /// Ceiling on this row's share of the end-to-end keystroke, at every size.
    max_share_of_end_to_end: Option<f64>,
}

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Config => "config load + intern",
            Row::Keystroke => "didChange, 1-char edit",
            Row::Batch4 => "didChange, 4 changes",
            Row::EndToEnd => "didChange + parse",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Row::Config => "config",
            Row::Keystroke => "keystroke",
            Row::Batch4 => "batch4",
            Row::EndToEnd => "end_to_end",
        }
    }

    fn contract(self) -> Contract {
        match self {
            // Size-independent by construction: config resolution never reads
            // the document. A scaling failure here means an O(N) step landed on
            // the config path. No share ceiling: this row measures a cost that
            // is on the keystroke path only until it is taken off it, after
            // which its share of a keystroke it no longer runs in means
            // nothing.
            Row::Config => Contract {
                max_scaling: Some(CONFIG_SCALING_MAX),
                max_share_of_end_to_end: None,
            },
            // The headline. Linear in the document, and bounded as a share of
            // the keystroke it is part of.
            Row::Keystroke => Contract {
                max_scaling: Some(KEYSTROKE_SCALING_MAX),
                max_share_of_end_to_end: Some(KEYSTROKE_SHARE_MAX),
            },
            // Same, plus the per-change fan-out checked separately against
            // `keystroke` (see `BATCH_FANOUT_MAX`).
            Row::Batch4 => Contract {
                max_scaling: Some(KEYSTROKE_SCALING_MAX),
                max_share_of_end_to_end: None,
            },
            // The denominator. It is the parse, so it has no ceiling of its own
            // and its scaling is the parser's business, not this bench's.
            Row::EndToEnd => Contract {
                max_scaling: None,
                max_share_of_end_to_end: None,
            },
        }
    }

    /// Iteration count for this row on a document of `bytes`, before the
    /// `PANACHE_LSP_WRITE_BENCH_ITERATIONS` scale factor. The parse row is
    /// milliseconds where the others are microseconds, so it gets far fewer.
    fn iterations(self, bytes: usize) -> usize {
        let big = bytes > 100_000;
        match (self, big) {
            (Row::EndToEnd, true) => 40,
            (Row::EndToEnd, false) => 200,
            (_, true) => 400,
            (_, false) => 2_000,
        }
    }
}

struct Stats {
    median_us: f64,
    mean_us: f64,
    p95_us: f64,
}

fn summarize(mut per_iter_us: Vec<f64>) -> Stats {
    per_iter_us.sort_by(f64::total_cmp);
    let len = per_iter_us.len();
    if len == 0 {
        return Stats {
            median_us: 0.0,
            mean_us: 0.0,
            p95_us: 0.0,
        };
    }
    let median = if len.is_multiple_of(2) {
        (per_iter_us[len / 2 - 1] + per_iter_us[len / 2]) / 2.0
    } else {
        per_iter_us[len / 2]
    };
    let p95_idx = ((len as f64 - 1.0) * 0.95).round() as usize;
    Stats {
        median_us: median,
        mean_us: per_iter_us.iter().sum::<f64>() / len as f64,
        p95_us: per_iter_us[p95_idx.min(len - 1)],
    }
}

/// Time `f` in blocks, returning the per-iteration cost of each. Warms with a
/// tenth of the iterations first so the first block is not paying for a cold
/// branch predictor or a lazily-grown allocation.
fn time_blocks(iterations: usize, mut f: impl FnMut()) -> Stats {
    for _ in 0..(iterations / 10).max(1) {
        f();
    }
    let blocks = BLOCKS.min(iterations.max(1));
    let per_block = (iterations / blocks).max(1);
    let mut per_iter_us = Vec::with_capacity(blocks);
    for _ in 0..blocks {
        let start = Instant::now();
        for _ in 0..per_block {
            f();
        }
        let elapsed = start.elapsed().as_nanos() as f64 / 1_000.0;
        per_iter_us.push(elapsed / per_block as f64);
    }
    summarize(per_iter_us)
}

#[derive(Clone, Copy)]
struct Document {
    label: &'static str,
    file: &'static str,
}

const DOCUMENTS: [Document; 3] = [
    Document {
        label: "small",
        file: "small.qmd",
    },
    Document {
        label: "medium",
        file: "large_authoring.qmd",
    },
    Document {
        label: "large",
        file: "pandoc_manual.md",
    },
];

/// A position mid-way along the first non-empty line at or after
/// `numerator/denominator` of the way through the document.
///
/// Mid-line rather than at column 0 so the splice memmoves a real tail, and
/// non-empty so the insert this position anchors has a character to delete
/// again on the next iteration. The column is in UTF-16 code units, as LSP
/// requires; the bench computes it itself because `LineIndex` is crate-private.
fn edit_position(text: &str, numerator: usize, denominator: usize) -> Position {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Position::new(0, 0);
    }
    let start = (lines.len() * numerator / denominator).min(lines.len() - 1);
    let line = (start..lines.len())
        .chain((0..start).rev())
        .find(|index| !lines[*index].trim().is_empty())
        .unwrap_or(start);
    let text_of_line = lines[line];
    let utf16_len: usize = text_of_line.chars().map(char::len_utf16).sum();
    Position::new(line as u32, (utf16_len / 2).max(1) as u32)
}

fn insert_change(at: Position) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
        range: Some(Range::new(at, at)),
        range_length: None,
        text: "z".to_string(),
    }
}

fn delete_change(at: Position) -> TextDocumentContentChangeEvent {
    let after = Position::new(at.line, at.character + 1);
    TextDocumentContentChangeEvent {
        range: Some(Range::new(at, after)),
        range_length: None,
        text: String::new(),
    }
}

fn params_for(
    uri: &Uri,
    changes: Vec<TextDocumentContentChangeEvent>,
) -> DidChangeTextDocumentParams {
    DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 1,
        },
        content_changes: changes,
    }
}

struct Fixture {
    /// Dropped last, after the tester: removing the tree out from under an open
    /// document would make the config row measure a missing file.
    _dir: tempfile::TempDir,
    tester: LspTester,
    uri: Uri,
    uri_string: String,
    text: String,
    /// One insert and its undo, and the same as a four-change batch.
    insert: Vec<TextDocumentContentChangeEvent>,
    delete: Vec<TextDocumentContentChangeEvent>,
    insert_batch: Vec<TextDocumentContentChangeEvent>,
    delete_batch: Vec<TextDocumentContentChangeEvent>,
}

fn build_fixture(document: Document) -> Option<Fixture> {
    let source = Path::new("benches/documents").join(document.file);
    let text = fs::read_to_string(&source).ok()?;

    let dir = tempfile::TempDir::new().expect("create temp fixture dir");
    let root = dir.path();
    fs::write(
        root.join("panache.toml"),
        "flavor = \"quarto\"\ncache = false\n",
    )
    .expect("write panache.toml");
    // `project_boundary` stops the ancestor walk at a repository root. Without
    // one the walk would run all the way to `/`, which no real project does.
    fs::create_dir_all(root.join(".git")).expect("create .git marker");

    let mut nested = root.to_path_buf();
    for level in 0..DOCUMENT_DEPTH {
        nested = nested.join(format!("level{level}"));
    }
    fs::create_dir_all(&nested).expect("create nested dirs");
    let path = nested.join(document.file);
    fs::write(&path, &text).expect("write document");

    let uri_string = format!("file://{}", path.display());
    let uri: Uri = uri_string.parse().expect("valid file uri");

    let mut tester = LspTester::new();
    tester.initialize(&format!("file://{}", root.display()));
    tester.open_document(&uri_string, &text, "quarto");
    // Prime the parse so the end-to-end row's first iteration is not paying for
    // a cold document on top of its own edit.
    black_box(tester.get_document_tree(&uri_string));

    let at = edit_position(&text, 4, 5);
    // Four sites on distinct lines, ordered bottom-up so each change's position
    // is unaffected by the ones applied before it in the same notification.
    let mut batch: Vec<Position> = (1..=4)
        .map(|fifth| edit_position(&text, fifth, 5))
        .collect();
    batch.sort_by_key(|position| std::cmp::Reverse(position.line));
    batch.dedup_by_key(|position| position.line);

    Some(Fixture {
        _dir: dir,
        tester,
        uri,
        uri_string,
        text,
        insert: vec![insert_change(at)],
        delete: vec![delete_change(at)],
        insert_batch: batch.iter().copied().map(insert_change).collect(),
        delete_batch: batch.iter().copied().map(delete_change).collect(),
    })
}

impl Fixture {
    /// Restore the document to its base text and resync the parse. Untimed, and
    /// run between rows: a row that ends on an odd iteration leaves one stray
    /// character behind, and the end-to-end row must not start from a base
    /// thousands of edits stale.
    fn reset(&mut self) {
        let full = vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: self.text.clone(),
        }];
        self.tester.edit_document(&self.uri_string, full);
        black_box(self.tester.get_document_tree(&self.uri_string));
    }

    fn run(&mut self, row: Row, iterations: usize) -> Stats {
        self.reset();
        let stats = match row {
            Row::Config => {
                let uri = self.uri.clone();
                time_blocks(iterations, || {
                    black_box(self.tester.reload_and_intern_config(&uri));
                })
            }
            Row::Keystroke => {
                let (uri, insert, delete) =
                    (self.uri.clone(), self.insert.clone(), self.delete.clone());
                let mut flip = false;
                time_blocks(iterations, || {
                    flip = !flip;
                    let changes = if flip { insert.clone() } else { delete.clone() };
                    self.tester.apply_did_change(params_for(&uri, changes));
                })
            }
            Row::Batch4 => {
                let (uri, insert, delete) = (
                    self.uri.clone(),
                    self.insert_batch.clone(),
                    self.delete_batch.clone(),
                );
                let mut flip = false;
                time_blocks(iterations, || {
                    flip = !flip;
                    let changes = if flip { insert.clone() } else { delete.clone() };
                    self.tester.apply_did_change(params_for(&uri, changes));
                })
            }
            Row::EndToEnd => {
                let (uri, uri_string, insert, delete) = (
                    self.uri.clone(),
                    self.uri_string.clone(),
                    self.insert.clone(),
                    self.delete.clone(),
                );
                let mut flip = false;
                time_blocks(iterations, || {
                    flip = !flip;
                    let changes = if flip { insert.clone() } else { delete.clone() };
                    self.tester.apply_did_change(params_for(&uri, changes));
                    black_box(self.tester.get_document_tree(&uri_string));
                })
            }
        };
        self.reset();
        stats
    }
}

#[derive(Serialize)]
struct RowResult {
    document: String,
    bytes: usize,
    row: String,
    iterations: usize,
    median_us: f64,
    mean_us: f64,
    p95_us: f64,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    /// The mode measured. Contracts are calibrated for one; recording it keeps
    /// two runs from being compared across a silent env override.
    incremental_parsing: bool,
    results: Vec<RowResult>,
}

fn median(results: &[RowResult], document: &str, row: Row) -> Option<f64> {
    results
        .iter()
        .find(|result| result.document == document && result.row == row.key())
        .map(|result| result.median_us)
}

/// Check the measured grid against the declared contracts, printing each check
/// with its margin so drift is visible well before it fails. Returns every
/// failure rather than stopping at the first, so one run tells you everything
/// that moved.
fn check_expectations(results: &[RowResult]) -> Vec<String> {
    let mut failures = Vec::new();
    let mut checks: Vec<(bool, String)> = Vec::new();

    println!("\nThresholds");
    println!("==========");

    for row in ROWS {
        let contract = row.contract();

        if let Some(max) = contract.max_scaling
            && let (Some(medium), Some(large)) = (
                median(results, "medium", row),
                median(results, "large", row),
            )
        {
            let scaling = large / medium;
            checks.push((
                scaling <= max || large <= MIN_ABSOLUTE_US,
                format!(
                    "{}: medium -> large scaling {scaling:.2}x <= {max:.2}x (10x bytes)",
                    row.key()
                ),
            ));
        }

        if let Some(max) = contract.max_share_of_end_to_end {
            for document in DOCUMENTS {
                let (Some(measured), Some(end_to_end)) = (
                    median(results, document.label, row),
                    median(results, document.label, Row::EndToEnd),
                ) else {
                    continue;
                };
                let share = measured / end_to_end;
                checks.push((
                    share <= max || measured <= MIN_ABSOLUTE_US,
                    format!(
                        "{}/{}: {:.1}% of the end-to-end keystroke <= {:.0}%",
                        document.label,
                        row.key(),
                        share * 100.0,
                        max * 100.0
                    ),
                ));
            }
        }
    }

    // Cross-row: four changes in one notification against one change.
    for document in DOCUMENTS {
        let (Some(batch), Some(keystroke)) = (
            median(results, document.label, Row::Batch4),
            median(results, document.label, Row::Keystroke),
        ) else {
            continue;
        };
        let fanout = batch / keystroke;
        checks.push((
            fanout <= BATCH_FANOUT_MAX || batch <= MIN_ABSOLUTE_US,
            format!(
                "{}/batch4: 4-change fan-out {fanout:.2}x <= {BATCH_FANOUT_MAX:.2}x",
                document.label
            ),
        ));
    }

    for (passed, description) in checks {
        println!("  {} {description}", if passed { "ok  " } else { "FAIL" });
        if !passed {
            failures.push(description);
        }
    }

    failures
}

/// Fail before measuring anything if a document the gate depends on is absent:
/// `build_fixture` skips a missing one silently, and a gate that measures one
/// size passes by not looking.
fn check_required_documents() -> Vec<String> {
    REQUIRED_DOCUMENTS
        .iter()
        .filter(|name| !Path::new("benches/documents").join(name).is_file())
        .map(|name| format!("benches/documents/{name} is missing"))
        .collect()
}

fn main() {
    let scale = env::var("PANACHE_LSP_WRITE_BENCH_ITERATIONS")
        .ok()
        .and_then(|raw| raw.parse::<f64>().ok())
        .unwrap_or(1.0)
        .max(0.001);

    // The gate. Off by default, so a run that only wants the numbers stays a
    // measurement and never fails the shell it was typed into.
    let assert_mode = matches!(
        env::var("PANACHE_LSP_WRITE_BENCH_ASSERT").as_deref(),
        Ok("1") | Ok("true")
    );

    if assert_mode {
        let missing = check_required_documents();
        if !missing.is_empty() {
            eprintln!("PANACHE_LSP_WRITE_BENCH_ASSERT=1 needs the real-document corpus:");
            for entry in &missing {
                eprintln!("  {entry}");
            }
            eprintln!("Run `benches/documents/download.sh` (or `task bench:write-phase-gate`).");
            std::process::exit(1);
        }
    }

    println!("LSP write-phase benchmarks");
    println!("==========================");
    println!("Median per-iteration cost in microseconds. The write phase runs on");
    println!("the main loop; the parse in `end_to_end` does not.\n");

    let mut results: Vec<RowResult> = Vec::new();
    let mut incremental_parsing = CALIBRATED_INCREMENTAL_PARSING;

    for document in DOCUMENTS {
        let Some(mut fixture) = build_fixture(document) else {
            println!(
                "{:<8} skipped: benches/documents/{} is missing",
                document.label, document.file
            );
            continue;
        };
        incremental_parsing = fixture.tester.experimental_incremental_parsing_enabled();
        let bytes = fixture.text.len();
        let size = if bytes < 1024 {
            format!("{bytes} B")
        } else {
            format!("{} KB", bytes / 1024)
        };
        println!("=== {} ({}, {size}) ===", document.label, document.file);

        for row in ROWS {
            let iterations = ((row.iterations(bytes) as f64) * scale).round().max(4.0) as usize;
            let stats = fixture.run(row, iterations);
            println!(
                "{:<26}{:>10.2} us  (mean {:>9.2}, p95 {:>9.2}, n={iterations})",
                row.label(),
                stats.median_us,
                stats.mean_us,
                stats.p95_us
            );
            results.push(RowResult {
                document: document.label.to_string(),
                bytes,
                row: row.key().to_string(),
                iterations,
                median_us: stats.median_us,
                mean_us: stats.mean_us,
                p95_us: stats.p95_us,
            });
        }

        println!();
    }

    let mut failures = Vec::new();
    if assert_mode {
        if incremental_parsing != CALIBRATED_INCREMENTAL_PARSING {
            failures.push(format!(
                "incremental parsing is {incremental_parsing}, but the contracts were calibrated \
                 against {CALIBRATED_INCREMENTAL_PARSING} (check PANACHE_INCREMENTAL_PARSING)"
            ));
        }
        failures.extend(check_expectations(&results));
    }

    // Written before the verdict: a failing gate is exactly when the numbers
    // are worth keeping.
    if let Ok(path) = env::var("PANACHE_LSP_WRITE_BENCH_OUTPUT_JSON") {
        let report = Report {
            schema_version: 1,
            incremental_parsing,
            results,
        };
        let json = serde_json::to_string_pretty(&report)
            .expect("failed to serialize write-phase benchmark JSON report");
        fs::write(&path, json)
            .unwrap_or_else(|e| panic!("failed to write benchmark JSON report to '{path}': {e}"));
        println!("\nWrote JSON report to {path}");
    }

    if !failures.is_empty() {
        eprintln!("\n{} threshold(s) failed:", failures.len());
        for failure in &failures {
            eprintln!("  {failure}");
        }
        std::process::exit(1);
    }
}
