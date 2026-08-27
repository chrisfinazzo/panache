---
name: math-parser-formatter
description: Implement or debug Panache's TeX math parser and formatter
  internals, including the lossless CST, semantic model, diagnostics, and
  Badness parity. Use when modifying math implementation code or regression
  tests. Do not use for explanatory questions, configuration or API design
  discussions, or documentation-only work.
---

# Maintain the math parser and formatter

Treat math formatting as a stable, issue-driven subsystem. The migration
roadmap in `TODO.md` is complete; do not use it as a work queue.

Project source paths below are relative to the repository root. Skill resources
are relative to this `SKILL.md`.

## Scope and sources of truth

- The TeX content parser is
  `crates/panache-parser/src/parser/math.rs`; host embedding lives in
  `crates/panache-parser/src/parser/inlines/math.rs`, and typed accessors live
  in `crates/panache-parser/src/syntax/{math,inlines}.rs`.
- The formatter lives in `crates/panache-formatter/src/formatter/math/` and
  `crates/panache-formatter/src/formatter/math.rs`. Its user-facing gate is the
  stable `[format] format-math` option, which defaults to off. The
  `[experimental] format-math` spelling is a deprecated compatibility alias.
- For changes to user-visible formatting behavior, read
  `crates/panache-formatter/src/formatter/math/STYLE.md`; it is the canonical
  style and preservation contract.
- For cross-layer parser, semantic, or formatter work, read
  [REFERENCE.md](REFERENCE.md) before changing behavior. It records the
  non-obvious implementation constraints, intentional oracle differences, and
  stabilization baseline.
- Pandoc is not an oracle for math formatting because it preserves math
  content. Use the exact, pinned `badness-parser` and `badness-formatter`
  development dependencies as structural and output oracles. Retain
  independent MathML and TeX/PDF checks when meaning preservation is at risk.

Follow the parser, math, formatter, linter, and language-server invariants in
the repository's root `AGENTS.md` for every layer the change touches.

## Invariants

- Keep parsing unconditional, single-pass, lossless, and error-tolerant. The
  formatter option controls rewriting; it is not a Pandoc `Extensions` flag.
- Derive `MathDiagnostic` values through `math_diagnostics()` as a side
  channel. Do not encode diagnostics as CST structure.
- Keep Markdown delimiters, Bookdown equation labels, Pandoc attributes, and
  container prefixes at the host layer where possible. Equation labels remain
  host tokens between ordered `MATH_CONTENT` segments.
- Keep `MATH_SPACE` and `MATH_NEWLINE` distinct from host trivia, and read math
  source through `syntax::math::math_content_text()` when container prefixes
  may be interleaved.
- Keep the lexical CST neutral. Derive operator class, delimiter role,
  contextual unary coercion, and break priority through the shared semantic
  atom model rather than formatter-local token interpretation.
- Keep Badness test-only. Production code must not retain or project a Badness
  tree, delegate parsing or formatting to Badness, or depend on Badness at
  runtime. Test projectors may only perform mechanical kind and wrapper
  normalization; they must not parse, infer attachment, or repair a tree.
- Preserve source bytes when syntax is malformed or semantics cannot be proven.
  Do not broaden the formatter's rewrite boundary merely to increase oracle
  parity.
- Do not rewrite macros, canonicalize `\frac` and `\dfrac`, or format constructs
  whose meaning would require macro expansion.
- If a pinned Badness defect explains a divergence, record the defect and tell
  the user. Do not copy the defect into production code or silently reclassify
  the case.

## Workflow

1. Reproduce the smallest failing behavior and identify the owning layer:
   host parsing, TeX CST, semantic atoms, formatter lowering/layout,
   configuration, diagnostics, or position mapping. Inspect the CST before
   adding a formatter workaround.
2. Read the relevant source of truth above. Verify current implementation when
   documentation, reports, and behavior disagree; correct only stale material
   within the requested scope.
3. Add the smallest focused failing test first. Use parser goldens for CST
   shape, formatter goldens for user-visible output, and unit or differential
   tests for semantic behavior.
4. Compare against the pinned Badness oracle when the shared behavior applies.
   Keep named intentional differences and preservation cases explicit. Use
   MathML or representative TeX/PDF checks when byte parity does not establish
   semantic equivalence.
5. Run the focused suite for each changed layer:

   - Parser CST:
     `cargo test -p panache-parser --test math_badness_parity`
   - Semantic model:
     `cargo test -p panache-parser --test math_semantic_parity`
   - Formatter output:
     `cargo test -p panache-formatter --test math_badness_oracle`
   - Formatter behavior and host integration: the smallest matching tests under
     `crates/panache-formatter/tests/` and the root golden suite.
6. If the formatter corpus, route census, or parity classification changes,
   regenerate and review the tracked report with:

   ```bash
   cargo test -p panache-formatter --test math_badness_oracle \
     math_badness_full_report -- --ignored --nocapture
   ```

7. Before landing a code change, run the workspace validation required by root
   `AGENTS.md`. Review every parser snapshot diff for losslessness, verify
   formatter idempotency, and keep output byte-identical when `format-math` is
   disabled. Do not rerun a focused suite after an equivalent workspace gate
   on the same tree state.
8. Update `STYLE.md`, user documentation, configuration schema, oracle reports,
   or this skill only when the behavior or durable maintenance contract changes.
   Do not update the completed roadmap or historical baselines for an unrelated
   fix. Commit only when the user has requested it, following repository
   guidance.
