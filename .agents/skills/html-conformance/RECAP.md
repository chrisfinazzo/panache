# HTML conformance — running session recap

Rolling, terse handoff between `html-conformance` sessions. Read the
Persistent traps + Phase status + Latest session's "next sub-targets"
at start. At end: **rewrite** the Latest session entry, demote the old
one to the Earlier log, fold still-relevant traps into Persistent. Keep
≤ 400 lines (see `SKILL.md`).

--------------------------------------------------------------------------------

## Persistent traps & invariants (cross-session)

"You'd warn a future session" knowledge. Function/gate names are
load-bearing; the code holds the full detail.

### Disk + tooling

- **Disk lint cache `~/.cache/panache/`** serves stale linter results
  after `cargo build`. Fix with `cargo run -- clean --all`, or disable the cache.
  Validate via unit tests first.
- **Conformance compare is whitespace-insensitive** (`normalize_native`
  collapses to one line) — visual diffs mislead. Check the oracle version:
  the shell has Pandoc 3.11, but the corpus pins 3.10.2 (available in the
  Nix store). Generate new expectations with the pinned binary.
- **Config walks up from the INPUT FILE's dir, not CWD.** A stray
  `/tmp/panache.toml` (`flavor="myst"`, CommonMark → no `<div>` lift)
  shadows test files under `/tmp/…`, faking `undefined-anchor` on `<div
  id>`. NOT a bug — reproduce anchor cases under the repo's `target/`.

### Parser shape & losslessness

- **HTML_ATTRS is the structural attr pattern; never add synthetic
  tokens.** Tokenize existing bytes (`TEXT + WS + HTML_ATTRS{TEXT} +
  TEXT`); use source-byte slices (`&rest[..4]`), never literals, for
  case-insensitive prefix matches.
