#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
CORPUS_REPO=https://github.com/rust-lang/book.git
CORPUS_REVISION=917544888a55e4da7109bdba8c88c893c0da70f4
CORPUS_DIR=${PANACHE_LSP_MEMORY_CORPUS:-"$ROOT/benches/lsp-memory-corpus/rust-book"}
OUTPUT=${PANACHE_LSP_MEMORY_OUT:-"$ROOT/docs/guide/performance_lsp_memory_data.json"}
STDERR_DIR=${PANACHE_LSP_MEMORY_STDERR_DIR:-"$ROOT/benches/lsp-memory-logs"}
RUNS=${PANACHE_LSP_MEMORY_RUNS:-3}
OPEN_FILE_COUNT=${PANACHE_LSP_MEMORY_OPEN_FILES:-5}
EDIT_COUNT=${PANACHE_LSP_MEMORY_EDITS:-1000}
QUIET_SECONDS=${PANACHE_LSP_MEMORY_QUIET_SECONDS:-5}
SETTLE_TIMEOUT=${PANACHE_LSP_MEMORY_SETTLE_TIMEOUT:-120}

for setting in "$RUNS" "$OPEN_FILE_COUNT" "$EDIT_COUNT"; do
  if [[ ! $setting =~ ^[1-9][0-9]*$ ]]; then
    echo "error: run, file, and edit counts must be positive integers" >&2
    exit 1
  fi
done

for tool in git python3; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: $tool is required" >&2
    exit 1
  fi
done

if [[ ! -d "$CORPUS_DIR/.git" ]]; then
  if [[ -e "$CORPUS_DIR" ]]; then
    echo "error: corpus path exists but is not a Git checkout: $CORPUS_DIR" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$CORPUS_DIR")"
  git clone --filter=blob:none "$CORPUS_REPO" "$CORPUS_DIR"
fi

if [[ $(git -C "$CORPUS_DIR" remote get-url origin) != "$CORPUS_REPO" ]]; then
  echo "error: corpus checkout has an unexpected origin: $CORPUS_DIR" >&2
  exit 1
fi

git -C "$CORPUS_DIR" fetch --quiet --depth 1 origin "$CORPUS_REVISION"
git -C "$CORPUS_DIR" checkout --quiet --detach "$CORPUS_REVISION"

if [[ $(git -C "$CORPUS_DIR" rev-parse HEAD) != "$CORPUS_REVISION" ]]; then
  echo "error: failed to check out pinned Rust Book revision" >&2
  exit 1
fi

if [[ -n $(git -C "$CORPUS_DIR" status --porcelain --untracked-files=all) ]]; then
  echo "error: corpus checkout is dirty: $CORPUS_DIR" >&2
  exit 1
fi

if [[ -z ${PANACHE_BIN:-} ]]; then
  cargo build --manifest-path "$ROOT/Cargo.toml" --release --quiet --bin panache
  PANACHE_BIN="$ROOT/target/release/panache"
fi

if [[ ! -x "$PANACHE_BIN" ]]; then
  echo "error: Panache executable not found: $PANACHE_BIN" >&2
  exit 1
fi

if [[ -z ${MARKSMAN_BIN:-} ]]; then
  MARKSMAN_BIN=$(command -v marksman || true)
fi

if [[ -z "$MARKSMAN_BIN" || ! -x "$MARKSMAN_BIN" ]]; then
  echo "error: Marksman is required; enter the devenv shell or set MARKSMAN_BIN" >&2
  exit 1
fi

mapfile -t RELATIVE_FILES < <(
  while IFS= read -r path; do
    printf '%012d\t%s\n' "$(stat -c %s "$CORPUS_DIR/$path")" "$path"
  done < <(git -C "$CORPUS_DIR" ls-files -- ':(glob)src/**/*.md') |
    sort -k1,1nr -k2,2 |
    sed -n "1,${OPEN_FILE_COUNT}p" |
    cut -f2-
)

if [[ ${#RELATIVE_FILES[@]} -ne $OPEN_FILE_COUNT ]]; then
  echo "error: expected $OPEN_FILE_COUNT tracked Markdown files under src/" >&2
  exit 1
fi

FILES=()
for path in "${RELATIVE_FILES[@]}"; do
  FILES+=("$CORPUS_DIR/$path")
done

TEMP_DIR=$(mktemp -d -t panache-lsp-memory.XXXXXXXX)
cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

printf 'flavor = "gfm"\n' >"$TEMP_DIR/panache.toml"
mkdir -p "$STDERR_DIR"

PANACHE_VERSION=$("$PANACHE_BIN" --version | head -n 1)
MARKSMAN_VERSION=$("$MARKSMAN_BIN" --version | head -n 1)

python3 "$ROOT/benches/lsp_memory.py" \
  --project "$CORPUS_DIR" \
  --files "${FILES[@]}" \
  --out "$OUTPUT" \
  --server "panache=$PANACHE_BIN --config $TEMP_DIR/panache.toml lsp" \
  --server "marksman=$MARKSMAN_BIN server" \
  --server-version "panache=$PANACHE_VERSION" \
  --server-version "marksman=$MARKSMAN_VERSION" \
  --runs "$RUNS" \
  --edits "$EDIT_COUNT" \
  --quiet-seconds "$QUIET_SECONDS" \
  --settle-timeout "$SETTLE_TIMEOUT" \
  --stderr-dir "$STDERR_DIR" \
  --corpus-name "The Rust Programming Language" \
  --corpus-repo "$CORPUS_REPO" \
  --corpus-revision "$CORPUS_REVISION"
