use panache_formatter::format;

#[test]
fn atx_trailing_hashes_are_removed() {
    let input = "### A level-three heading ###\n";
    let expected = "### A level-three heading\n";
    let out = format(input, None, None);
    assert_eq!(out, expected);

    // idempotent
    assert_eq!(format(&out, None, None), expected);
}

#[test]
fn atx_trailing_hashes_are_kept_in_front_of_a_brace_block() {
    // The closing run is what keeps the braces out of the heading's attributes:
    // pandoc reads `# foo {#id} #` as `Header 1 (foo-id) [Str "foo", Space,
    // Str "{#id}"]`, so dropping the run would rewrite the id to `id`.
    let input = "# foo {#id} #\n";
    let out = format(input, None, None);
    assert_eq!(out, input);
    assert_eq!(format(&out, None, None), input);
}

#[test]
fn atx_trailing_hashes_are_kept_between_content_and_attributes() {
    let input = "# foo {#id} # {.cls}\n";
    let out = format(input, None, None);
    assert_eq!(out, input);
    assert_eq!(format(&out, None, None), input);
}

#[test]
fn atx_trailing_hashes_are_removed_without_a_space_in_front() {
    // pandoc closes the heading on the run even with no space in front:
    // `# foo#` is `Header 1 (foo) [Str "foo"]`.
    let input = "# foo#\n";
    let expected = "# foo\n";
    let out = format(input, None, None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, None, None), expected);
}

#[test]
fn atx_escaped_trailing_hash_is_content() {
    // `# foo \##` is `[Str "foo", Space, Str "#"]`: only the last hash is
    // decoration, and the escaped one has to survive with its backslash.
    let input = "# foo \\##\n";
    let expected = "# foo \\#\n";
    let out = format(input, None, None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, None, None), expected);

    let input = "# foo\\#\n";
    let out = format(input, None, None);
    assert_eq!(out, input);
    assert_eq!(format(&out, None, None), input);
}

#[test]
fn atx_trailing_hashes_are_kept_in_front_of_a_content_hash() {
    // Content of `# foo # #` is `[Str "foo", Space, Str "#"]`. Dropping the run
    // would leave `# foo #`, whose trailing hash pandoc reads as the closing
    // run instead --- so the run stays.
    let input = "# foo # #\n";
    let out = format(input, None, None);
    assert_eq!(out, input);
    assert_eq!(format(&out, None, None), input);
}

#[test]
fn atx_trailing_hash_is_content_under_commonmark() {
    // CommonMark requires a space in front of the run, so the hash is content:
    // `# foo#` is `<h1>foo#</h1>`.
    let cfg = commonmark_config();

    let input = "# foo#\n";
    let out = format(input, Some(cfg.clone()), None);
    assert_eq!(out, input);
    assert_eq!(format(&out, Some(cfg), None), input);
}

#[test]
fn atx_trailing_hashes_are_kept_in_front_of_a_content_hash_under_commonmark() {
    // CommonMark closes `# foo # #` on the last hash too, since that one *is*
    // preceded by a space --- so the run stays load-bearing here as well.
    let cfg = commonmark_config();

    let input = "# foo # #\n";
    let out = format(input, Some(cfg.clone()), None);
    assert_eq!(out, input);
    assert_eq!(format(&out, Some(cfg), None), input);
}

fn commonmark_config() -> panache_formatter::Config {
    let flavor = panache_formatter::config::Flavor::CommonMark;
    panache_formatter::Config {
        flavor,
        parser_extensions: panache_formatter::config::Extensions::for_flavor(flavor),
        ..Default::default()
    }
}

#[test]
fn atx_leading_spaces_are_normalized() {
    let input = "   ##   Title   \n";
    let expected = "## Title\n";
    let out = format(input, None, None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, None, None), expected);
}

#[test]
fn consecutive_atx_headings_without_blank_lines_stay_separate() {
    let input = "# unremarkable header 1\n## unremarkable header 2\n### unremarkable header 3\n### unremarkable header 3 ##\n";
    let out = format(input, None, None);
    assert_eq!(format(&out, None, None), out);
}

#[test]
fn atx_heading_interrupting_paragraph_keeps_document_order() {
    let mut cfg = panache_formatter::Config::default();
    // Only the parser extension drives this behavior; the formatter's
    // `blank_before_header` consumer never fires for this input.
    cfg.parser_extensions.blank_before_header = false;

    let input = "Text\n## Title\nMore\n";
    let expected = "Text\n\n## Title\n\nMore\n";
    let out = format(input, Some(cfg.clone()), None);

    assert_eq!(out, expected);
    assert_eq!(
        format(&out, Some(cfg), None),
        expected,
        "must be idempotent"
    );
}

#[test]
fn horizontal_rule_before_setext_like_paragraph_stays_idempotent() {
    let input = "---\nSIL OPEN FONT LICENSE Version 1.1 - 26 February 2007\n-----------------------------------------------------------\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn list_nested_heading_normalizes_inline_code_like_top_level() {
    // Headings inside list items went through a separate formatter that dumped
    // raw `child.text()` instead of formatting inline nodes. Verify the code
    // span is normalized (over-fenced ``code`` collapses to `code`) the same in
    // a list-nested heading as at the top level. (Surrounding-space padding is
    // now preserved verbatim in both paths, so backtick-count normalization is
    // the discriminator that a raw dump would fail.)
    let input = "- # ``code``\n";
    let out = format(input, None, None);
    let top = format("# ``code``\n", None, None);
    assert!(out.contains("`code`"), "code span not normalized: {out:?}");
    assert!(!out.contains("``code``"), "raw code span left: {out:?}");
    assert!(top.contains("`code`"), "top-level baseline: {top:?}");
    assert_eq!(format(&out, None, None), out, "must be idempotent");
}

#[test]
fn list_nested_heading_normalizes_attributes_like_top_level() {
    // The list-nested heading path also skipped attribute normalization.
    let input = "- # Title {#id .a key=val}\n";
    let out = format(input, None, None);
    assert!(
        out.contains("key=\"val\""),
        "attributes not normalized: {out:?}"
    );
    assert_eq!(format(&out, None, None), out, "must be idempotent");
}

#[test]
fn horizontal_rule_expands_to_line_width() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(12)
        .build();
    let input = "***\n";
    let expected = "------------\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn blockquote_horizontal_rule_respects_available_width() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(12)
        .build();
    let input = "> ***\n";
    let expected = "> ----------\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}
