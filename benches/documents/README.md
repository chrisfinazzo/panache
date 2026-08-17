# Benchmark Documents

This directory contains documents used for benchmarking panache performance.

## Document Sources

The benchmark corpus mixes realistic project documents, upstream test fixtures,
and a few targeted stress cases. The setup script refreshes the copied or
downloaded files into this directory.

### Standard Benchmark Suite

The benchmark suite includes:

1. **Pandoc testsuite fixture** (\~9KB) - downloaded from upstream pandoc
   `test/testsuite.txt` as `pandoc_testsuite.md`
2. **Configuration guide** (\~24KB) - copied from `docs/guide/configuration.qmd`
3. **Table-heavy** (\~19KB) - Quarto tables documentation
4. **Math-heavy** (\~29KB) - Quarto computational document with extensive math
5. **Large authoring guide** (\~30KB) - Quarto markdown authoring guide
6. **Pandoc MANUAL stress doc** (\~8000 lines) - downloaded from upstream pandoc
   `MANUAL.txt` as `pandoc_manual.md`

## Setup

Download the benchmark documents:

```bash
./download.sh
```

## Pinned revisions

`download.sh` fetches every upstream document at a **pinned commit**, recorded in
`PANDOC_REV` and `QUARTO_WEB_REV` at the top of the script. This is not
housekeeping; several thresholds read these documents' exact size.

`benches/lsp_incremental.rs` establishes that incremental speedup is a function
of window share and nothing else, so a document that grows upstream moves every
floor calibrated against it. That is not hypothetical: tracking `main` once
grew `pandoc_manual.md` from 300 856 to 304 665 bytes, which slid a fixed edit
from a 7.0% window to a 7.5% one and cost the two `pandoc_manual` floors
5.0x -> 4.5x for no code change at all.

Pinning also makes a gate run byte-stable. `pandoc_testsuite.md` is tracked in
git, and re-downloading it from a moving branch dirtied the working tree on
every run.

To bump a revision, change the variable, re-run `./download.sh`, and re-run
`task bench:incremental-gate` in the same commit so the threshold movement is
recorded next to its cause.

## Regenerating Benchmarks

To run benchmarks with the downloaded documents:

```bash
cargo bench --bench formatting
```

## Directory Structure

```
benches/documents/
├── README.md           # This file
├── download.sh         # Download script
├── configuration.qmd   # Copied from docs/guide/configuration.qmd
├── pandoc_testsuite.md # Downloaded from upstream pandoc testsuite
├── large_authoring.qmd # Downloaded - not in git
├── tables.qmd          # Downloaded - not in git
├── math.qmd            # Downloaded - not in git
└── pandoc_manual.md    # Downloaded from upstream pandoc MANUAL.txt - not in git
```
