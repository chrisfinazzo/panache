//! Seeded property harness for incremental reparsing.
//!
//! For every hazard snippet below, this harness applies pseudo-random edits
//! drawn from a hazard-biased insert alphabet (strings that can change block
//! structure at a distance) and asserts, per edit:
//!
//! 1. **Losslessness** — the incrementally reparsed tree round-trips to the
//!    edited text.
//! 2. **Structural identity** — its [`fingerprint`] equals a from-scratch
//!    parse of the edited text (in debug builds the in-crate oracle in
//!    `parser/verify.rs` checks the same invariant before we ever see the
//!    result; the comparison here also covers release runs).
//! 3. **Error identity** — its spliced syntax errors equal that parse's.
//!    Malformed YAML is the only error source there is, so the frontmatter
//!    and hashpipe snippets carry this half of the invariant.
//!
//! Chained batches additionally feed each spliced tree *and its errors* back
//! in as the next edit's base, mirroring how the LSP chains trees across
//! keystrokes.
//!
//! The generator is a plain LCG (MMIX constants) with fixed per-test seeds:
//! runs are fully deterministic, and every assertion message carries the
//! snippet name, seed, and edit so a failure is reproducible by copying the
//! reported case into a unit test. Iteration counts scale with the
//! `PANACHE_FUZZ_ITERS` environment variable (a multiplier; the default is
//! sized for `cargo test`).
//!
//! A failure here is an incremental-parser bug: minimize it into an
//! `#[ignore]`d red test (see the roadmap in `TODO.md`, "Incremental
//! Parsing") and fix it by adding a bail-to-full-parse condition — never by
//! relaxing these asserts.
//!
//! The hazard snippets are fuzzed with the window-size cutoff *off*
//! ([`CostGuards::Ignored`]). That cutoff declines any window covering more
//! than 85% of the document, which on snippets tens of bytes long is almost
//! every window: enforcing it here drops the share of edits that reach a splice
//! from 78% to 23%, and the guards this harness exists to test stop being
//! exercised. It is a *cost* guard with no soundness content, and the seams it
//! hides on a 30-byte snippet are the same seams that occur mid-document in a
//! real file, where the cutoff admits them. The real-document corpus below runs
//! with the production setting, so the shipped configuration is fuzzed too.
//!
//! Every driver tallies how many edits actually spliced and asserts a floor on
//! that share, because a harness whose edits all decline still passes every
//! invariant above while exercising nothing.

use std::panic::{AssertUnwindSafe, catch_unwind};

use panache_parser::parser::{CostGuards, SyntaxError, fingerprint, parse_with_errors};
use panache_parser::syntax::SyntaxKind;
use panache_parser::{Dialect, Extensions, Flavor, ParserOptions};

mod common;
use common::reparse_or_full_with_cost_guards;

struct Tier {
    name: &'static str,
    flavor: Flavor,
    /// Single edits per snippet.
    singles: usize,
    /// Chained batches per snippet.
    batches: usize,
    /// Single edits per real corpus document (0 = skip that corpus).
    real_docs: usize,
}

impl Tier {
    fn options(&self) -> ParserOptions {
        ParserOptions {
            flavor: self.flavor,
            extensions: Extensions::for_flavor(self.flavor),
            dialect: Dialect::for_flavor(self.flavor),
            ..Default::default()
        }
    }
}

/// The option tiers, chosen for *reach* rather than popularity — each brings
/// a hazard the others cannot express:
///
/// - `Pandoc` is the default and the baseline, and the only tier with
///   `pandoc_title_block`;
/// - `Gfm` is the CommonMark-dialect flavor that still enables
///   `yaml_metadata_block`, so it is the one that can reach the
///   mid-document-YAML refusal (plain `CommonMark` leaves the extension off
///   and cannot);
/// - `Quarto` brings hashpipe `#|` YAML, the only source of syntax errors
///   besides frontmatter;
/// - `MultiMarkdown` brings `mmd_title_block`.
///
/// The budgets **split** the old pandoc-only per-snippet counts rather than
/// multiplying them, so a default `cargo test` costs about what it did
/// before. `PANACHE_FUZZ_ITERS` multiplies every tier together, so the
/// graduation gate exercises all four deeply.
const TIERS: &[Tier] = &[
    Tier {
        name: "pandoc",
        flavor: Flavor::Pandoc,
        singles: 80,
        batches: 12,
        real_docs: 12,
    },
    Tier {
        name: "gfm",
        flavor: Flavor::Gfm,
        singles: 30,
        batches: 5,
        real_docs: 0,
    },
    Tier {
        name: "quarto",
        flavor: Flavor::Quarto,
        singles: 30,
        batches: 5,
        real_docs: 8,
    },
    Tier {
        name: "multimarkdown",
        flavor: Flavor::MultiMarkdown,
        singles: 20,
        batches: 3,
        real_docs: 0,
    },
];

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 33) % n as u64) as usize
    }
}

