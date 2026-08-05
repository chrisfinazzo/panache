---
name: external-tools
description: Work on Panache's delegation of embedded code blocks to third-party
  formatters and linters (ruff, shfmt, shellcheck, rustfmt, ...) — add or change
  a preset, fix the offset mapping that translates a tool's line/column
  diagnostics back onto document positions, or debug why a tool is not being
  invoked. Use when touching the `[formatters]` / `[linters]` config surface or
  anything under the external tool execution paths.
---

Use this skill when adding a tool preset, changing how an external tool is
invoked, or debugging wrong spans on diagnostics that came from one.

**Key architectural fact**: this is entirely a **host concern**.
`crates/panache-formatter` never spawns a process. If a change wants to run a
subprocess from the formatter crate, the design is wrong — the formatter crate
is dependency-lean by intent and is consumed as a published crate by
`jolars/dprint-plugin-panache`.

## Where things live

| Path | Role |
|---|---|
| `src/config/formatter_presets.rs` | `PRESETS` table of `FormatterPresetMetadata` |
| `src/linter/external_linters.rs` | Linter preset table + tool output parsing |
| `src/external_tools_common.rs` | Shared process plumbing, invocation budget, warning de-dup |
| `src/external_formatters_sync.rs` | Synchronous formatter execution (CLI + LSP) |
| `src/linter/external_linters_sync.rs` | Synchronous linter execution |
| `src/external_formatters_common.rs` | Reindentation and fence-boundary handling of tool output |
| `src/linter/code_block_collector.rs` | Gathers the code blocks to dispatch |
| `src/linter/offsets.rs` | Maps tool line/column back onto document offsets |

## Adding a preset

Add an entry to the `PRESETS` table:

```rust
FormatterPresetMetadata {
    name: "air",
    url: "https://github.com/posit-dev/air",
    description: "R formatter for reproducible style conventions.",
    cmd: "air",
    args: &["format", "{}"],
    stdin: false,
    supported_languages: &["r"],
},
```

- `{}` in `args` is the **filename placeholder**.
- `stdin: false` means the tool rewrites a temp file in place rather than
  reading stdin; `stdin: true` means it reads stdin and writes stdout.
- `supported_languages` is matched against the code block's language token, so
  include every alias users actually write (`sh`/`bash`, `r`/`R`).

`build.rs` reruns on `src/config/formatter_presets.rs` and
`src/linter/external_linters.rs`, so adding an entry regenerates
`docs/reference/_formatter-presets-details.qmd` and
`_linter-presets-details.qmd`. Never hand-edit those.

Verify the preset resolves and the tool actually runs before committing —
a wrong `cmd` or arg order fails silently into "tool not found" warnings.

## Offset mapping — the part that actually breaks

The external tool sees **dedented code with no fence and no container prefix**.
Its reported line and column numbers are relative to that stripped view, so
they must be translated back onto the original document
(`src/linter/offsets.rs`).

Anything that changes the stripped view changes the mapping:

- code blocks inside list items or blockquotes (container prefix)
- indented code blocks
- hashpipe option lines preceding the body
- CRLF input

When a diagnostic points at the wrong line, suspect the mapping before
suspecting the tool. Write the failing case as a fixture with the block nested
in a list item — that is where the prefix arithmetic goes wrong.

## The invocation budget

`init_external_tool_budget` in `src/external_tools_common.rs` bounds how many
external processes a single run may spawn, so a large document or a directory
walk cannot fork thousands of subprocesses. If a tool appears to stop being
applied partway through a big run, check the budget before assuming a
detection bug. `external-max-parallel` is the related config key.

Missing-tool warnings are de-duplicated per run, so a tool absent from `PATH`
warns once, not once per code block.

## Testing

```bash
cargo test --test external_formatters
cargo test --test external_linters
```

These require the **real binaries on `PATH`** and skip when a tool is absent —
which means they can pass locally for the wrong reason. Run them inside the
devenv shell, which installs `shfmt`, `ruff`, `shellcheck`, `stylua`, `eslint`,
and the rest.

## Debugging a tool that seems not to run

1. Is the language token in `supported_languages`?
2. Is the tool on `PATH` as the CLI sees it?
3. Is the on-disk lint cache serving a stale result? Run
   `cargo run -- clean --all` or pass `--no-cache`. The cache key is only
   `panache@<version>`, so it does not invalidate on code changes under a fixed
   in-development version.
4. Has the invocation budget been exhausted?
5. `RUST_LOG=debug cargo run -- format document.qmd` to see the dispatch
   decisions.
