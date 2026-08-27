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

**Math formatting is stable.** The user-facing option is now `[format]
format-math = true`; the former `[experimental] format-math` spelling remains a
deprecated alias, emits a CLI warning, and loses to the stable spelling when
both are present. The formatter's public config and benchmark harness use the
stable `format_math` field, the generated schema advertises the stable key, and
all in-repo behavior fixtures use it.

- The configuration and formatting guides document the complete style,
  parser/formatter boundary, configured and document-derived command
  signatures, preservation boundary, and every intentional difference from
  the pinned Badness oracle. The canonical `STYLE.md` matches that contract.
- On Linux 6.18.45 with Rust 1.97.1, nine alternating 400-iteration release
  runs over the identical 30,112-byte `benches/documents/math.qmd` compared
  `e4dcaf00` (the parent of the native migration) with the stabilized tree.
  Median parse time was 566.86 → 565.86 µs (-0.18%), formatter-only time was
  1,014.75 → 1,008.93 µs (-0.57%), and the full pipeline was 1,670.16 →
  1,664.71 µs (-0.33%). `PANACHE_BENCH_FORMAT_MATH=1` makes the comparison
  reproducible.
- Production-style `wasm-pack --release --target web` output grew from
  1,502,384 to 1,710,641 bytes (+208,257, 13.86%); gzip size grew from 545,616
  to 620,750 bytes (+75,134, 13.77%). `llvm-size` attributes about 128 KB of the
  increase to code and 80 KB to data, consistent with the native typed
  formatter and exhaustive 2,448-symbol semantic tables. The increase is
  explicit and reviewed; no Badness code ships at runtime.
- `cargo check --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
  `cargo fmt --all -- --check` pass. Both changed guide pages also pass Panache's
  own formatting check and render independently with Quarto.

### Suggested next sub-targets

1. Keep non-colon scripted composite relations as a named intentional
   difference until the pinned Badness formatter defect is corrected.
2. Remove `[experimental] format-math` only in a future major release, after the
   documented deprecation window.
3. Track the optimized WASM module size when the semantic tables or typed
   formatter grow; the stabilization review establishes 1,710,641 bytes raw and
   620,750 bytes gzip as the new reference point.