const INSERTS: &[&str] = &[
    "", // pure deletion
    "\n",
    "\n\n",
    " ",
    "    ",
    "\t",
    "text",
    "e",
    "# ",
    "#",
    "> ",
    ">",
    "- ",
    "* ",
    "+ ",
    "1. ",
    "```",
    "```\n",
    "~~~",
    ":::",
    "::: note\n",
    "---",
    "---\n",
    "===",
    "===\n",
    "|",
    "| a |",
    "$",
    "$$",
    "`",
    "*",
    "_",
    "[",
    "]",
    "(",
    ")",
    "[^1]",
    "[^1]: note\n",
    "[x]: /url\n",
    "\\",
    "\\\n",
    "<div>",
    "</div>",
    "<!--",
    "-->",
    "α",
    "παρά",
    "\r\n",
    "\r\n\r\n",
    "%",
    "% Title\n",
    ":",
    "Key: value\n",
    "---\nk: v\n---\n",
    "#| echo: [\n",
];

const HAZARD_SNIPPETS: &[(&str, &str)] = &[
    ("setext_candidate", "alpha\nbeta\n\ngamma\ndelta\n"),
    ("lazy_blockquote", "> quoted\ncontinuation\n\ntail para\n"),
    ("lazy_list", "- item one\ncontinuation\n\n- item two\n"),
    ("fenced_code", "```r\ncode <- 1\n```\n\npara\n"),
    ("tilde_fence", "~~~\nliteral\n~~~\n\npara\n"),
    ("unterminated_fence", "```\ncode\n\npara after\n"),
    ("fenced_div", "::: note\nbody\n:::\n\npara\n"),
    (
        "nested_div",
        ":::: outer\n::: inner\nbody\n:::\n::::\n\npara\n",
    ),
    ("list_tightness", "- one\n\n- two\n- three\n\npara\n"),
    ("ordered_list", "1. first\n2. second\n\npara\n"),
    ("pipe_table", "| a | b |\n|---|---|\n| 1 | 2 |\n\npara\n"),
    ("refdef", "[foo]: /url\n\nsee [foo] and [bar] here\n"),
    (
        "use_before_refdef",
        "see [x] and [foo] here\n\nmore prose\n\n[foo]: /url\n",
    ),
    ("frontmatter", "---\ntitle: x\n---\n\nbody para\n"),
    ("html_block", "<div>\nhtml body\n</div>\n\npara\n"),
    ("html_comment", "<!-- note\n\nstill comment -->\n\npara\n"),
    ("display_math", "$$\nx^2 + y\n$$\n\npara\n"),
    ("inline_spans", "text $x$ and `code` and *emph* span\n"),
    ("footnote", "text[^1] more\n\n[^1]: note body\n"),
    ("hard_break", "line one\\\nline two\n\ntail\n"),
    ("unicode", "αβγ δε ζη\n\nπαρά two λ\n"),
    ("nested_blockquote", "> outer\n> > inner\n\npara\n"),
    ("hr_vs_setext", "- a\n\n---\n\n- b\n"),
    ("tiny", "a\n"),
    (
        "link_shapes",
        "[text](url) and [ref][foo] end\n\n[foo]: /u\n",
    ),
    (
        "atx_sections",
        "# One\n\nbody one\n\n## Two\n\nbody two\n\n# Three\n\nbody three\n",
    ),
    ("pandoc_title", "% Title\n% Author\n% Date\n\nbody para\n"),
    ("mmd_title", "Title: Doc\nAuthor: Me\n\nbody para\n"),
    (
        "mid_document_yaml",
        "intro para\n\n---\nkey: value\n---\n\ntail para\n",
    ),
    ("bad_frontmatter", "---\ntitle: [\n---\n\nbody para\n"),
    (
        "hashpipe",
        "intro\n\n```{r}\n#| echo: false\n1 + 1\n```\n\ntail\n",
    ),
    (
        "crlf_sections",
        "# One\r\n\r\nbody one\r\n\r\n## Two\r\n\r\nbody two\r\n\r\n# Three\r\n\r\nbody three\r\n",
    ),
    (
        "crlf_lazy_blockquote",
        "> quoted\r\ncontinuation\r\n\r\ntail para\r\n",
    ),
];

