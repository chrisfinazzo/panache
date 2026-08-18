use panache_formatter::{ConfigBuilder, TabStopMode, format};

fn formatted(input: &str) -> String {
    format(input, None, None)
}

#[test]
fn tab_expands_from_the_span_source_column() {
    assert!(
        formatted("a`x\ty`b\n").contains("`x y`"),
        "{:?}",
        formatted("a`x\ty`b\n")
    );
}

#[test]
fn tab_expands_from_column_zero_when_the_span_starts_the_line() {
    assert!(
        formatted("`x\ty`\n").contains("`x  y`"),
        "{:?}",
        formatted("`x\ty`\n")
    );
}

#[test]
fn tab_on_a_continuation_line_expands_from_that_line() {
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
    let out = format("a`x\ty`b\n", Some(config), None);
    assert!(out.contains("`x y`"), "{out:?}");
}
