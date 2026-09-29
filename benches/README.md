# Benchmarking and Profiling Guide

## Quick Start

### Running Benchmarks

```bash
# Download test documents (first time only)
cd benches/documents && ./download.sh && cd ../..

# Run benchmarks
cargo bench --bench formatting

# Isolate the stable math formatter on a selected corpus document (`reflow` is
# the default; set `PANACHE_BENCH_FORMAT_MATH=0` for a verbatim comparison)
PANACHE_BENCH_DOC=math.qmd \
    cargo bench --bench formatting

# Run LSP incremental didChange benchmarks
cargo bench --bench lsp_incremental

# Same run, with every case checked against the contract it declares
task bench:incremental-gate

# Run LSP write-phase benchmarks (what a keystroke costs before any parse)
cargo bench --bench lsp_write_phase

# Same run, gated; `task bench:lsp-gate` runs both LSP gates
task bench:write-phase-gate

# Run the LSP settle benchmark (what publishing one document costs per settle)
cargo bench --bench lsp_settle

# Compare Panache and Marksman language-server speed and memory on Linux
task bench:lsp

# Compare Panache and q2 on Quarto diagnostics and document symbols
task bench:lsp-quarto

# Run interned key impact benchmark
cargo bench --bench interned_keys

# Run CLI cache cold-vs-warm benchmark
cargo bench --bench cli_cache

# Generate docs + machine-readable JSON
./benches/generate_docs.sh
```

### Profiling

For line-level profiling with flame graphs:

```bash
# Install flamegraph
cargo install --locked flamegraph

# Profile the benchmark
cargo flamegraph --bench formatting

# Opens flamegraph.svg showing hotspots
```

For large-document bottlenecks (e.g. Pandoc MANUAL):

```bash
# Ensure pandoc_manual.md exists first
cd benches/documents && ./download.sh && cd ../..

# Profile only the selected document
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
    cargo flamegraph --bench formatting

# LSP incremental benchmark knobs
# PANACHE_LSP_BENCH_ITERATIONS=80 (default)
# PANACHE_LSP_BENCH_OUTPUT_JSON=benches/lsp_incremental_results.json
# PANACHE_LSP_BENCH_ASSERT=1 (check thresholds; exit 1 on a violation)

# LSP write-phase benchmark knobs
# PANACHE_LSP_WRITE_BENCH_ITERATIONS=1.0 (scale factor on every row)
# PANACHE_LSP_WRITE_BENCH_OUTPUT_JSON=benches/lsp_write_phase_results.json
# PANACHE_LSP_WRITE_BENCH_ASSERT=1 (check thresholds; exit 1 on a violation)

# LSP settle benchmark knobs
# PANACHE_LSP_SETTLE_BENCH_ITERS=200 (default)
```

For more detailed profiling:

```bash
# Linux perf (CPU profiling)
perf record --call-graph dwarf cargo bench --bench formatting
perf report

# Line-level annotation for selected stress document
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
    perf record --call-graph dwarf cargo bench --bench formatting
perf annotate

# Valgrind (memory profiling)
valgrind --tool=cachegrind cargo bench --bench formatting
```

## Benchmark Infrastructure

### Document Management

- **`benches/documents/`**: Test documents for benchmarking
  - `small.qmd`: Committed baseline (747 bytes)

  - `pandoc_manual.md`: Stress-test doc downloaded from upstream pandoc
    `MANUAL.txt`

  - Other files: Downloaded on-demand from Quarto docs

  - `.gitignore`: Excludes downloaded files from repo
- **`benches/documents/download.sh`**: Downloads real Quarto documents
  - Reproducible: same sources every time
  - Lightweight: doesn't bloat repo

### Benchmark Code

- **`benches/formatting.rs`**: Main benchmark suite
  - Tests parse, format, and full pipeline

  - Multiple document sizes and types

  - Reports throughput in KB/s
- **`benches/interned_keys.rs`**: Key interning measurement harness
  - Compares owned vs interned key map build costs
  - Reports repeated-byte potential from duplicated keys