fn iterations(default: usize) -> usize {
    std::env::var("PANACHE_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|mult| default * mult)
        .unwrap_or(default)
}

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

fn clamp_to_char_boundary(text: &str, mut pos: usize) -> usize {
    while !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

#[derive(Default)]
struct FuzzStats {
    /// Edits whose *full* parse was lossy or panicked, so the splice could not
    /// be judged against it.
    skipped_lossy: usize,
    /// Edits the guard cascade accepted and spliced.
    spliced: usize,
    /// Edits it declined, which cost a full parse and prove nothing.
    declined: usize,
    /// Splices the token tier took, counted separately because it is the only
    /// tier a uniformly-placed edit almost never reaches --- so a run that
    /// splices heavily can still be leaving it entirely untested.
    token_tier: usize,
    /// Splices the region tier took. Counted for the same reason as
    /// [`Self::token_tier`], and needed for the same reason: a tier that
    /// declines everything is *sound*, so nothing else in the suite fails when
    /// a guard silently turns this one off.
    region_tier: usize,
    /// The two window tiers, tallied so the ladder's *shape* is measured rather
    /// than assumed. Neither has a floor: unlike the two above, they are not at
    /// risk of being silently switched off --- they are the fallback, so a guard
    /// that broke them would show up as a collapsing splice rate. They are here
    /// because the roadmap planned to delete them once the region tier landed,
    /// and this is the evidence that says not to.
    section_window: usize,
    suffix_window: usize,
    /// Corpus documents that were not on disk. Counted rather than only
    /// printed: the corpus is gitignored, so a run on a clean checkout skips
    /// the strictest tier entirely and would otherwise report a full pass.
    skipped_absent: usize,
}

impl FuzzStats {
    fn splice_rate(&self) -> f64 {
        let judged = self.spliced + self.declined;
        if judged == 0 {
            return 0.0;
        }
        self.spliced as f64 / judged as f64
    }

    fn assert_exercised_the_splice(&self, what: &str) {
        eprintln!(
            "{what}: {} spliced, {} declined ({:.1}% spliced), {} skipped with a lossy full \
             parse, {} corpus documents absent",
            self.spliced,
            self.declined,
            self.splice_rate() * 100.0,
            self.skipped_lossy,
            self.skipped_absent
        );
        eprintln!(
            "{what}: tiers token={} region={} section_window={} suffix_window={}",
            self.token_tier, self.region_tier, self.section_window, self.suffix_window
        );
        assert!(
            self.splice_rate() >= 0.25,
            "{what}: only {:.1}% of edits reached the splice; the harness is \
             judging full parses against full parses",
            self.splice_rate() * 100.0
        );
    }

    /// Report the token tier's share and fail if it is untested.
    ///
    /// Distinct from [`Self::assert_exercised_the_splice`] because the two fail
    /// for different reasons: that one catches a harness judging full parses
    /// against full parses, this one catches a *guard* that silently turned the
    /// token tier off. A tightened guard leaves every other test green --- the
    /// tier declining is always sound --- so without a floor here the phase
    /// could regress to a no-op and nothing would say so.
    ///
    /// The floor is 10% against a measured 14.0% on the prose snippets. It is
    /// low because it should be: a third of [`PROSE_INSERTS`] is deliberately
    /// hazardous and declines correctly, the snippets are short enough that the
    /// line-marker-zone guard refuses many placements outright, and several
    /// prose snippets are near-misses whose whole job is to be declined. The
    /// number to watch is a *collapse*, not a few points of drift.
    fn assert_exercised_the_token_tier(&self, what: &str) {
        let judged = self.spliced + self.declined;
        let rate = if judged == 0 {
            0.0
        } else {
            self.token_tier as f64 / judged as f64
        };
        eprintln!(
            "{what}: {} of {judged} judged edits took the token tier ({:.1}%)",
            self.token_tier,
            rate * 100.0
        );
        assert!(
            rate >= 0.10,
            "{what}: only {:.1}% of edits took the token tier; a guard has \
             turned it off and every other assertion would still pass",
            rate * 100.0
        );
    }

    /// Report the region tier's share and fail if it is untested.
    ///
    /// The token tier's floor exists because a uniformly-placed edit rarely
    /// lands inside prose. This one exists for a different reason: the region
    /// tier is tried *behind* the window tiers, so it only ever answers what a
    /// window declined. A driver that measured it on short documents would
    /// measure the window tiers instead, which is why
    /// [`region_snippets`] are kilobytes rather than the tens of bytes the
    /// hazard corpus uses, and why this driver runs with the cost guards
    /// *enforced*.
    fn assert_exercised_the_region_tier(&self, what: &str, floor: f64) {
        let judged = self.spliced + self.declined;
        let rate = if judged == 0 {
            0.0
        } else {
            self.region_tier as f64 / judged as f64
        };
        eprintln!(
            "{what}: {} of {judged} judged edits took the region tier ({:.1}%)",
            self.region_tier,
            rate * 100.0
        );
        assert!(
            rate >= floor,
            "{what}: only {:.1}% of edits took the region tier (floor {:.1}%); a \
             guard has turned it off and every other assertion would still pass",
            rate * 100.0,
            floor * 100.0
        );
    }
}

struct Run<'a> {
    options: &'a ParserOptions,
    cost_guards: CostGuards,
    stats: &'a mut FuzzStats,
}

fn random_edit(rng: &mut Lcg, text: &str) -> ((usize, usize), &'static str) {
    let start = clamp_to_char_boundary(text, rng.below(text.len() + 1));
    let max_delete = (text.len() - start).min(24);
    let end = clamp_to_char_boundary(text, start + rng.below(max_delete + 1)).max(start);
    let insert = INSERTS[rng.below(INSERTS.len())];
    ((start, end), insert)
}

/// Inserts biased toward ordinary prose, for the token-tier driver.
///
/// Two thirds of the alphabet is text the tier should accept and one third is
/// deliberately hazardous, because a prose-bias mode that only ever inserts
/// safe bytes tests the happy path and calls it coverage. The hazardous share
/// is what makes the driver check that the tier *declines* correctly from
/// inside a token, which the uniform driver almost never reaches.
const PROSE_INSERTS: &[&str] = &[
    "a",
    "e",
    "x",
    " ",
    "word",
    " and ",
    "ing",
    ".",
    ",",
    "'",
    "\"",
    "?",
    ";",
    "0",
    "42",
    "(",
    ")",
    "/",
    "&",
    "\t",
    "é",
    "中",
    "😀",
    "",
    // Hazards reachable only from inside a token.
    "*",
    "_",
    "`",
    "[",
    "]",
    "|",
    "$",
    "~",
    "^",
    "@",
    "\\",
    ":",
    "-",
    "=",
    "#",
    ">",
    "!",
    "%",
    "\n",
    "  ",
    "http://x.com",
    "www.example.com",
];

fn prose_edit(rng: &mut Lcg, base: &Base) -> Option<((usize, usize), &'static str)> {
    let tokens: Vec<_> = base
        .tree
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::TEXT && !token.text().is_empty())
        .collect();
    let token = tokens.get(rng.below(tokens.len().max(1)))?;

    let range = token.text_range();
    let (t0, t1) = (usize::from(range.start()), usize::from(range.end()));
    let text = token.text();

    let start = clamp_to_char_boundary(text, rng.below(text.len() + 1));
    let max_delete = (text.len() - start).min(6);
    let end = clamp_to_char_boundary(text, start + rng.below(max_delete + 1)).max(start);
    debug_assert!(t0 + end <= t1);

    let insert = PROSE_INSERTS[rng.below(PROSE_INSERTS.len())];
    Some(((t0 + start, t0 + end), insert))
}

