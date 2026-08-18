---
paths:
  - "crates/panache-parser/src/parser/yaml.rs"
  - "crates/panache-parser/src/parser/yaml/**/*.rs"
  - "crates/panache-parser/src/parser/blocks/metadata.rs"
  - "crates/panache-parser/src/syntax/yaml.rs"
  - "crates/panache-parser/tests/**/*yaml*"
  - "crates/panache-parser/tests/fixtures/yaml-test-suite/**"
  - "crates/panache-parser/tests/fixtures/cases/*yaml*/**"
  - "crates/panache-parser/tests/fixtures/cases/crlf_yaml_metadata/**"
---

YAML parser work should stay lossless and indentation-aware.

- Keep YAML parsing CST-first and lossless, including trivia.
- Keep one core parser model for plain and hashpipe-prefixed YAML.
- Keep host/embedded range mapping explicit and deterministic.
- Keep parser and formatter policy separate.
- Guard behavior with yaml-test-suite parity plus losslessness checks.
- Add focused deterministic tests for new YAML behavior.
