use panache_formatter::format;

#[test]
fn strong_opening_with_code_span_keeps_space() {
    let input = "See **`reg_schema` treats** the block.\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "See **`reg_schema` treats** the block.\n");
    similar_asserts::assert_eq!(format(&output, None, None), output);
}

#[test]
fn emphasis_opening_with_code_span_keeps_space() {
    let input = "_`x` y_\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "*`x` y*\n");
    similar_asserts::assert_eq!(format(&output, None, None), output);
}

#[test]
fn strong_with_code_span_in_middle_is_unchanged() {
    let input = "**text `code` word**\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "**text `code` word**\n");
    similar_asserts::assert_eq!(format(&output, None, None), output);
}
