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
- **Free-display definition relations follow Badness's two layout policies.**
  Automatic wrapping treats `:=` and `:=_i` as ordinary relations and aligns a
  later relation under the definition colon. In authored `\\` rows, a
  definition-led chain stays flush at the display indent. Assignment-arrow
  commands retain their RHS anchor.

--------------------------------------------------------------------------------

## Latest session

**Free-display definition relations.** Supported definition relations no longer
force a free display onto the legacy renderer.

- Typed lowering now coalesces a CST-separated definition head with a scripted
  equals, so `:=_i` and `::=_i` remain one relation piece without weakening the
  compatibility guard for `<=_i`, `>=_i`, and `==_i`.
- Automatic relation wrapping and authored-row layout are pinned byte-for-byte
  against Badness for unscripted and scripted definitions. Automatic wrapping
  aligns later relations under the definition colon; authored definition-led
  rows stay flush.
- Assignment-arrow commands still anchor ordinary continuations under their
  RHS. `STYLE.md` and the formatter regression now distinguish that behavior
  from definition relations.
- The complete formatter oracle and all workspace validation gates pass. The
  corpus and its parity classifications did not change, so the committed report
  did not require regeneration.

### Suggested next sub-targets

1. Expand structured-delimiter environment lowering to mixed bodies only when a
   representative oracle case can pin the spacing and break policy.
2. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.
3. Revisit non-colon scripted composite relations only after the pinned Badness
   formatter defect is corrected; keep their compatibility path in the
   meantime.
