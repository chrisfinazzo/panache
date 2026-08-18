use panache_formatter::format;

#[test]
fn preserves_leading_spaces() {
    let input = "text `  | a | b |` more\n";
    let output = format(input, None, None);
    assert!(
        output.contains("`  | a | b |`"),
        "leading spaces should be preserved: {output:?}"
    );
}

#[test]
fn preserves_trailing_spaces() {
    let input = "text `code  ` more\n";
    let output = format(input, None, None);
    assert!(
        output.contains("`code  `"),
        "trailing spaces should be preserved: {output:?}"
    );
}

#[test]
fn preserves_spaces_on_both_sides() {
    let input = "a `  x  ` b\n";
    let output = format(input, None, None);
    assert!(
        output.contains("`  x  `"),
        "spaces on both sides should be preserved: {output:?}"
    );
}

#[test]
fn preserves_all_space_content() {
    let input = "a `   ` b\n";
    let output = format(input, None, None);
    assert!(
        output.contains("`   `"),
        "all-space content should be preserved: {output:?}"
    );
}

#[test]
fn verbatim_code_span_is_idempotent() {
    let input = "text `  | a | b |` more\n";
    let once = format(input, None, None);
    let twice = format(&once, None, None);
    assert_eq!(once, twice, "formatting should be idempotent");
}