/// A pseudo-random edit landing inside one top-level `DOCUMENT` child of
/// `base` -- the placement the region tier is defined over.
///
/// The uniform generator lands anywhere, including on the blank lines between
/// children, where the region widens to swallow both neighbours. That is a
/// shape worth fuzzing, but it is not the *common* one, and a driver made of it
/// would spend its time on the widened case. This one picks a real child first,
/// then an offset inside it, which is what an editor's keystroke looks like.
///
/// Uses the hazard-biased [`INSERTS`] rather than prose, because the region
/// tier's interesting guards are the ones that fire on delimiters: a fence, a
/// `:::`, a dash rule, a pipe row. Those must decline, and declining for the
/// right reason is half of what this driver checks.
///
/// Returns `None` when the tree has no non-blank child.
fn region_edit(rng: &mut Lcg, base: &Base) -> Option<((usize, usize), &'static str)> {
    let text = base.tree.text().to_string();
    let children: Vec<_> = base
        .tree
        .children()
        .filter(|child| child.kind() != SyntaxKind::BLANK_LINE && !child.text_range().is_empty())
        .collect();
    let child = children.get(rng.below(children.len().max(1)))?;

    let range = child.text_range();
    let (c0, c1) = (usize::from(range.start()), usize::from(range.end()));
    let start = clamp_to_char_boundary(&text, c0 + rng.below(c1 - c0));
    let max_delete = (c1 - start).min(8);
    let end = clamp_to_char_boundary(&text, start + rng.below(max_delete + 1)).max(start);

    Some(((start, end), INSERTS[rng.below(INSERTS.len())]))
}

