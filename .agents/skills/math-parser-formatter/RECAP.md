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
  relations on the typed path; exclude them from mandatory byte parity until
  the pinned oracle is corrected. Definition relations, including `:=_i`, now
  have byte parity.
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
  environments in one segment cross the verbatim preservation boundary.
- **Environment composition follows semantic atoms from the original CST.**
  Ordinary delimiters and punctuation can share one lexical word token, so
  composition must use the semantic atom stream for those boundaries while
  retaining source ranges for typed lowering and authored-space decisions.

--------------------------------------------------------------------------------

## Latest session

**The flattened formatter was deleted.** Every supported math shape now lowers
through the parser-owned semantic atom stream and the shared document IR.

- The exact-match migration census classifies all 321 corpus/context runs: 291
  typed, 0 legacy, and 30 verbatim. The legacy route and its reason tracker no
  longer exist.
- The old flat-token stream, script sentinels, display line breaker, and
  formatter-local operator table were deleted. Operator classes, delimiter
  roles, contextual unary coercion, and break priority now have one owner in
  `panache_parser::semantic::math`.
- The final legacy families—bare known commands, optional brackets, control
  symbols, scripted composite relations, authored comment rows, and display
  equation labels—now lower through typed documents. Nested environments in
  cells also compose through the same typed environment path.
- Malformed environment spellings now cross the verbatim preservation boundary
  instead of being normalized by a compatibility renderer. Conservative
  argument-domain handling remains explicit: proven math arguments recurse,
  while nonmath and unknown domains remain opaque.
- The generated migration census is pinned to LF in `.gitattributes`, so its
  exact-match test compares the same bytes on Windows, macOS, and Linux.

### Suggested next sub-targets

1. Close the inline/display/raw host matrix with representative simple,
   scripted, commented, wrapped, and environment bodies.
2. Regenerate the complete formatter report, and audit every preserved case
   against the named preservation boundary.
3. Run the remaining final gates, including corpus convergence, MathML/TeX
   checks, performance comparison, and WASM size review.
4. Keep non-colon scripted composite relations as a named intentional
   difference until the pinned Badness formatter defect is corrected.
