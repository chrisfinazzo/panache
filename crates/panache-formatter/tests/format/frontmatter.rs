use panache_formatter::format;

#[test]
fn front_matter_and_paragraph() {
    let input = "---\ntitle: hi\n---\n\nHello world\n";
    let output = format(input, None, None);

    assert!(output.contains("title: hi"));
    assert!(output.contains("Hello world"));
}

#[test]
fn leading_blank_before_front_matter_is_idempotent() {
    let input =
        "\n---\ntitle: \"OTexts.com views by country\"\ndate: \"2026-03-26\"\nformat: html\n---\n";
    let once = format(input, None, None);
    let twice = format(&once, None, None);

    assert_eq!(once, twice);
    assert_eq!(
        once,
        "---\ntitle: \"OTexts.com views by country\"\ndate: \"2026-03-26\"\nformat: html\n---\n"
    );
}
