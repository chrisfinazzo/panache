---
paths:
  - "crates/panache-parser/tests/pandoc.rs"
  - "crates/panache-parser/tests/pandoc/**"
  - "crates/panache-parser/tests/fixtures/pandoc-conformance/**"
  - "crates/panache-parser/src/pandoc_ast.rs"
  - "crates/panache-parser/scripts/update-pandoc-conformance-corpus.sh"
---

Pandoc conformance is fixture-driven work under `Flavor::Pandoc`.

- Treat `pandoc -f markdown -t native` as the behavioral reference.
- Keep harness behavior Pandoc-only; do not mix in other flavor goals.
- Triage failures as projector gap, parser-shape gap, flavor-default gap, or
  missing feature.
- Add parser fixtures before allowlisting parser behavior changes.
- Add formatter golden cases only for structural parser changes that alter
  user-visible formatting.
- Never add to `tests/pandoc/allowlist.txt` without rerunning
  `pandoc_full_report` and confirming pass status in that report.
- Do not hand-edit generated reports or `expected.native` corpus outputs.
