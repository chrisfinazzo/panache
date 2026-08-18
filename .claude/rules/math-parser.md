---
paths:
  - "crates/panache-parser/src/parser/math.rs"
  - "crates/panache-parser/src/syntax/math.rs"
  - "crates/panache-parser/src/syntax/inlines.rs"
  - "crates/panache-parser/src/parser/inlines/math.rs"
  - "crates/panache-parser/tests/fixtures/cases/*math*/**"
---

Math-parser work should stay lossless, tolerant, and parser-policy-only.

- Preserve losslessness (`tree.text() == content`) and never hard-fail parsing.
- Keep diagnostics in side channels; do not encode errors as CST structure.
- Emit only `MATH_*` token kinds for math-content internals.
- Keep parser single-pass; avoid post-parse repair passes.
- Pass flavor-specific behavior through parser options, not global state.
- Keep formatting policy in formatter paths, not parser.
- Add focused parser fixtures for new math behavior.
