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

    // The blank lines around the inner quote are dropped by the fenced div
    // formatter itself (it does the same at the top level); what matters here
    // is that the div stays quoted and the inner quote stays at depth two.
    let result = format(input, None, None);
    assert_eq!(result, "> ::: note\n> > inner quote\n> :::\n");
    assert_eq!(format(&result, None, None), result);
}
