---
name: add-syntax-construct
description: Add a new block-level or inline-level syntax construct to Panache's
  parser and formatter — confirm the pandoc-native shape first, add SyntaxKinds
  for every byte category, gate it behind an extension or flavor, slot the block
  parser at the right precedence in the registry, teach the formatter, and pin
  it with fixtures in both golden suites. Use when asked to support a new
  Markdown/Quarto/Pandoc construct, or when a construct parses but formats
  wrongly because it was only half-wired.
---

Use this skill when adding syntax Panache does not yet understand, or when
finishing a construct that was wired into the parser but not the formatter.

This is the most error-prone change in the codebase. Skipping a step here does
not fail loudly at the point of the mistake — it surfaces later as a
losslessness failure, a silently unformatted node, or a global precedence
regression in a conformance suite.

## Scope boundaries

- **Not for lint rules.** Use the `add-lint-rule` skill.
- **Not for conformance triage.** If the goal is "make example N pass", use
  `commonmark-conformance` or `html-conformance` instead — those start from a
  failing case, not from a construct.
- **Not for inline IR migration.** Moving an *existing* Pandoc inline onto the
  unified IR is `pandoc-ir-migrate`.

## Step 1 — Confirm the pandoc-native shape first

Pandoc's AST decides what the CST must be able to express. Do not design the
CST from the surface syntax alone.

```bash
printf '<your construct>' | pandoc -f markdown -t native
printf '<your construct>' | cargo run -- parse --to pandoc-ast
```

The second command prints the same shape as the first, so divergences diff
directly. Note what pandoc *nests* and what it *flattens* — that is the
structure the CST has to support, even if the surface syntax looks flat.

If the construct is Quarto- or MyST-specific and pandoc does not know it, find
the closest pandoc construct it degrades to, and check `assets/pandoc-spec/`.

## Step 2 — Add `SyntaxKind` variants

In `crates/panache-parser/src/syntax/kind.rs`, SCREAMING_SNAKE_CASE.

Add a distinct kind for **every byte category you need to round-trip**: the
marker, the delimiters, the content, and any attribute payload. A construct
that reuses `TEXT` for its markers cannot be formatted without re-lexing later,
which is the usual reason a construct ends up needing a second pass.

## Step 3 — Gate it behind a flag

New syntax almost always belongs to an extension or a flavor.

1. Add the flag to `Extensions` in `crates/panache-parser/src/options.rs`.
2. Wire the host-side default in `src/config.rs`.
3. Set it per flavor.

A construct that is unconditionally live changes behavior for `commonmark` and
will break that conformance suite. This is not optional.

If the flag adds a config key, regenerate the schema:
`UPDATE_EXPECTED=1 cargo test config_schema`.

## Step 4a — Block-level constructs

Add a module under `crates/panache-parser/src/parser/blocks/` exporting
`try_parse_*()` / `emit_*()`. Implement `BlockParser` in
`crates/panache-parser/src/parser/block_dispatcher.rs` and insert it into the
`BlockParserRegistry::new()` vector.

**Registry order is precedence.** The vector is deliberately aligned with
pandoc's reader order, documented in a doc comment on the registry citing
`pandoc/src/Text/Pandoc/Readers/Markdown.hs:487-515`. Placing a parser in the
wrong slot is the single most common cause of subtly wrong output. Existing
load-bearing constraints, each with a comment at its entry:

- fenced code must precede YAML metadata
- MyST directives must precede fenced code (brace-tagged opener wins)
- close-parsers precede their open-parsers (fenced divs, MyST directives)
- headings must precede horizontal rules
- admonitions must precede indented code (4-space body would be eaten)
- indented code must follow fenced code

Justify your position in a comment next to the entry, as the existing ones do.

`detect_prepared()` may return a payload (`Box<dyn Any>`) that
`parse_prepared()` consumes, so emission never re-parses. Use it instead of
scanning the line twice — re-scanning in emission is the quiet path back to
two-pass parsing.

## Step 4b — Inline-level constructs

Add a module under `crates/panache-parser/src/parser/inlines/` and hook it into
`inlines/core.rs`.

Check whether the surrounding dialect is already on the unified inline IR
(`inlines/inline_ir.rs`) **before** choosing where to put the logic. The IR
currently backs CommonMark only; the Pandoc dialect is mid-migration. Adding to
the wrong side means the work is undone by the next migration step.

## Step 5 — Typed AST wrapper

Add one in `crates/panache-parser/src/syntax/` if the linter or LSP needs to
interrogate the node. Consumers should never match on raw `SyntaxKind`
sequences — that is what the wrappers exist to prevent.

## Step 6 — Teach the formatter

Block dispatch is the `match node.kind()` in
`crates/panache-formatter/src/formatter/core.rs`; inline handling lives in
`formatter/inline.rs`.

An unhandled kind typically falls through to verbatim-ish output. That looks
correct in a flat smoke test and breaks as soon as wrapping, indentation, or
container nesting is involved — which is why step 7 insists on nested fixtures.

## Step 7 — Fixtures in both suites

Both, not either:

- **Parser golden case** in `crates/panache-parser/tests/fixtures/cases/<name>/`
  (`input.md` + optional `parser-options.toml`) pins the CST via `insta` and
  proves losslessness.
- **Formatter golden case** in `tests/fixtures/cases/<name>/` (`input.*`,
  `expected.*`, optional `panache.toml`). Register the directory name in the
  `golden_test_cases!` macro at the bottom of `tests/golden_cases.rs` or it
  silently never runs.

Cover the construct **nested inside a list item and inside a blockquote**.
Container prefixes are where most constructs break, and a top-level-only
fixture will not catch it.

## Step 8 — Verify

```bash
# Losslessness + idempotency on a real document using the construct
cargo run -- debug format --checks all scratch.qmd

# The real regression signal: a new block parser changes precedence globally
cargo test -p panache-parser --test commonmark
cargo test -p panache-parser --test pandoc

cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If a conformance suite regresses, the registry slot is the first suspect.

## Step 9 — Document

Update the relevant page under `docs/`. If a config key was added, the schema
regeneration from step 3 must be committed alongside.

## Failure modes, in the order they usually appear

| Symptom | Cause |
|---|---|
| Losslessness failure | A byte category has no `SyntaxKind`; markers folded into `TEXT` |
| Construct parses, output unchanged | Formatter `match` has no arm for the kind |
| Unrelated CommonMark examples regress | Registry slot too early, or the construct is not extension-gated |
| Breaks only inside a list or quote | No nested fixture; container prefix handling untested |
| Formatting is not idempotent | Formatter re-emits a marker the parser already captured |
