---
paths:
  - "crates/panache-parser/tests/yaml.rs"
  - "crates/panache-parser/tests/yaml/**/*.txt"
  - "crates/panache-parser/tests/fixtures/yaml-test-suite/**"
---

YAML harness changes must stay fixture-driven and parity-oriented.

- Treat each yaml-test-suite case directory as source of truth.
- Use `test.event` for expected event behavior and `error` for expected-failure
  behavior.
- Do not allowlist cases without checking both event and error contracts.
- Keep triage/regression reporting reproducible and generated from the harness.
- Prefer structured snapshots over ad-hoc text dumps.
