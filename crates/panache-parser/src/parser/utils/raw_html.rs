//! Attribute emission for raw HTML, without parsing its body as Markdown.

use std::ops::Range;

use crate::Dialect;
use crate::parser::inlines::inline_html::{
    inspect_inline_html, open_tag_attribute_range, parse_close_tag,
};
use crate::parser::inlines::sink::InlineSink;
use crate::parser::utils::attributes::emit_html_attrs_node;
use crate::parser::utils::helpers::strip_newline;
use crate::syntax::SyntaxKind;

pub(crate) fn attribute_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find('<') {
        let start = cursor + relative;
        let raw = &source[start..];
        let len = match inspect_inline_html(raw, Dialect::CommonMark) {
            Ok(Some(len)) => len,
            // Later tag-like bytes may belong to an unfinished construct.
            Err(_) => break,
            Ok(None) if raw.starts_with("<!") || raw.starts_with("<?") => break,
            Ok(None) => {
                cursor = start + 1;
                continue;
            }
        };
        if let Some(attrs) = open_tag_attribute_range(&raw[..len]) {
            ranges.push(start + attrs.start..start + attrs.end);
        }
        cursor = start + len;

        // Browsers treat these elements' contents as text, including strings
        // that look like HTML tags. The opening element can still have an ID.
        let Some(name) = literal_element_name(raw) else {
            continue;
        };
        if name == "plaintext" {
            break;
        }
        let Some(end) = raw_text_end(source, cursor, name) else {
            break;
        };
        cursor = end;
    }
    ranges
}

pub(crate) fn literal_element_name(raw: &str) -> Option<&'static str> {
    if !raw.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let name_end = 1 + raw.as_bytes()[1..]
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
        .count();
    let name = &raw[1..name_end];
    [
        "script",
        "style",
        "textarea",
        "title",
        "xmp",
        "iframe",
        "noembed",
        "noframes",
        "plaintext",
    ]
    .into_iter()
    .find(|tag| name.eq_ignore_ascii_case(tag))
}

pub(crate) fn raw_text_end(source: &str, mut cursor: usize, name: &str) -> Option<usize> {
    while let Some(relative) = source[cursor..].find('<') {
        cursor += relative;
        let closing = &source[cursor..];
        if let Some(len) = parse_close_tag(closing)
            && closing[2..len - 1].trim().eq_ignore_ascii_case(name)
        {
            return Some(cursor + len);
        }
        cursor += 1;
    }
    None
}

pub(crate) fn emit_fragment(
    builder: &mut impl InlineSink,
    source: &str,
    attributes: &[Range<usize>],
    fragment: Range<usize>,
) {
    let mut cursor = fragment.start;
    for attrs in attributes
        .iter()
        .filter(|attrs| fragment.start <= attrs.start && attrs.end <= fragment.end)
    {
        emit_text(builder, &source[cursor..attrs.start]);
        emit_html_attrs_node(builder, &source[attrs.clone()]);
        cursor = attrs.end;
    }
    emit_text(builder, &source[cursor..fragment.end]);
}

fn emit_text(builder: &mut impl InlineSink, source: &str) {
    for line in source.split_inclusive('\n') {
        let (text, newline) = strip_newline(line);
        if !text.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), text);
        }
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
    }
}
