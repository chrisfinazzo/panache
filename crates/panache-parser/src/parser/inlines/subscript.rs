//! Parsing for subscript (~text~)
//!
//! This is a Pandoc extension.
//! Syntax: ~text~ produces subscript text.
//!
//! Rules:
//! - Must have exactly 1 tilde on each side
//! - Content cannot be empty
//! - Tildes cannot have whitespace immediately inside
//! - Must not be confused with ~~ (strikeout)

use super::core::parse_inline_text;
use super::sink::InlineSink;
use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;

/// Try to parse subscript (~text~)
/// Returns: (total_len, inner_content)
pub fn try_parse_subscript(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    if bytes.is_empty() || bytes[0] != b'~' {
        return None;
    }

    if bytes.len() > 1 && bytes[1] == b'~' {
        return Some((2, ""));
    }

    if bytes.len() > 1 && bytes[1].is_ascii_whitespace() {
        return None;
    }

    let mut pos = 1;
    let mut found_close = false;

    while pos < bytes.len() {
        if bytes[pos] == b'~' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'~' {
                return None;
            }
            found_close = true;
            break;
        }
        pos += 1;
    }

    if !found_close {
        return None;
    }

    let content = &text[1..pos];

    if content.trim().is_empty() {
        return None;
    }

    if content.ends_with(char::is_whitespace) {
        return None;
    }

    if contains_unescaped_whitespace(content) {
        return None;
    }

    let total_len = pos + 1; // Include closing ~
    Some((total_len, content))
}

fn contains_unescaped_whitespace(content: &str) -> bool {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if (b as char).is_whitespace() {
            return true;
        }
        i += 1;
    }
    false
}

/// Emit a subscript node with its content
pub fn emit_subscript(
    builder: &mut impl InlineSink,
    inner_text: &str,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    builder.start_node(SyntaxKind::SUBSCRIPT.into());

    builder.start_node(SyntaxKind::SUBSCRIPT_MARKER.into());
    builder.token(SyntaxKind::SUBSCRIPT_MARKER.into(), "~");
    builder.finish_node();

    parse_inline_text(builder, inner_text, config, false, suppress_footnote_refs);

    builder.start_node(SyntaxKind::SUBSCRIPT_MARKER.into());
    builder.token(SyntaxKind::SUBSCRIPT_MARKER.into(), "~");
    builder.finish_node();

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_subscript() {
        assert_eq!(try_parse_subscript("~2~"), Some((3, "2")));
        assert_eq!(try_parse_subscript("~n~"), Some((3, "n")));
    }

    #[test]
    fn test_subscript_with_multiple_chars() {
        assert_eq!(try_parse_subscript("~text~"), Some((6, "text")));
        assert_eq!(try_parse_subscript("~i+1~"), Some((5, "i+1")));
    }

    #[test]
    fn test_no_whitespace_inside_delimiters() {
        assert_eq!(try_parse_subscript("~ text~"), None);

        assert_eq!(try_parse_subscript("~text ~"), None);
    }

    #[test]
    fn test_empty_content() {
        assert_eq!(try_parse_subscript("~~"), Some((2, "")));
        assert_eq!(try_parse_subscript("~ ~"), None);
    }

    #[test]
    fn test_no_closing() {
        assert_eq!(try_parse_subscript("~text"), None);
        assert_eq!(try_parse_subscript("~hello world"), None);
    }

    #[test]
    fn test_double_tilde_unclosed_is_empty_subscript() {
        assert_eq!(try_parse_subscript("~~text~~"), Some((2, "")));
        assert_eq!(try_parse_subscript("~~unclosed"), Some((2, "")));
    }

    #[test]
    fn test_subscript_with_other_content_after() {
        assert_eq!(try_parse_subscript("~2~ text"), Some((3, "2")));
        assert_eq!(try_parse_subscript("~n~ of sequence"), Some((3, "n")));
    }

    #[test]
    fn test_internal_whitespace_rejected() {
        assert_eq!(try_parse_subscript("~some text~"), None);
        assert_eq!(
            try_parse_subscript("~some\\ text~"),
            Some((12, "some\\ text"))
        );
    }

    #[test]
    fn test_single_char() {
        assert_eq!(try_parse_subscript("~a~"), Some((3, "a")));
    }

    #[test]
    fn test_subscript_before_strikeout_marker() {
        assert_eq!(try_parse_subscript("~x~ ~"), Some((3, "x")));
    }
}
