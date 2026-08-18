---
paths:
  - "crates/panache-formatter/src/formatter/yaml.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.md"
  - "crates/panache-formatter/tests/yaml_cross_validation.rs"
  - "crates/panache-formatter/tests/fixtures/yaml_corpus/**"
  - "crates/panache-formatter/src/yaml_engine.rs"
---

YAML formatter work should stay spec-driven and idempotent.

- Treat `STYLE.md` as the source of truth for YAML formatting rules.
- Keep one YAML output path through `format_yaml`.
- Treat cross-validation mismatches as bugs to diagnose (formatter, parser, or
  oracle), not divergence to accept.
- Keep pretty_yaml as a dev-only reference; no runtime dependency.
- Keep YAML formatter logic in `panache-formatter`, not parser code.
- Keep plain metadata and hashpipe formatting on the same path.
- Ensure idempotency for every YAML corpus case.