- **`benches/cli_cache.rs`**: CLI cache warm-hit benchmark harness
  - Compares uncached (cold) vs cached (warm) runs for `format --check` and
    `lint`
  - Tunable with `PANACHE_CLI_CACHE_BENCH_FILES` and
    `PANACHE_CLI_CACHE_BENCH_ITERATIONS`
  - Optional JSON output via `PANACHE_CLI_CACHE_BENCH_OUTPUT_JSON`
- **`benches/compare_all.sh`**: Multi-formatter comparison
  - Compares panache, Prettier, Pandoc, rumdl, mdformat, and Yamark across six
    documents (Pandoc testsuite, tables, configuration, math, large, and the
    full Pandoc manual)
  - **Text mode (default)**: prints colored results and appends to
    `benchmark_results.txt`
  - **JSON mode**: `bash benches/compare_all.sh --json [--out PATH]` writes
    structured JSON consumed by `docs/guide/performance.qmd`. Default output
    path is `docs/guide/performance_data.json`.
  - Prefers [hyperfine](https://github.com/sharkdp/hyperfine) for stats when
    available (and `jq` for parsing); otherwise falls back to a simple shell
    timing loop emitting mean only (`stddev_ms`/`min_ms`/`max_ms` are `null`).
  - Drives `docs/guide/performance.qmd`. Refresh the JSON explicitly, then
    delete `docs/_freeze/guide/performance/` and re-render to display it.
- **`benches/compare_multifile.sh`**: Local Markdown corpus comparison
  - Compares panache, Prettier, rumdl, and Yamark in a single process per tool.
  - Restores the input files before every sample because tools format in place.
- **`benches/compare_repo_suite.sh`**: Repository formatting and linting
  comparisons
  - The formatting suite includes Yamark on both the Markdown and Quarto tracks.
  - Restores tracked documents before every sample. Failed runs are recorded
    with null timings and excluded from the performance plots.
- **`benches/compare_lsp_memory.sh`**: Linux language-server speed and memory
  comparison
  - Checks out a pinned revision of the Rust Book into a gitignored directory.
  - Opens the five largest tracked Markdown files under `src/`, exercises
    diagnostics and navigation, and performs 1,000 reference-label edits.
  - Runs three fresh processes per server in alternating order and records the
    median whole-process-tree RSS and PSS at baseline, settled, edited, and peak
    milestones.
  - Records process startup, estimated workspace and open-file readiness, and
    edit runtime separately from the deliberate memory-settling waits.
  - Measures document symbols on three files and hover, definition, references,
    and rename on the appended reference link. Each target gets two warmups and
    20 measured rounds per process. Rename edits are not applied. Each edit in
    the churn workload is also timed through its following definition response.
  - Pools request samples across processes for the median and nearest-rank p95.
    Keeps per-run summaries and returned item, file, and payload-size counts.
    Empty results are counted; errors and timeouts abort the run.
  - Launches Panache with an isolated GFM config and gives both servers isolated
    user config and cache directories.
  - Writes per-run measurements and aggregate comparisons to
    `docs/guide/performance_lsp_memory_data.json` by default. Override the run
    with the `PANACHE_LSP_MEMORY_RUNS`, `PANACHE_LSP_MEMORY_OPEN_FILES`,
    `PANACHE_LSP_MEMORY_EDITS`, `PANACHE_LSP_MEMORY_QUIET_SECONDS`, and
    `PANACHE_LSP_MEMORY_SETTLE_TIMEOUT` environment variables.
    `PANACHE_LSP_LATENCY_RUNS` and `PANACHE_LSP_LATENCY_WARMUPS` control request
    repetitions. `task bench:lsp-memory` remains an alias for `task bench:lsp`.
- **`benches/generate_docs.sh`**: Captures results for documentation
  - Generates `benches/benchmark_results.json` (machine-readable)
  - Renders `docs/benchmarks.qmd` from JSON
  - Deterministic output for CI checks

### Quarto Language Server Comparison

`task bench:lsp-quarto` runs the Quarto track in `benches/lsp_memory.py` against
Panache and `q2 lsp`. It builds Panache in release mode and downloads the pinned
q2 0.32.0 release for Linux x86-64 or ARM64, verifying the archive against a
committed SHA-256 hash. The archive is cached under `benches/lsp-quarto-tools/`.
Set `PANACHE_BIN` or `Q2_BIN` to use an existing executable instead.

The corpus consists of q2's computations, Markdown basics, and title-block
guides at revision `192a231d8da241f60368821b73c56edac5c5f0e7`. The runner checks
their hashes and copies them into a temporary standalone project. It launches
Panache with an isolated Quarto configuration and both servers with isolated
user configuration and cache directories. It never renders documents or executes
their code cells.

Each of three alternating process runs measures:

- Process launch through the `initialize` response.
- Each file's open notification through its diagnostic result, opening files
  serially without warmups.
- Document symbols for all three files, with two warmup rounds and 20 measured
  rounds per file.
- 100 serial edits to the computations guide, each through its diagnostic
  result. Both servers receive a full-document replacement that changes an
  appended heading. The document does not grow across successive edits.
- Process-tree RSS and PSS at the same milestones as the Markdown track.

Diagnostics use each server's supported delivery mode: Panache receives an
immediate pull request, while q2 pushes a notification. Timing includes the
notification and, for pull, the diagnostic request. This measures delivery
latency under those client policies; it does not compare equivalent rule sets or
Panache's debounced push mode. Only one edit is in flight because q2 omits
document versions from diagnostic notifications. Empty diagnostic sets are
valid. Errors on opening a corpus document, empty symbol results, protocol
errors, and timeouts abort the run. A final symbol request must observe the last
edited heading.

The runner writes `docs/guide/performance_lsp_quarto_data.json`. Override its
defaults with `PANACHE_LSP_QUARTO_OUT`, `PANACHE_LSP_QUARTO_RUNS`,
`PANACHE_LSP_QUARTO_EDITS`, `PANACHE_LSP_QUARTO_QUIET_SECONDS`,
`PANACHE_LSP_QUARTO_SETTLE_TIMEOUT`, `PANACHE_LSP_QUARTO_STDERR_DIR`, and
`PANACHE_LSP_QUARTO_TOOLS`. The shared `PANACHE_LSP_LATENCY_RUNS` and
`PANACHE_LSP_LATENCY_WARMUPS` variables control symbol requests. Each run
records the diagnostic mode, result counts, versions, and latency summaries;
aggregation pools the request samples before computing the median and
nearest-rank p95.

Run the harness regression tests with:

```bash
python3 -m unittest discover -s benches -p 'test_lsp*.py'
```

### Quarto First-Open Latency

`benches/lsp_first_open.py` isolates first-analysis cost from document cost. Use
the three hash-verified authoring guides pinned in `compare_lsp_quarto.sh`, copied
into a standalone directory:

```bash
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --bin panache
python3 benches/lsp_first_open.py \
  --server "taskset -c 0 $PWD/target/release/panache" \
  --project /tmp/q2-authoring \
  --files /tmp/q2-authoring/{computations,markdown-basics,title-blocks}.qmd \
  --out /tmp/panache-first-open.json
```

The harness discards three warmup processes, then launches 48 fresh processes.
It cycles through all six document orders, giving each document 16 samples at
each opening position. Each process uses isolated user configuration and cache
directories and an explicit Quarto configuration, as in the q2 comparison.
Opens begin immediately after initialization and use serial pull diagnostics.
The JSON retains individual timings, document positions, diagnostics, corpus
hashes, initialization time, and process-launch-to-first-diagnostics time. Stderr
logs live beside it in a `.logs` directory. Errors and incomplete reports fail
the run. This measures fresh processes with warm filesystem caches, not cold
disk startup or debounced push diagnostics.

On September 29, 2026, an Intel Core Ultra 7 155U pinned to CPU 0 showed a shared
first-analysis penalty for all three documents. The earlier q2 comparison's
33 ms maximum did not recur in this setup. A `cpu_core/cycles/` profile instead
identified the lazy Quarto schema loader: `SchemaNode::deserialize` accounted
for about 6% of worker self time, Serde's temporary `ContentVisitor` for another
6%, and allocation functions for about 16%. The tagged-enum decoder buffered
nested schema subtrees before constructing their typed nodes. Decoding fields
directly removes those temporary trees while preserving the vendored schema.

The before/after comparison used four blocks of 12 fresh processes per binary,
discarding three warmups per block and alternating which binary ran first.
Both binaries used the same release build settings. Median open-to-diagnostics
times in milliseconds were:

| Document | First, before | First, after | Second, before | Second, after | Third, before | Third, after |
|---|---:|---:|---:|---:|---:|---:|
| computations | 10.51 | 8.59 | 1.44 | 1.45 | 1.29 | 1.24 |
| markdown-basics | 10.76 | 8.89 | 1.45 | 1.41 | 1.23 | 1.26 |
| title-blocks | 10.38 | 8.74 | 1.32 | 1.27 | 1.12 | 1.14 |

Across the 48 first opens per binary, the median fell from 10.54 to 8.80 ms
(16.5%), and p95 fell from 11.43 to 9.16 ms. Each twelve-process block improved
by 15–20%. Process launch through first diagnostics fell from 13.67 to 11.70 ms;
initialization stayed at about 2.7 ms. Later opens stayed within measurement
noise, at 1.31 versus 1.28 ms. All 144 diagnostic results matched exactly.

Workspace tests, the CommonMark allowlist, check, Clippy, rustfmt, and the Python
LSP harness tests passed. No optimization was reverted. The next candidate is
the same loader's JSON scanning and decompression: after this change, map-key
scanning and `decompress_fast` account for about 8% and 7% of worker self time.
Minifying the embedded JSON at build time may reduce both; that remains
unmeasured. Configuration discovery outside this benchmark's explicit-config
setup and loading a larger project need separate workloads.

### Yamark

The formatting comparison scripts include
[Yamark](https://github.com/t-kalinowski/yamark) when `yamark` is on `PATH`. The
development environment provides a pinned build and sets its benchmark version
automatically. Outside `devenv`, install it before benchmarking:

```bash
uv tool install yamark==0.3.0
export PANACHE_BENCH_YAMARK_VERSION=0.3.0

bash benches/compare_all.sh --json
bash benches/compare_multifile.sh
bash benches/compare_repo_suite.sh --mode format --track markdown --out docs/guide/performance_repo_markdown_format_data.json
bash benches/compare_repo_suite.sh --mode format --track quarto --out docs/guide/performance_repo_quarto_format_data.json
```

Yamark 0.3.0 has no version command, so the scripts record
`PANACHE_BENCH_YAMARK_VERSION`, or `unknown` when it is unset.

Timed commands invoke `yamark` directly. They use `--config /dev/null` to avoid
ambient configuration and `--skip-embedded-formatters` to exclude external code
formatters. Single-document runs use stdin with `--stdin-file-path`; batch runs
format a fresh copy of the corpus in place. Yamark's default wrapping and style
settings apply. These comparisons measure each tool's formatting policy and
supported syntax, which differ across tools.

## What to Benchmark

Good targets for benchmarking: - **Full pipeline** (parse + format) - what users
experience - **Parse speed** - CST construction - **Format speed** - CST
traversal and output - **Document types** - simple text vs complex (tables,
math, divs) - **Document sizes** - small (1KB), medium (10-50KB), large (100KB+)

## Performance Tips

Current performance baseline: - \~20MB/s throughput on typical documents - \~1ms
to format a 30KB document - Parse takes \~30-40% of time, format \~60-70%

To improve performance, profile with flamegraph to find hotspots.

## Adding New Benchmarks

1. Add document to `benches/documents/` (or update `download.sh`)
2. Load in `benches/formatting.rs` with `load_document()`
3. Call `run_benchmark()` with appropriate iteration count
4. Run and verify results

## Integrating with Docs

After running benchmarks:

```bash
# Generate fresh benchmark page
./benches/generate_docs.sh

# Verify tracked artifacts are up to date (CI-friendly)
./benches/check_docs.sh

# Preview in Quarto
cd docs && quarto preview

# Commit to repo
git add benches/benchmark_results.json docs/benchmarks.qmd
git commit -m "docs: update benchmark results"
```
