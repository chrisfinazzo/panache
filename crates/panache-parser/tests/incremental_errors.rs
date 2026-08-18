//! Error-splicing matrix for incremental reparsing.
//!
//! The governing invariant covers the syntax-error vector as much as the tree.
//! The *window* merge has two buckets — errors in the retained prefix, carried
//! verbatim, and errors from the window parse, shifted to host coordinates —
//! because both window strategies parse to EOF, so nothing downstream of the
//! seam survives to be moved. The *region* merge has a third: a bounded region
//! leaves a live suffix, whose errors are carried and shifted by the edit delta.
//!
//! Each bucket can be *unchanged*, *fixed*, or *introduced* by an edit, and each
//! reparse strategy reaches them differently, so this file is that matrix:
//! {unchanged, fixed, introduced} x {suffix window, section window, region,
//! full-parse bail}.
//!
//! Malformed YAML is the only source of syntax errors, so every case is built
//! from a frontmatter or mid-document metadata block. Mid-document metadata is
//! a Pandoc-dialect feature, which is why these run under default options.
//!
//! The strategy is pinned in every case: an error assertion that silently
//! started running against a full-parse fallback would prove nothing.

use panache_parser::parser::parse_with_errors;

mod common;
use common::reparse_or_full;

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

fn check(input: &str, find: &str, insert: &str, expected_strategy: &str, expected_errors: usize) {
    let (old_tree, old_errors) = parse_with_errors(input, None);
    let start = input
        .find(find)
        .unwrap_or_else(|| panic!("{find:?} not in {input:?}"));
    let old_edit = (start, start + find.len());
    let updated = apply_edit(input, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());

    let inc = reparse_or_full(&updated, None, &old_tree, &old_errors, old_edit, new_edit);
    let (_, full_errors) = parse_with_errors(&updated, None);

    assert_eq!(
        inc.strategy, expected_strategy,
        "wrong strategy for {find:?} -> {insert:?}"
    );
    assert_eq!(
        inc.errors, full_errors,
        "spliced errors diverged from a full parse ({})",
        inc.strategy
    );
    assert_eq!(
        inc.errors.len(),
        expected_errors,
        "unexpected error count ({}): {:?}",
        inc.strategy,
        inc.errors
    );
}

#[test]
fn suffix_window_unchanged_error_in_the_retained_prefix() {
    check(
        "---\ntitle: [\n---\n\npara one\n\npara two\n",
        "para two",
        "para three",
        "suffix_window",
        1,
    );
}

#[test]
fn suffix_window_error_introduced_inside_the_window() {
    check(
        "para one\n\n---\ntitle: ok\n---\n\npara two\n",
        "ok",
        "[",
        "suffix_window",
        1,
    );
}

#[test]
fn suffix_window_error_fixed_inside_the_window() {
    check(
        "para one\n\n---\ntitle: [\n---\n\npara two\n",
        "[",
        "ok",
        "suffix_window",
        0,
    );
}

/// Both buckets at once: a prefix error that must survive and a window error
/// that must not be double-counted against it.
#[test]
fn suffix_window_carries_prefix_error_while_introducing_a_window_error() {
    check(
        "---\ntitle: [\n---\n\npara one\n\n---\nkey: ok\n---\n\npara two\n",
        "ok",
        "[",
        "suffix_window",
        2,
    );
}

const SECTIONS: &str =
    "# One\n\npara one\n\n## Two\n\n---\ntitle: ok\n---\n\npara two\n\n# Three\n\npara three\n";

const SECTIONS_BROKEN: &str =
    "# One\n\npara one\n\n## Two\n\n---\ntitle: [\n---\n\npara two\n\n# Three\n\npara three\n";

#[test]
fn section_window_unchanged_error_before_the_window() {
    check(
        SECTIONS_BROKEN,
        "para three",
        "para four",
        "section_window",
        1,
    );
}

#[test]
fn section_window_error_introduced_inside_the_window() {
    check(SECTIONS, "ok", "[", "section_window", 1);
}

#[test]
fn section_window_error_fixed_inside_the_window() {
    check(SECTIONS_BROKEN, "[", "ok", "section_window", 0);
}

#[test]
fn full_reparse_bail_still_reports_errors() {
    check(
        "---\ntitle: [\n---\n\npara one\n\n[x]: /url\n\npara two\n",
        "/url",
        "/other",
        "full_reparse",
        1,
    );
}

#[test]
fn full_reparse_bail_reports_an_error_the_edit_introduces() {
    check(
        "---\ntitle: ok\n---\n\n[x]: /url\n\npara two\n",
        "ok",
        "[",
        "full_reparse",
        1,
    );
}

#[test]
fn full_reparse_bail_reports_an_error_the_edit_fixes() {
    check(
        "---\ntitle: [\n---\n\n[x]: /url\n\npara two\n",
        "[",
        "ok",
        "full_reparse",
        0,
    );
}

fn with_trailing_filler(input: &str) -> String {
    let mut out = String::from(input);
    assert!(out.ends_with("\n\n"), "filler must start on a blank line");
    for index in 0..200 {
        out.push_str(&format!("Filler paragraph {index}.\n\n"));
    }
    out
}

#[test]
fn region_carries_an_error_before_it_and_shifts_one_after_it() {
    let input =
        with_trailing_filler("---\ntitle: [\n---\n\nAlpha para.\n\n---\ntrailing: [\n---\n\n");
    check(&input, "Alpha", "Alpha and then some more", "region", 2);
}

#[test]
fn region_reports_an_error_the_edit_introduces_inside_it() {
    let input = with_trailing_filler("Intro para.\n\n---\nkey: ok\n---\n\n");
    check(&input, "ok", "[", "region", 1);
}

#[test]
fn region_reports_an_error_the_edit_fixes_inside_it() {
    let input = with_trailing_filler("Intro para.\n\n---\nkey: [\n---\n\n");
    check(&input, "[", "ok", "region", 0);
}
