# Panache TODO

This document tracks implementation status for Panache's features.

## Language Server

### Code Actions

- [ ] Convert between table styles (simple, pipe, grid)
- [x] Convert between inline/reference links

### Navigation & Symbols

- [x] Find references - Find all uses of a reference link/footnote/citation
  - [x] Find references for citations - Find all `@cite` uses of a bibliography
    entry
  - [x] Find references for headings - Find all internal links to a heading
  - [x] Find references for reference links - Find all `[text][ref]` links

### Completion

- [ ] Reference link completion - Complete `[text][ref]` from defined references
- [ ] Heading link completion
- [ ] Attribute completion - Complete class names and attributes in
  `{.class #id}`
- [x] Shortcode completion - Complete Quarto shortcode names in `{{< name >}}`
- [x] Cross-reference completion - Complete `@fig-id` and `\@ref(fig-id)`
  cross-refs (also: file/shortcode path completion is implemented)

### Inlay Hints (low priority)

Personally I think inlay hints are distracting and I am not sure what we want to
support.

- [ ] Link target hints - Show link targets as inlay hints
- [ ] Reference definition hints - Show reference definitions as inlay hints
- [ ] Citation key hints - Show bibliography entries for `@cite` keys
- [ ] Footnote content hints - Show footnote content as inlay hints

### Advanced

- [x] Semantic tokens - Syntax highlighting via LSP (`semanticTokens/full`,
  additive + flavor-gated, custom legend;
  `src/lsp/handlers/semantic_tokens.rs`). Follow-ups: multi-line tokens
  (math/div bodies, per-line split); `full/delta`
  - `result_id`; `range` requests; widen the legend (emphasis/strong/links/
    headings --- only if we decide to contest the base grammar, which flips it
    to opt-in); raw-inline format tags (parser folds `{=fmt}` into a generic
    `ATTRIBUTE`, so a dedicated token kind is needed first).
- [ ] Rename
  - [x] Citations - Rename `@cite` keys and update bibliography
  - [x] Reference links - Rename `[ref]` labels and update definitions
  - [x] Headings - Rename heading text and update internal links
  - [x] Footnotes - Rename footnote labels and update definitions/links
  - [x] Files - Rename linked markdown files and update links
  - [x] Files - Rename other linked files, shortcodes, etc. Covers `embed`,
    `video`, and `placeholder` shortcode paths plus in-document frontmatter
    file paths (`bibliography`, `csl`, `css`). Deferred: raw HTML
    `src`/`href` and raw LaTeX `\input`/`\includegraphics` references;
    nested frontmatter paths such as `format.html.css`.
- [x] Configuration via LSP - `workspace/didChangeConfiguration` to reload
  config

### Spec coverage gaps

Markdown-relevant LSP methods we don't yet implement, surfaced by the 2026-06-18
spec-coverage audit (see `docs/guide/lsp.qmd` "LSP Specification Coverage").
`onTypeFormatting`, `semanticTokens`, `inlayHint`, and
`workspace/didChangeConfiguration` are tracked above and not repeated here.

- [x] Pull diagnostics - `textDocument/diagnostic` + `workspace/diagnostic` as a
  companion/alternative to the current push model (mode-switch: pull clients
  get pull only, push suppressed; cache + `workspace/diagnostic/refresh`)
  - [x] Populate `related_documents` in the document report for clients with
    `related_document_support` (the pulled document's project-graph closure
    carries its related files' cross-file diagnostics inline)
  - [x] Streaming/partial results (`DocumentDiagnosticReportPartialResult`,
    `WorkspaceDiagnosticReportPartialResult`): a `partialResultToken`
    streams the report's tail as `$/progress` chunks (response carries the
    first chunk). No token still returns the whole report
  - [ ] `workspace/diagnostic` only reports open documents + reachable project
    manifests, not every on-disk doc in the workspace (rust-analyzer pulls
    all workspace files). Decide whether closed-but-on-disk docs should
    surface.
- [x] `textDocument/documentHighlight` - highlight every occurrence of the
  reference/citation/footnote/heading under the cursor
- [ ] `textDocument/selectionRange` - structural smart-select expansion (word →
  inline → block → section)
- [x] `textDocument/linkedEditingRange` - edit a reference label and its
  definition simultaneously
- [x] `completionItem/resolve` - defer expensive completion detail (e.g.
  citation previews) until an item is focused
- [ ] `codeAction/resolve` + advertise `codeActionKinds` - compute edits lazily
  and let clients filter actions by kind
- [x] `workspace/didChangeWorkspaceFolders` - multi-root workspaces; config
  resolves per-document against the containing folder, and add/remove
  re-resolves open documents live
- [x] `workspace/configuration` - pull runtime settings from the client (after
  `initialized` and on `didChangeConfiguration`) instead of relying only on
  discovered config files and pushed settings
- [ ] `workspace/executeCommand` - server-side commands backing complex code
  actions
- [x] File operations beyond `willRenameFiles`: `didRenameFiles`,
  `didCreateFiles`, `didDeleteFiles` (hygiene-only;
  `willCreate`/`willDelete` intentionally omitted)

#### Out of scope for prose

These spec methods target compiled-language tooling and have no useful Markdown
analogue; do not re-audit them: call hierarchy, type hierarchy,
`textDocument/implementation`, `typeDefinition`, `declaration`, `inlineValue`,
`moniker`, document color, and code lens.

### Future Lint Rules

#### Syntax correctness

- [ ] Broken table structures
- [ ] Invalid citation syntax (`@citekey` malformations)
- [ ] Unclosed inline math/code spans
- [ ] Invalid shortcode syntax (Quarto-specific)

#### Style/Best practices

- [ ] Multiple top-level headings
- [ ] Empty links/images
- [ ] Unused reference definitions
- [ ] Hard-wrapped text in code blocks
- [ ] Use blanklines around horizontal rules

### Linter bugs and performance (quarto-web triage, 2026-08)

#### False positives: `undefined-anchor` on render-generated anchors

- [ ] `undefined-anchor` flags anchors that only exist after render: Quarto
  listing categories (`gallery/index.qmd` → `#articles-reports`, etc.) and
  include-partials that link to a heading in their *parent*
  (`_incremental-pause.md` → `#creating-slides`, linted standalone). Static
  analysis can't see these targets; consider a heuristic or an opt-out for
  known render-time / cross-document anchors.

### Configuration

- [ ] Severity levels (error, warning, info)
- [ ] Auto-fix capability per rule (infrastructure exists, rules need
  implementation)
