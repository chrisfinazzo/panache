# Panache agent guide

Use this file for durable, high-signal guidance only. Keep procedural playbooks
in `.claude/skills/` and path-specific constraints in `.claude/rules/`.

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
