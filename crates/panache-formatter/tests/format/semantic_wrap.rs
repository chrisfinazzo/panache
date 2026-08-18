use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

fn cfg_semantic() -> Config {
    Config {
        wrap: Some(WrapMode::Semantic),
        ..Default::default()
    }
}

fn run(input: &str) -> String {
    let out = format(input, Some(cfg_semantic()), None);
    let out2 = format(&out, Some(cfg_semantic()), None);
    assert_eq!(out, out2, "semantic wrapping must be idempotent");
    out
}

#[test]
fn adds_sentence_breaks_and_preserves_existing_breaks() {
    let input = "First sentence ends here. A question asks:\nthen it continues.\n";
    let expected = "First sentence ends here.\nA question asks:\nthen it continues.\n";
    assert_eq!(run(input), expected);
}

#[test]
fn soft_break_only_breaks_on_newline_not_space() {
    let input = "one two three four five.\n";
    assert_eq!(run(input), "one two three four five.\n");
}

#[test]
fn long_sentence_without_breaks_stays_on_one_line() {
    let input =
        "This is one long sentence that runs well past eighty columns yet carries no soft break.\n";
    assert_eq!(run(input), input);
}

#[test]
fn trailing_newline_does_not_emit_empty_line() {
    let input = "Only one sentence.\n";
    assert_eq!(run(input), "Only one sentence.\n");
}

#[test]
fn preserves_authored_clause_break_after_comma() {
    let input = "First clause,\nsecond clause. Next sentence. Done.\n";
    let expected = "First clause,\nsecond clause.\nNext sentence.\nDone.\n";
    assert_eq!(run(input), expected);
}

#[test]
fn abbreviations_do_not_trigger_breaks() {
    let input = "We use tools, e.g. the parser,\nand more. End.\n";
    let expected = "We use tools, e.g. the parser,\nand more.\nEnd.\n";
    assert_eq!(run(input), expected);
}

#[test]
fn sentence_break_keeps_inline_list_markers_off_line_start() {
    let input = "Hear from us in 60 days. 1. Tell us your name. 2. Describe the error.\n";
    let expected = "Hear from us in 60 days. 1.\nTell us your name. 2.\nDescribe the error.\n";
    assert_eq!(run(input), expected);
}