- [ ] Unwrap the CLI's top-level error print. `main() -> io::Result<()>` renders
  a returned error via `Debug`, so a config (or any other) error surfaces as
  `Error: Custom { kind: InvalidData, error: ... }`. The inner message is
  now readable (`ConfigError`'s `Debug` mirrors `Display`), but the
  `Custom { kind }` wrapper is noise. Fixing it properly means handling
  errors at the \~13 `load_config_for_cli(...)?` call sites (or switching
  `main` to a custom error type with a `Display`-based `Termination`) so the
  user sees just `Error: invalid config <path>: ...`. Affects all
  `io::Error`s, not only config.

## Incremental Parsing

Multi-session effort to harden, unify, and graduate incremental reparsing to
default-on, then add token/region tiers. Reference implementations audited for
this plan: rust-analyzer (`reparsing.rs`), `../arity`
(`crates/arity-parser/src/parser/reparse.rs`), and `../fatou`
(`crates/fatou-parser/src/parser/reparse.rs`, `src/incremental.rs`) --- fatou is
the primary model and both siblings are on disk for re-reading.

**Governing invariant** (fatou "Tenet 4 strong form"): a successful incremental
reparse must yield a green tree and syntax-error vector byte-identical to a full
parse of the edited text, enforced by a `#[cfg(debug_assertions)]` oracle on
every reparse. Every guard failure bails to full parse --- never an error.

Work happens on the `feat/incremental-parsing-graduation` branch; the user files
the PR themselves. Full design detail (phase entry/exit criteria, the
salsa-unification design, flip acceptance criteria) lives in the plan document
at `~/.claude/plans/i-want-to-promote-splendid-stardust.md`.

**Handover protocol:** a fresh session reads this section, picks the first
unchecked phase, verifies its entry criteria (previous phase's boxes checked,
workspace green on the branch), and works TDD with atomic conventional commits.
On completion it checks the phase box, updates the status line below, and
records any deviation or discovered follow-up as an indented bullet under the
phase. Never leave a phase half-landed: partial work is noted in the status line
with the exact next step.

**Current status / next step:** Phases 1--6a done. There is one authoritative
tree, the reparse lives inside salsa's `parsed_document`, the window-size cutoff
keeps a losing shape from ever being slower than a full parse, and the bench
thresholds are machine-checked (`task bench:incremental-gate`). Everything so
far sits behind `experimental.incrementalParsing`, still default-*off*, so none
of it has changed behavior for anyone --- which is what makes this the natural
PR boundary, with the flip landing separately.

Next step is Phase 6b (default flip). Its entry criteria are met; what it needs
is the gate *run*, not more code: oracle-clean fuzz at 10x iterations, the suite
green with the flag forced both ways, `task bench:incremental-gate` green, and
the week of oracle-live dogfooding. Run the gate at the default iteration count
(see the phase's own note on `multi_change_large_8`).

`incremental_regressions.rs` carries no ignored *incremental* tests; the three
`#[ignore]`d tests there pin two full-parser bugs (setext-after-setext, and a
trailing-`:` line promoting a list item's lazy continuation to a definition
term), both tracked on `main` under "Parser bugs found by the incremental
fuzzer" like the five the fuzzer found earlier. Neither fix has landed on `main`
yet --- the branch is rebased onto it and they are still ignored --- so they
stay ignored until they do.

Hardening applied after the phases above, from a review of the branch:

- Line endings. Every seam test in the cascade is textual, and the blank-line
  check was a `"\n\n"` suffix test, so a CRLF document (blank line `"\r\n\r\n"`)
  was refused at the first guard and never spliced at all --- safe, and a total
  loss of the feature for anything authored on Windows. `ends_with_blank_line`
  now strips one terminator and looks for another, which is line-ending
  agnostic. The fuzz corpus grew two CRLF snippets and two CRLF insert-alphabet
  entries, so the gap is measured rather than accidental.
- Guard parity. `reparse_section_window` ran a strictly weaker guard set than
  the suffix path. Two of the missing three cannot fire while the window is
  anchored at a top-level `HEADING`, but the thematic-break/dash-rule one can,
  and "the window starts at a heading" is a property of how the window is
  *chosen* --- which Phase 8 changes. All are applied on both paths now.
- Release-build safety. Both oracles are `cfg(debug_assertions)` (the host one
  also wants `PANACHE_REPARSE_ORACLE=1`), so a release build checked nothing,
  while `parsed_document` now feeds LSP formatting, which writes the user's
  file. `splice_length_agrees` in `src/salsa.rs` checks the one part of the
  invariant that is `O(1)` --- the spliced tree spans exactly its text --- in
  every build, and *falls back* to the full parse rather than panicking.
- The reshuffled corpus found one more full-parser bug: a trailing `:`/`~` line
  promoting a preceding list item's lazy continuation into a definition term,
  where the *splice* matched pandoc and the full parse did not. Declined by
  `first_block_has_trailing_definition_marker` so the splice keeps matching the
  full parse, pinned `#[ignore]`d in `incremental_regressions.rs`, and tracked
  on `main` under "Parser bugs found by the incremental fuzzer" (the entry says
  to delete the guard with the fix). It is not CRLF-specific --- the CRLF
  inserts only reshuffled the draws onto it --- and reproduces on LF.

No test in this section may read a document from `benches/documents/`: the
corpus is gitignored, and `download.sh` does not even produce every name the
repo still references (`medium_quarto.qmd` is gone). A corpus-reading test fails
on every clean checkout, and a corpus-*skipping* one never runs in CI at all, so
reproducers are synthetic and pin their strategy instead.

- [x] Phase 1: oracle --- `pub fn fingerprint` + debug
  `assert_matches_full_parse` on every non-fallback reparse; RA-style
  `do_check` structural tests (full `{:#?}` equality, pinned strategy +
  reparse-range length); delete the dead `src/range_utils.rs` copy of
  `find_incremental_restart_offset`.
  - Oracle lives in `crates/panache-parser/src/parser/verify.rs`; the existing
    suite (parser + LSP integration) already runs clean under it, so no
    divergence surfaced from the current strategies' happy paths.

- [x] Phase 2: seeded fuzz harness
  (`crates/panache-parser/tests/incremental_fuzz.rs`) with hazard-biased
  alphabet (setext, lazy continuation, fences, `:::` divs, list markers,
  table pipes, refdefs, YAML delimiters, HTML blocks, `$$`, footnotes) +
  commented hazard snippets + `benches/documents/` corpus;
  `PANACHE_FUZZ_ITERS` scaling. The known refdef-reuse bug is expected to
  surface here; capture divergences as minimized `#[ignore]`d red tests.
  - Delivered with deviations. The harness skips (and counts) inputs where the
    *full parser* itself is lossy or panics --- with a broken oracle the splice
    cannot be judged; every skip prints its reproducer, and the minimized cases
    are pinned in `crates/panache-parser/tests/incremental_regressions.rs` and
    tracked under "Parser bugs found by the incremental fuzzer" in the Parser
    section below.
  - Several incremental divergences the harness found were fixed in-session
    instead of parked (Phase 3 work pulled forward): restart-past-edit guard,
    textual + structural seam decoupling, fence-pairing parity over the prefix
    (heuristic; precise old-tree check deferred to Phase 8), list/blockquote
    continuation guard, and a refdef-proximity guard (`edit_may_touch_refdefs`,
    textual; the precise set comparison lands with the host layer in Phase 4).
    The section-window strategy was redesigned: it parses from the window start
    to EOF (list-item buffering depends on unbounded lookahead, so a bounded
    standalone window parse is untrustworthy) and re-adopts the old suffix
    children only on structural equality, else degrades to a suffix splice.

- [x] Phase 3: refdef-set-change guard (cheap bail to full parse); error
  carrying in the incremental result + three-bucket merge (RA recipe);
  oracle/fuzz extended to error equality; un-ignore red tests; error-matrix
  tests {unchanged/fixed/introduced} x strategy.
  - The merge has **two** buckets, not RA's three, as predicted when the phase
    was scoped: both strategies parse their window to EOF and both window starts
    are `<= edit.0`, where `map_old_offset_to_new` is the identity, so the seam
    sits at the same offset in the old and new text and nothing can straddle it.
    That case is a `debug_assert!` plus a bail. The real third bucket waits for
    the bounded region tier in Phase 8; the module doc says so.
  - `parse_incremental_suffix[_with_refdefs]` gained an `old_errors` parameter
    (the shape Phase 4's `reparse` already wanted), and `DocumentState` carries
    the errors beside its tree so `did_change` can feed the prefix's share to
    the next reparse. Both retire with `DocumentState.tree` in Phase 4. The LSP
    still serves diagnostics from salsa's independent full parse, so this is
    plumbing for the oracle, not a behavior change.
  - The **document-start-only construct guard** landed as a cheap textual bail
    on the window's first line (pandoc `%` title block, MultiMarkdown title
    block, CommonMark-dialect `---`). Splitting "byte 0 of the document" from
    "blank-line separated fragment start" in `BlockContext` is the principled
    fix and belongs with Phase 8 --- every other `at_document_start` consumer is
    `||`-ed with `has_blank_before`, which the seam guard already guarantees.
  - The fuzz harness runs four option tiers (pandoc, gfm, quarto,
    multimarkdown), chosen for reach: plain `commonmark` leaves
    `yaml_metadata_block` off and cannot reach the mid-document-YAML hazard at
    all, so `gfm` carries it. Budgets **split** the old pandoc-only counts
    rather than multiplying them, so a default `cargo test` costs about what it
    did before; `PANACHE_FUZZ_ITERS` scales every tier together.
  - The tiers found four more splice bugs, all fixed with regression tests:
    definition-marker and table-caption lines reaching back across the seam, a
    retained thematic break re-read as a multiline-table rule, a refdef-guard
    slice landing inside a multi-byte token, and an `old_edit` past the old
    tree's end. The last two were rowan panics, not divergences. The harness now
    also checks that the *base* parse round-trips, not only the parse of the
    edited text.
  - Nothing to un-ignore: the earlier red tests were fixed on `main` before the
    phase started. The tiers did surface one *new* full-parser bug
    (setext-after-setext), pinned `#[ignore]`d here and tracked on `main` under
    "Parser bugs found by the incremental fuzzer", where the fix belongs; it is
    not an incremental bug.

- [x] Phase 4: salsa unification --- reparse moves into `parsed_document` with a
  side-channel reparse base (fatou model, no staged edit chain: whole-text
  `diff_edit` recovers the single combined edit); base keyed on config +
  refdef set; admission-gated by the runtime flag; delete
  `DocumentState.tree` and the edit-range coalescing helpers; new
  `tests/salsa_incremental.rs`. Staged commits S1-S4, each green.
  - The design doc's three-bucket section-window error splice was already
    obsolete: Phase 3 shipped `merge_incremental_errors` with **two** buckets
    (both strategies parse to EOF), so S2 reused it unchanged.
  - `ReparseCache` has no admitted-set beside its map: presence *is* admission,
    so the two cannot drift apart. Fatou's hot/cold eviction classes are
    unnecessary here for the same reason --- a sweep or a sibling-config parse
    never enters the cache at all, so plain LRU over admitted entries suffices.
  - The parser's textual `edit_may_touch_refdefs` guard stays even though the
    host now compares the sets exactly: it is cheap, and it is the only refdef
    protection the parser-crate entry point has (it holds no refdef history).
  - The host oracle is gated on `cfg(debug_assertions)` +
    `PANACHE_REPARSE_ORACLE=1`, not `cfg(test)` as designed --- integration
    tests link the library built *without* `cfg(test)`, so a `cfg(test)` gate
    would have excluded exactly the suites (`tests/lsp.rs`,
    `tests/salsa_incremental.rs`) it exists to cover.
  - New: `PANACHE_INCREMENTAL_PARSING=1|0` overrides the client setting for the
    whole server process. Phase 6 wanted an escape hatch anyway, and it is what
    lets the suite run green with the feature forced on *and* forced off (the
    handful of tests that assert the setting's own plumbing skip under it).
  - Phase 7's `parser/incremental.rs` extraction was pulled forward into S4 as
    `parser/reparse.rs`, since S4 was already moving the surrounding code. The
    retired `parse_incremental_suffix*` fallback policy now lives with the
    callers that want it: `crates/panache-parser/tests/common/mod.rs` for the
    suites, a `#[cfg(test)]` shim in `reparse.rs` for the in-crate tests, and
    `try_reparse` in `benches/lsp_incremental.rs`.

- [x] Phase 5: benchmark repair --- fix `benches/lsp_incremental.rs`
  multi-change path (currently degenerates to full reparse), add
  fallback-rate + bail-cost accounting, commit results table in module doc.
  - The multi-change path is fixed by *mirroring* `parsed_document` rather than
    approximating it: whole-text `diff_edit` per notification, a base chained
    from step to step, and the host's refdef-set comparison ahead of the
    parser's textual guard. A case is now a *stream* of notifications, which is
    what makes a fallback rate mean anything --- the old per-case rate was 0 or
    1 by construction.
  - Three cases were measuring nothing. `synthetic_document` emitted adjacent
    lines, so every "paragraph" was one giant paragraph with no blank line for
    the seam guard to decouple at, and every synthetic edit got a window
    starting at byte 0; it now separates paragraphs and emits a `##` heading
    every ten, so the section-window strategy has something to find.
    `pandoc_manual_single_edit` edited line 200, which is ``[`setspace`]: ...``,
    and rewrote the *label* --- it is kept, renamed to
    `pandoc_manual_refdef_label_edit`, as the host-side-decline case, and a
    genuine early-prose edit was added beside it. `fallback_invalid_range` was
    dropped: the server validates client ranges before touching its buffer, so
    it modelled nothing, and with `diff_edit` it merely duplicated
    `full_replace`.
  - The accounting distinguishes **window** bytes (window start to EOF, what
    both strategies actually re-parse) from **spliced** bytes (green children
    replaced). Only the former predicts the speedup; printing the latter is what
    made `tables_single_edit` look like "7% reparsed, 0.98x". The bench also
    reports a per-strategy histogram and a fallback-reason histogram, and
    verifies the governing invariant at every step of every case before timing
    anything.
  - Headline: speedup is a function of window share and nothing else --- 5.6x on
    a late edit or a typing stream in the pandoc manual (7% window), 1.0x where
    the window is \~98%. A *successful* wide reparse is 5-10% slower than a full
    parse (`pandoc_manual_early_edit` at 0.9x, `full_replace` at 0.2x).
    Guard-cascade bail cost is 15.7% of a full parse; a host-side decline costs
    one refdef scan.
  - Three consequences for the phases below, folded into them: the window-size
    cutoff is promoted out of Phase 8 into its own phase *ahead of the flip*
    (5b), Phase 6's gate grows a regression ceiling and names its cases, and
    Phase 7's exit criterion is restated in the harness's terms.

- [x] Phase 5b: window-size cutoff --- decline in `reparse_ranges` when the
  window start leaves more than a threshold share of the document
  downstream, before the guard cascade and the window parse run. Threshold
  picked from the bench (the crossover is around 85-90% window, where the
  5-10% splice surcharge stops being repaid); a bench case per side of it.
  - Promoted out of Phase 8 and ahead of Phase 6 because the flip is what makes
    the losing shapes the *default*. Today a whole-document replace measures
    0.2x and an early edit in the pandoc manual 0.9x; the cutoff turns both into
    a clean \~1.0x fallback, so the flip ships something that is never worse
    than the status quo. `full_replace` is not an exotic shape: a client that
    answers format-on-save with a whole-document replace takes that path on
    every save.
  - Independent of the region tier --- it compares `reparse_range.0` against a
    fraction of the document length and returns `None`, which is the existing
    refusal-first contract. Phase 8 keeps the cutoff and re-tunes the threshold
    once regions change what a window costs.
  - Landed as `MAX_WINDOW_SHARE_PERCENT = 85` and **two** checks, not one. The
    cheap one runs before the old tree is touched at all: every window this
    entry point can choose starts at or before the edit, so the edit offset is a
    sound upper bound on the window start, and a whole-document replace declines
    there for a tenth of a microsecond (0.2x -> 0.9x). The precise one runs
    after the restart is known, because a single top-level block spanning most
    of the document puts the restart arbitrarily far ahead of the edit.
  - A too-wide *section* window declines the strategy, not the reparse: the
    section anchor is the previous top-level heading, which can sit far earlier
    than the edited block, so the suffix window below it is often narrower and
    still admissible.
  - Bench, before -> after: `full_replace` 0.2x -> 0.9x,
    `pandoc_manual_early_edit` 0.9x -> 1.0x, `multi_change_large_8` 0.9x -> 0.9x
    (see below), `tables_single_edit` / `math_single_edit` /
    `large_authoring_single_edit` 1.0-1.1x -> 1.0x. Nothing that won lost: the
    typing streams and `pandoc_manual_late_edit` are unchanged at 2.8x and 5.4x.
    New cases `window_cutoff_accepted` (79.9% window) and
    `window_cutoff_declined` (87.8%) bracket the threshold on one document.
  - **Phase 6's 0.95x ceiling is not met by three cases, and none of them is a
    wide-window splice.** `bail_refdef_edit` (0.9x) exists to price a decline,
    which is definitionally slower than the full parse it wraps --- the ceiling
    needs the same explicit exemption the fallback-rate threshold already gives
    it. `multi_change_utf16_4` (0.7x) is a 74-byte document against a 1.8 us
    fixed attempt cost; it lost at 0.8x *while splicing successfully* before
    this phase, so it is Phase 7's fixed-overhead problem, not a threshold
    problem. `multi_change_large_8` (0.9x) declines in under a microsecond and
    still costs \~100 us more than a full parse of its 76 KB: that residual is
    host-side per-step work (whole-text `diff_edit` plus the 67 KB `insert` it
    allocates, the refdef-set clone, the base text copy), it is unattributed
    between those, and it wants a profile in Phase 6 rather than a threshold.
  - A document-size floor was tried for the 74-byte case and reverted: it would
    also refuse small documents with *narrow* windows, which do win
    (`single_change_small`, 1.6 KB, 1.3x), and it made every reuse assertion in
    the suite untestable through the production entry point.
  - The cutoff cost the fuzz harness two thirds of its coverage --- its hazard
    snippets are tens of bytes, so nearly every window is a wide one, and the
    share of edits reaching a splice fell from 78% to 23% while every assertion
    still passed. `CostGuards::{Enforced,Ignored}` on a new
    `reparse_with_cost_guards` is the opt-out: the snippets fuzz with the cost
    guard off (the seams they encode occur mid-document in real files, where the
    cutoff admits them), the real-document corpus keeps the production setting,
    and each driver now asserts a floor on its splice rate so a future guard
    cannot silently empty the harness again.

- [x] Phase 6a: mechanize the gate --- the thresholds Phase 6b is gated on were
  printed but never checked, so "the gate passed" was an eyeball judgement.
  `PANACHE_LSP_BENCH_ASSERT=1` now checks every case and exits non-zero on a
  violation; `task bench:incremental-gate` fetches the corpus and runs it.
  - **The fallback-rate criterion was stale and had to be replaced, not
    implemented.** It read "< 20% on every case except the two that price a
    decline", which was written before Phase 5b: the window-size cutoff makes a
    decline the *correct* outcome for a wide-window edit, and ten of the
    eighteen cases now fall back on every step by design. A global rate rule
    cannot express that. Each case instead declares an `Expect` (`Reuse::Always`
    or `Reuse::Never`, plus an optional speedup floor), so the old exemption
    list is gone: the exempted cases are simply the ones that declare `Never`,
    and a new case cannot be added without saying what it is for.
  - Every ratio rule carries an absolute-microsecond escape
    (`MAX_ABSOLUTE_OVERHEAD_US = 20`), because a ratio on a 2 us baseline is
    noise. That retires the by-name exemptions Phase 5b recorded for
    `full_replace` (+0.3 us) and `multi_change_utf16_4` (+1.7 us, 44% bail on a
    3.7 us parse) and puts them on a stated principle, and it lets
    `bail_refdef_edit` (+15.8 us) pass the ceiling without the carve-out the
    roadmap reserved for it. Presence is checked too: the real-document corpus
    is gitignored and `load_document` skips silently, so without it a gate run
    on a fresh checkout passes by not measuring exactly the strictest cases.
  - **`multi_change_large_8`'s \~95 us is profiled, and Phase 5b's guess at it
    was wrong.** It is not host-side per-step work: measured directly on the
    case, `diff_edit` is 7.1 us, the config clone 0.1 us, and the declined
    attempt 0.2 us --- under 8 us of the \~95, and the base text copy the guess
    also named was never inside the timed region at all. The rest is the
    *fallback full parse itself* running \~5% slower on the incremental path
    (1861 us vs 1963 us for the same call on the same text), with the previous
    green tree and the 64 KB edit buffer resident across it. That residual sits
    inside the run-to-run spread of the same parse, which is why the case
    straddles 0.95x rather than failing outright; it carries a documented 0.90
    ceiling naming the profile, and the printed reason keeps the exemption
    visible on every run.
    - A real mis-attribution *was* found and fixed on the way, but it is not
      this one. The bench modelled `refdef_set` as a bare scan and then compared
      whole `RefdefMap`s on the incremental path only, while in production the
      comparison happens inside the query, is charged to both paths, and hands
      back the same `Arc` when the set is unchanged --- which is what makes
      `parsed_document`'s check a pointer compare. `refdef_query` now models the
      backdating. It moves this case by nothing measurable (its synthetic
      document has no reference definitions, so the sets are empty and the
      comparison was free); it matters for refdef-carrying documents, and for
      the harness continuing to mirror the query it claims to mirror.
  - Deferred deliberately: no CI workflow yet. The gate needs
    `benches/documents/download.sh` and a release build, and a timing-assert job
    on shared runners would land flaky next to the flip. The mode and the task
    target are what make wiring it a later one-liner.

- [ ] Phase 6b: default flip --- incremental parsing is **always on**, with no
  new setting. `panache.experimental.incrementalParsing` stays exactly where
  it is and keeps working, but inverts its meaning: absent means on, and the
  only reason to write it is `false`, which turns the side channel off for
  debugging. No `panache.incrementalParsing` key, no alias, no
  `deprecationMessage`, no setting migration --- a second key buys nothing
  when the only value anyone would set is the one the existing key already
  accepts.
  - Work: default the setting to on where it is read
    (`experimental_incremental_parsing_from_initialize` in
    `src/lsp/dispatch.rs`, and the `workspace/configuration` pull and
    `didChangeConfiguration` paths in `src/lsp/handlers/configuration.rs`, which
    share `runtime_incremental_parsing_from_value`); flip the VS Code
    `package.json` default to `true` and reword its description, which currently
    sells it as an unstable experiment rather than a debug switch; update
    `docs/guide/lsp.qmd` (opt-in -> opt-out), `docs/development/lsp.qmd`, and
    the AGENTS.md admission sentence; flip the LSP tests that assert the default
    is off (`tests/lsp/test_incremental_edits.rs`,
    `tests/lsp/test_config_pull.rs`, `tests/lsp/test_config_reload.rs`).
    `PANACHE_INCREMENTAL_PARSING=1|0` keeps overriding the setting in both
    directions and needs no change.
  - The server-side default is only *one* of three:
    `editors/code/src/extension.ts` passes its own hard-coded `false` fallback
    into `initializationOptions`, and `editors/code/README.md` documents
    `default: false`. Both need flipping with `package.json`, or VS Code keeps
    sending an explicit `false` and the server default never governs that client
    at all.
  - `apply_runtime_settings` in `src/lsp/handlers/configuration.rs` applies no
    default: an absent key keeps the current value rather than resetting. That
    is correct and stays, but it means the flip cannot be made there --- only
    the initialize path decides the default.
  - The `experimental.` prefix becomes a misnomer the day this lands. Renaming
    it is deliberately declined: a rename needs precisely the alias and
    migration this phase drops, and the cost of a stale prefix on a debug-only
    switch is lower than the cost of carrying two keys forever. The *internal*
    name (`runtime_settings.experimental_incremental_parsing`) has no wire
    impact and can be renamed freely --- Phase 9 material.
  - Gate: oracle-clean fuzz at 10x iterations; workspace + LSP suite green with
    the flag forced on and off; 1 week oracle-live dogfooding with zero panics;
    and `task bench:incremental-gate` green.
  - **The bench thresholds are no longer restated here.** Phase 6a moved them
    into the cases themselves (`Expect` in `benches/lsp_incremental.rs`), which
    is the only copy: a number in this file could not be checked and drifted
    from the harness within one phase. The floors the roadmap named survive
    verbatim as declarations --- `typing_stream_medium` >= 2x,
    `pandoc_manual_late_edit` and `pandoc_manual_typing_stream` >= 5x --- as
    does the 0.95x regression ceiling and the 20%-of-a-full-parse bail budget.
    Read the gate's output, not this bullet.
    - Both 5x floors measure 5.4-5.7x, run to run. That is a thin margin by
      design: the floors are the roadmap's, and the margin printed on every run
      is what makes drift visible before it fails.
    - Run the gate at the default iteration count. `multi_change_large_8` fails
      at 4 iterations and passes at 80 --- its margin is a few percent on a 1.9
      ms parse, so a shortened run measures the sampling noise, not the feature.

- [ ] Phase 7: token tier --- edit inside plain `TEXT`; newline ban,
  construct-character ban list kept honest by a grammar-grepping test, relex
  kind stability, join probes, error non-touch; char-by-char typing test.
  (The module extraction this phase also carried landed early, in Phase 4's
  S4: `crates/panache-parser/src/parser/reparse.rs`.)
  - Phase 5's typing-stream numbers *raise* this phase's value rather than
    lowering it. The section window already gets streams to 2.7x/5.6x, but
    `pandoc_manual_typing_stream` still costs 1.9 ms per keystroke, because a 7%
    window on a 300 KB document is still 21 KB re-parsed for a one-character
    insert. This tier is O(token) instead of O(document tail) and is the only
    thing that removes that.
  - Exit criterion in the harness's terms: the typing-stream cases must show a
    step change, not an improvement. And the tier has to skip the *fixed*
    overhead too, not only the block parse --- `full_replace` puts a \~10 us
    floor on a 1.6 KB document for machinery that ultimately parsed 27 bytes
    (materializing the root from green, walking the cascade).

- [ ] Phase 8: region tier over top-level `DOCUMENT` children replacing
  section/suffix windows --- symmetric newline-decoupling scans in old and
  new text, `no_straddle` seam primitive, fence/div balance,
  setext/lazy-continuation/list-tightness/HTML-block coupling guards. Fixes
  the suffix-window reparse-to-EOF gap. (The too-wide bail this phase used
  to carry is Phase 5b; re-tune its threshold here.)
  - Phase 5 confirms the premise: window share is the only lever on speedup, and
    the current tier gets 92-98% windows on all three real documents, so it wins
    nothing on early or mid-document edits in real files.
  - The payoff depends on top-level *child* granularity, not on headings.
    `tables_single_edit` edits line 40, the section window fires, and the window
    still starts \~450 bytes in, because that is where the nearest top-level
    heading sits. More headings would not help; regions over `DOCUMENT` children
    are what does.

- [ ] Phase 9: closeout --- architecture docs, dead-path pruning, record
  deferrals.

**Deferred (explicit non-goals):** nested-container regions (inside list items,
blockquotes, divs) are unsound without a context-parameterized fragment-parser
entry point carrying container stack, open fences, and refdef scope --- fatou's
recorded lesson (fatou `TODO.md`, "Incremental" section). Regions stay
restricted to top-level `DOCUMENT` children until that exists. NodePtr
re-anchoring across edits (arity's `map_range_through_edits`) is only needed if
panache starts caching NodePtrs across edits.

## Parser

- [ ] `table_grid_starts_at`'s bare-dash-run branch was dead:
  `parse_single_dash_run` and `try_parse_multiline_separator` accept exactly
  the same lines, so the multiline check above it always won and the
  closing-dash gate its comment advertised never ran. The branch is gone and
  the comment now says what the probe really does (it does not check either
  kind's closer — a caption whose table then fails to parse falls back to a
  paragraph). If the multiline check is ever narrowed, the single-column
  shape needs its own gate back.

- [ ] Simple tables inside a blockquote are **not idempotent**:
  `> A    B\n> --- ---\n> x    y\n` reformats to `>   A B` / `>   x y` on
  pass 2, shifting cells out of their columns. Unrelated to the div bound
  (reproduces on the bare quote, and before the fix too), but it means
  `debug format --checks all` fails on any such document.

### Architecture

- [x] Give a container's **line extent** a single owner, the way the typed frame
  verdict owns its **prefix strip**. The two destructive terminators landed
  on the `ends_container_lines` seam, one commit each with pandoc-verified
  pins; the updated cross-product:

  | terminator                     | pandoc                    | panache                          |
  | ------------------------------ | ------------------------- | -------------------------------- |
  | blank line                     | all containers            | every scan                       |
  | dedent past the content column | after a blank line        | caption probe only               |
  | fenced-div closer              | `notFollowedByDivCloser`  | the two simple-table end scans   |
  | new note marker (`[^x]:`)      | `noteBlock`'s `rawLine`   | the two simple-table end scans   |
  | new list start                 | `listContinuationLine`    | the two simple-table end scans   |
  | HTML closer                    | `notFollowedByHtmlCloser` | the end scans + the quote gobble |

  - **Note marker**: `ContainerPrefix::from_stack` records the outermost
    `FootnoteDefinition`'s op index (`note_marker_op_bound`); a line whose
    verdict fails at or before that op and whose resolved tail parses as a
    marker ends the run. Presence gates it, not ordering --- pandoc's fence
    lives in `noteBlock` only, so inside a plain list item pandoc itself
    collects `[^2]: two` as a table row (pinned), and a marker at the note's
    content column stays note content. No config capture needed: a
    `FootnoteDefinition` on the stack implies the extension.

  - **List start**: `from_stack` captures the marker-detection config bits
    (`ListMarkerDetect`, split out of `try_parse_list_marker`) whenever a
    `ListAdvance` op exists. Three-way split, all pinned: a marker failing the
    item's advance whose outer frame resolves `Inside` with <= 3 leading columns
    ends the run; at the content column it is nested-list content; in between it
    is a lazy continuation.

  Both destructive repros are fixed and pinned in both golden suites
  (`simple_table_in_footnote_stops_at_note_marker`,
  `simple_table_in_list_item_stops_at_sibling_marker`). The remaining cells and
  everything found on the way are the follow-ups below.

- [x] **HTML closer** (`notFollowedByHtmlCloser`): landed on the seam. The
  divergent shape was a line-collected container pushed above the item
  inside markdown-in-html --- `- <div>` + blank + quoted table + `  </div>`
  (the blank is essential: without it the whole item stays in the
  `ListItemBuffer` and the emit-time HTML lift already bounds the span) ---
  where the quoted table swallowed the closer as a sliced row while pandoc
  stops the quote's run.

  The previously sketched param threading turned out impossible for a second
  reason beyond the `Container::HtmlBlock` marker: the interrupting block that
  pushes the container above the item *clears the item's buffer first*
  (`emit_list_item_buffer_if_needed`), so `unclosed_pandoc_matched_pair_tag`
  answers `None` at every dispatch where the terminator matters. What landed
  instead:

  - `ListItemBuffer::clear` folds the open tag into a carried field
    (`carried_unclosed_html_tag`), dropped again when a later chunk's closes
    catch up; `open_matched_pair_tag` reads through it. The dispatcher gate
    stays on the segments-only accessor.

  - `from_stack` reads the buffer straight off the `ListItem` stack entry (it
    already has `config`) --- no param threading. `html_closer_tag` on
    `ContainerPrefix` carries `(tag, content_col)`, ordering-gated exactly like
    the div flag with the tag-holding item in the `FencedDiv` role, so the
    item's own run is never fenced (pandoc slices the closer into the item's own
    table --- pinned).

  - The line test (`tables::html_closer_ends_lines`, OR-ed into
    `ends_container_lines` and consulted by `blockquote_gobble_ends_at` for the
    lazy fold) is a column bound, not a frame resolve: the lazy blockquote
    gobble skips a marker-less line's indent wholesale, which would erase the
    distinction pandoc draws. Probed: the closer fires at up to the item's
    content column of indent (`  </div>` under a 2-column item) and not one
    space more (`   </div>` stays lazy quote content).

  Pinned in `frame_pinning.rs` (gate shape, flush survival, dialect gate, column
  split) and `blocks/tests/tables.rs` (quoted table, column-0 closer, item's own
  table, wrong tag, lazy quote fold), plus golden cases
  `quoted_table_in_list_item_stops_at_html_closer` (parser; the formatter side
  would trip the quoted-table width wobble filed below) and
  `quoted_para_in_list_item_stops_at_html_closer` (both suites). Residual
  divergence is the missing markdown-in-html `Div` lift (panache keeps the tags
  as raw siblings), tracked in the pandoc allowlist 0464 rationale.

- [x] A **lazy block-HTML line folded into a quoted paragraph** stays inline
  text: `> a\n<hr>\n` is `BlockQuote [Plain "a", RawBlock "<hr>"]` in pandoc
  but `BlockQuote [Para ["a", SoftBreak, RawInline "<hr>"]]` here. General
  (no markdown-in-html involved), found while probing the HTML-closer fence:
  the same shape puts a deeper-indented `</div>` (past the closer's column
  bound, so correctly gobbled) inline instead of as the quote's `RawBlock`.

  Landed as an `interrupts_via_html` probe in both lazy gates (paragraph and
  list-item buffer), sharing the dispatcher's `html_block_cannot_interrupt`
  classification: strict-block tags (open and close forms, any indent --- the
  fold drops it) interrupt and demote the paragraph to `Plain`; inline-block
  tags, comments, declarations, CDATA, and unclosed opens stay lazy text;
  CommonMark closes the quote instead (spec 5.1). Two adjacent pre-existing bugs
  surfaced and fixed on the way: the dispatcher's `pandoc_html_open_tag_closes`
  mangled line 0 with the innermost `ListAdvance` (so `- a`/`<hr>` never
  interrupted even unquoted), and the formatter's BLOCK_QUOTE arm dropped the
  `> ` prefix on a direct `PLAIN` child. Pinned in
  `blocks/tests/blockquotes.rs`, pandoc corpus 0525-0527, and golden
  `blockquote_lazy_html_block_interrupt`.

  Design follow-ups from that fix, each bounded:

  - [x] Extract a **shared lazy-interrupt predicate**. Landed as
    `Parser::lazy_interrupts` with a per-gate `LazyInterruptContext`
    (`for_paragraph()`: backtick-anchored fence + div-closer probe;
    `for_list_item()`: any-fence, no div closer), returning per-probe flags
    because the paragraph gate's `[Plain, RawBlock]` follow-up keys on
    `html`/`ends_gobble`. Kept a probe list rather than `detect_prepared`
    dispatch, per the rationale above. The formerly unpinned probe families
    got pandoc corpus pins first: 0528 (lazy div closer), 0529 (byte-0
    backtick fence gobble end), 0530 (lazy heading stays text under default
    pandoc; the positive interrupt needs `-blank_before_header`, which the
    corpus harness can't express --- unit-pinned instead). The footnote-body
    HTML dispatch and definition continuation policy stay separate, with
    cross-reference comments saying why.

  - [x] The definition continuation policy's **HTML probe lacked
    `html_block_cannot_interrupt`** (`definition_plain_can_continue`,
    `parser/utils/continuation.rs`): any `try_parse_html_block_start` hit
    ended the definition PLAIN, so `<!-- c -->` or `<button>` broke a
    definition body even though the same line stays lazy text in a quote.
    Pandoc confirmed the divergence was a bug --- comments, PIs,
    inline-block tags (`<button>`, `<style>`), and void inline-block tags
    (`<embed>`) all stay `RawInline`s in the open PLAIN. The probe now
    routes through the shared `html_block_cannot_interrupt`, gated on
    `plain_open` so a tag with no PLAIN open is still the body's next block
    and lifts to a `RawBlock`. Pandoc corpus 0531-0535 pin the fixed shapes,
    0536 (`<hr>`, strict-block void) and 0537 (leading comment) are the
    controls that must keep lifting.

  - [x] Stopped `ContainerPrefix`'s **`ListAdvance` eating non-whitespace
    bytes**. The blind `advance_columns` is now
    `advance_emitted_marker_columns`, reserved for the one case where the
    skipped bytes are known not to be whitespace: the list marker the core
    already emitted on a marker-line dispatch. Every other `ListAdvance`
    (continuation lines, lookahead, outer sections on line 0, `split`'s
    capture) reads the op as the item's content indent and strips through
    `strip_list_indent`, so `strip` now agrees with the emission-side walk
    on every list advance and the `strip_at` / `peek_prefix_at` divergence
    is gone. Detection scans anchored at column 0 of the dispatch line (grid
    borders, HTML tag balance) get `strip_dispatch_line`, which applies the
    marker rule explicitly instead of relying on the blind walk. Pinned by
    `strip_consumes_only_container_prefix_bytes` (what the strip consumes is
    indent or `>`, never content) plus the flipped `frame_pinning` rows; no
    fixture or conformance case moved.

    Left standing: `strip_content_indent`'s lazy trim still claims a blank
    line's newline (`trim_start` takes `\n`), which is why lookaheads gate
    blanks with `is_blank_line` first. Same conflation, different op --- worth
    closing when a caller is bitten by it.

  - [x] Made the formatter's **BLOCK_QUOTE arm fail closed**. The `_` fallback
    now renders the child to a temp buffer and re-prefixes every line, so a
    block kind with no per-kind arm stays inside the quote instead of
    escaping it. `FENCED_DIV`, `FIGURE`, `FOOTNOTE_DEFINITION`, and
    `REFERENCE_DEFINITION` were all escaping. The save/format/re-prefix
    copy-paste is now `render_to_buffer`, and the nested-quote-aware
    re-prefixing (a nested `BLOCK_QUOTE` derives its own depth, so it must
    not be prefixed twice) is `append_blockquote_prefixed_nested_block`,
    shared with the `ALERT` arm. `BLOCK_QUOTE` got an explicit arm for that
    reason.

    Fixing the escape exposed two parser divergences behind it, both now fixed
    and pinned by pandoc corpus 0538/0539: a blockquote could not open as the
    first child of a fenced div nested in a quote (`can_start_blockquote`'s div
    hatch existed only at depth 0 --- `opens_fenced_div_at_depth` now strips the
    enclosing markers first), and the closing `> :::` folded into the nested
    quote as lazy text instead of closing the div (`quoted_div_closes_at` probes
    the reduced-marker form when the div was opened inside a quote the line
    still carries).

- [ ] Probe findings from the seam migration, **untriaged candidates** (each
  needs confirming against existing fixtures/allowlists before being called
  a bug):

  - A pipe table starting on a list item's marker line (`- a | b` / `  ---|---`
    / `  c | d`) parses as a `Plain` here but as `BulletList [Table]` in pandoc;
    same for a footnote body whose marker line is bare (`[^1]:` + indented pipe
    table).

  - A pipe table claims a `[^1]: a | b` dispatch line as its header row where
    pandoc reads the note first (`Note [Table]`) --- registry precedence, not a
    scan bound.

  - Pandoc accepts a two-dash pipe delimiter row (`--|--`); panache requires
    three.

  - A spaced dash run after a list (`- x` items, then `- - - -`) is a sibling
    `HorizontalRule` in pandoc but nests as the list's child here, and the
    pandoc-ast projector then drops it from the projection entirely (CST is
    lossless).

- [ ] Terminator-adjacent **headered simple tables should degrade to a
  paragraph** the way pandoc's footer rule makes them: when a contiguous
  terminator (div closer, note marker, list start) ends the run, the
  collected raw has no trailing blank line, pandoc's `simpleTable` footer
  (`blanklines`) fails, and the block reparses as a paragraph.
  `find_table_end` treats the terminator like a blank line instead, so
  panache keeps the Table (lossless and idempotent; pinned with divergence
  comments in `blocks/tests/tables.rs`). Applies to the shipped div closer
  too when it ends a *list item's* run --- the existing div pins only cover
  blockquote/definition containers, whose reparse appends the blank.

- [ ] The list-start fence stops at pandoc's plain `nonindentSpaces` (<= 3
  columns in the outer frame). For a **nested list with a positive base
  indent**, pandoc's tolerance is base+3 in the outer list's frame --- a
  marker in the base+1..base+3 band (e.g. 4-5 columns under a base-2 inner
  list) terminates there but stays collected-as-content here. Honoring it
  today trips a formatter reindent cascade that destroys the marker on the
  second pass (`- x` at 5 columns reflows to 4, then loses its marker), so
  the band is deliberately excluded (`list_start_detect` doc comment). Fix
  the downstream cascade first, then widen the bound to
  `List::base_indent_cols + 3`.

- [ ] A **simple table in a blockquote in a footnote body is not lossless**:
  `[^1]: body\n\n    > A    B\n    > --- ---\n    > x    y\n` parses with
  four duplicated indent columns on the table's dispatch line
  (`    > A    B` re-emits as `    >     A    B`), so
  `debug format --checks losslessness` fails before any formatting question
  arises. Pre-existing (reproduces on the commit before the note-marker
  fence); the note-marker golden pins had to route around it via a
  list-in-footnote shape instead.

- [ ] Simple-table rows holding a **sliced multi-space cell** are not idempotent
  even where the slicing matches pandoc: `- <div>` + indented table +
  `</div>` collects the closer as a `</di` / `v>` row (as pandoc does), but
  pass 1 renders `A       B` / `</di    v>` and pass 2 re-measures to
  `A      B` / `</di   v>`. Same family as the "simple tables inside a
  blockquote are not idempotent" entry above, but reproduces without a
  blockquote.

- [ ] Stop letting `pandoc_ast.rs` drift into a second-stage parser. Load-
  bearing byte-walkers (`split_html_block_by_tags`, `parse_pandoc_blocks`
  and the refs/heading-id reparse helpers) re-tokenize source the CST should
  already encode. This violates the single-pass invariant in `AGENTS.md` and
  hides structural decisions from downstream consumers (linter, salsa, LSP,
  formatter) which all walk the CST, not the projector. The guiding
  principle: when the parser computes a structural fact during its single
  pass, it must emit that fact into the CST (wrapping existing source bytes,
  `HTML_ATTRS`-style --- never synthetic tokens) instead of forcing the
  projector to recompute it. Each bucket below is its own bounded step,
  verified against pandoc-native + CommonMark (both must stay byte-identical
  or improve).

## Parser - Coverage

This section tracks implementation status of Pandoc Markdown features based on
the spec files in `assets/pandoc-spec/`.

**Focus**: Prioritize **default Pandoc extensions**. Non-default extensions are
lower priority and may be deferred until after core formatting features are
implemented.

### Block-Level Elements

### Paragraphs ✅

- [x] Basic paragraphs
- [x] Paragraph wrapping/reflow
- [x] Extension: `escaped_line_breaks` (backslash at line end)

### Headings ✅

- [x] ATX-style headings (`# Heading`)
- [x] Setext-style headings (underlined with `===` or `---`)
- [x] Heading identifier attributes (`# Heading {#id}`)
- [x] Extension: `blank_before_header` - Require blank line before headings
  (default behavior)
- [x] Extension: `header_attributes` - Full attribute syntax
  `{#id .class key=value}`
- [x] Extension: `implicit_header_references` - Auto-generate reference links

### Block Quotations ✅

- [x] Basic block quotes (`> text`)
- [x] Nested block quotes (`> > nested`)
- [x] Block quotes with paragraphs
- [x] Extension: `blank_before_blockquote` - Require blank before quote (default
  behavior)
- [x] Block quotes containing lists
- [x] Block quotes containing code blocks

### Lists 🚧

- [x] Bullet lists (`-`, `+`, `*`)
- [x] Ordered lists (`1.`, `2.`, etc.)
- [x] Nested lists
- [x] List item continuation
- [x] Complex nested mixed lists
- [x] Extension: `fancy_lists` - Roman numerals, letters `(a)`, `A)`, etc.
- [ ] Extension: `startnum` - Start ordered lists at arbitrary number (low
  priority, if we even should support this)
- [x] Extension: `example_lists` - Example lists with `(@)` markers
- [x] Extension: `task_lists` - GitHub-style `- [ ]` and `- [x]`
- [x] Extension: `definition_lists` - Term/definition syntax

### Code Blocks

- [x] Fenced code blocks (backticks and tildes)
- [x] Code block attributes (language, etc.)
- [x] Indented code blocks (4-space indent)
- [x] Extension: `fenced_code_attributes` - `{.language #id}`
- [x] Extension: `backtick_code_blocks` - Backtick-only fences
- [x] Extension: `inline_code_attributes` - Attributes on inline code

### Horizontal Rules

- [x] Basic horizontal rules (`---`, `***`, `___`)

### Fenced Divs

- [x] Basic fenced divs (`::: {.class}`)
- [x] Nested fenced divs
- [x] Colon count normalization based on nesting
- [x] Proper formatting with attribute preservation
- [x] Top-level indented lone `:::` diverges from pandoc. Panache accepted up to
  3 leading spaces on a closing fence, so `::: outer\ntext\n  :::` closed
  the div; pandoc instead treats the indented `:::` as paragraph text and
  leaves the div implicitly closed at EOF (`Str ":::"`). Fixed by tracking
  the opener's indent on `Container::FencedDiv` and rejecting a closer more
  indented than its opener in `FencedDivCloseParser` (scoped to the no-list
  frame so the #439 in-list handling is untouched). Surfaced 2026-07-28
  while fixing #439.
- [ ] Nested fenced divs inside a list item are mis-parsed: the outer div is
  left unclosed and its trailing `:::` becomes stray text, which surfaces as
  a `stray-fenced-div-markers` lint false positive (e.g.
  `docs/authoring/markdown-basics.qmd`). Minimal repro: a `- -` nested list
  whose inner item opens a `pad` div, contains a fully closed `light` div,
  then a trailing `:::` to close `pad`. Pandoc closes both divs
  (`Div .pad [ Div .light ]`); panache leaves `pad` open and strays the
  closer. Surfaced 2026-08 in the quarto-web triage.

### Tables

- [x] Extension: `simple_tables` - Simple table syntax (parsing complete,
  formatting deferred)
- [x] Extension: `table_captions` - Table captions (both before and after
  tables)
- [x] Extension: `pipe_tables` - GitHub/PHP Markdown tables (all alignments,
  orgtbl variant)
- [x] Extension: `multiline_tables` - Multiline cell content (parsing complete,
  formatting deferred)
- [x] Extension: `grid_tables` - Grid-style tables (parsing complete, formatting
  deferred)

### Line Blocks

- [x] Extension: `line_blocks` - Poetry/verse with `|` prefix

### Inline Elements

#### Emphasis & Formatting

- [x] `*italic*` and `_italic_`
- [x] `**bold**` and `__bold__`
- [x] Nested emphasis (e.g., `***bold italic***`)
- [x] Overlapping and adjacent emphasis handling
- [x] Extension: `intraword_underscores` - `snake_case` handling
- [x] Extension: `strikeout` - `~~strikethrough~~`
- [x] Extension: `superscript` - `^super^`
- [x] Extension: `subscript` - `~sub~`
- [x] Extension: `bracketed_spans` - Small caps `[text]{.smallcaps}`, underline
  `[text]{.underline}`, etc.

#### Code & Verbatim

- [x] Inline code (`code`)
- [x] Multi-backtick code spans (\`\`\`\`\`)
- [x] Code spans containing backticks
- [x] Proper whitespace preservation in code spans
- [x] Fenced code blocks (\`\`\` and \~\~\~)
- [x] Indented code blocks

#### Links

- [x] Inline links `[text](url)`
- [x] Automatic links `<http://example.com>`
- [x] Nested inline elements in link text (code, emphasis, math)
- [x] Reference links `[text][ref]`
- [x] Extension: `shortcut_reference_links` - `[ref]` without second `[]`
- [x] Extension: `link_attributes` - `[text](url){.class}`
- [x] Extension: `implicit_header_references` - `[Heading Name]` links to header

#### Images

- [x] Inline images `![alt](url)`
- [x] Nested inline elements in alt text (code, emphasis, math)
- [x] Reference images `![alt][ref]`
- [x] Image attributes `![alt](url){#id .class key=value}`
- [x] Extension: `implicit_figures`

#### Math

- [x] Inline math `$x = y$`
- [x] Display math `$$equation$$`
- [x] Multi-dollar math spans (e.g., `$$$ $$ $$$`)
- [x] Math containing special characters
- [x] Extension: `tex_math_dollars` - Dollar-delimited math

#### Footnotes

- [x] Inline footnotes `^[note text]`
- [x] Reference footnotes `[^1]` with definition block
- [x] Extension: `inline_notes` - Inline note syntax
- [x] Extension: `footnotes` - Reference-style footnotes

#### Citations

- [x] Extension: `citations` - `[@cite]` and `@cite` syntax with complex key
  support

- [x] Pandoc `notAfterString` for bare `@key`: a citation glued to a preceding
  word character is literal text, not a citation (`word@key`,
  `user@example.com`, `違法編訂@jzkhl`). Handled at the shared detection
  site via a char-before-`@` check (alphanumeric or `.` suppresses); the
  `-@` suppress-author form is exempt. Backs the `unspaced-citation` lint
  rule. Closes #448.

- [x] `notAfterString` delimiter-adjacent corner: a bare `@key` glued to a
  *resolved closing emphasis/strong delimiter* is now suppressed to match
  pandoc (`*em*@key` and `**strong**@key` are `Emph`/`Strong` +
  `Str "@key"`). The IR consults the emphasis pass's result when building
  the construct plan (`demote_bare_citation_after_emphasis_closer`), keying
  off resolved closers only, so `*@key*` (opener) and `*em*-@key`
  (suppress-author) keep the citation. No extra scan: the correction reads
  already-computed delimiter state rather than re-classifying.

- [ ] `unspaced-citation` covers citations only. A crossref glued to a word
  (`x@fig-plot`) is likewise left as text by the parser but not flagged by
  the rule; extend it to crossref keys (gated on `quarto_crossrefs`) as a
  follow-up.

#### Spans

- [x] Extension: `bracketed_spans` - `[text]{.class}` inline
- [x] Extension: `native_spans` - HTML `<span>` elements with markdown content

### Metadata & Front Matter

#### Metadata Blocks

- [x] Extension: `yaml_metadata_block` - YAML frontmatter
- [x] Extension: `pandoc_title_block` - Title/author/date at top

### Raw Content & Special Syntax

#### Raw HTML

- [x] Extension: `raw_html` - Inline and block HTML
- [x] Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks

#### Raw LaTeX

- [x] Extension: `raw_tex` - Inline LaTeX commands (`\cite{ref}`,
  `\textbf{text}`, etc.)
- [x] Extension: `raw_tex` - Block LaTeX environments
  (`\begin{tabular}...\end{tabular}`)
- [x] Extension: `latex_macros` - Expand LaTeX macros (conversion feature, not
  formatting concern)

#### Other Raw

- [x] Extension: `raw_attribute` - Generic raw blocks `{=format}`

### Escapes & Special Characters

#### Backslash Escapes

- [x] Extension: `all_symbols_escapable` - Backslash escapes any symbol
- [x] Extension: `angle_brackets_escapable` - Escape `<` and `>`
- [x] Escape sequences in inline elements (emphasis, code, math)

#### Line Breaks

- [x] Extension: `escaped_line_breaks` - Backslash at line end = `<br>`

### Non-Default Extensions (Future Consideration)

These extensions are **not enabled by default** in Pandoc and are lower priority
for initial implementation.

#### Non-Default: Emphasis & Formatting

- [x] Extension: `mark` - `==highlighted==` text (non-default)

#### Non-Default: Links

- [x] Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- [x] Extension: `mmd_link_attributes` - MultiMarkdown link attributes
  (non-default)

#### Non-Default: Math

- [x] Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default,
  enabled for RMarkdown)
- [x] Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
  (non-default)
- [x] Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

#### Non-Default: Metadata

- [x] Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

#### Non-Default: Headings

- [x] Extension: `mmd_header_identifiers` - MultiMarkdown style IDs
  (non-default)

#### Non-Default: Lists

- [x] Extension behavior: lists can start without a preceding blank line
  (non-default compatibility behavior).
- [x] Add explicit extension-gated handling/config semantics for
  `lists_without_preceding_blankline`.
- [x] Extension behavior: four-space list indentation rules are supported in
  compatibility mode.
- [x] Add explicit extension-gated handling/config semantics for
  `four_space_rule`.

#### Non-Default: Line Breaks

- [x] Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- [ ] Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- [x] Extension: `east_asian_line_breaks` - Smart line breaks for CJK
  (non-default)

#### Non-Default: GitHub/CommonMark

- [x] Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- [x] Extension: `emoji` - `:emoji:` syntax (non-default)
- [x] Extension: `wikilinks_title_after_pipe` - `[[url|title]]` (opt-in; no
  flavor default)
- [x] Extension: `wikilinks_title_before_pipe` - `[[title|url]]` (opt-in; no
  flavor default)

#### Non-Default: Quarto-Specific

- [x] Quarto executable code cells with output
- [x] Quarto cross-references `@fig-id`, `@tbl-id`

#### Non-Default: RMarkdown-Specific

- [x] RMarkdown code chunks with output
- [x] Bookdown-style references (`\@ref(fig-id)`, etc.)

#### Non-Default: Other

- [ ] Extension: `abbreviations` - Abbreviation definitions (non-default)
- [ ] Extension: `attributes` - Universal attribute syntax (non-default,
  commonmark only)
- [ ] Extension: `gutenberg` - Project Gutenberg conventions (non-default)
- [ ] Extension: `markdown_attribute` - `markdown="1"` in HTML (non-default)
- [ ] Extension: `old_dashes` - Old-style em/en dash parsing (non-default)
- [ ] Extension: `rebase_relative_paths` - Rebase relative paths (non-default)
- [ ] Extension: `short_subsuperscripts` - MultiMarkdown `x^2` style
  (non-default)
- [ ] Extension: `sourcepos` - Include source position info (non-default)
- [ ] Extension: `space_in_atx_header` - Allow no space after `#` (non-default)
- [x] Extension: `spaced_reference_links` - Allow space in `[ref] [def]`
  (non-default)

### Won't Implement

- Format-specific output conventions (e.g., `gutenberg` for plain text output)

### Quarto Shortcodes

- [x] Parser support for `{{< name args >}}` syntax

- [x] Parser support for `{{{< name args >}}}` escape syntax

- [x] Formatter with normalized spacing

- [x] Extension flag `quarto_shortcodes` (enabled for Quarto flavor)

- [x] Golden test coverage

- [x] LSP diagnostics for malformed shortcodes

- [x] Completion for built-in shortcode names

## Additional Markdown flavors

### mdsvex / Svelte-flavored Markdown

MVP support for [mdsvex](https://mdsvex.pngwn.io) (`.svx`, `.svelte.md`). mdsvex
(≤0.12.x) builds on `remark-parse@8`, whose options default to `gfm: true`, so
tables, strikethrough, bare autolinks, and task lists work with **no plugins**
(confirmed by the getting-started example and real plugin-free
`svelte.config.js` setups; `remark-gfm` is only for modern remark). So
`Flavor::Mdsvex` is a CommonMark-*dialect* flavor with the GFM extension set +
`raw_html` + `yaml_metadata_block` + `svelte-template`, minus the extras mdsvex
does not enable by default (footnotes, math, emoji, alerts). The `{...}`
attribute "collision" with Pandoc syntax evaporates because the CommonMark
dialect leaves every attribute extension (`header_attributes`,
`bracketed_spans`, `fenced_divs`, `raw_attribute`) off, so `{` is free for
Svelte. `svelte-template` is off for every other flavor (zero behavior change
elsewhere).

- [x] MVP: `Flavor::Mdsvex` + `svelte-template` extension; `.svx`/`.svelte.md`
  detection; CLI/WASM/schema surfaces.

- [x] Opaque, sigil-distinguished inline spans (`SVELTE_BLOCK_LOGIC` for
  `{#…}`/`{:…}`/`{/…}`, `SVELTE_TAG` for `{@…}`, `SVELTE_EXPRESSION` for
  `{expr}`), content preserved verbatim. Balanced-brace scan reused from the
  shortcode parser. Parser golden + formatter golden + unit tests landed.

- [x] **Tier 2: block-level `{#if}`/`{#each}` pairing.** Standalone Svelte spans
  (block logic `{#if}`/`{:else}`/`{/each}`, tags `{@html}`, and expressions
  `{expr}`) that occupy a whole line are now emitted as an opaque
  `SVELTE_BLOCK` leaf block (mirroring the MyST leaf-block pattern) that
  acts as a block boundary. This fixes the prior quirk where a lone-span
  paragraph adjacent to a *tight* list (no blank line) got joined onto one
  line and its inner whitespace collapsed. The equivalent quirk for Quarto
  shortcode lines (`{{< ... >}}`) is still a separate pre-existing issue and
  is not addressed here.

- [ ] **Tier 3: format the JS/Svelte inside spans** (prettier-plugin-svelte
  territory). Likely out of scope.

- [ ] String-literal-aware brace matching: a `}` inside a JS string (`{ "}" }`)
  can terminate a span early (depth-counting only). Lossless fallback
  (literal `{`), but a real Svelte tokenizer would fix it.

- [ ] AST wrappers (`syntax/svelte.rs`), LSP semantic tokens, and lint rules for
  Svelte constructs.

### MyST

MyST (`mystmd.org`, `myst-parser`) support, modeled the same way as mdsvex: a
CommonMark-*dialect* flavor whose `myst_defaults` enables MyST-specific
extensions (`myst-directives`, `myst-roles`, `myst-targets`, `myst-comments`,
`myst-block-breaks`) plus the GFM-superset rules `myst-parser` turns on
(`pipe-tables`, `footnotes`, `yaml-metadata-block`). Behavior is gated on those
extension flags, never on `Flavor::Myst` directly, so other flavors can borrow
the same shapes. Markup extras (`myst-colon-fence`, `myst-substitutions`,
dollar-math, deflists, ...) stay opt-in.

- [x] **AST wrappers (`syntax/myst.rs`).** Typed wrappers over the existing
  `MYST_*` CST kinds, wired through `syntax.rs`, each with cast-from-`parse`
  unit tests (follow the `syntax/shortcodes.rs` pattern). Landed:

  - `MystTarget` (`label()`) --- the anchor side of MyST's cross-reference
    graph; keystone for goto-def/rename/undefined-target lint.

  - `MystRole` (`name()` brace-stripped, `content()`) --- the reference side
    (`` {ref}`label` ``); pairs with `MystTarget` for reference resolution.

  - `MystDirective` (`name()`, `argument()`, `options()` over
    `MystDirectiveOption` `name()`/`value()`, `body()`) --- richest construct;
    unlocks the most lint rules.

  - `MystSubstitution` (`name()`, trimmed) --- enables the "key not defined in
    frontmatter `substitutions:`" lint.

  - Skipped `MystComment`/`MystBlockBreak` wrappers (no name/label semantics to
    expose yet); add when a rule needs them.

- [ ] **LSP semantic tokens for MyST.** Wrapper-driven classification of
  directive/role names, target labels, and substitution names. Depends on
  the AST wrappers.

- [ ] **Lint rules for MyST constructs.** Gate on the `myst-*` extension flags
  (never `Flavor::Myst` directly), via the `add-lint-rule` skill. Start with
  `undefined-references` (role target resolves to a `MystTarget`) and an
  unknown-directive/role check. Depends on the AST wrappers.

## Math Parser and Formatter

Multi-session effort --- see the `math-parser-formatter` skill
(`.claude/skills/math-parser-formatter/`) for the phased roadmap, locked-in
design decisions, and per-session workflow. Parser invariants:
`.claude/rules/math-parser.md`.

- [x] Math parser producing a lossless structural TeX CST for inline and display
  math (`MATH_CONTENT` subtree; groups, environments, commands, alignment,
  scripts, comments, and `\left`/`\right` delimiter pairs). Landed in
  `crates/panache-parser/src/parser/math.rs`.

- [x] Surface math diagnostics (unclosed/mismatched braces and environments,
  unbalanced `\left`/`\right`) through the linter and LSP. Landed as the
  always-on `math-syntax` lint rule (`src/linter/rules/math_content.rs`),
  surfaced via the registry to CLI + LSP. All diagnostics derive from the
  embedded `MATH_CONTENT` CST shape via the single shared
  `syntax::math_diagnostics` (no re-parse, no side-channel; also consumed by
  the formatter to leave malformed math verbatim); spans are the offending
  tokens' host ranges.

- [ ] Migrate the math formatter's `\left`/`\right` line-break tracking to the
  `MATH_DELIMITED` node. The break-candidate scan
  (`crates/panache-formatter/src/formatter/math/linebreak.rs`) and
  `command_class` (`operators.rs`) still track delimiter depth by command
  *text* (`name == "left"`/`"right"`), which is now partly redundant with
  the structural node. Harmless as a fallback today (formatter goldens are
  byte-identical), but node-awareness would let the scan treat a delimited
  run as one opaque operand instead of re-deriving depth.

- [x] Math formatter that reformats content semantics-safely (align `&` columns,
  indent environment bodies, normalize `\\`) while preserving idempotency
  (`format(format(math)) == format(math)`), behind an experimental gate.
  Landed as `[experimental] format-math` (default off) routing
  `$$`/`$`/`\[`/`\(` math content through
  `crates/panache-formatter/src/formatter/math/`. Standalone `\begin{env}`
  TeX blocks stay opaque (parser keeps them as `TEX_BLOCK`) --- a possible
  follow-up.
