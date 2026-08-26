# Math parser/formatter — running session recap

Concise handoff between sessions of the `math-parser-formatter` skill. Read it
for the latest result and suggested next sub-targets. At the end of a session,
rewrite those sections instead of accumulating history.

--------------------------------------------------------------------------------

## Persistent implementation details

- **Read math content via `syntax::math::math_content_text()`**, never
  `MATH_CONTENT.text()`. The block machinery interleaves container prefixes
  (`LINE_PREFIX`, and sometimes host `NEWLINE`) into `MATH_CONTENT` on
  continuation lines; the helper strips them by whitelisting `MATH_*` tokens.
  Reading `.text()` directly leaks the `>` and re-accumulates it every format
  pass (a real idempotency bug that was fixed in Phase 1).
- **`MATH_SPACE`/`MATH_NEWLINE` are intentionally distinct** from host
  `WHITESPACE`/`NEWLINE` — that distinction is what makes the helper above work.
  `MATH_SPACE` is load-bearing (collides with blockquote-prefix `WHITESPACE`
  otherwise); `MATH_NEWLINE` is kept for symmetry.
- Operator class and precedence are semantic interpretation, not CST shape;
  macros can override them. Keep the lexical CST neutral and share the
  Panache-owned interpretation between consumers.
- **Scripts are native CST structure.** `MATH_SCRIPTED` owns one base atom and
  `MATH_SUBSCRIPT`/`MATH_SUPERSCRIPT` children. Unbraced text bases and
  arguments split at Unicode-scalar boundaries; comments and blank lines stop
  attachment. Formatter interpretation must inherit the base atom's class
  across the script—especially for scripted relation and assignment breaks.
- **Contextual coercion follows Badness's role model, not full Appendix G.** A
  `Bin` becomes `Ord` at list start, after an effective binary or relation, or
  after an atom with `DelimiterRole::Open`. `Punct`, `Op`, and an `Open` class
  without a genuine delimiter role remain operands for this purpose.
- **Authored `\\` breaks layout rows but not the semantic atom stream.** Lower
  each row separately while deriving every row's atoms from one source-ordered
  stream; otherwise, a sign after `\\` is incorrectly coerced as though it
  started a new math list.
- **Some scripted composite relations expose a pinned Badness defect.** Badness
  still splits a non-colon relation head from its CST-separated scripted tail
  (`<=_i` → `< =_i`, likewise `>=_i` and `==_i`). Panache preserves those
  relations through the compatibility path; exclude them from mandatory byte
  parity until the pinned oracle is corrected. Definition relations, including
  `:=_i`, now have byte parity.

--------------------------------------------------------------------------------

## Latest session

**Typed definition relations in inline and environment contexts.** Plain
definition relations no longer force the whole supported body onto the legacy
renderer outside free displays.

- Typed lowering coalesces the semantic stream's contiguous punctuation-colon
  atoms and following `=` relation into one formatter piece. It does so only
  inside the same lexical `MATH_WORD`, preserving authored whitespace and
  leaving CST-separated scripted composites on their compatibility path.
- `x:=y`, `a::=b`, `x:=-y`, and `\mu:=\nu` now have direct lowering coverage
  and mandatory byte parity with Badness in inline and environment contexts.
  Relation-layout inspection now reads the lowered piece metadata, since one
  definition-relation piece can correspond to several semantic atoms.
- Free displays containing definition relations intentionally remain on the
  legacy path. Panache's documented legacy breaker treats `:=` as an assignment
  and anchors a later ordinary relation under its RHS; Badness aligns the later
  relation under the definition colon. Migrating only the unscripted display
  shape would make `:=` and `:=_i` inconsistent.
- The focused typed-lowering suite and complete Badness formatter oracle pass.
  The corpus and its parity classifications did not change.

### Suggested next sub-targets

1. Resolve the free-display definition-relation policy as one slice: pin
   Badness's automatic and authored-row behavior, migrate unscripted and
   scripted `:=` consistently, and update `STYLE.md` if the oracle requires a
   visible alignment change. Keep the known non-colon scripted-relation defect
   on its compatibility path.
2. Expand structured-delimiter environment lowering to mixed bodies only when a
   representative oracle case can pin the spacing and break policy.
3. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.
