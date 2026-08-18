---
paths:
  - "crates/panache-parser/tests/commonmark.rs"
  - "crates/panache-parser/tests/commonmark/**"
  - "crates/panache-parser/tests/fixtures/commonmark-spec/**"
---

CommonMark conformance is fixture-driven work under `Flavor::CommonMark`.

- Treat `spec.txt` and byte-equal HTML matching (after shared `<li>` whitespace
  normalization) as the source of truth.
- Keep the harness CommonMark-only; do not add cross-flavor behavior here.
- Classify failures as renderer gap, parser-shape gap, flavor leak, dialect
  divergence, or missing feature.
- Use pandoc (`-f commonmark` vs `-f markdown`) to distinguish dialect
  divergence from extension-default leaks.
- Add focused parser fixtures before allowlisting parser behavior changes.
- Add formatter goldens only when parser structure changes produce different
  user-visible CommonMark formatting.
- Never add to `tests/commonmark/allowlist.txt` without rerunning
  `commonmark_full_report` and confirming the example passes in the new report.
- Do not hand-edit generated reports or vendored spec fixtures.
