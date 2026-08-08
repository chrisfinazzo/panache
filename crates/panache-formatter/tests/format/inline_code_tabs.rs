use panache_formatter::{ConfigBuilder, TabStopMode, format};

// A tab inside a code span is expanded before the reader runs, from the tab's
// column in the *source line* — so the number of spaces it stands for depends
// on what precedes the span, not on the span's own content. Expanding from
// column 0 of the content rewrote `` a`x\ty`b `` to three spaces where pandoc
// reads one, i.e. formatting changed the document's meaning.
//
// Every expectation below is `pandoc 3.9.0.2 -f markdown -t native` output,
// modulo the leading/trailing padding pandoc trims and panache preserves.

fn formatted(input: &str) -> String {
    format(input, None, None)
}

#[test]
fn tab_expands_from_the_span_source_column() {
    // The tab sits at column 3, so it reaches the stop at column 4: one space.
    assert!(
        formatted("a`x\ty`b\n").contains("`x y`"),
        "{:?}",
        formatted("a`x\ty`b\n")
    );
}

#[test]
fn tab_expands_from_column_zero_when_the_span_starts_the_line() {
    // Backtick at column 0, so the tab sits at column 2: two spaces.
    assert!(
        formatted("`x\ty`\n").contains("`x  y`"),
        "{:?}",
        formatted("`x\ty`\n")
    );
}

#[test]
fn tab_on_a_continuation_line_expands_from_that_line() {
    // The joined line break is worth one space and the leading tab expands
    // from column 0 of its own line: four more.
    assert!(
        formatted("`x\n\ty`\n").contains("`x     y`"),
        "{:?}",
        formatted("`x\n\ty`\n")
    );
}

#[test]
fn tab_expands_past_a_blockquote_marker() {
    let out = formatted("> a\n> \t`x\n> \ty`\n");
    assert!(out.contains("`x   y`"), "{out:?}");
}

#[test]
fn tab_expands_past_a_list_item_content_column() {
    // The item's content column (2) is gobbled off the continuation line, so
    // only the columns past it survive into the payload.
    let out = formatted("- a\n\t`x\n\ty`\n");
    assert!(out.contains("`x   y`"), "{out:?}");
}

#[test]
fn tab_expansion_is_idempotent() {
    for input in [
        "a`x\ty`b\n",
        "`x\n\ty`\n",
        "> a\n> \t`x\n> \ty`\n",
        "- a\n\t`x\n\ty`\n",
    ] {
        let once = formatted(input);
        let twice = formatted(&once);
        assert_eq!(once, twice, "not idempotent for {input:?}");
    }
}

#[test]
fn preserve_mode_leaves_tabs_alone() {
    let config = ConfigBuilder::default()
        .tab_stops(TabStopMode::Preserve)
        .build();
    let out = format("a`x\ty`b\n", Some(config), None);
    assert!(out.contains("`x\ty`"), "{out:?}");
}

#[test]
fn tab_width_two_uses_two_column_stops() {
    let config = ConfigBuilder::default().tab_width(2).build();
    // `x` sits at column 2, so the tab at column 3 reaches the stop at 4.
    let out = format("a`x\ty`b\n", Some(config), None);
    assert!(out.contains("`x y`"), "{out:?}");
}
