---
paths:
  - "src/linter/**/*.rs"
  - "src/linter.rs"
  - "src/diagnostic_renderer.rs"
  - "tests/linting.rs"
  - "tests/cli/lint.rs"
  - "docs/guide/linting.qmd"
  - "docs/reference/linter-rules.qmd"
---

Linter changes should prioritize precise diagnostics and safe fixes.

- Keep rule code, severity, and span ranges accurate.
- Only add auto-fixes when replacements preserve document intent.
- Keep CLI diagnostics clear and concise without regressing LSP mappings.
- Reuse shared linter orchestration paths rather than duplicating flows.
- Add focused lint tests for user-visible behavior changes.
- Keep lint docs synchronized with rule and output changes.
