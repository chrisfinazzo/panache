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
//! 1. `take_line_index` --- the index the previous edit left behind, taken out
//!    of the shared line-index cache so the caller holds the only reference.
//!    Rebuilds only on a first edit or when another writer has moved the text
//!    underneath.
//! 2. `content_change_span` + `LineIndex::replace_range` per change
//!    (`src/lsp/conversions.rs`, `src/lsp/line_index.rs`) --- a binary search,
//!    an in-place table patch, and the two copies that splicing an `Arc<str>`
//!    costs.
//! 3. `update_file_text` --- hands salsa the index's own `Arc` (a refcount
//!    bump, not a copy) (`src/salsa.rs`).
//! 4. `store_line_index` + `arm_settle` --- a map insert and a deadline stamp.
//!    No parse, no dispatch.
//!
//! What is *not* here is the record of what came off this path: config
//! resolution per keystroke, a copy of the document out of salsa, a `String`
//! and a `LineIndex::new` per change, and finally the per-notification index
//! rebuild. The results tables below are that history.
//!
//! # Rows
//!
//! | row | what runs | what it proves |
//! | --- | --------- | -------------- |
//! | `config` | one config resolution + intern | that resolving config stays **size-independent**, and how much a keystroke would pay if it were ever put back on this path |
//! | `keystroke` | a whole `didChange`, one 1-char ranged edit | the headline: what a keystroke costs the main loop |
//! | `read_after_keystroke` | `keystroke` plus the position-resolving read an editor issues after one | that a reader reuses the index the keystroke patched instead of rebuilding it |
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
//! A keystroke in a small document costs 58x less than it did. What was left
//! was the text work: two redundant copies of the document and a from-scratch
//! index per *change*. After splicing once through one index per notification:
//!
//! ```text
//!                            small (756 B)   medium (29 KB)   large (293 KB)
//! config load + intern             52.33            52.86            52.89
//! didChange, 1-char edit            0.94             6.00            97.68
//! didChange, 4 changes              1.33             8.72           147.34
//! didChange + parse                83.05           691.37        10 139.70
//! ```
//!
//! Against the original baseline: 58x on a small document, 12x on a medium one,
//! 2.9x on a large one, and 6.2x for a four-change notification -- where the
//! per-change fan-out fell from 3-4x the cost of a single change to about 1.5x.
//!
//! Every table above was measured with incremental parsing off, which was the
//! default until it was flipped. Only `didChange + parse` moves with that flag
//! --- the write-phase rows never parse --- so the contracts are recalibrated
//! against the shipped mode. Same corpus, same run pair, median us:
//!
//! ```text
//!                    small (756 B)   medium (25 KB)   large (297 KB)
//! parse off                 105.05         1 024.89         10 132.03
//! parse on                   20.70           977.26          1 392.49
//! ```
//!
//! Re-measured after the region tier landed. The medium row used to
//! be the embarrassing one: `large_authoring.qmd` declined the window cutoff on
//! every edit, so its keystroke was a full parse *plus* the cost of preparing
//! and rejecting a reuse, and across three run pairs it landed 13-17% **slower**
//! with the flag on. That regression is gone --- it is now marginally faster
//! (1.05x) rather than materially slower --- and the large document went from
//! 3.2x to 7.3x, most of which is the refdef guard no longer reading its
//! old-text window out of the whole tree.
//!
//! The medium row is still only break-even, and that is worth chasing rather
//! than accepting: `benches/lsp_incremental.rs` splices the same document at
//! 4.0x. The two differ in where they edit --- this bench types at 4/5 of the way
//! through, that one at line 60 --- so the gap is either a shape the region tier
//! declines at that position or host-side work this bench includes and that one
//! does not. It is recorded in `TODO.md` as an open item.
//!
//! Finally, the write phase stopped rebuilding the line index. It read it from
//! the salsa `line_index` memo, which every keystroke invalidates --- so inside
//! a typing burst, which is the only time the write phase runs, it was always
//! cold and always re-scanned the whole document. `GlobalState` now keeps the
//! patched index per open document and hands it to the next keystroke, which
//! also removes the `Arc::make_mut` clone of the tables (the memo used to hold
//! the other reference; taking the index out of the cache leaves one). Same
//! machine, one run pair, median us:
//!
//! ```text
//!                            small (756 B)   medium (24 KB)   large (297 KB)
//! config load + intern             52.42            52.95            54.10
//! didChange, 1-char edit            0.93             6.51           109.33
//! didChange, 4 changes              1.16             8.70           139.37
//! didChange + parse                21.60           991.75         1 386.59
//! ```
//!
//! ```text
//!                            small (756 B)   medium (24 KB)   large (297 KB)
//! config load + intern             52.41            52.73            52.71
//! didChange, 1-char edit            0.35             1.10             8.54
//! didChange, 4 changes              0.56             3.17            34.27
//! didChange + parse                20.75           962.79         1 207.62
//! ```
//!
//! 2.7x on a small document, 5.9x on a medium one, and 12.8x on a large one ---
//! the last against an original baseline of 282 us, so 33x in total. What is
//! left is the two copies `LineIndex::replace_range` makes to splice an
//! `Arc<str>`: at 8.54 us for 297 KB the large row is now memory bandwidth and
//! nothing else, which is the floor this bench has been driving toward.
//!
//! Two consequences for the checks below. The keystroke's medium -> large
//! scaling fell from 16.8x to 7.8x, which is the sharpest remaining signal that
//! the rebuild has not come back, so it is ratcheted hard. The four-change
//! fan-out *rose*, to almost exactly 4.0x on the large document, and that is
//! correct: with the shared fixed cost gone, four changes are four splices. See
//! [`BATCH_FANOUT_MAX`] for why that check has lost its original meaning.
//!
//! Note the direction of the share check below: a cheaper parse is a smaller
//! denominator, so the write phase's share *rises* when the feature works.
//!
//! Then the *reader* stopped rebuilding it too. The cache above lived on
//! `GlobalState`, which is main-thread-only, so a worker read still went through
//! the salsa memo --- and a keystroke invalidates it, so every request an editor
//! issues between keystrokes re-scanned the whole document while the write phase
//! held an index for those very bytes. The `read_after_keystroke` row is what
//! priced that, and one shared cache
//! (`crate::lsp::line_index::LineIndexCache`) is what removed it. Same machine,
//! medians of three runs, large document:
//!
//! ```text
//!                                   before    after
//! didChange, 1-char edit              9.47     9.33
//! didChange, 4 changes               37.15    37.61
//! didChange + line index read        81.94     9.51
//! ```
//!
//! 8.6x on the read row, which now sits *on* the keystroke it follows: the read
//! adds 0.2 us at every document size, i.e. a lock, a hash lookup and a refcount
//! bump. The two write-phase rows do not move, which is the other half of the
//! claim --- the reader's index is the writer's index, not a second one.
//!
//! It also removed a coupling that had nothing to do with reading. While the memo
//! existed, a read left a *second* live `Arc` to the index, so the next
//! keystroke's `Arc::make_mut` copied the tables instead of patching them: with
//! this row placed before `batch4`, that row went 34.24 -> 143.71 us on the large
//! document (fan-out 16.73x against 3.19x on medium). One holder, one index, and
//! the row order stops mattering --- see [`ROWS`].
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
/// Measured at 7.8x (8.54 / 1.10) once the write phase stopped rebuilding the
/// line index, against 16.3-21x while it did. That collapse is what makes this
/// the sharp check now: a returning full-document scan puts the large row back
/// near 109 us against a medium row that barely moves, i.e. straight back to
/// ~17x. The ceiling keeps ~1.5x headroom over the measurement, the same slack
/// the 25.0 it replaces encoded.
///
/// Caveat on the denominator: the medium row is now 1.10 us, *below*
/// [`MIN_ABSOLUTE_US`], while the waiver only fires on the large row. So this
/// ratio is taken over a near-noise-floor denominator and will drift more than
/// it used to. It is kept because nothing else catches the regression it
/// catches; if the medium row gets much cheaper still, re-base it on a document
/// between these two rather than widening the ceiling.
const KEYSTROKE_SCALING_MAX: f64 = 12.0;

