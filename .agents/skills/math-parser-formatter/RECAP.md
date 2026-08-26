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
- **Tight environment grids discard cell-relative continuation offsets.** When
  any non-final cell is multiline, Badness removes grid padding and resets
  every multiline cell's continuation to the environment body indent. Preserve
  the atom-relative offset separately for ordinary aligned grids; do not bake it
  into the cell document.
- **Multiple environments inside `\left`/`\right` require punctuation
  boundaries.** Each top-level punctuation-delimited segment may own one
  well-formed environment. The next environment glues to the punctuation on the
  preceding `\end` line, then hangs from that actual source column. Adjacent
  environments in one segment remain on the compatibility path.

--------------------------------------------------------------------------------

## Latest session

**Comment-bearing environments after free display expressions.** A display
shape such as `x+\begin{matrix}...%...\end{matrix}` now uses a typed mixed
document instead of declining the entire math body.

- The mandatory oracle regression pins Badness's distinct layout byte for byte:
  the free expression breaks before `+`, the environment begins on that
  continuation line, and its body hangs from the resulting `\begin` column.
- The prefix is lowered through the shared semantic display path. A forced
  zero-width layout reproduces the operator break caused by the environment's
  hard lines without adding formatter-local operator interpretation.
- The safety gate remains deliberately narrow: exactly one well-formed,
  unscripted environment must end the expression after a binary operator.
  Comments or authored breaks in the free content, trailing expression content,
  nested non-block environments, and multiple environments retain the
  compatibility path.
- `STYLE.md` records the display rule. The complete formatter oracle and all
  workspace validation gates pass. The shared corpus and its parity
  classifications did not change, so the committed report needed no update.

### Suggested next sub-targets

1. Move environments toward first-class typed atom documents so the separate
   structured-delimiter and mixed-environment paths can converge and the
   display-specific compositor can shrink.
2. Pin Badness's layout for other comment-bearing display surroundings—such as
   a relation before the environment or trailing free content—before widening
   the new safety gate.
3. Revisit unpunctuated multiple environments only with a pinned structural
   composition rule; keep the current fallback.
4. Revisit non-colon scripted composite relations only after the pinned Badness
   formatter defect is corrected; keep their compatibility path in the
   meantime.
