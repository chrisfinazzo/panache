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

### Write-phase performance

The write phase is what `did_change` does before any parse: splice the text,
update the line index, hand salsa a new input. `benches/lsp_write_phase.rs`
times it (`task bench:write-phase-gate`) and its module doc carries the tables;
the three findings from fatou's 2026-08-16 `Arc<str>`-vs-rope survey that
prompted the bench are all fixed, taking a keystroke on a 293 KB document from
282 us to 98 us.

- [ ] **Two full scans per keystroke remain on a large document** --- the line
  index is rebuilt, on top of the two splice passes that are the floor for
  an `Arc<str>` input on stable. Maintaining an `Arc<LineIndex>` on
  `DocumentState`, validated against salsa's text by `Arc::ptr_eq` so a
  stale one is discarded rather than trusted, would remove them. The salsa
  `line_index` memo does not help: it is warm only when a reader has run
  since the last keystroke, which during a typing burst it has not.

- [ ] **Bypassing `diff_edit` for a single staged edit is fatou's remaining
  pipeline idea and does *not* pay here** --- measured, `diff_edit` is 7.1
  us against a \~1.9 ms parse (`multi_change_large_8`). Recorded so it is
  not re-derived; revisit only if the parse gets cheap enough for 7 us to
  matter, which the incremental tier ladder is steadily making more
  plausible.

## Parser

### Issues

Note any known parser issues here.

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