/// The same check for a four-change notification, which needs its own ceiling
/// now that the two have diverged: measured at 9.9x (36.60 / 3.70) against the
/// keystroke row's 7.3x, because four splices leave the medium row's small
/// fixed cost a smaller share of the total than one splice does. Sharing
/// [`KEYSTROKE_SCALING_MAX`] would either fail this row or have to be loosened
/// enough to blunt the row that matters. Same ~1.5x headroom.
const BATCH_SCALING_MAX: f64 = 15.0;

/// The write phase's ceiling as a share of the end-to-end keystroke. The write
/// phase runs on the main loop and the parse does not, so this is the number
/// that decides whether typing stays responsive. Measured at 1.7% / 0.1% / 0.7%
/// (small / medium / large) once the line index stopped being rebuilt, against
/// 4.6% / 0.7% / 6.4% before it, 4.1% / 0.6% / 2.6% before the region tier, and
/// 1.1% / 2.8% / 2.0% before the incremental flip.
///
/// Ratcheted to 5%, which is ~3x the worst measured share. It is deliberately
/// not tighter: a cheaper parse is a smaller denominator, so this share *rises*
/// when the parser gets faster (the large row more than doubled once, without
/// the write phase changing at all). Making the parser faster must not fail
/// this gate.
const KEYSTROKE_SHARE_MAX: f64 = 0.05;

