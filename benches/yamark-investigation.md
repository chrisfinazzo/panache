# Yamark performance investigation

Measured on September 16, 2026, using an AMD Ryzen 9 7900, Linux, Rust 1.97.1,
and Yamark 0.3.0. Panache's baseline was
`9a054356d677d29687a6185bd2888ab08ed0f1ed` (3.10.0). These measurements use a
different host from the published comparison plots.

## Why the workloads differ

[Yamark's Markdown formatting
path](https://github.com/t-kalinowski/yamark/blob/v0.3.0/src/core/markdown.rs)
uses source spans and formatting plans. Display math uses an opaque emit plan.
Panache constructs a lossless Rowan CST shared with the linter and language
server, resolves inline syntax, and parses and formats TeX math. Their default
wrapping widths and formatting policies also differ. The comparison measures
their default workloads, not identical transformations.

Yamark also enables thin LTO and one code-generation unit in its [release
profile](https://github.com/t-kalinowski/yamark/blob/v0.3.0/Cargo.toml). Panache
previously used Cargo's default release profile.

## Profile and changes

On the 304,665-byte Pandoc manual, baseline parsing and formatting each took
roughly half the library's total time. `perf` attributed about 10% of the
combined benchmark's CPU samples to Rowan's `PreorderWithTokens::next`. Caller
stacks showed repeated text reconstruction in the host formatter, paragraph
checks, and definition-list checks.

The changes remove allocations and tree walks on paths that do not need text:

- Reconstruct the complete document only when external formatters are
  configured.
- Reject ordinary prose before copying it to check for grid-table continuations
  or three-line colon fences. Whitespace-only first tokens still take the full
  check, preserving behavior across token boundaries.
- Check the preceding block before copying a definition list to recognize a
  grid-table caption.
- Enable thin LTO and one code-generation unit for release builds. This adds
  build time but requires no change to formatting behavior.

## Library measurements

Each build ran the existing `formatting` benchmark 12 times, with 40 iterations
per phase and the harness's 10 warmup iterations. Build order alternated between
rounds. The table gives medians after discarding the first three rounds.
Processes were pinned to CPU 9; earlier CPU 0 runs suffered contention and were
discarded. No Panache builds or test suites ran alongside these measurements.

  | Build                 | Parse + format (ms) | Parse (ms) | Format existing CST (ms) |
  | --------------------- | ------------------: | ---------: | -----------------------: |
  | Baseline              |               21.85 |      10.61 |                    11.03 |
  | Deferred text copying |               19.63 |      10.67 |                     8.84 |
  | Release profile only  |               20.75 |      10.42 |                    10.27 |
  | Both changes          |               18.57 |      10.41 |                     7.99 |

Together, the changes reduce full-pipeline time by 15% and formatting time by
28%. The full-pipeline sample standard deviation was 2.2% of the mean for the
baseline and 0.8% for the combined changes. Neither experiment was reverted.

For the manual through the CLI, medians over 30 runs after three warmups were
25.81 ms before, 22.14 ms after, and 9.43 ms for Yamark. Thus, the CLI improved
14%, reducing its time relative to Yamark from 2.74× to 2.35×. Hyperfine ran
without a shell, read stdin from the same document, and pinned each process to
CPU 9. Panache's before/after output was byte-identical on all six documents in
the single-document comparison suite.

## Repository measurements

These are medians over 12 runs after three warmup rounds. Tool order rotated
between rounds, and original tracked documents were restored before every run.
Files were flattened into a temporary corpus, as in `compare_repo_suite.sh`.
Panache ran with caching disabled, with GFM for Markdown and Quarto for QMD.
Yamark used an empty configuration and disabled external formatters. Repository
runs used the available cores and included file discovery, reads, and writes.

  | Corpus                | Before (ms) | After (ms) | Yamark (ms) | Reduction |
  | --------------------- | ----------: | ---------: | ----------: | --------: |
  | Rust Reference        |       17.82 |      16.60 |       11.80 |        7% |
  | mlr3book              |       27.79 |      24.55 |        8.40 |       12% |
  | Pandoc Markdown files |      141.98 |     128.65 |       37.17 |        9% |

The corpus revisions were `4245282b9749` (Rust Reference, 154 files),
`9e93397f1f14` (mlr3book, 31 files), and `896774b0aa98` (Pandoc, 1,161 files).
The Reference timings were noisier: sample standard deviations were 15% of the
baseline mean and 9% after the change, so its smaller improvement is indicative.
Pandoc's corresponding variation was about 2% for both builds.

## Validation and next targets

The workspace check and test suite, CommonMark allowlist, all-target/all-feature
Clippy, Rust formatting check, and all 512 formatter golden cases passed. No
expected output or parser structure changed. The published comparison JSON files
were not regenerated by this investigation.

The remaining gap is substantial. Parsing now takes about 10.4 of the manual's
18.6 ms library time. Next targets, in order:

1. Parser list/container processing and CST allocation. The parser profile
   showed `parse_inner_content`, `parse_line`, container-prefix stripping, and
   list-marker recognition among the largest costs.
2. Remaining formatter text reconstruction. Avoid repeated subtree copies when a
   source slice or a bounded prefix inspection would suffice.
3. Per-word allocation in wrapping.
   `StreamingCoreSink::emit_piece_with_boundary_text` accounted for about 4% of
   the baseline combined profile.

## Reproduction

Build and save each executable before switching configurations or revisions. Use
the same input bytes and alternate build order when collecting samples. The
manual's SHA-256 was
`d638b172d2d7aca2661425f12f1d21409d3ebb0a008c681827ff64fd62faa503`.

```bash
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --bin panache --bench formatting
CARGO_PROFILE_RELEASE_DEBUG=true cargo bench --bench formatting --no-run

# Repeat the built benchmark directly to exclude compilation from timings.
# Use its executable path reported by cargo, with CPU 9 or another quiet core.
taskset -c 9 env PANACHE_BENCH_DOC=pandoc_manual.md \
  PANACHE_BENCH_ITERATIONS=40 target/release/deps/formatting-<hash>

hyperfine -N --warmup 3 --runs 30 --input benches/documents/pandoc_manual.md \
  'taskset -c 9 target/release/panache format --isolated --stdin-filename benches/documents/pandoc_manual.md' \
  'taskset -c 9 yamark format --config /dev/null --skip-embedded-formatters --stdin-file-path benches/documents/pandoc_manual.md'
```
