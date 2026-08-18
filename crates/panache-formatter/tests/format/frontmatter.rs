use panache_formatter::format;

#[test]
fn front_matter_and_paragraph() {
    let input = "---\ntitle: hi\n---\n\nHello world\n";
    let output = format(input, None, None);

    assert!(output.contains("title: hi"));
    assert!(output.contains("Hello world"));
}
