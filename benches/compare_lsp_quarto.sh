#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
Q2_VERSION=0.32.0
CORPUS_REVISION=192a231d8da241f60368821b73c56edac5c5f0e7
OUTPUT=${PANACHE_LSP_QUARTO_OUT:-"$ROOT/docs/guide/performance_lsp_quarto_data.json"}
STDERR_DIR=${PANACHE_LSP_QUARTO_STDERR_DIR:-"$ROOT/benches/lsp-quarto-logs"}
TOOLS_DIR=${PANACHE_LSP_QUARTO_TOOLS:-"$ROOT/benches/lsp-quarto-tools"}

if [[ $(uname -s) != Linux ]]; then
  echo "error: the LSP memory benchmark requires Linux" >&2
  exit 1
fi

TEMP_DIR=$(mktemp -d -t panache-lsp-quarto.XXXXXXXX)
trap 'rm -rf "$TEMP_DIR"' EXIT
PROJECT="$TEMP_DIR/q2-authoring"
mkdir -p "$PROJECT" "$TEMP_DIR/bin" "$STDERR_DIR"

if [[ -z ${Q2_BIN:-} ]]; then
  case $(uname -m) in
  x86_64)
    platform=linux_amd64
    checksum=178fd345b3d31c2ae608eee8e3fe0b3e5dd12144f590e952c6441485f0e6a353
    ;;
  aarch64)
    platform=linux_arm64
    checksum=9284627b2fbe0570a555d6fa4a3cd218cb92826e176684354d32da79371bdd0c
    ;;
  *)
    echo "error: set Q2_BIN to a q2 build for this architecture" >&2
    exit 1
    ;;
  esac
  mkdir -p "$TOOLS_DIR"
  archive="$TOOLS_DIR/q2-$Q2_VERSION-$platform.tar.gz"
  if [[ ! -f "$archive" ]]; then
    curl -fLsS --retry 2 \
      "https://github.com/quarto-dev/q2/releases/download/v$Q2_VERSION/q2-$Q2_VERSION-$platform.tar.gz" \
      -o "$TEMP_DIR/q2.tar.gz"
    printf '%s  %s\n' "$checksum" "$TEMP_DIR/q2.tar.gz" | sha256sum --check --status
    cp "$TEMP_DIR/q2.tar.gz" "$archive"
  fi
  printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
  tar -xzf "$archive" -C "$TEMP_DIR/bin" q2
  Q2_BIN="$TEMP_DIR/bin/q2"
fi

if [[ ! -x "$Q2_BIN" ]]; then
  echo "error: q2 executable not found: $Q2_BIN" >&2
  exit 1
fi
Q2_BIN=$(realpath "$Q2_BIN")

# A standalone copy avoids measuring discovery of q2's unrelated workspace.
FILES=()
while read -r checksum name; do
  curl -fLsS --retry 2 \
    "https://raw.githubusercontent.com/quarto-dev/q2/$CORPUS_REVISION/docs/guides/authoring/$name.qmd" \
    -o "$PROJECT/$name.qmd"
  printf '%s  %s\n' "$checksum" "$PROJECT/$name.qmd" | sha256sum --check --status
  FILES+=("$PROJECT/$name.qmd")
done <<'CORPUS'
b405c8c82448b9c7742cd1fd5f2c094fe206ab14f8086f98512219d4bcbea4de computations
979d2c7fb9bbd41bd76fc9dd1c1a6a7796982a1d773f6fc14680557358255371 markdown-basics
32b1857baa5c8b2743eb52eabdc7f17d4156241b9b15a69e04b9e7a1f91988d7 title-blocks
CORPUS

if [[ -z ${PANACHE_BIN:-} ]]; then
  cargo build --manifest-path "$ROOT/Cargo.toml" --release --quiet --bin panache
  PANACHE_BIN="$ROOT/target/release/panache"
fi

if [[ ! -x "$PANACHE_BIN" ]]; then
  echo "error: Panache executable not found: $PANACHE_BIN" >&2
  exit 1
fi
PANACHE_BIN=$(realpath "$PANACHE_BIN")

printf 'flavor = "quarto"\n' >"$TEMP_DIR/panache.toml"
PANACHE_VERSION=$("$PANACHE_BIN" --version)
Q2_ACTUAL_VERSION=$("$Q2_BIN" --version)

# The Python harness parses command strings with shlex, including quoted paths.
PANACHE_COMMAND=$(python3 -c 'import shlex, sys; print(shlex.join(sys.argv[1:]))' \
  "$PANACHE_BIN" --config "$TEMP_DIR/panache.toml" lsp)
Q2_COMMAND=$(python3 -c 'import shlex, sys; print(shlex.join(sys.argv[1:]))' "$Q2_BIN" lsp)

python3 "$ROOT/benches/lsp_memory.py" \
  --track quarto \
  --project "$PROJECT" \
  --files "${FILES[@]}" \
  --out "$OUTPUT" \
  --server "panache=$PANACHE_COMMAND" \
  --server "q2=$Q2_COMMAND" \
  --server-version "panache=$PANACHE_VERSION" \
  --server-version "q2=$Q2_ACTUAL_VERSION" \
  --runs "${PANACHE_LSP_QUARTO_RUNS:-3}" \
  --edits "${PANACHE_LSP_QUARTO_EDITS:-100}" \
  --latency-runs "${PANACHE_LSP_LATENCY_RUNS:-20}" \
  --latency-warmups "${PANACHE_LSP_LATENCY_WARMUPS:-2}" \
  --quiet-seconds "${PANACHE_LSP_QUARTO_QUIET_SECONDS:-5}" \
  --settle-timeout "${PANACHE_LSP_QUARTO_SETTLE_TIMEOUT:-120}" \
  --stderr-dir "$STDERR_DIR" \
  --corpus-name "q2 authoring guides" \
  --corpus-repo https://github.com/quarto-dev/q2 \
  --corpus-revision "$CORPUS_REVISION"
