---
paths:
  - "tests/**/*.rs"
  - "tests/fixtures/cases/**"
  - "crates/panache-formatter/tests/**/*.rs"
---

Integration tests should assert user-visible behavior with minimal brittleness.

- Prefer focused assertions for one behavior change at a time.
- Use existing fixture layouts; avoid ad-hoc test directories.
- Formatter golden fixtures belong in `tests/fixtures/cases/` and must be wired
  into `tests/golden_cases.rs`.
- Parser golden fixtures belong in
  `crates/panache-parser/tests/fixtures/cases/`.
- Keep expected-output updates intentional and reviewed.
- Prefer stable substring/order assertions for CLI diagnostics.
