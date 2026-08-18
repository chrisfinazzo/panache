# Panache agent guide

Use this file for durable, high-signal guidance.

## Project snapshot

- Rust 2024 workspace with root `panache` crate (library + CLI).
- Workspace crates: `crates/panache-parser`, `crates/panache-formatter`,
  `crates/panache-wasm`.
- `editors/zed` is intentionally outside the workspace.

## Validation commands

Run these before and after non-trivial changes:

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt -- --check
```

Useful targeted commands:

```bash
cargo test --test golden_cases <case_name>
cargo test -p panache-parser --test golden_parser_cases <case_name>
cargo run -- debug format --checks all document.qmd
```

## Parser and formatter invariants

- Parser is lossless CST capture; formatter owns style policy.
- Keep parser single-pass (block parsing + inline emission).
- Preserve all bytes in CST; formatting belongs in formatter only.
- Use typed syntax wrappers in downstream code (LSP/linter), not raw kind
  matching where wrappers exist.
- CommonMark and Pandoc are different dialects; use `Dialect` for structural
  parser differences and `Extensions` for feature toggles.
- Pandoc-native output is the behavioral reference for parser ambiguity.

## Scoped engineering guidance

### Parser and syntax

Applies to `crates/panache-parser/src/parser/**`,
`crates/panache-parser/src/syntax/**`, `src/parser.rs`, and `src/syntax.rs`.

- Preserve structural markers, whitespace, trivia, and all source bytes in the
  CST.
- Keep parser policy separate from formatter and linter policy.
- Reuse existing dispatchers and parser utilities; do not introduce post-parse
  repair passes.
- Add focused parser tests or fixtures before changing behavior, and review CST
  snapshot diffs intentionally.

### Math parser

Applies to math parsing and syntax code under `crates/panache-parser` and parser
fixtures whose names contain `math`.

- Keep math parsing lossless and tolerant; parsing must not hard-fail.
- Report diagnostics through side channels rather than encoding errors as CST
  structure.
- Emit only `MATH_*` token kinds for math-content internals.
- Pass flavor-specific behavior through parser options, never global state.
- Keep formatting policy out of the math parser and add focused parser fixtures
  for new behavior.

### YAML parser

Applies to YAML parser/syntax code, metadata block parsing, YAML parser tests,
and YAML parser fixtures under `crates/panache-parser`.

- Keep YAML parsing CST-first, lossless, indentation-aware, and
  trivia-preserving.
- Use one core parser model for plain and hashpipe-prefixed YAML.
- Keep host-to-embedded range mapping explicit and deterministic.
- Guard behavior with yaml-test-suite parity, losslessness checks, and focused
  deterministic tests.

### Formatter

Applies to `src/formatter.rs`, `crates/panache-formatter/**`, and formatter
golden fixtures.

- Enforce idempotency: `format(format(x)) == format(x)`.
- When idempotency fails, inspect parser CST shape before adding a formatter
  workaround.
- Reuse existing wrapping, list, table, and related helpers.
- Keep formatter core logic in `crates/panache-formatter`; keep host runtime and
  process integration in top-level `src/`.
- Add or update the smallest relevant formatter golden and documentation for
  user-visible formatting changes.

### YAML formatter

Applies to YAML formatter code, its `STYLE.md`, cross-validation tests, and YAML
corpus fixtures under `crates/panache-formatter`.

- Treat `STYLE.md` as the source of truth for YAML formatting rules.
- Keep one YAML output path through `format_yaml`, shared by plain metadata and
  hashpipe formatting.
- Treat cross-validation mismatches as formatter, parser, or oracle bugs to
  diagnose, not accepted divergence.
- Keep `pretty_yaml` as a development-only reference with no runtime dependency.
- Ensure every YAML corpus case is idempotent.

### Linter

Applies to `src/linter.rs`, `src/linter/**`, `src/diagnostic_renderer.rs`, lint
tests, and lint documentation.

- Prioritize accurate rule codes, severity, spans, and precise diagnostics.
- Add auto-fixes only when replacements preserve document intent.
- Keep CLI diagnostics concise without regressing LSP mappings.
- Reuse shared linter orchestration instead of duplicating flows.
- Add focused lint tests and synchronize docs for user-visible changes.

### LSP

Applies to `src/lsp.rs`, `src/lsp/**`, LSP tests, and `docs/guide/lsp.qmd`.

- Preserve open/change/save/close behavior and stable document state.
- Keep UTF-16/UTF-8 position conversion correct.
- Prefer typed syntax wrappers and shared conversion/state helpers.
- Make state transitions explicit; avoid silent failure paths.
- Add targeted protocol-visible tests and update the LSP guide for user-visible
  behavior changes.

### Configuration

Applies to `src/config.rs`, `src/config/types/**`, configuration docs,
`panache.schema.json`, and schema tests.

- Preserve config discovery precedence and explicit `--config` failure behavior.
- Merge deterministically: flavor defaults first, then user overrides.
- Keep canonical keys kebab-case; aliases are compatibility shims.
- Make deprecations explicit and actionable.
- Add focused tests for parsing, precedence, and merge changes; update docs and
  regenerate `panache.schema.json` when keys, defaults, or enums change.

### External formatter presets

Applies to `src/config/formatter_presets.rs` and the generated formatter preset
reference.

- Keep `PRESETS` metadata and `formatter_preset_names()` synchronized.
- Preset names are free-form config strings, so adding one does not require
  schema regeneration.
- For stdin tools needing filename hints, use `{}` argument placeholders and
  ensure language extensions are mapped.
- Add focused preset-resolution tests for new or changed presets.

### VS Code extension

Applies to `editors/code/**`.

- Keep settings aligned across implementation, schema, and README.
- Preserve activation behavior for supported languages and workspaces.
- Reuse existing process, download, and configuration helpers.
- Keep `panache lsp` startup explicit and predictable.
- Validate changes with `npm run compile` in `editors/code/`.

## Architecture boundaries

- `crates/panache-formatter` is dependency-lean formatter core.
- Top-level `src/formatter.rs` and related host modules own runtime/process
  integrations (external tools, CLI/LSP wiring).
- Do not move external process execution into formatter core crate.

## Conformance and fixtures

- Keep conformance work incremental; avoid broad rewrites in one session.
- Never add allowlist entries without rerunning the relevant report and
  confirming the example/case is currently passing.
- Parser behavior changes require focused parser fixtures; conformance
  allowlists are regression guards, not primary behavior specs.
- If parser structure changes user-visible formatting, add/update formatter
  golden cases.

### CommonMark conformance

Applies to `crates/panache-parser/tests/commonmark.rs`, its support modules,
CommonMark spec fixtures, and the fixture update script.

- Run conformance only under `Flavor::CommonMark`; keep the HTML renderer
  test-only.
- Treat `spec.txt` and byte-equal HTML after shared `<li>` whitespace
  normalization as the source of truth.
- Classify failures as renderer gap, parser-shape gap, flavor leak, dialect
  divergence, or missing feature.
- Compare pandoc `-f commonmark` with `-f markdown` when distinguishing dialect
  divergence from extension-default leakage.
- Keep `blocked.txt` reasons specific; never use it to hide regressions.
- Add parser fixtures before allowlisting parser behavior changes. Add formatter
  goldens only when changed structure affects user-visible formatting.
- Never edit the allowlist without rerunning `commonmark_full_report` and
  confirming the example passes in the fresh report.
- Do not hand-edit generated reports or vendored spec fixtures.

### Pandoc conformance

Applies to `crates/panache-parser/tests/pandoc.rs`, its support modules, Pandoc
conformance fixtures, `pandoc_ast.rs`, and the corpus update script.

- Run conformance only under `Flavor::Pandoc` and treat
  `pandoc -f markdown -t native` as the behavioral reference.
- Classify failures as projector gap, parser-shape gap, flavor-default gap, or
  missing feature.
- Add parser fixtures before allowlisting parser behavior changes. Add formatter
  goldens only when changed structure affects user-visible formatting.
- Never edit the allowlist without rerunning `pandoc_full_report` and confirming
  the case passes in the fresh report.
- Do not hand-edit generated reports or `expected.native` corpus outputs.

### YAML conformance harness

Applies to `crates/panache-parser/tests/yaml.rs`, its support files, and
yaml-test-suite fixtures.

- Treat each yaml-test-suite case directory as the source of truth.
- Use `test.event` for expected event behavior and `error` for expected-failure
  behavior.
- Do not allowlist a case without checking both event and error contracts.
- Keep triage and regression reports reproducible and harness-generated.
- Prefer structured snapshots over ad-hoc text dumps.

### Integration tests

Applies to workspace integration tests and fixture directories.

- Assert one user-visible behavior at a time with minimal brittleness.
- Use existing fixture layouts rather than ad-hoc test directories.
- Keep expected-output updates intentional and reviewed.
- Prefer stable substring and ordering assertions for CLI diagnostics.

## Generated artifacts

Do not hand-edit generated files. Regenerate using existing project commands.
Important generated outputs include:

- `docs/reference/cli.qmd`
- `docs/reference/_formatter-presets-details.qmd`
- `docs/reference/_linter-presets-details.qmd`
- `panache.schema.json`

## High-impact gotchas

- `cargo run -- format <file>` edits files in place. Use stdin or `--check` for
  inspection.
- Lint cache can mask linter changes (`~/.cache/panache/`); use
  `cargo run -- clean --all` or `--no-cache` when debugging.
- Never “fix” pandoc divergence in the projector if CST is wrong; fix parser
  shape first.

## Test locations

- Parser golden fixtures: `crates/panache-parser/tests/fixtures/cases/`
- Formatter golden fixtures: `tests/fixtures/cases/` (must be registered in
  `tests/golden_cases.rs`)
- Linter docs parity gate: `tests/linter_rules_docs.rs`
- Config schema parity gate: `tests/config_schema.rs`

## Commits and release hygiene

- Use Conventional Commits per `CONTRIBUTING.md`.
- Do not push or open PRs unless explicitly asked.
- Do not skip hooks (`--no-verify`).
- Do not hand-edit `CHANGELOG.md`.
- Only `v*` CLI releases should publish GitHub release assets in this repo.
