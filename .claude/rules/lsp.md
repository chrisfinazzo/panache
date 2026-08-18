---
paths:
  - "src/lsp/**/*.rs"
  - "src/lsp.rs"
  - "tests/lsp/**/*.rs"
  - "tests/lsp.rs"
---

LSP changes must preserve protocol correctness and stable document state.

- Preserve open/change/save/close flow behavior.
- Keep UTF-16/UTF-8 position conversions correct.
- Prefer typed syntax wrappers and shared conversion/state helpers.
- Keep state transitions explicit; avoid silent failure paths.
- Add targeted LSP tests for protocol-visible behavior changes.
- Keep `docs/guide/lsp.qmd` aligned with user-visible LSP behavior.