fn fuzz_region_edits(
    tier: &Tier,
    name: &str,
    text: &str,
    batches: usize,
    seed: u64,
    cost_guards: CostGuards,
    stats: &mut FuzzStats,
) {
    let mut rng = Lcg(seed);
    let options = tier.options();
    let mut run = Run {
        options: &options,
        cost_guards,
        stats,
    };
    for batch in 0..batches {
        let mut current = text.to_string();
        let Some(mut base) = Base::parse(&current, run.options) else {
            eprintln!(
                "base parse is lossy (known-bug class, skipped): snippet {name}, tier {}",
                tier.name
            );
            run.stats.skipped_lossy += 1;
            break;
        };
        let chain_len = 3 + rng.below(6);
        for step in 0..chain_len {
            let Some((old_edit, insert)) = region_edit(&mut rng, &base) else {
                break;
            };
            let context = format!(
                "snippet {name}, tier {}, seed {seed}, region batch #{batch}, step #{step}",
                tier.name
            );
            let Some(next) = check_edit(&context, &current, &mut run, &base, old_edit, insert)
            else {
                break;
            };
            base = next;
            current = apply_edit(&current, old_edit, insert);
        }
    }
}

/// The parse a splice builds on: the previous tree and the syntax errors that
/// go with it. Chains carry both forward, because both are spliced.
struct Base {
    tree: panache_parser::SyntaxNode,
    errors: Vec<SyntaxError>,
}

impl Base {
    fn parse(text: &str, options: &ParserOptions) -> Option<Self> {
        let (tree, errors) = catch_unwind(AssertUnwindSafe(|| {
            parse_with_errors(text, Some(options.clone()))
        }))
        .ok()?;
        (tree.text() == text).then_some(Self { tree, errors })
    }
}

/// Apply one edit incrementally against `base` and check the invariants.
/// Returns the spliced tree and its errors so chains can build on them, or
/// `None` when the case must be skipped because the *full parser* is lossy on
/// the edited text: with a broken oracle the splice cannot be judged. Every
/// skip prints its reproducing input, because a skip is a *full-parser* bug
/// worth minimizing into a red test in `incremental_regressions.rs` (that is
/// where the refdef-in-list-item reorder, the `---`-after-blockquote marker
/// duplication, and the line-block panic came from); when a block-parser fix
/// lands, the skip counter drops.
fn check_edit(
    context: &str,
    before: &str,
    run: &mut Run,
    base: &Base,
    old_edit: (usize, usize),
    insert: &str,
) -> Option<Base> {
    let (old_tree, old_errors) = (&base.tree, &base.errors[..]);
    let updated = apply_edit(before, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());

    let (full, full_errors) = match catch_unwind(AssertUnwindSafe(|| {
        parse_with_errors(&updated, Some(run.options.clone()))
    })) {
        Ok(full) => full,
        Err(_) => {
            eprintln!(
                "full parser panicked (known-bug class, skipped): {context}\n  \
                 before: {before:?}\n  edit {old_edit:?} insert {insert:?}"
            );
            run.stats.skipped_lossy += 1;
            return None;
        }
    };
    let round_tripped = full.text().to_string();
    if round_tripped != updated {
        eprintln!(
            "full parse is lossy (known-bug class, skipped): {context}\n  \
             input:  {updated:?}\n  output: {round_tripped:?}"
        );
        run.stats.skipped_lossy += 1;
        return None;
    }

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        reparse_or_full_with_cost_guards(
            &updated,
            Some(run.options.clone()),
            old_tree,
            old_errors,
            old_edit,
            new_edit,
            run.cost_guards,
        )
    }));
    let inc = match outcome {
        Ok(inc) => inc,
        Err(_) => panic!(
            "in-crate oracle diverged: {context}\n  before: {before:?}\n  \
             edit {old_edit:?} insert {insert:?}\n  after: {updated:?}"
        ),
    };

    if inc.strategy == "full_reparse" {
        run.stats.declined += 1;
    } else {
        run.stats.spliced += 1;
        if inc.strategy == "token" {
            run.stats.token_tier += 1;
        }
        match inc.strategy {
            "region" => run.stats.region_tier += 1,
            "section_window" => run.stats.section_window += 1,
            "suffix_window" => run.stats.suffix_window += 1,
            _ => {}
        }
    }

    assert_eq!(
        inc.tree.text().to_string(),
        updated,
        "losslessness violated ({}): {context}\n  before: {before:?}\n  \
         edit {old_edit:?} insert {insert:?}",
        inc.strategy
    );

    assert_eq!(
        fingerprint(&inc.tree),
        fingerprint(&full),
        "structural divergence ({}): {context}\n  before: {before:?}\n  \
         edit {old_edit:?} insert {insert:?}\n  after: {updated:?}",
        inc.strategy
    );

    assert_eq!(
        inc.errors, full_errors,
        "syntax-error divergence ({}): {context}\n  before: {before:?}\n  \
         edit {old_edit:?} insert {insert:?}\n  after: {updated:?}",
        inc.strategy
    );

    Some(Base {
        tree: inc.tree,
        errors: inc.errors,
    })
}

