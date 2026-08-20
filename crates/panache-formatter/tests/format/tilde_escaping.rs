use panache_formatter::config::{Extensions, Flavor, FormatterExtensions, WrapMode};
use panache_formatter::{Config, format};

fn config(flavor: Flavor, wrap: WrapMode, line_width: usize) -> Config {
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        formatter_extensions: FormatterExtensions::for_flavor(flavor),
        wrap: Some(wrap),
        line_width,
        ..Config::default()
    }
}

#[test]
fn literal_tildes_stay_unescaped_in_every_flavor() {
    for flavor in [Flavor::Pandoc, Flavor::Gfm, Flavor::CommonMark] {
        let cfg = config(flavor, WrapMode::Sentence, 80);
        for input in ["/label ~bug\n", "~\n"] {
            let output = format(input, Some(cfg.clone()), None);
            similar_asserts::assert_eq!(output, input, "flavor: {flavor:?}");
            similar_asserts::assert_eq!(
                format(&output, Some(cfg.clone()), None),
                output,
                "idempotency for flavor: {flavor:?}"
            );
        }
    }
}

#[test]
fn explicit_tilde_escape_is_preserved() {
    for flavor in [Flavor::Pandoc, Flavor::Gfm, Flavor::CommonMark] {
        let cfg = config(flavor, WrapMode::Reflow, 80);
        let input = "\\~bug\n";
        let output = format(input, Some(cfg.clone()), None);
        similar_asserts::assert_eq!(output, input, "flavor: {flavor:?}");
        similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn wrapping_does_not_create_commonmark_tilde_fences() {
    for flavor in [Flavor::Gfm, Flavor::CommonMark] {
        for wrap in [WrapMode::Reflow, WrapMode::Sentence, WrapMode::Semantic] {
            let cfg = config(flavor, wrap.clone(), 18);
            let input = "Alpha beta gamma. ~~~info trailing words.\n";
            let output = format(input, Some(cfg.clone()), None);
            assert!(
                !output
                    .lines()
                    .any(|line| line.trim_start().starts_with("~~~")),
                "formatter created a fence for {flavor:?}/{wrap:?}: {output:?}"
            );
            assert!(
                !output.contains("\\~"),
                "literal tilde was escaped: {output:?}"
            );
            similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn wrapping_does_not_create_pandoc_tilde_definition_marker() {
    let cfg = config(Flavor::Pandoc, WrapMode::Reflow, 18);
    let input = "alpha beta gamma delta ~ definition trailing\n";
    let output = format(input, Some(cfg.clone()), None);
    assert!(
        !output
            .lines()
            .any(|line| line.trim_start().starts_with("~ ")),
        "formatter created a definition marker: {output:?}"
    );
    assert!(
        !output.contains("\\~"),
        "literal tilde was escaped: {output:?}"
    );
    similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn emitted_pandoc_definition_marker_is_escaped_after_term_candidate() {
    let cfg = config(Flavor::Pandoc, WrapMode::Reflow, 80);
    let input = "T\n\n\n~ definition\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, "T\n\n\\~ definition\n");
    similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn commonmark_flavors_do_not_guard_pandoc_definition_markers() {
    for flavor in [Flavor::Gfm, Flavor::CommonMark] {
        let cfg = config(flavor, WrapMode::Reflow, 80);
        let input = "T\n\n\n~ definition\n";
        let output = format(input, Some(cfg.clone()), None);
        similar_asserts::assert_eq!(output, "T\n\n~ definition\n");
        similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn east_asian_join_does_not_create_pandoc_subscript() {
    let mut cfg = config(Flavor::Pandoc, WrapMode::Reflow, 80);
    cfg.formatter_extensions.east_asian_line_breaks = true;
    let input = "~漢\n字~\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, "\\~漢字\\~\n");
    similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn semantic_wrap_does_not_escape_preserved_east_asian_break() {
    let mut cfg = config(Flavor::Pandoc, WrapMode::Semantic, 80);
    cfg.formatter_extensions.east_asian_line_breaks = true;
    let input = "~漢\n字~\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, input);
    similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
}