- **Same-line `<div>foo</div>` is ONE `HTML_BLOCK_TAG`** — close lives
  in a TEXT child; scan to first **unquoted** `>` (naive
  `strip_suffix('>')` grabs the close's). Quote-aware scanners thread
  state across lines (`count_tag_balance`, `find_multiline_open_end`,
  `pandoc_html_open_tag_closes`). **`<div/>` DOES bump depth** in
  Pandoc; keep `count_tag_balance` and `matched_close_offset` consistent.
  Other slash-suffixed tags retain their existing handling.
- **`input.lines()` strips newlines** — losslessness tests use
  `split_lines_inclusive`.
- **A new wrapper retag (`HTML_BLOCK_RAW`/`_DIV`/…) must reach EVERY
  consumer of the old kind** or the block mis-formats/drops. Grep the
  old kind across `crates/` + `src/`: formatter arms (`core.rs`,
  `lists.rs`, `utils.rs`), both `directives.rs` copies, list-item lift
  gate (`list_item_buffer.rs`), LSP `folding_ranges.rs`, linter
  `html_entities.rs`. Retag fires under `Dialect::Pandoc` (Quarto/RMd
  see it too).
- **Baked multi-tag TEXT vs structural split.** The parser bakes
  consecutive standalone tags on one line into a SINGLE `HTML_BLOCK_TAG`
  TEXT (`</p></div>`); 7b's `try_parse_standalone_block_tags_split` emits
  one tag each (top-level + bq, via `strip_line_0_for_emission`), so the
  projector predicate `html_block_is_standalone_tag_sequence` (≥ 2
  `HTML_BLOCK_TAG`, no `HTML_BLOCK_CONTENT`) is safe. Don't loosen it to
  single-tag (would merge baked-multi); single tags + multi-line stay
  byte-walker.
- **Void strict-block tags (`col`, `hr`, `meta`) close on the open
  line.** In `PANDOC_BLOCK_TAGS` (strict: always split, DO interrupt)
  but no close form → `PANDOC_VOID_STRICT_BLOCK_TAGS` emits
  `closes_at_open_tag:true`/`depth_aware:false`, so `<hr>\n<hr>` is
  siblings not nested. Excluded from `is_pandoc_lift_eligible_block_tag`
  + `is_pandoc_matched_pair_tag`; NOT in the dispatcher `cannot_interrupt`
  void set (distinct from `PANDOC_VOID_BLOCK_TAGS` =
  `area`/`embed`/`source`/`track`, which DON'T interrupt). `<hr id>`
  stays opaque (pandoc lifts no anchor). Don't add col/hr/meta to
  `PANDOC_VOID_BLOCK_TAGS`.

### Pandoc tag categorization

- **THREE tag sets**: strict block (`PANDOC_BLOCK_TAGS`, always splits),
  inline-block non-void (`PANDOC_INLINE_BLOCK_TAGS`), inline-block void
  (`PANDOC_VOID_BLOCK_TAGS`). Non-void/void follow `inline_pending` +
  matched-pair/single lift. Source: pandoc `TagCategories.hs` +
  `Readers/HTML.hs` (`isBlockTag`/`isInlineTag`). CM and Pandoc lists
  differ ~15 tags both directions — don't merge; parser gates on
  `is_commonmark`, projector runs Pandoc only.
- **`eitherBlockOrInline` is context-dependent** — needs parser-side
  `cannot_interrupt` (don't break a running paragraph) AND projector-side
  `inline_pending` (don't split mid-text).
- **pandoc `isInlineTag` special cases (#10643):** `<style>` o+c,
  `</script>`, PIs, comments, math-`<script>` (ci, single-line) cannot
  interrupt a paragraph; `<pre>`/non-math `<script>`/`<textarea>` DO.
  Lives in `detect_prepared`'s `cannot_interrupt`; needs `is_closing`
  + `is_pandoc_lift_eligible_block_tag`. Indented `isInlineTag` demotes
  to `Para [RawInline]` (`detect_prepared` returns `None` when
  `leading_spaces(ctx.content) > content_col`; `ctx.content` keeps
  list-item indent but bq markers ARE stripped).
- **`HtmlBlockType::BlockTag.is_closing` — identity-pivoting guards MUST
  check it.** `pandoc_html_open_tag_closes` is true for both `<div>` and
  `</div>`; a bare `</div>` keeps opaque `HTML_BLOCK` → single RawBlock.
  Closing forms of matched-pair sets ARE block starts
  (`closes_at_open_tag:true`). `HtmlBlockType::BlockTag` is
  `Box<dyn Any>`-roundtripped — adding a field auto-works (E0063 flags
  literal sites).
- **Block-level tags mid-paragraph force a boundary in pandoc; panache
  inlines them.** Same-line inter-tag text between NON-DIV matched-pair
  strict-block tags (`<p>a</p> b <p>c</p>`, 0472/0475-0477) is FIXED via
  `same_line_trailing_forces_opaque` (keep line opaque → projector
  `split_html_block_by_tags` does the flat RawBlock/Plain split). Still
  divergent: paragraph-LEADS-tag (`foo <p>bar</p>`, broad inline-parser
  boundary — deferred).

### Projector tag splitting

- **`split_html_block_by_tags` walks BYTES, opaque HTML_BLOCKs only**
  (comments, PI, verbatim, void, unmatched strict/inline-block). Matched
  `<div>`/strict-block/inline-block are parser-lifted now.
  Context-tracked via `inline_pending` (resets on ≥ 2 newlines;
  inter-tag text demotes `Para`→`Plain` when butted, tail does not — use
  `flush_html_block_text` vs `_tail_text`). Inline-block open with no
  matched close emits RawBlock (falling to `inline_pending=true` stack-
  overflows via tail reparse).
- **Bq-wrapped opaque HTML needs `collect_html_block_text_skip_bq_markers`
  / `walk_skip_bq_markers`** — the parser keeps `BLOCK_QUOTE_MARKER + WS`
  as tokens; `node.text()` re-recognizes `> ` as nested bq.
  `walk_skip_bq_markers` also strips leading line-start `WHITESPACE`
  (list-item indent re-injection); threads `skip_next_ws` +
  `at_line_start`.
- **`open_tag_raw_block_text` canonicalizes multi-line opens** (walk
  `children_with_tokens`, take `<tagname` TEXT, join HTML_ATTRS trimmed
  with single spaces, append `>`) and strips bq markers + leading 1-3
  space indent for single-line opens. Pandoc RawBlock text = tag bytes
  only.
- **Architectural smell**: `pandoc_ast.rs` is the public
  `to_pandoc_ast`; linter/salsa/LSP/formatter walk the CST, not the
  projector, so byte-walking there shrinks over time. Retag mechanism
  (7a/7c/`HTML_BLOCK_DIV` precedent): `wrapper_kind` stays `HTML_BLOCK`
  (gates + child tokens byte-identical), only the node kind flips at the
  two `start_node` sites via `html_block_node_kind`. Load-bearing
  byte-walker remainder: `split_html_block_by_tags`, `parse_pandoc_blocks`
  (inter-tag reparse), `collect_html_block_text_skip_bq_markers`,
  table-cell reparses.

### Refs / footnotes / heading-id resolution

- **Recursive reparse uses `parse_with_refdefs`, not `parse`** (`parse`
  re-runs `populate_refdef_labels` on inner text, hiding outer refdefs).
  `parse_pandoc_blocks` swaps in an inner `RefsCtx` (swap belongs IN it);
  `build_refs_ctx` mutates `REFS_CTX` mid-build — `mem::take` outer FIRST.
  `heading_id_by_offset` is offset-keyed (don't copy outer `heading_ids`
  in); inherit cross-boundary refs via `build_refs_ctx_inherited`.
  `fenced_div` walks via `collect_block`, not `parse_pandoc_blocks`.
- **`AttributeNode::can_cast` accepts `HTML_ATTRS`** — the salsa walk
  picks up `<div id>`/`<span id>`/`<section id>` automatically (no
  parallel walk). Diverges from pandoc-native (RawBlock lifts no attr)
  but matches anchor-link intent. **THREE readers see those values; a
  semantics change must hit all.**
  `AttributeNode::{id,classes,key_values}` prefer structured `ATTR_*`
  tokens, falling back to `reparse()` → `parse_html_attribute_list`;
  `pandoc_ast::attr_from_html_attrs_node` walks the same tokens.
  `HTML_ATTRS` DOES emit structured children (`emit_html_attrs_node`), so
  patching only the reparse helper misses the live path (symptom:
  projector right, salsa/linter still raw). Corollary: `id_value_range`
  must stay SOURCE-byte measured — deriving a length from `id()` breaks
  once decoded (`a&amp;b` = 7 source bytes, 3 semantic).

### Structural lift (the lift family)

- **Lifted `HTML_BLOCK[_DIV]` MUST route structural, not byte.**
  `collect_block`→`html_div_block`; `emit_html_block`→
  `emit_html_block_structural` (NOT `split_html_block_by_tags`, whose
  `parse_pandoc_blocks` builds a fresh `RefsCtx` → stray `-1` auto-id).
  Signal: no `HTML_BLOCK_CONTENT` child. `html_div_block` `debug_assert!`s
  when `HTML_BLOCK_DIV` lacks a structural inner shape — prefer "fall
  back to opaque `HTML_BLOCK`" over a one-child `HTML_BLOCK_DIV`.
  `div_has_structural_inner` accepts a missing close (implicit-EOF
  `Div`): 1 clean open tag + structural body + no `HTML_BLOCK_CONTENT`.
- **`try_emit_html_block_lift` (in `list_item_buffer.rs`) is the shared
  lift**: reparse dedented → validate single/`≥2`-tag/2-child-trailing →
  graft with per-line prefix re-injection. Args: `content_col` (strip
  list indent from lines 1+), `use_paragraph` (loose→`Para`,
  tight→`Plain`; only the 2-child trailing split), `line0_prefix`
  (injected INSIDE the open tag — a sibling `WHITESPACE` under
  `DEFINITION` is dropped by the formatter, escaping the div),
  `allow_unclosed_div` (single open tag + structural body, no
  `HTML_BLOCK_CONTENT`). **Core extracted**:
  `emit_html_block_lift_from_stripped(builder, parse_text, config,
  Vec<ContainerPrefixLine>, use_paragraph, allow_unclosed_div)` takes
  caller-pre-stripped text + explicit prefixes.
- **Prefix capture for bq/content-indent stacks**: `ContainerPrefix`
  strips (bq markers outermost via `from_scalars(bq,0,true,ci,false)`,
  then content indent); `strip` returns a suffix, so capture =
  `&line[..len - strip(line).len()]`. Re-inject via `ContainerPrefixLine`
  (`list_only` = one WHITESPACE; `bq_only` = byte-by-byte
  BLOCK_QUOTE_MARKER/WHITESPACE) + `ContainerPrefixState`
  (`emit_container_prefix_tokens`; both NEWLINE and BLANK_LINE advance
  `line_idx`). Injected `>`/WS inside opaque HTML_BLOCK is projector-
  stripped; inside a lifted PARAGRAPH/PLAIN the `>` is skipped by
  `inlines_from` and leading WS edge-trimmed by `coalesce_inlines`.
- **Content-container later-line HTML** (`:   text\n\n    <div>…`)
  dispatches in `parse_inner_content`, gated Pandoc + innermost
  Definition/FootnoteDefinition/Admonition + content_indent>0, split by bq
  depth: `bq_depth==0` → `try_dispatch_content_indent_html_block`
  (`line0_prefix`=content-indent, `allow_unclosed_div`); `bq_depth>0` →
  `try_dispatch_bq_content_indent_html_block` (pre-strips bq+indent,
  captures `>     ` per line, `emit_html_block_lift_from_stripped` with
  bq_only prefixes; line 0's `>` already emitted upstream). Without these
  the general dispatcher drops the line-0 indent (losslessness) and parses
  the body as `Div [CodeBlock]`. The block_dispatcher de-fang guard
  (`!(blockquote_depth>0 && content_indent>0)`) STAYS as a fallback.
- **Marker-line HTML** (`:   <div>…`, `[^1]: <div>…`) dispatches via
  `try_dispatch_definition_html_block` (def cascade) /
  `try_dispatch_footnote_html_block` (`handle_footnote_open_effect`).
  Throwaway-builder probe on `probe_consumed`: `==1` byte-lossless
  marker-line emit; `>1` reuse `try_emit_html_block_lift`. Footnote
  gate `!html_block_cannot_interrupt` (extracted `isInlineTag`) — bodies
  keep comments/PIs/`<span>`/void-inline-block INLINE, unlike def bodies.
  `marker_line_html_block_wrapper_kind` mirrors the dispatcher retag
  gate. Comment/PI trailing softbreak fusion:
  `try_fuse_definition_comment_trailing` (stop at list-item start — lists
  interrupt in a def body but not top-level where the reparse runs; do
  NOT reuse `SoftbreakFusion`, its strip collapses blanks).
- **`SoftbreakFusion` enum** (`ToDocEnd`/`ToFencedDivClose`/
  `ToBlockquoteEnd`/`None`) bounds comment/PI trailing fusion
  (`<!-- hi --> t\nmore` → one `Para`): reparse `trailing +
  lines[close+1..fusion_end]`, graft first block, map `text_range().end()`
  → consumed lines. `fenced_div_body_end`/`blockquote_body_end` find the
  container close (excluding the close marker, so a bare `:::` doesn't
  fuse); bq re-injects each continuation `> ` via `ContainerPrefixState`.
  List/content-indent containers stay `None` (deferred). Corpus
  0390/0481/0482.
- **`graft_document_children` is a sibling-emit helper** — call AFTER
  `finish_node()` on HTML_BLOCK to graft at parent level (Comment/PI
  trailing split). `LastParaDemote`: `Never` (Para kept),
  `SkipTrailingBlanks` (div close-butted), `OnlyIfLast` (non-div
  strict-block close). **Line-consumption boundary trap**: returning
  `lines.len()` from inside a container eats close markers (`:::`, `> `);
  sibling-emit helpers consume only the current line.
- **List-item HTML** (`ListItemBuffer::emit_as_block` →
  `try_emit_html_block_lift`): strict gate (line 0 is HTML start, reparse
  = one `HTML_BLOCK[_DIV]` covering all bytes, div needs ≥ 2 tags OR
  `allow_unclosed_div` — item-close site `true`, mid-item flush `false`
  since a lone open may be a partial pair). Empty-body unclosed `<div>`
  DOES lift to `Div []` (corpus 0496). Multi-line close gated on
  `BlockContext::list_item_unclosed_html_block_tag`; indent norm via
  `strip_list_item_indent` + re-inject. **`format_list_item` drops
  `LIST_MARKER` when the item has no PLAIN/PARAGRAPH content_node** —
  per-kind arms emit it on `no_content_emitted && is_first_real_child`
  (HORIZONTAL_RULE, HTML_BLOCK|HTML_BLOCK_DIV); a new block-only lift
  MUST add the pattern. Same class:
  `FOOTNOTE_DEFINITION`/`format_node_sync` marker-space drop for a
  non-PARAGRAPH first child (`first` branch special-cases HTML_BLOCK|
  _RAW|_DIV).
- **`<div>` inter-tag peel (`graft_same_line_div_peel`)**: `<div>x</div>
  y <div>z</div>` peels each pair into a sibling `HTML_BLOCK_DIV`,
  interstitial→demoted `Plain`, tail→`Para`; reparses each segment fresh
  (only the final carries the newline). Wired single-line +
  multi-line-first-div, `bq_depth==0`.
- **Unclosed fenced div in a `<div>` body suspends the `</div>` close**
  (`<div>\n:::x\n</div>` → `Div[Div(x)[RawBlock "</div>"]]`).
  `body_fence_depth` (bumped `try_parse_div_fence_open`, dropped
  `is_div_closing_fence`) returns `line_closes=false` while >0 without
  advancing `depth`; body lifts on the implicit-EOF path. Corpus 0478.
- **Bq-in-listitem first line** (`- > <x>`): `lists::add_list_item`
  returns `ListItemFinish::BqDispatch{content}` — ALL call sites +
  `start_nested_list` must feed `dispatch_bq_after_list_item` (decrements
  `self.pos`), else line 0 is lost. `pandoc_html_open_tag_closes` reads
  raw `lines[line_pos]` (strips bq but NOT `- `), so HTML in
  `- > <div>` needs list-content-col threading (0452/0453 later unblocked
  by a `ContainerPrefix` session). `find_content_node` skips a
  PLAIN/PARAGRAPH trailing a leading HTML_BLOCK[_DIV] (else non-idempotent
  wrap-source pick).

### Out of scope / known divergences

- **HTML entity decoding: attribute VALUES done (08-05), rest open.**
  `decode_html_attr_entities` covers lifted `<div>`/`<span>` attrs, and is
  **HTML_ATTRS-only** — pandoc rejects an `&`-bearing brace body as an
  attribute block outright (`:::{#a&amp;b}` → literal class), so never
  decode Pandoc `{...}` values. Still open (general-conformance scope):
  refs in text, heading text (auto-ids change), URLs, autolinks; NOT
  code/raw HTML. Residual: `[x](#a&amp;b)` false-positives
  `undefined-anchor` until *link URLs* decode too (declaration side is now
  correct). Smaller known gaps: semicolon-less legacy refs (`id="a&amp b"`
  → `a& b`); entity in attribute NAME must block the lift entirely
  (`<div a&amp;b>` → pandoc `RawBlock`, panache lifts a `Div`); `&#0;`
  prints `\0` where pandoc's writer prints `\NUL`.
- **Definition-list-in-blockquote broad gap**: `> Term\n>\n> :   text` →
  pandoc `DefinitionList [([Term],[[Para text]])]`; panache emits
  `Para [Term]` + empty-term `DefinitionList` with `Plain [text]`.
  Verified div-agnostic — blocks bq-nested def+HTML cases from
  conformance. NOT HTML scope (general pandoc-conformance target).
- **Def-body list-adjacent trailing looseness** (`:   <!-- --> t\n    -
   item` → pandoc `Plain [t]`, panache `Para [t]`) — pre-existing def
  looseness gap, in the plain path too.
- **Multi-line-SECOND-div inter-tag** (`<div>a</div> y <div>\nz\n</div>`):
  content-scan depth model treats `</div>…<div>` on the close line as
  depth-neutral → one div; pandoc = fresh block. Needs depth-model rework.
- Ref-conflict + cross-boundary cite numbering (pandoc doc-order,
  panache inner-wins); `<!ENTITY x "y">` smart-quote gap; tab-indented
  list-item `Div [Para]`→`Div [CodeBlock]` formatter non-idempotency — all
  deferred, non-html-scope.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (`HTML_BLOCK_DIV` + `HTML_ATTRS`) | **Landed** 05-08 — issue #263. Inner content lifted in Phase 6. |
| 2 | `<span>` inline lift (`INLINE_HTML_SPAN`) | **Landed** 05-08. |
| 3 | Sectioning + verbatim pin; `eitherBlockOrInline` | **Landed** — non-void 05-09, void `area`/`embed`/`source`/`track` 05-10. |
| 4 | Comments, PIs, declarations, CDATA | **Landed** 05-08; type-4 CM lowercase gappy. |
| 5 | `markdown_in_html_blocks` edge cases | **Landed** — superseded by Phase 6 structural lift. |
| 6 | Lift inner HTML content into structural CST children | **All non-bq + bq shapes** for `<div>` + non-div strict-block + inline-block matched-pair (clean, open-trailing, butted/indented-close, same-line, empty, multi-line-open, depth-aware nested, multi-close, unclosed). List items + bq-in-listitem. `PARAGRAPH→PLAIN` at adjacency. **Pass 105 → 257.** Slash-suffixed div openers fixed 09-15. |
| 7a | Single-construct opaque → `HTML_BLOCK_RAW` | **Landed** 06-17. Comment/PI/verbatim retag (`html_raw_block`). |
| 7b | Standalone-tag split (≥ 2 tags/line) | **Landed** 06-29; bq 07-02. `try_parse_standalone_block_tags_split`. |
| 7c | Open-only body lift (open + trailing, no close) | **Landed** 07-02 (+bq). `emit_html_block_body` non-div arm. |
| 7e | Multi-tag interleave (inter-tag text) | Non-div same-line **FIXED** 07-02 (`same_line_trailing_forces_opaque`, 0472/0475-0477). para-leads-tag deferred. |
| A | fenced-div-in-html-div (`<div>\n:::x\n</div>`) | **Landed** 07-02. `body_fence_depth`. Corpus 0478. |
| B | `<div>` inter-tag lift (`<div>x</div> y <div>z</div>`) | **Landed** 07-02. `graft_same_line_div_peel`. Corpus 0479 (ws), 0480 (no-ws). Multi-line-2nd-div deferred. |
| C | Comment/PI trailing softbreak fusion | **Landed** 07-02; fenced-div + bq containers 07-08. `SoftbreakFusion` enum. Corpus 0390/0481/0482. List/content-indent containers deferred. |
| D | Definition-body marker-line HTML (`:   <div>…`) | **Landed** 07-08. `try_dispatch_definition_html_block`; multi-line body + comment-trailing fusion. Corpus 0483/0484/0487/0488. |
| E | Footnote-body marker-line HTML (`[^1]: <div>…`) | **Landed** 07-08. `try_dispatch_footnote_html_block`, gated `!html_block_cannot_interrupt`. Corpus 0485/0486. |
| G | Character references in lifted attribute values | **Landed** 08-05. `decode_html_attr_entities`, read-time only. Corpus 0501-0505. Semicolon-less legacy form + entity-in-attr-NAME deferred. |
| F | Later-line HTML in a content-container body | **Landed** 07-08 + variants 08-02. `try_dispatch_content_indent_html_block` (bq0) + `try_dispatch_bq_content_indent_html_block` (bq>0). Unclosed-div (later-line 0494, list-item 0495/0496). bq-nested-def **fully fixed** 08-02 (goldens only, no corpus — def-list-in-bq gap). Corpus 0489. |

--------------------------------------------------------------------------------

## Latest session — 2026-09-15 (Phase 6: slash-suffixed div openers)

Conformance: **html-block 267/267 → 274/274; html-inline 33/33 → 33/33**.
Total **548/548 → 555/555**. Workspace **6139 → 6161 passing**, zero
failures. No red tests remain.

### What landed

- **Parser-shape gap:** `<div/>` opens a div in Pandoc. Both the line
  balance scanner and same-line close finder now count it as an opener.
  The existing structural lift owns the body; no projector changes.
- Confirmed the regression with a failing CST containment assertion and
  five failing corpus cases before the fix. Pinned seven corpus cases
  (0549-0555), including multiline and spaced-slash controls that already
  passed. Generated expectations with the pinned Pandoc 3.10.2 binary;
  the shell's 3.11 agrees on all probes.
- Seven paired Pandoc/CommonMark parser fixtures and seven formatter
  goldens cover implicit EOF, explicit close, same-line nesting,
  blockquotes, list items, multiline tags, and `/ >` spelling. Goldens
  preserve the slash and check losslessness and idempotency.
- `cargo check`, workspace and parser tests, both conformance allowlists,
  all-feature/all-target Clippy, and rustfmt pass. Updated native-divs
  documentation and regenerated the report before allowlisting.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/` — two scanner conditions
  and their semantic contract (parser-shape bucket).
- `crates/panache-parser/tests/` — focused containment assertion, paired
  goldens, snapshots, corpus 0549-0555, allowlist, and generated reports.
- `tests/` — formatter fixture registration and seven round-trip goldens.
- `docs/guide/` and `.agents/skills/html-conformance/` — behavior and recap.

### Suggested next sub-targets (ranked)

1. **Entity in attribute NAME** — `<div a&amp;b>` must stay raw in
   Pandoc. Add a lift-eligibility gate after probing name validity.
2. **Semicolon-less legacy refs** (`id="a&amp b"` → `a& b`) — probe
   TagSoup's name charset before implementing.
3. **Multi-line-second-div inter-tag** — remains a depth-model gap.
4. **Paragraph text before a block tag** — `one<div/>two</div>` in a
   div body stays inline. This is the existing general paragraph-boundary
   gap, beyond slash handling; nested openers at the body start now work.
5. **Definition-list-in-blockquote broad gap** — general conformance scope.

### New trap

- A slash closes neither a native div nor its body. Do not generalize this
  to other tag families; both depth scanners must agree. Folded above,
  along with the pinned-oracle version mismatch.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. date — sub-target — pass delta — lever.

- 2026-08-05 — Phase G attribute-value entities — html 295 → 300 —
  read-time decoding in all three attribute readers; source-byte id spans.

- 2026-08-02 — Phase F unclosed-div + bq-nested-def later-line — html
  292 → 295 — container-aware lift made the shape byte-lossless; corpus
  0494-0496.
- 2026-07-08 — Phases D/E/F marker-line + later-line container HTML —
  html 282 → 292 — definition, footnote, and content-indent dispatch;
  corpus 0480-0489.
- 2026-07-02 — Phases A/B/C + 7e cluster — html 262 → 282 — softbreak
  `ToDocEnd` (0390), inter-tag peel (0479), `body_fence_depth` (0478),
  `same_line_trailing_forces_opaque` (0472/0475-0477), void strict-block
  (0470-0471).
- 2026-06-17→07-02 — Phases 7a/7b/7c — html 259 → 271 — raw retag,
  standalone-tag splitting, and blockquote lifts; corpus 0464-0469.
- 2026-05-08→18 — Phases 1-6 seed + waves — html 0 → 257 — structural
  div/span/attribute lifts, tag categorization, and blockquote dispatch.
