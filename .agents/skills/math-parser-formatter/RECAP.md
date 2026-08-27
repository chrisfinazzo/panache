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
- **Contextual coercion retains Panache's established full TeXbook rule.** A
  `Bin` becomes `Ord` at list start and after `Bin`, `Rel`, `Open`, `Punct`, or
  `Op`. Badness leaves a binary atom binary after punctuation; the intentional
  differential is pinned by `panache_coerces_after_punctuation_where_badness_does_not`.
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
- **Synthetic word expansion is for classification only.** Its temporary Rowan
  tree starts at byte zero, so range-based typed lowering must consume the
  original CST elements. Use expanded elements only for atom-by-atom delimiter
  and segment scans.

--------------------------------------------------------------------------------

## Latest session

**Operator-bearing suffixes after comment-bearing display environments.** A
single top-level environment now participates in typed free-display layout as a
multiline operand instead of forcing the mixed-environment compatibility path.

- `Piece` records the final-line width of a multiline atom. Typed document
  composition uses that width to hang the environment body from the actual
  `\begin` column and to place any same-line suffix relative to `\end` without
  flattening the environment.
- The shared relation/binary hierarchy now sees the environment as an opaque
  semantic operand. A following binary operator starts flush on a new display
  line; a following relation aligns with the leading relation, including after
  an assignment-arrow head when the forced multiline operand separates them.
- Mandatory oracle cases cover `+ environment +`, `= environment =`, and
  `\gets environment \leq` byte for byte and verify idempotency. The host golden
  covers the binary and relation forms, and `STYLE.md` records the rule.
- The narrow gate still requires exactly one well-formed top-level environment,
  a nested comment, and a binary- or relation-led prefix. The unary-prefix and
  multiple-environment shapes remain on their existing compatibility paths.
- The shared corpus and its parity classifications did not change, so the
  committed report needs no update.

### Suggested next sub-targets

1. Extend the typed multiline-atom path to scripted environment bases, pinning
   how scripts glue to `\end` before any following operator.
2. Continue converging the separate structured-delimiter and mixed-environment
   paths now that a typed environment atom composes in free displays; remove the
   superseded prefix-only compositor only after its remaining shapes migrate.
3. Revisit unpunctuated multiple environments only with a pinned structural
   composition rule; keep the current fallback.
4. Revisit non-colon scripted composite relations only after the pinned Badness
   formatter defect is corrected; keep their compatibility path in the
   meantime.
