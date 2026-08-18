---
paths:
  - "crates/panache-parser/tests/commonmark.rs"
  - "crates/panache-parser/tests/commonmark/**"
  - "crates/panache-parser/tests/fixtures/commonmark-spec/**"
  - "crates/panache-parser/scripts/update-commonmark-spec-fixtures.sh"
---

CommonMark conformance harness changes must stay fixture-driven and
flavor-gated.

- Keep conformance runs `Flavor::CommonMark` only.
- Keep the test renderer (`tests/commonmark/html_renderer.rs`) test-only.
- Verify parser behavior against pandoc when a construct may differ between
  CommonMark and Pandoc markdown.
- Use `Dialect` for structural parser differences and `Extensions` for
  per-feature toggles.
- Do not allowlist without first confirming pass status in a fresh full report.
- Keep `blocked.txt` reasons specific; do not use it to hide regressions.
- Do not hand-edit generated conformance report files.
