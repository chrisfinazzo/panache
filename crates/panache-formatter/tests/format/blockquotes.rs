use panache_formatter::format;

/// Every block kind that can appear inside a blockquote must come back out
/// with its `> ` prefix. Kinds without a dedicated arm in the formatter used
/// to fall through to a default that emitted them unprefixed, which silently
/// moved the content out of the quote.
#[test]
fn fenced_div_stays_inside_blockquote() {
    let input = "> ::: note\n> inner\n> :::\n";

    let result = format(input, None, None);
    assert_eq!(result, "> ::: note\n> inner\n> :::\n");
    assert_eq!(format(&result, None, None), result);
}

#[test]
fn figure_stays_inside_blockquote() {
    let input = "> ![caption](img.png)\n";

    let result = format(input, None, None);
    assert_eq!(result, "> ![caption](img.png)\n");
    assert_eq!(format(&result, None, None), result);
}

#[test]
fn footnote_definition_stays_inside_blockquote() {
    let input = "> [^1]: a footnote\n";

    let result = format(input, None, None);
    assert_eq!(result, "> [^1]: a footnote\n");
    assert_eq!(format(&result, None, None), result);
}

#[test]
fn reference_definition_stays_inside_blockquote() {
    let input = "> [ref]: http://example.com\n";

    let result = format(input, None, None);
    assert_eq!(result, "> [ref]: http://example.com\n");
    assert_eq!(format(&result, None, None), result);
}

/// A nested blockquote prefixes itself depth-aware, so the enclosing quote
/// must not prefix it a second time.
#[test]
fn nested_blockquote_is_not_double_prefixed() {
    let input = "> > nested\n";

    let result = format(input, None, None);
    assert_eq!(result, "> > nested\n");
    assert_eq!(format(&result, None, None), result);
}

#[test]
fn nested_blockquote_inside_fenced_div_keeps_its_depth() {
    let input = "> ::: note\n>\n> > inner quote\n>\n> :::\n";

    let result = format(input, None, None);
    assert_eq!(result, "> ::: note\n> > inner quote\n> :::\n");
    assert_eq!(format(&result, None, None), result);
}

/// A `>` that is inline text inside a quoted list item is content, not a
/// container marker, so it must survive reflow. The list reflow path used to
/// drop every standalone `>` piece when the list sat in a blockquote, which
/// silently deleted pandoc's `Str ">"`.
#[test]
fn quoted_item_keeps_literal_angle_bracket_word() {
    let input = "> - a > b\n";

    let result = format(input, None, None);
    assert_eq!(result, "> - a > b\n");
    assert_eq!(format(&result, None, None), result);
}

/// The lazy-continuation variant: `>   > nested quote` continues the item's
/// paragraph, so pandoc reads the inner `>` as `Str ">"`. Reflowing the item
/// onto one line must keep it.
#[test]
fn quoted_item_keeps_lazy_blockquote_marker_text() {
    let input = "> - a\n>   > nested quote\n";

    let result = format(input, None, None);
    assert_eq!(result, "> - a > nested quote\n");
    assert_eq!(format(&result, None, None), result);
}

/// Same invariant on the sentence-wrap path, which set the stripping flag too.
#[test]
fn quoted_item_keeps_literal_angle_bracket_word_in_sentence_wrap() {
    use panache_formatter::Config;
    use panache_formatter::config::WrapMode;

    let config = Config {
        wrap: Some(WrapMode::Sentence),
        ..Default::default()
    };
    let input = "> - a > b\n";

    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, "> - a > b\n");
    assert_eq!(format(&result, Some(config), None), result);
}