/// Four changes in one notification against one. Measured at 1.6x / 2.9x / 4.0x
/// (small / medium / large).
///
/// **This check no longer means what it was written to mean.** It existed to
/// catch a return to per-change rebuilding, which showed as 3-4x against a
/// 1.4-1.5x baseline --- but that gap was an artifact of a large *shared* fixed
/// cost per notification, and removing the index rebuild removed it. Four
/// changes are now four splices and essentially nothing else, so the honest
/// floor is 4.0x, which is also roughly what per-change rebuilding would
/// produce. The two are no longer distinguishable here;
/// [`KEYSTROKE_SCALING_MAX`] is what catches that regression now.
///
/// What survives is a ceiling on work that is *worse* than linear in the change
/// count --- a per-change rescan of the whole notification, say. Hence 5.0: just
/// above the four-splice floor, and far below anything quadratic.
const BATCH_FANOUT_MAX: f64 = 5.0;

/// A read straight after a keystroke, against the keystroke alone. This is the
/// gate on the two phases sharing one line index: a reader that rebuilds instead
/// of reusing pays a full document scan here, and nothing else in the suite
/// notices (reuse changes no answer, only how long it takes to give it).
///
/// Measured at 0.99x / 1.19x / 1.47x (large / medium / small), against 8.6x on
/// the large document while readers went through a salsa memo every keystroke
/// invalidated.
///
/// In practice this gates the large document alone: the other two sit under
/// [`MIN_ABSOLUTE_US`] and are waived, which is the right outcome rather than a
/// gap. A read costs a lock, a hash lookup and a refcount bump, and that fixed
/// cost is half again a 0.37 us keystroke while being nothing at all next to a
/// 9.4 us one -- so a ratio is only meaningful where the document is big enough
/// for a rebuild to dwarf it, and that is exactly where a rebuild would show. The
/// 2.5 ceiling leaves the large row ~2.5x headroom over its 0.99x and still sits
/// far below the 8.6x a returning rebuild puts there.
const READ_REUSE_MAX: f64 = 2.5;

/// The mode the thresholds above were calibrated against: the shipped default,
/// which is on. Incremental parsing changes what the end-to-end row measures,
/// and `PANACHE_INCREMENTAL_PARSING` can flip it out from under a gate run, so
/// assert mode refuses to compare against contracts calibrated for the other
/// mode. This constant is what caught the default flip; re-measure both modes
/// before changing it again.
const CALIBRATED_INCREMENTAL_PARSING: bool = true;

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
    ReadAfterKeystroke,
    Batch4,
    EndToEnd,
}

