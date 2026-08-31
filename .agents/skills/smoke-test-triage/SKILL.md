---
name: smoke-test-triage
description: Triage and fix panache smoke-test regressions (idempotency,
  losslessness, parse/format checks) from CI debug-format reports and linked
  issues, closing verified stale reports when possible.
---

Use this skill when asked to investigate failures reported by smoke-test scans
or `debug format` CI issues (especially idempotency/losslessness regressions).

## Goals

1. Reproduce the exact failure from the report.
2. Minimize to a stable local fixture.
3. Add regression coverage in the right test surface.
4. Fix root cause (not symptom).
5. Validate targeted cases, then full repository checks.
6. Close a report that no longer reproduces when the verification and GitHub
   permissions support that conclusion.

## Triage workflow

1. Read the issue/report details first:
   - failing check type (`idempotency`, `losslessness`, parse, etc.)
   - sample file path
   - upstream repo + commit SHA
   - panache commit/version used by scan
   - diff excerpt and approximate line

   **Close verified stale reports.** The issue may have been fixed between the
   scan and triage. If the exact report command, pinned upstream commit, and
   matching configuration no longer reproduce the reported failure, scan the
   whole target repository with the current checkout. If that also passes,
   close the issue automatically when GitHub authentication permits it. Add a
   concise closure comment naming the original command, the upstream commit,
   the whole-repository before/after counts, and the fixing Panache commit when
   it can be identified, then run:

   ```bash
   gh issue close <issue-number> --reason completed --comment "<evidence>"
   ```

   When practical, reproduce with the scan's Panache revision as a baseline to
   establish that a later commit fixed the failure. Do not close an issue when
   the reproduction setup is uncertain—for example, the target revision or its
   configuration could not be matched. If GitHub credentials or permissions
   prevent closing it, leave it open and report the failed closure command.

2. Reproduce in a local clone of the target repository:
   - checkout the exact target commit from the report
   - run:
     - `panache debug format --checks all --report <sample-file>`
   - if needed, collect pass artifacts with:
     - `panache debug format --checks all --dump-dir <dir> --dump-passes <file>`

3. Minimize:
   - reduce to smallest snippet that still reproduces
   - keep source syntax realistic (especially chunk/hashpipe/YAML edge cases)
   - confirm reproduction is deterministic across repeated runs

4. Classify the failure before fixing — and **verify against pandoc-native
   before any formatter-side fix**:
   - **Mandatory pandoc check.** On the minimized reproducer, run:
     ```
     pandoc <repro>.md -f markdown -t native
     ```
     and compare to panache's CST (`cargo run -- parse < <repro>.md`).
     Pandoc-native is the behavioral reference (per the root `AGENTS.md`).
     If panache's CST differs *structurally* from pandoc — different block
     types (e.g. BulletList where pandoc has CodeBlock), missing/extra
     nesting, wrong attribute attachment — **the bug is parser-side, no
     matter which pass shows the symptom**. Idempotency is a downstream
     symptom of upstream shape divergence.
   - Quick heuristic for the *initial* hypothesis (still subject to the
     pandoc check above):
     - parser issue: losslessness mismatch, CST/marker/trivia dropped, parse
       shape changes, syntax not captured correctly.
     - formatter issue: idempotency drift, wrapping/whitespace churn,
       hashpipe/YAML reflow instability, output normalization changing across
       passes.
   - Use debug artifacts (`--dump-passes`) to compare:
     - input vs parsed output (parser/losslessness boundary)
     - first format vs second format (formatter/idempotency boundary)
   - **Anti-pattern: fixing in the formatter because the symptom lives there.**
     If you find yourself reaching for a formatter helper to make pass1 ==
     pass2 (e.g. propagating looseness, normalizing markers, injecting
     separators), stop and re-run the pandoc check. A formatter fix is only
     correct when panache's CST already matches pandoc's structure and the
     divergence is purely in rendering. If the CST is wrong, the formatter
     fix is papering over a parser bug — fix the parser and add a
     corpus case under
     `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
     so the regression is guarded by the pandoc conformance harness.
   - If uncertain, state the best hypothesis and why before implementing —
     and include the pandoc-native output in the hypothesis.

5. Add regression fixture(s):
   - Formatter user-visible cases:
     - `tests/fixtures/cases/<case-name>/input.{md,qmd,Rmd}`
     - `tests/fixtures/cases/<case-name>/expected.{md,qmd,Rmd}`
     - add case name to `tests/golden_cases.rs`
   - Parser-only behavior:
     - add parser golden case under
       `crates/panache-parser/tests/fixtures/cases/<case-name>/`
     - wire in parser golden runner

6. Fix implementation at root cause:
   - parser lossless/CST bugs -> parser crate
   - formatting/idempotency bugs -> formatter crate
   - avoid papering over by changing expected outputs only
   - preserve existing behavior for unrelated fixtures

7. Validate:
   - targeted first:
     - `cargo test --test golden_cases <case-name>`
     - `panache debug format --checks all --report <fixture-input>`
   - then full validation:
     - `cargo check --workspace`
     - `cargo test --workspace`
     - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
     - `cargo fmt -- --check`

## Panache-specific guidance

- For `.qmd` smoke regressions, ensure tests run with correct flavor context
  (Quarto detection matters for hashpipe/chunk behavior).
- Prefer adding one focused regression fixture per bug.
- Do not update unrelated golden fixtures.
- Be careful with trailing whitespace and wrapping in hashpipe/YAML paths: these
  are common idempotency triggers.
- Use Pandoc behavior as reference for ambiguous syntax.

## Report-back format

When done, report:

1. Whether the issue reproduced (and exact command).
2. Minimal reproducer summary.
3. Fixture(s) added/updated.
4. Root cause and code path changed.
5. Validation commands run and outcomes.
6. What you did **not** fix: remaining failure buckets by root cause, which
   are open modeling gaps worth work. Say plainly whether the issue can be
   closed or should stay open—and whether you closed it automatically.
