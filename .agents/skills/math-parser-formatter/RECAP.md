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
- **Environment composition follows semantic atoms from the original CST.**
  Ordinary delimiters and punctuation can share one lexical word token, so
  composition must use the semantic atom stream for those boundaries while
  retaining source ranges for typed lowering and authored-space decisions.

--------------------------------------------------------------------------------

## Latest session

**Typed environment composition completed.** The formatter now has a measured
preservation boundary and one typed path for every well-formed environment
shape in the shared corpus.

- A committed, exact-match migration census classifies all 321 corpus/context
  runs: 266 typed, 28 legacy, and 27 verbatim. Every non-typed route carries a
  closed reason instead of an `unsupported` catch-all.
- Environment rows, cells, alignment, comments, nesting, scripts, and mixed or
  delimited compositions lower into the shared document IR. The separate
  environment string assembler and row/grid renderer have been deleted.
- Semantic atom boundaries now drive ordinary-delimiter and punctuation layout
  even when several atoms share one CST word token. Authored spaces after an
  environment remain source-range decisions.
- No well-formed corpus environment uses the legacy route. Only the malformed
  `environments/recovery/trivia_before_name.tex` fixture remains there, with an
  explicit `malformed-environment-syntax` reason in all three contexts.
- Conservative argument-domain handling is explicit: proven math arguments
  recurse, nonmath and unknown domains remain opaque, and incomplete known
  signatures still preserve through the compatibility boundary.

### Suggested next sub-targets

1. Delete the flattened formatter by migrating the remaining 28 classified
   legacy routes, starting with the repeated missing-lowering families visible
   in `tests/math_badness/migration_census.txt`.
2. Remove formatter-local operator semantics and script sentinels only as their
   last census callers disappear; keep the exact route report green after each
   coherent deletion.
3. Close the inline/display/raw host matrix after the legacy count reaches zero.
4. Keep non-colon scripted composite relations as a named intentional
   difference until the pinned Badness formatter defect is corrected.