/// Every row in measurement order. Adding a variant without adding it here is
/// a compile error at [`Row::contract`]'s exhaustive match, which is the point:
/// a row cannot exist without saying what it claims.
/// `ReadAfterKeystroke` sits next to the `Keystroke` row it is compared against.
///
/// That ordering was briefly impossible. While readers went through a salsa memo,
/// a read left *two* live `Arc`s to the index -- the memo's and the cache's --- so
/// the next row's `Arc::make_mut` in `did_change` copied the index tables instead
/// of patching them in place, and `reset()` could not clear it because the memo
/// was keyed on the text rather than on the fixture. Measured from this bench:
/// putting this row before `batch4` took that row's large-document fan-out to
/// 16.73x against 3.19x on medium (34.24 -> 143.71 us). With one shared cache the
/// write phase takes the only reference again, so a read no longer perturbs the
/// row after it.
const ROWS: [Row; 5] = [
    Row::Config,
    Row::Keystroke,
    Row::ReadAfterKeystroke,
    Row::Batch4,
    Row::EndToEnd,
];

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
            Row::ReadAfterKeystroke => "didChange + line index read",
            Row::Batch4 => "didChange, 4 changes",
            Row::EndToEnd => "didChange + parse",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Row::Config => "config",
            Row::Keystroke => "keystroke",
            Row::ReadAfterKeystroke => "read_after_keystroke",
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
            // The keystroke plus the read an editor issues right after it. Both
            // phases share one line index, so the read finds the index the
            // keystroke just patched and this row sits on top of `keystroke`:
            // measured at 9.51 us against 9.33 on the large document. Its own
            // scaling is therefore the keystroke's, under the same ceiling.
            //
            // The claim that matters is the ratio, checked against `keystroke`
            // separately (see `READ_REUSE_MAX`); a rebuilding reader shows there
            // long before it shows here.
            Row::ReadAfterKeystroke => Contract {
                max_scaling: Some(KEYSTROKE_SCALING_MAX),
                max_share_of_end_to_end: Some(KEYSTROKE_SHARE_MAX),
            },
            // Same, under its own ceiling, plus the per-change fan-out checked
            // separately against `keystroke` (see `BATCH_FANOUT_MAX`).
            Row::Batch4 => Contract {
                max_scaling: Some(BATCH_SCALING_MAX),
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
            // The keystroke plus what an editor does right after one: a request
            // that resolves a position, and so needs the line index. It reads
            // through a `StateSnapshot`, i.e. the salsa memo the keystroke just
            // invalidated -- the reader's path, not the write phase's cache.
            Row::ReadAfterKeystroke => {
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
                    black_box(self.tester.snapshot_line_index_len(&uri_string));
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

    // Cross-row: a read straight after a keystroke against the keystroke alone.
    for document in DOCUMENTS {
        let (Some(with_read), Some(keystroke)) = (
            median(results, document.label, Row::ReadAfterKeystroke),
            median(results, document.label, Row::Keystroke),
        ) else {
            continue;
        };
        let reuse = with_read / keystroke;
        checks.push((
            reuse <= READ_REUSE_MAX || with_read <= MIN_ABSOLUTE_US,
            format!(
                "{}/read_after_keystroke: {reuse:.2}x the keystroke alone \
                 <= {READ_REUSE_MAX:.2}x",
                document.label
            ),
        ));
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

/// What a reader adds to a keystroke: `read_after_keystroke` minus `keystroke`.
///
/// This was a whole-document rebuild, because the write phase's cache lived on
/// `GlobalState` and the reader went through a salsa memo every keystroke
/// invalidated -- 68.6 us on the large document, 5.5% of an end-to-end keystroke.
/// Sharing one cache took it to ~0, and this is the line that says so in
/// microseconds rather than as a ratio. Reported, never asserted;
/// [`READ_REUSE_MAX`] is the gate.
fn report_reader_rebuild(results: &[RowResult]) {
    println!("\nWhat a read adds to a keystroke");
    println!("===============================");
    for document in DOCUMENTS {
        let (Some(with_read), Some(keystroke), Some(end_to_end)) = (
            median(results, document.label, Row::ReadAfterKeystroke),
            median(results, document.label, Row::Keystroke),
            median(results, document.label, Row::EndToEnd),
        ) else {
            continue;
        };
        let rebuild = with_read - keystroke;
        println!(
            "  {:<7} {rebuild:>9.2} us  ({:.1}% of the end-to-end keystroke, \
             {:.1}x the write phase)",
            document.label,
            rebuild / end_to_end * 100.0,
            rebuild / keystroke,
        );
    }
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

    // Printed in both modes: it is a measurement, not a threshold, and it is the
    // number the reader-side decision in `TODO.md` turns on.
    report_reader_rebuild(&results);

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
