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

**Typed width-driven free-display wrapping.** Supported free-display rows now
stay in the typed lowering path whether they fit, wrap automatically, or occur
after an authored `\\` marker. The final over-width authored-row compatibility
check in `render_display` is gone.

- The typed document layout ranks top-level relations above binaries, keeps
  fixed and `\left`/`\right` delimiter interiors opaque, aligns equality chains
  at the first relation, and anchors ordinary relations after assignments at
  the assignment RHS. Over-width relation segments then split at each binary
  operator with the established nested indentation.
- Width accounting subtracts the host `math-indent` once and subtracts an
  authored continuation's semantic alignment before laying out that row. The
  resulting relative geometry composes through `Ir::Align` and remains
  idempotent.
- A mandatory narrow-display oracle case pins relation-first and binary-second
  byte parity with Badness. The regenerated corpus report keeps the same
  classification counts, but four display outputs now use typed/Badness
  spelling for scripts and paired delimiters instead of legacy flattening.
- The complete formatter crate suite passes, including Badness parity, corpus
  properties, and MathML cross-validation. All four project-required workspace
  validation commands also pass.

### Suggested next sub-targets

1. Expand structured-delimiter environment lowering to mixed bodies only when a
   representative oracle case can pin the spacing and break policy.
2. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.
3. Retire more of the legacy free-display breaker only as its remaining
   unsupported definition-relation and scripted-composite seams gain safe typed
   handling; keep the known Badness-defect cases on compatibility paths.
