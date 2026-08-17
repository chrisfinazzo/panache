#!/usr/bin/env bash

set -e

DOCS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DOCS_DIR"

# Pinned upstream revisions.
#
# These documents are benchmark *fixtures*, and several thresholds are
# calibrated against their exact size: `benches/lsp_incremental.rs` shows that
# speedup is a function of window share and nothing else, so a document that
# grows upstream silently moves every floor that reads it. Tracking a branch
# once cost the two `pandoc_manual` floors 5.0x -> 4.5x for no code change at
# all. Pinning also makes re-running the gate byte-stable, which matters
# because `pandoc_testsuite.md` is tracked in git and was rewritten on every
# run.
#
# Bump deliberately, in its own commit, and re-run
# `task bench:incremental-gate` to see what moved.
PANDOC_REV="cd77c632a8ee0dfe34ba9b16a92e940f47cb970c"
QUARTO_WEB_REV="ccf7a9eaaa757b439b77546fd95cfdaf9462eeed"

echo "Downloading benchmark documents..."
echo "  jgm/pandoc            @ ${PANDOC_REV}"
echo "  quarto-dev/quarto-web @ ${QUARTO_WEB_REV}"
echo

# Local realistic doc + upstream fixture
echo "📄 Copying configuration.qmd..."
cp ../../docs/guide/configuration.qmd configuration.qmd

echo "📄 Downloading pandoc_testsuite.md..."
curl -sL --fail -o pandoc_testsuite.md \
  "https://raw.githubusercontent.com/jgm/pandoc/${PANDOC_REV}/test/testsuite.txt"

# Large: Markdown basics (comprehensive)
echo "📄 Downloading large_authoring.qmd..."
curl -sL --fail -o large_authoring.qmd \
  "https://raw.githubusercontent.com/quarto-dev/quarto-web/${QUARTO_WEB_REV}/docs/authoring/markdown-basics.qmd"

# Table-heavy
echo "📄 Downloading tables.qmd..."
curl -sL --fail -o tables.qmd \
  "https://raw.githubusercontent.com/quarto-dev/quarto-web/${QUARTO_WEB_REV}/docs/authoring/tables.qmd"

# Math-heavy (using computational documents as they have more math)
echo "📄 Downloading math.qmd..."
curl -sL --fail -o math.qmd \
  "https://raw.githubusercontent.com/quarto-dev/quarto-web/${QUARTO_WEB_REV}/docs/computations/julia.qmd"

echo "📄 Downloading pandoc_manual.md..."
curl -sL --fail -o pandoc_manual.md \
  "https://raw.githubusercontent.com/jgm/pandoc/${PANDOC_REV}/MANUAL.txt"

echo
echo "✅ Benchmark documents downloaded successfully!"
echo
echo "File sizes:"
du -h ./*.qmd ./*.md 2>/dev/null || true
echo
echo "Run benchmarks with: cargo bench --bench formatting"