- [ ] `prepare_yaml_content` is still worth a second look now that the indent
  gate landed: the dispatcher reaches `detect_prepared` for the same line
  2-3 times per parse (via `continuation.rs`'s re-detection), so the gate is
  recomputed for content it has already seen. A per-parse memo would
  collapse that, but it needs parse-scoped storage (a thread-local cache
  would retain green trees across parses and would also flatter the
  benchmark, which reuses one process).

### Incremental Parsing

The LSP splices its tree off the previous parse through a four-tier ladder ---
token, region, section window, suffix window --- in
`crates/panache-parser/src/parser/reparse.rs`. The architecture and the reason
behind every guard live with the code: that module's doc carries the ladder and
the guard catalogue, `docs/development/lsp.qmd` the host side, and
`benches/lsp_incremental.rs` the measurements. The multi-session roadmap that
built it is finished; what follows is only what it left open.

**Governing invariant:** a successful reparse yields a green tree and
syntax-error vector byte-identical to a full parse of the edited text, enforced
by a `cfg(debug_assertions)` oracle on every reparse. Every guard failure bails
to a full parse --- never an error, never a best effort. A divergence is fixed
by adding a bail, never by relaxing an oracle assert.

**Gates before touching any of this:** `task bench:lsp-gate` (both bench gates,
release, default iteration count --- a shortened run measures sampling noise),
`PANACHE_FUZZ_ITERS=20 cargo test -p panache-parser --test incremental_fuzz`,
and the workspace suite with `PANACHE_INCREMENTAL_PARSING` forced both ways.
Speedup floors are calibrated \~5% under the lowest of three runs on an idle
machine; a floor set against a loaded one produces a gate that fails later for
no reason.

#### Cost ceilings

- [ ] **`splice_children` and `replace_with` rebuild every ancestor's child
  vector**, so both are `O(top-level arity)` --- about 220 us on the pandoc
  manual's 2662 children. Now that the region and token tiers have removed
  the parse from a cheap splice, this is what is left of one: it is the
  floor under every tier, and the reason a 186-byte region on a 300 KB
  document still costs \~1.1 ms. Attacking it needs either a
  structural-sharing splice rowan does not offer, or a nested `DOCUMENT`
  shape --- and that shape is visible to every consumer that walks top-level
  children, so it is not a local change.

- [ ] **`refdef_set` re-scans the whole document on every revision**
  (`src/salsa.rs`), which is `O(document)` per keystroke and is paid whether
  or not a reparse is attempted. The parser side is fine --- the host passes
  the cached set through `reparse_with_refdefs`, so `populate_refdef_labels`
  short-circuits --- so the work to do is incrementalizing the *set*, not
  the scan's caller. `diff_edit` re-deriving an edit the LSP already knows
  is the same shape of waste and is cheaper to fix.

- [ ] **`Parser::new` builds a `BlockParserRegistry` per parse.** That fixed
  cost is why three small parses lose to one on a 74-byte document, which is
  in turn why the region tier carries no small-document exemption where
  fatou's equivalent does. A shared or lazily-built registry would let that
  exemption exist and would speed up every boundary parse.

- [ ] **The write-phase medium row is only break-even.** `large_authoring.qmd`
  measures 1.05x end to end (`benches/lsp_write_phase.rs`) where
  `benches/lsp_incremental.rs` splices the same document at 4.0x. The two
  edit different positions --- 4/5 of the way through against line 60 --- so
  the gap is either a shape the region tier declines at that position or
  host-side work only the end-to-end bench includes. Worth finding out
  which, because the end-to-end number is the one a user feels.

#### Tier coverage

- [ ] **Widen the token tier.** It answers only an edit strictly inside a plain
  `TEXT` token of a top-level `PARAGRAPH`. Deferred and still open: edits
  that touch a token boundary (needs join probes), non-`TEXT` leaf kinds,
  and `PLAIN` / heading-content / code-body parents. Each is a decline today
  that a keystroke could have.

- [ ] **Nested-container regions.** Regions are restricted to top-level
  `DOCUMENT` children, because a fragment lifted out of a list item or a div
  needs a fragment entry point carrying container stack, open fences, and
  refdef scope --- fatou records the same lesson. `ParseOrigin` is the
  degenerate top-level case of exactly that entry point, so the shape to
  generalize already exists.

- [ ] **The long-range pairing guard is coarse.** `long_range_pairing_lines`
  declines whenever the region's fence, div, math, comment, or bare `-`/`=`
  rule lines differ before and after the edit --- sound, but it refuses
  every edit that so much as retypes a fence. A precise version would ask
  the *old tree* whether a candidate line actually parsed as a delimiter,
  which is the check `prefix_fence_state_is_stable`'s parity heuristic also
  wants.

- [ ] **The retained-`DEFINITION_LIST` guard is coarse** in the same way.
  Definition items merge into one list node across blank lines, and the
  marker that opens the merging item need not lead the window, so the guard
  pairs a retained `DEFINITION_LIST` with a `:`/`~` marker line *anywhere*
  in the window (`has_definition_marker_line`) and declines. A precise
  version would bound the reach at the first window block that cannot belong
  to a definition list --- a heading or a fence breaks the chain --- but
  deciding that textually is re-implementing the block parser, so it wants
  the same ask-the-old-tree treatment as the item above. Pinned by
  `definition_marker_suffix_after_a_retained_definition_list` and
  `definition_list_grown_below_a_retained_definition_list`.

#### Infrastructure

- [ ] **Neither bench gate runs in CI.** Both need
  `benches/documents/download.sh` and a release build, and a timing assert
  on shared runners would land flaky. The corpus is pinned and the gates are
  one task target each, so wiring them is a small job whenever someone
  decides flakiness is acceptable --- or when the gates grow a non-timing
  mode.

- [ ] **A gate run dirties the working tree.** `benches/documents/download.sh`
  rewrites `pandoc_testsuite.md`, which is tracked in git, on every run.

- [ ] **`experimental.incrementalParsing` is deprecated toward removal.** It is
  still honored and still warns once at `initialize`;
  `PANACHE_INCREMENTAL_PARSING` does the one job it has left. Remove it a
  release after the deprecation shipped, along with the internal
  `runtime_settings.experimental_incremental_parsing` name, which has no
  wire impact and can be renamed freely.

- [ ] **`panache-parser`'s `pub mod range_utils` has no consumer outside the
  crate**, and the host keeps its own copy of `expand_byte_range_to_blocks`
  in `src/range_utils.rs`. Narrowing the module is semver-visible on a
  published crate, so it is a decision rather than a cleanup --- but the
  duplication is real and one of the two copies should win.

**Explicit non-goal:** NodePtr re-anchoring across edits (arity's
`map_range_through_edits`) is only needed if panache starts caching NodePtrs
across edits. It does not.

### Coverage

This section tracks implementation status of Pandoc Markdown features based on
the spec files in `assets/pandoc-spec/`.

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
