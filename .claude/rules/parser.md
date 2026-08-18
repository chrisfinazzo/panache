---
paths:
  - "crates/panache-parser/src/parser/**/*.rs"
  - "crates/panache-parser/src/parser.rs"
  - "crates/panache-parser/src/syntax/**/*.rs"
  - "crates/panache-parser/src/syntax.rs"
  - "src/parser.rs"
  - "src/syntax.rs"
---

Parser and syntax changes must preserve lossless CST behavior.

- Treat pandoc-native output as the reference for ambiguous syntax.
- CommonMark and Pandoc are different dialects; use `Dialect` for structural
  parser differences and `Extensions` for feature toggles.
- Preserve structural markers and whitespace in CST.
- Keep parser policy separate from formatter/linter policy.
- Keep parsing single-pass and reuse existing dispatcher/parser utilities.
- Add focused parser tests or fixtures before behavior changes.
- Review CST snapshot diffs intentionally when snapshots change.
