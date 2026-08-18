---
paths:
  - "src/config.rs"
  - "src/config/types.rs"
  - "src/config/types/**"
  - "docs/guide/configuration.qmd"
  - "panache.schema.json"
  - "tests/config_schema.rs"
---

Configuration changes should preserve predictable defaults and compatibility.

- Keep config discovery precedence and explicit `--config` failure behavior.
- Merge in deterministic order: flavor defaults, then user overrides.
- Keep canonical keys kebab-case; aliases are compatibility shims.
- Keep deprecation behavior explicit and actionable.
- Update config docs for user-visible config changes.
- Add focused config tests for parse, precedence, and merge behavior changes.
- Regenerate `panache.schema.json` when keys/defaults/enums change.