fn fuzz_single_edits(
    tier: &Tier,
    name: &str,
    text: &str,
    iters: usize,
    seed: u64,
    cost_guards: CostGuards,
    stats: &mut FuzzStats,
) {
    let mut rng = Lcg(seed);
    let options = tier.options();
    let Some(base) = Base::parse(text, &options) else {
        eprintln!(
            "base parse is lossy (known-bug class, skipped): snippet {name}, tier {}",
            tier.name
        );
        stats.skipped_lossy += 1;
        return;
    };
    let mut run = Run {
        options: &options,
        cost_guards,
        stats,
    };
    for i in 0..iters {
        let (old_edit, insert) = random_edit(&mut rng, text);
        let context = format!(
            "snippet {name}, tier {}, seed {seed}, single edit #{i}",
            tier.name
        );
        check_edit(&context, text, &mut run, &base, old_edit, insert);
    }
}

fn fuzz_chained_edits(
    tier: &Tier,
    name: &str,
    text: &str,
    batches: usize,
    seed: u64,
    cost_guards: CostGuards,
    stats: &mut FuzzStats,
) {
    let mut rng = Lcg(seed);
    let options = tier.options();
    let mut run = Run {
        options: &options,
        cost_guards,
        stats,
    };
    for batch in 0..batches {
        let mut current = text.to_string();
        let Some(mut base) = Base::parse(&current, run.options) else {
            eprintln!(
                "base parse is lossy (known-bug class, skipped): snippet {name}, tier {}",
                tier.name
            );
            run.stats.skipped_lossy += 1;
            break;
        };
        let chain_len = 2 + rng.below(3);
        for step in 0..chain_len {
            let (old_edit, insert) = random_edit(&mut rng, &current);
            let context = format!(
                "snippet {name}, tier {}, seed {seed}, batch #{batch}, chain step #{step}",
                tier.name
            );
            let Some(next) = check_edit(&context, &current, &mut run, &base, old_edit, insert)
            else {
                break;
            };
            base = next;
            current = apply_edit(&current, old_edit, insert);
        }
    }
}

fn fuzz_prose_edits(
    tier: &Tier,
    name: &str,
    text: &str,
    batches: usize,
    seed: u64,
    cost_guards: CostGuards,
    stats: &mut FuzzStats,
) {
    let mut rng = Lcg(seed);
    let options = tier.options();
    let mut run = Run {
        options: &options,
        cost_guards,
        stats,
    };
    for batch in 0..batches {
        let mut current = text.to_string();
        let Some(mut base) = Base::parse(&current, run.options) else {
            eprintln!(
                "base parse is lossy (known-bug class, skipped): snippet {name}, tier {}",
                tier.name
            );
            run.stats.skipped_lossy += 1;
            break;
        };
        let chain_len = 3 + rng.below(6);
        for step in 0..chain_len {
            let Some((old_edit, insert)) = prose_edit(&mut rng, &base) else {
                break;
            };
            let context = format!(
                "snippet {name}, tier {}, seed {seed}, prose batch #{batch}, step #{step}",
                tier.name
            );
            let Some(next) = check_edit(&context, &current, &mut run, &base, old_edit, insert)
            else {
                break;
            };
            base = next;
            current = apply_edit(&current, old_edit, insert);
        }
    }
}

/// Per-tier seed: the tier index must participate, or every tier would
/// replay the identical edit sequence and the extra work would buy nothing.
fn seed(base: u64, snippet_index: usize, tier_index: usize) -> u64 {
    base ^ ((snippet_index as u64) << 8) ^ ((tier_index as u64) << 24)
}

