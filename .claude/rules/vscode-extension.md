---
paths:
  - "editors/code/**/*.ts"
  - "editors/code/package.json"
  - "editors/code/README.md"
---

VS Code extension changes should preserve reliable LSP startup.

- Keep settings aligned across implementation, schema, and README docs.
- Preserve activation behavior for supported languages/workspaces.
- Reuse existing process/download/config helpers where possible.
- Keep `panache lsp` launch wiring explicit and predictable.
- Validate extension changes with `npm run compile` in `editors/code/`.
