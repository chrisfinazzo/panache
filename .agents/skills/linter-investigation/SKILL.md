---
name: linter-investigation
description: Investigate panache's linter (and, secondarily, its parser) against a
  real-world Quarto/Markdown codebase. Clone a target repo, lint it, and triage
  the diagnostics for false positives, incorrect spans, and unsafe autofixes
  (fixes that change document meaning); mis-parses of valid Markdown are caught
  along the way. Suspected bugs are confirmed against `pandoc`/`quarto` as the
  ground-truth AST before being called bugs. Use when asked to stress-test,
  investigate, or triage the linter (or parser) over an external repo or corpus.
---

Point panache's linter at a large body of real Quarto/Markdown and hunt for
**linter quality bugs**: false positives, incorrect spans, and unsafe fixes. This
is the primary goal. **Parse problems are a secondary catch**—Markdown is
permissive, so parser bugs rarely surface as hard errors; they show up as a
construct panache parses *differently* from pandoc, or as a mis-parse that makes
a lint rule misfire. Report those, but keep the center of gravity on the linter,
not a full parser/AST audit.

This is **distinct from the `smoke-test-triage` skill.** That one reacts to the
weekly automated corpus scan's *formatter* regressions (losslessness,
idempotence, format-error, panic) filed as GitHub issues. This skill is
proactive and interactive: you choose a repo and go looking for linter/parser
quality problems. Formatter losslessness and idempotence are out of scope
here—leave them to `smoke-test-triage`.

## The core principle (read first)

**A finding is only a bug once pandoc/quarto shows panache is wrong.** Real
corpora mix flavors (Pandoc, Quarto, R Markdown, GFM, CommonMark, MyST), and a
construct's meaning depends on the active flavor—so always triage with the right
`--flavor`. Classify each suspicious finding into exactly one of:

- **True positive** — panache is right; move on.
- **False positive** — panache flags legitimate Markdown. The highest-value find.
- **Incorrect span** — the finding is real but the highlighted range is wrong.
- **Unsafe fix** — an autofix that changes the *rendered document* (Markdown is
  full of significant whitespace, list indentation, and inline-boundary rules),
  breaks the source, or mangles a fenced code block / YAML block. Test it;
  `--unsafe-fixes` fixes especially warrant scrutiny.
- **Parser bug** — valid Markdown that panache parses differently from pandoc in
  a way that matters (wrong block/inline structure). Confirm against the pandoc
  AST.

## The oracle (pandoc and quarto are installed)

- **pandoc AST** — the ground truth for how a construct parses. Compare
  structure with the native/JSON AST:

  ```sh
  pandoc -f markdown -t native <<'EOF'
  ...snippet...
  EOF
  ```

  Match the reader to the flavor (`-f gfm`, `-f commonmark`, `-f markdown` for
  Pandoc). For a `.qmd`, `quarto` is the higher-level oracle.
- **panache's own view** — `panache parse <file>` (with `--to` for projection,
  `--flavor` to pin the dialect) prints the CST; `panache debug format --checks
  all --dump-dir <dir> <file>` dumps input/parse/format artifacts (this is the
  losslessness/idempotence diagnostic path—use it to *understand* a mis-parse,
  even though fixing those regressions belongs to `smoke-test-triage`).

A linter false positive is usually a *Markdown-semantics* judgment settled from
the pandoc AST plus panache's parse tree; reach for `quarto` when the question is
Quarto-specific (shortcodes, div syntax, execution blocks).

## Workflow

1. **Target.** Take the repo from the user's argument (GitHub `owner/name`, clone
   URL, or local path). If none is given, propose a good default (`hadley/r4ds` or
   `rstudio/bookdown` for R Markdown; `quarto-dev/quarto-web` for Quarto) and
   confirm before cloning.

2. **Setup (parallel/background).** Build the release binary and shallow-clone
   into the **session scratchpad directory** (not bare `/tmp`), at once:

   ```sh
   cargo build --release
   git clone --depth 1 https://github.com/<owner>/<name>.git "$SCRATCH/<name>"
   ```

   panache is a workspace; the parser lives in `crates/panache-parser`.