#[test]
fn hazard_snippets_single_edits() {
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        let iters = iterations(tier.singles);
        for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
            fuzz_single_edits(
                tier,
                name,
                text,
                iters,
                seed(0x9E3779B9, index, tier_index),
                CostGuards::Ignored,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("single edits");
}

#[test]
fn hazard_snippets_chained_edits() {
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        let batches = iterations(tier.batches);
        for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
            fuzz_chained_edits(
                tier,
                name,
                text,
                batches,
                seed(0x51ED2701, index, tier_index),
                CostGuards::Ignored,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("chained edits");
}

/// Real documents from `benches/documents/`: a second corpus tier with
/// random edits at random offsets. Iteration counts are low by default
/// (each check costs multiple full parses of a large document); scale with
/// `PANACHE_FUZZ_ITERS` for the graduation gate.
///
/// The corpus is `.qmd`, so only the tiers that would actually be used on it
/// (`pandoc`, `quarto`) get a budget here; the option holes the other tiers
/// exist for are covered by the hazard snippets, which are cheap.
#[test]
fn real_documents_random_edits() {
    let docs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benches/documents");
    let names = ["small.qmd", "configuration.qmd", "tables.qmd", "math.qmd"];
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        if tier.real_docs == 0 {
            continue;
        }
        let iters = iterations(tier.real_docs);
        for (index, name) in names.iter().enumerate() {
            let path = docs_dir.join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                eprintln!("skipping absent corpus document {}", path.display());
                stats.skipped_absent += 1;
                continue;
            };
            fuzz_single_edits(
                tier,
                name,
                &text,
                iters,
                seed(0xC0FFEE, index, tier_index),
                CostGuards::Enforced,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("real documents");
}

/// The token tier's own driver: every edit lands inside a `TEXT` token, which
/// is the only placement that reaches it.
///
/// Runs with [`CostGuards::Ignored`] like the other snippet drivers, though it
/// makes no difference here --- the token tier has no window and consults no
/// cost guard. Kept the same so the snippets behave identically across drivers.
/// The hit-rate floor is asserted over [`PROSE_SNIPPETS`] only, and the two
/// corpora are tallied separately on purpose. [`HAZARD_SNIPPETS`] is
/// construct-heavy by design --- most of its `TEXT` tokens sit in code blocks,
/// attributes, and table cells, which the tier declines *correctly* --- so a
/// floor over the union would measure the corpus mix rather than the tier, and
/// would move every time a snippet is added. Both corpora are still *walked*,
/// because declining correctly is the half of the tier that has to stay sound.
#[test]
fn prose_placed_chained_edits() {
    let mut hazard = FuzzStats::default();
    let mut prose = FuzzStats::default();

    for (tier_index, tier) in TIERS.iter().enumerate() {
        let batches = iterations(tier.batches);
        for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
            fuzz_prose_edits(
                tier,
                name,
                text,
                batches,
                seed(0x5EED_1DEA, index, tier_index),
                CostGuards::Ignored,
                &mut hazard,
            );
        }
        for (index, (name, text)) in PROSE_SNIPPETS.iter().enumerate() {
            fuzz_prose_edits(
                tier,
                name,
                text,
                batches,
                seed(0x9BADF00D, index, tier_index),
                CostGuards::Ignored,
                &mut prose,
            );
        }
    }

    hazard.assert_exercised_the_splice("prose-placed edits on hazard snippets");
    prose.assert_exercised_the_splice("prose-placed edits on prose snippets");
    prose.assert_exercised_the_token_tier("prose-placed edits on prose snippets");
}

/// The region tier's own driver: every edit lands inside a top-level child of a
/// document long enough that the window tiers decline it.
///
/// Two things separate this from every other driver here, and both follow from
/// the tier being tried *behind* the window tiers.
///
/// It runs with [`CostGuards::Enforced`], not `Ignored`. The other snippet
/// drivers turn the cost guards off because their inputs are tens of bytes, so
/// every window covers most of the document and the cutoff would decline
/// everything before a correctness guard ran. Here the cutoff is exactly what
/// *routes* work to this tier: with it off, a window would answer first and this
/// driver would measure the window tiers under a region name.
///
/// And its corpus is kilobytes rather than tens of bytes, for the same reason.
/// [`region_snippets`] are built from many small blocks so that an edit in the
/// first fifth leaves more than the reparse cascade's window-share cutoff of the document
/// downstream --- the population the tier exists for --- while the region itself
/// stays well under the always-try floor.
///
/// The floor is 68% against a measured 71.9-72.3%, stable across 1x, 4x, and
/// 10x iterations --- 5% under the lowest observed run, the same margin
/// convention the bench floors use.
///
/// It was 16% while the tier was tried *behind* the window tiers, because only
/// an edit in roughly the first sixth of a document left enough downstream for
/// a window to be declined and the tier to be offered the rest. Promoting it
/// took the same corpus and the same seeds from 17.5% to 72.1%, which is the
/// clearest single statement of what the promotion did: on multi-block
/// documents a region is what answers an edit, and a window is the exception.
///
/// The floor is not a rate to hold at a decimal. It is there to catch a guard
/// that turns the tier off, which nothing else in the suite would notice,
/// because declining is always sound.
#[test]
fn region_placed_chained_edits() {
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        let batches = iterations(tier.batches);
        for (index, (name, text)) in region_snippets().iter().enumerate() {
            fuzz_region_edits(
                tier,
                name,
                text,
                batches,
                seed(0x3E9_1015, index, tier_index),
                CostGuards::Enforced,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("region-placed edits");
    stats.assert_exercised_the_region_tier("region-placed edits", 0.68);
}

/// Multi-block documents for the region tier, a few KB each.
///
/// Generated rather than written out because the size is the point: the tier
/// only answers what a window declines, and a window is declined by leaving
/// more than the cascade's 85% window-share cutoff of the document downstream.
/// A tens-of-bytes snippet cannot express that.
///
/// Each shape puts a different construct at top level, because the region is a
/// run of top-level children and what those children *are* decides which
/// boundary-parse guard has to fire: a fenced div and a table can absorb their
/// neighbour, a list can continue across a blank line, a setext-able paragraph
/// can be promoted from below.
fn region_snippets() -> Vec<(String, String)> {
    let mut snippets = Vec::new();

    let mut paragraphs = String::new();
    for i in 0..120 {
        paragraphs.push_str(&format!(
            "Paragraph number {i} with several words in it.\n\n"
        ));
    }
    snippets.push(("many_paragraphs".to_owned(), paragraphs.clone()));

    let mut mixed = String::new();
    for i in 0..60 {
        mixed.push_str(&format!(
            "## Section {i}\n\nBody prose for section {i}.\n\n"
        ));
        mixed.push_str(&format!("- item {i} one\n- item {i} two\n\n"));
    }
    snippets.push(("headings_prose_and_lists".to_owned(), mixed));

    let mut divs = String::new();
    for i in 0..50 {
        divs.push_str(&format!("Prose before div {i}.\n\n"));
        divs.push_str(&format!("::: note\nDiv body {i} here.\n:::\n\n"));
    }
    snippets.push(("fenced_divs_between_paragraphs".to_owned(), divs));

    let mut tables = String::new();
    for i in 0..40 {
        tables.push_str(&format!("Prose before table {i}.\n\n"));
        tables.push_str("| a | b |\n|---|---|\n| 1 | 2 |\n\n");
    }
    snippets.push(("tables_between_paragraphs".to_owned(), tables));

    let mut fences = String::new();
    for i in 0..50 {
        fences.push_str(&format!("Prose before block {i}.\n\n"));
        fences.push_str(&format!("```rust\nlet x = {i};\n```\n\n"));
    }
    snippets.push(("code_blocks_between_paragraphs".to_owned(), fences));

    snippets.push((
        "many_paragraphs_crlf".to_owned(),
        paragraphs.replace('\n', "\r\n"),
    ));

    snippets
}

const PROSE_SNIPPETS: &[(&str, &str)] = &[
    (
        "plain_paragraphs",
        "Some ordinary prose in a paragraph.\n\nA second paragraph of ordinary prose.\n",
    ),
    (
        "prose_with_punctuation",
        "Words, clauses; and more -- with 'quotes' and \"doubles\" and (parens).\n",
    ),
    (
        "ordered_marker_near_miss",
        "12 apples in a basket here\n\n12.apples in a basket\n",
    ),
    (
        "unmatched_delimiters_in_prose",
        "a*b c*d and more words\n\nx_y_z with underscores\n",
    ),
    (
        "trailing_whitespace_lines",
        "a line with one trailing space \nand a continuation line\n",
    ),
    (
        "pipe_above_a_delimiter_row",
        "foo bar and more\n--- | ---\n",
    ),
    (
        "indented_paragraph_beside_a_list",
        "- item\n\n foo bar and more prose\n",
    ),
    (
        "prose_around_a_refdef",
        "See the docs for more detail.\n\n[docs]: https://example.com/docs\n",
    ),
    (
        "multiline_paragraph",
        "The first line of a paragraph\nthe second line of the same one\nand a third line here\n",
    ),
    (
        "prose_in_containers",
        "> quoted prose with several words\n\n- list prose with several words\n",
    ),
];