3. **Lint the tree, capture everything.** Capture both streams (per-violation
   diagnostics may print to stdout, errors to stderr; `lint` exits non-zero on
   violations):

   ```sh
   target/release/panache lint "$SCRATCH/<name>" >lint.out 2>lint.err
   ```

   panache lints `.qmd/.md/.Rmd/.Rmarkdown`. Set `--flavor` if the repo's files
   need a specific dialect and extension inference isn't enough.

4. **Summarize by rule.** Count findings per rule to prioritize the high-volume
   and high-risk buckets:

   ```sh
   grep -oE '(warning|error): [a-z-]+' lint.err lint.out | sort | uniq -c | sort -rn
   ```

5. **Triage (the heart of the work).** For each priority rule, pull real findings,
   open the cited source line, and **reduce each suspect to a minimal
   reproducer** piped to the tool:

   ```sh
   printf '...\n' | target/release/panache lint --flavor <flavor>
   printf '...\n' | target/release/panache parse --flavor <flavor>   # inspect CST
   ```

   For a suspected mis-parse, **isolate the trigger by bisecting context** (block
   vs inline, inside a list/blockquote/fenced block, which flavor), varying one
   axis at a time until the minimal shape is pinned—then diff panache's structure
   against pandoc's AST for the same snippet and flavor.

6. **Verify against the oracle.** Promote a suspicion to a bug only after
   pandoc/quarto agrees the construct means what you claim—under the flavor the
   file actually uses.

7. **Fan out for volume (recommended).** For a big finding set, spawn parallel
   triage subagents—one per rule-bucket—each given the absolute
   `target/release/panache` path, the `lint.out`/`lint.err` paths, the
   classification scheme (with the flavor caveat), and the pandoc/quarto oracle
   recipe. Each returns minimal reproducers, per-category verdicts, and an
   FP-rate assessment.

8. **Fix or record.** For the cleanest, well-isolated bugs, fix TDD-style,
   honoring panache's tenets (parser bugs fixed in the parser; losslessness
   sacred; a fix must not change rendered meaning):

   - Add a failing golden/fixture case first and **watch it fail**, following
     panache's `add-lint-rule` and the golden-case conventions under
     `tests/fixtures/cases/` and `crates/panache-parser/tests/` (reduce from the
     corpus).
   - Fix at the root cause; re-verify against pandoc/quarto.
   - Run the gates: `cargo test`, `cargo clippy --all-targets --all-features --
     -D warnings`, `cargo fmt -- --check`; `cargo insta accept` after reviewing
     new snapshots.

   Record everything you don't fix as follow-ups in `TODO.md` in the house style,
   each with a minimal reproducer, the flavor, and the pandoc/quarto behavior.
   Commit only if the user asks—atomic, Conventional Commits.

9. **Report back.** State plainly: bugs found (fixed vs. documented) with
   copy-pasteable reproducers (and the flavor each assumes); false-positive
   categories per rule; incorrect-span issues; which rules you verified clean;
   and follow-ups recorded. Be faithful about which flavor each verdict was
   checked under.

## panache-specific notes

- **Flavor is load-bearing.** The same text is valid-but-different across Pandoc,
  Quarto, GFM, CommonMark, and MyST. Never triage without pinning `--flavor`, and
  always report which flavor a finding assumes—an "FP" under GFM may be correct
  under Pandoc.
- **YAML front matter and fenced code blocks are hotspots.** Rules and fixes that
  touch a `---` metadata block or a code fence can corrupt structured content;
  test any such fix by re-parsing and by round-tripping through pandoc.
- **Unsafe fixes change rendered meaning.** Significant whitespace, list-item
  indentation, and inline emphasis boundaries make "cosmetic" edits risky.
  `--unsafe-fixes` edits deserve the most scrutiny.
- **Workspace layout:** the parser is `crates/panache-parser`; `debug format
  --checks all --dump-dir` is the artifact-dump path for understanding a
  mis-parse (fixing losslessness/idempotence itself is `smoke-test-triage`'s job).
