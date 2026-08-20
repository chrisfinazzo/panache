//! Citation parsing for Pandoc's citations extension.
//!
//! Syntax:
//! - Bracketed: `[@doe99]`, `[@doe99; @smith2000]`
//! - With locator: `[see @doe99, pp. 33-35]`
//! - Suppress author: `[-@doe99]`
//! - Author-in-text: `@doe99` (bare, without brackets)

use super::sink::InlineSink;
use crate::syntax::SyntaxKind;

/// Try to parse a bracketed citation starting at the current position.
/// Returns Some((length, content)) if successful, None otherwise.
///
/// Bracketed citations have the syntax: [@key], [@key1; @key2], [see @key, pp. 1-10]
pub(crate) fn try_parse_bracketed_citation(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    if bytes.is_empty() || bytes[0] != b'[' {
        return None;
    }

    let mut has_citation = false;
    let mut pos = 1;
    let mut bracket_depth = 0;

    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => {
                pos += 2;
                continue;
            }
            b'`' => match code_span_end(bytes, pos) {
                Some(end) => pos = end,
                None => pos += 1,
            },
            b'[' => {
                bracket_depth += 1;
                pos += 1;
            }
            b']' => {
                if bracket_depth == 0 {
                    if pos + 1 < bytes.len()
                        && bytes[pos + 1] == b'('
                        && inline_destination_end(bytes, pos + 1).is_some()
                    {
                        return None;
                    }
                    break;
                }
                bracket_depth -= 1;
                pos += 1;
                if pos < bytes.len()
                    && bytes[pos] == b'('
                    && let Some(end) = inline_destination_end(bytes, pos)
                {
                    pos = end;
                }
            }
            b'@' => {
                try_parse_bare_citation(text, pos)?;
                has_citation = true;
                pos += 1;
            }
            _ => {
                pos += 1;
            }
        }
    }

    if !has_citation {
        return None;
    }

    pos = 1;
    bracket_depth = 1;

    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => {
                pos += 2;
                continue;
            }
            b'`' => match code_span_end(bytes, pos) {
                Some(end) => pos = end,
                None => pos += 1,
            },
            b'[' => {
                bracket_depth += 1;
                pos += 1;
            }
            b']' => {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    let content = &text[1..pos];
                    return Some((pos + 1, content));
                }
                pos += 1;
                if pos < bytes.len()
                    && bytes[pos] == b'('
                    && let Some(end) = inline_destination_end(bytes, pos)
                {
                    pos = end;
                }
            }
            _ => {
                pos += 1;
            }
        }
    }

    None
}

/// Try to parse a bare citation (author-in-text) at byte offset `pos` in `text`.
/// Returns Some((length, key, has_suppress)) if successful, None otherwise. The
/// returned length is measured from `pos`.
///
/// Bare citations have the syntax: @key or -@key
///
/// `text` is the full inline buffer and `pos` the citation start, so the parser
/// can apply pandoc's `notAfterString` rule (see
/// [`prev_char_suppresses_bare_citation`]) using the character before `pos`.
pub(crate) fn try_parse_bare_citation(text: &str, pos: usize) -> Option<(usize, &str, bool)> {
    let parsed @ (_, _, has_suppress) = parse_bare_citation_unchecked(text, pos)?;

    if !has_suppress && prev_char_suppresses_bare_citation(text, pos) {
        return None;
    }

    Some(parsed)
}

/// Parse a bare citation at `pos` **without** pandoc's `notAfterString` guard.
/// Shared by [`try_parse_bare_citation`] (which adds the guard) and
/// [`suppressed_bare_citation`] (which reports exactly the citations the guard
/// rejects).
fn parse_bare_citation_unchecked(text: &str, pos: usize) -> Option<(usize, &str, bool)> {
    let cite = &text[pos..];
    let bytes = cite.as_bytes();

    if bytes.is_empty() {
        return None;
    }

    let mut p = 0;
    let has_suppress = bytes[p] == b'-';

    if has_suppress {
        p += 1;
        if p >= bytes.len() {
            return None;
        }
    }

    if bytes[p] != b'@' {
        return None;
    }
    p += 1;

    if p >= bytes.len() {
        return None;
    }

    let key_start = p;
    let key_len = parse_citation_key(&cite[p..])?;

    if key_len == 0 {
        return None;
    }

    let total_len = p + key_len;
    let key = &cite[key_start..total_len];

    Some((total_len, key, has_suppress))
}

/// The bare `@key` at `pos` that pandoc's `notAfterString` rule suppresses (so
/// panache, like pandoc, leaves it as literal text rather than a citation).
///
/// Returns `Some((len, key, false))` when a well-formed author-in-text citation
/// starts at `pos` but is glued to a preceding word character; `len` is measured
/// from `pos` and spans `@key`. Returns `None` when there is no bare citation at
/// `pos`, when it is the suppress-author `-@` form (never suppressed by
/// context), or when the citation is actually recognized. The linter uses this
/// to flag `@key` glued to a word when `key` resolves to a real reference.
pub fn suppressed_bare_citation(text: &str, pos: usize) -> Option<(usize, &str, bool)> {
    let parsed @ (_, _, has_suppress) = parse_bare_citation_unchecked(text, pos)?;
    if has_suppress {
        return None;
    }
    prev_char_suppresses_bare_citation(text, pos).then_some(parsed)
}

/// Pandoc's `notAfterString` guard for author-in-text citations: whether the
/// character immediately before `pos` suppresses a bare `@key` starting there.
///
/// Empirically (pandoc `-f markdown`) the suppressing set is Unicode
/// alphanumerics plus `.`: `word@key`, `1@key`, `café@key`, `違法編訂@jzkhl`,
/// and `x.@key` are all literal text, while other punctuation keeps the citation
/// (`)@key`, `_@key`, and a leading `[` for bracketed citations). At the very
/// start of the buffer there is no preceding character, so the citation stands.
fn prev_char_suppresses_bare_citation(text: &str, pos: usize) -> bool {
    match text[..pos].chars().next_back() {
        Some(ch) => ch.is_alphanumeric() || ch == '.',
        None => false,
    }
}

/// Try to parse a Quarto cross-reference key (e.g., @fig-plot, @eq-energy).
pub fn is_quarto_crossref_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let mut parts = lower.splitn(2, '-');
    let prefix = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if rest.is_empty() {
        return false;
    }
    matches!(
        prefix,
        "fig"
            | "tbl"
            | "lst"
            | "tip"
            | "nte"
            | "wrn"
            | "imp"
            | "cau"
            | "thm"
            | "lem"
            | "cor"
            | "prp"
            | "cnj"
            | "def"
            | "exm"
            | "exr"
            | "sol"
            | "rem"
            | "alg"
            | "eq"
            | "sec"
    )
}

/// Like [`is_quarto_crossref_key`], but also accepts any key whose prefix
/// appears in `custom_prefixes`. Used to recognize cross-reference prefixes
/// injected by Quarto extensions (e.g. pseudocode's `@algo-`) that aren't
/// built in. Matching is case-insensitive on the prefix, consistent with the
/// built-in check.
pub fn is_crossref_key(key: &str, custom_prefixes: &[String]) -> bool {
    is_quarto_crossref_key(key) || has_custom_crossref_prefix(key, custom_prefixes)
}

/// Whether `key`'s prefix (the segment before the first `-`) appears in
/// `custom_prefixes`. Unlike [`is_quarto_crossref_key`], this matches *only*
/// the configured extension prefixes, so callers can tell an extension-injected
/// cross-reference (whose target panache can't resolve) apart from a built-in
/// one (whose target it can and should validate).
pub fn has_custom_crossref_prefix(key: &str, custom_prefixes: &[String]) -> bool {
    if custom_prefixes.is_empty() {
        return false;
    }
    let lower = key.to_ascii_lowercase();
    let mut parts = lower.splitn(2, '-');
    let prefix = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if rest.is_empty() {
        return false;
    }
    custom_prefixes
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

pub const BOOKDOWN_LABEL_PREFIXES: &[&str] = &[
    "eq", "fig", "tab", "thm", "lem", "cor", "prp", "cnj", "def", "exm", "exr", "sol", "rem",
    "alg", "sec", "hyp",
];

pub fn is_bookdown_label(label: &str) -> bool {
    BOOKDOWN_LABEL_PREFIXES.contains(&label)
}

pub fn has_bookdown_prefix(label: &str) -> bool {
    let mut parts = label.splitn(2, ':');
    let prefix = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if rest.is_empty() {
        return false;
    }
    is_bookdown_label(prefix)
}

pub(crate) fn emit_crossref(builder: &mut impl InlineSink, key: &str, has_suppress: bool) {
    builder.start_node(SyntaxKind::CROSSREF.into());

    if has_suppress {
        builder.token(SyntaxKind::CROSSREF_MARKER.into(), "-@");
    } else {
        builder.token(SyntaxKind::CROSSREF_MARKER.into(), "@");
    }

    if key.starts_with('{') && key.ends_with('}') {
        builder.token(SyntaxKind::CROSSREF_BRACE_OPEN.into(), "{");
        builder.token(SyntaxKind::CROSSREF_KEY.into(), &key[1..key.len() - 1]);
        builder.token(SyntaxKind::CROSSREF_BRACE_CLOSE.into(), "}");
    } else {
        builder.token(SyntaxKind::CROSSREF_KEY.into(), key);
    }

    builder.finish_node();
}

pub(crate) fn emit_bookdown_crossref(builder: &mut impl InlineSink, key: &str) {
    builder.start_node(SyntaxKind::CROSSREF.into());
    builder.token(SyntaxKind::CROSSREF_BOOKDOWN_OPEN.into(), "\\@ref(");
    builder.token(SyntaxKind::CROSSREF_KEY.into(), key);
    builder.token(SyntaxKind::CROSSREF_BOOKDOWN_CLOSE.into(), ")");
    builder.finish_node();
}

/// Parse a citation key following Pandoc's rules.
/// Returns the length of the key, or None if invalid.
///
/// Citation keys:
/// - Must start with letter, digit, or _
/// - Can contain alphanumerics and single internal punctuation: :.#$%&-+?<>~/
/// - Keys in braces @{...} can contain anything
/// - Double internal punctuation terminates key
/// - Trailing punctuation not included
fn parse_citation_key(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }

    if text.starts_with('{') {
        let mut escape_next = false;

        for (idx, ch) in text.char_indices().skip(1) {
            if escape_next {
                escape_next = false;
                continue;
            }

            match ch {
                '\\' => escape_next = true,
                '}' => return Some(idx + ch.len_utf8()),
                _ => {}
            }
        }

        return None;
    }

    let mut iter = text.char_indices();
    let (_, first_char) = iter.next()?;
    if !first_char.is_alphanumeric() && first_char != '_' {
        return None;
    }

    let mut last_alnum_end = first_char.len_utf8();
    let mut last_included_end = last_alnum_end;
    let mut last_punct_start: Option<usize> = None;
    let mut prev_was_punct = false;

    for (idx, ch) in iter {
        if ch.is_alphanumeric() || ch == '_' {
            prev_was_punct = false;
            last_alnum_end = idx + ch.len_utf8();
            last_included_end = last_alnum_end;
            last_punct_start = None;
        } else if is_internal_punctuation(ch) {
            if prev_was_punct {
                return Some(last_punct_start.unwrap_or(last_alnum_end));
            }
            prev_was_punct = true;
            last_punct_start = Some(idx);
            last_included_end = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if prev_was_punct {
        return Some(last_alnum_end);
    }

    if last_included_end == 0 {
        None
    } else {
        Some(last_included_end)
    }
}

/// If `bytes[pos]` begins a backtick code-span opener, return the index just
/// past the matching closing run. Returns `None` when there is no closing run
/// of equal length, in which case the backticks are literal text.
///
/// Code spans are verbatim, so citation markers (`@`), separators (`;`), and
/// brackets (`]`) inside them must not influence citation detection — this
/// matches pandoc, which parses `` [`@foo`] `` as a link, not a citation.
fn code_span_end(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut open_end = pos;
    while open_end < bytes.len() && bytes[open_end] == b'`' {
        open_end += 1;
    }
    let run = open_end - pos;

    let mut i = open_end;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let close_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            if i - close_start == run {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }

    None
}

fn inline_destination_end(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut i = pos + 1;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'<' {
        i += 1;
        while i < bytes.len() && bytes[i] != b'>' {
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        if i >= bytes.len() {
            return None;
        }
        i += 1;
    } else {
        let mut depth = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'(' => {
                    depth += 1;
                    i += 1;
                }
                b')' if depth == 0 => break,
                b')' => {
                    depth -= 1;
                    i += 1;
                }
                b' ' | b'\t' | b'\n' => break,
                _ => i += 1,
            }
        }
    }
    let mut j = i;
    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n') {
        j += 1;
    }
    if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
        let quote = bytes[j];
        j += 1;
        while j < bytes.len() && bytes[j] != quote {
            j += if bytes[j] == b'\\' { 2 } else { 1 };
        }
        if j >= bytes.len() {
            return None;
        }
        j += 1;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n') {
            j += 1;
        }
    }
    (j < bytes.len() && bytes[j] == b')').then(|| j + 1)
}

fn is_internal_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ':' | '.' | '#' | '$' | '%' | '&' | '-' | '+' | '?' | '<' | '>' | '~' | '/'
    )
}

/// Emit a bracketed citation node to the builder.
pub(crate) fn emit_bracketed_citation(builder: &mut impl InlineSink, content: &str) {
    builder.start_node(SyntaxKind::CITATION.into());

    builder.token(SyntaxKind::LINK_START.into(), "[");

    emit_bracketed_citation_content(builder, content);

    builder.token(SyntaxKind::LINK_DEST.into(), "]");

    builder.finish_node();
}

fn emit_bracketed_citation_content(builder: &mut impl InlineSink, content: &str) {
    let mut text_start = 0;
    let mut iter = content.char_indices().peekable();

    while let Some((idx, ch)) = iter.next() {
        if ch == '\\' {
            iter.next();
            continue;
        }

        if ch == '`'
            && let Some(end) = code_span_end(content.as_bytes(), idx)
        {
            while matches!(iter.peek(), Some((next_idx, _)) if *next_idx < end) {
                iter.next();
            }
            continue;
        }

        if ch == '@' || (ch == '-' && matches!(iter.peek(), Some((_, '@')))) {
            if idx > text_start {
                builder.token(
                    SyntaxKind::CITATION_CONTENT.into(),
                    &content[text_start..idx],
                );
            }

            let mut marker_len = 1;
            let marker_text = if ch == '-' {
                iter.next();
                marker_len = 2;
                "-@"
            } else {
                "@"
            };
            builder.token(SyntaxKind::CITATION_MARKER.into(), marker_text);

            let key_start = idx + marker_len;
            if key_start >= content.len() {
                text_start = key_start;
                continue;
            }

            if let Some(key_len) = parse_citation_key(&content[key_start..]) {
                let key_end = key_start + key_len;
                let key = &content[key_start..key_end];
                if key.starts_with('{') && key.ends_with('}') {
                    builder.token(SyntaxKind::CITATION_BRACE_OPEN.into(), "{");
                    if key.len() > 2 {
                        builder.token(SyntaxKind::CITATION_KEY.into(), &key[1..key.len() - 1]);
                    }
                    builder.token(SyntaxKind::CITATION_BRACE_CLOSE.into(), "}");
                } else {
                    builder.token(SyntaxKind::CITATION_KEY.into(), key);
                }
                while matches!(iter.peek(), Some((next_idx, _)) if *next_idx < key_end) {
                    iter.next();
                }
                text_start = key_end;
                continue;
            }

            text_start = key_start;
            continue;
        }

        if ch == ';' {
            if idx > text_start {
                builder.token(
                    SyntaxKind::CITATION_CONTENT.into(),
                    &content[text_start..idx],
                );
            }
            builder.token(SyntaxKind::CITATION_SEPARATOR.into(), ";");
            text_start = idx + ch.len_utf8();
            continue;
        }
    }

    if text_start < content.len() {
        builder.token(SyntaxKind::CITATION_CONTENT.into(), &content[text_start..]);
    }
}

/// Emit a bare citation node to the builder.
pub(crate) fn emit_bare_citation(builder: &mut impl InlineSink, key: &str, has_suppress: bool) {
    builder.start_node(SyntaxKind::CITATION.into());

    if has_suppress {
        builder.token(SyntaxKind::CITATION_MARKER.into(), "-@");
    } else {
        builder.token(SyntaxKind::CITATION_MARKER.into(), "@");
    }

    if key.starts_with('{') && key.ends_with('}') {
        builder.token(SyntaxKind::CITATION_BRACE_OPEN.into(), "{");
        builder.token(SyntaxKind::CITATION_KEY.into(), &key[1..key.len() - 1]);
        builder.token(SyntaxKind::CITATION_BRACE_CLOSE.into(), "}");
    } else {
        builder.token(SyntaxKind::CITATION_KEY.into(), key);
    }

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_citation_key() {
        assert_eq!(parse_citation_key("doe99"), Some(5));
        assert_eq!(parse_citation_key("smith2000"), Some(9));
    }

    #[test]
    fn test_parse_citation_key_with_internal_punct() {
        assert_eq!(parse_citation_key("Foo_bar.baz"), Some(11));
        assert_eq!(parse_citation_key("author:2020"), Some(11));
    }

    #[test]
    fn test_parse_citation_key_trailing_punct() {
        assert_eq!(parse_citation_key("Foo_bar.baz."), Some(11));
        assert_eq!(parse_citation_key("key:value:"), Some(9));
    }

    #[test]
    fn test_parse_citation_key_double_punct() {
        assert_eq!(parse_citation_key("Foo_bar--baz"), Some(7)); // key is "Foo_bar"
    }

    #[test]
    fn test_parse_citation_key_with_braces() {
        assert_eq!(parse_citation_key("{https://example.com}"), Some(21));
        assert_eq!(parse_citation_key("{Foo_bar.baz.}"), Some(14));
    }

    #[test]
    fn test_parse_citation_key_invalid_start() {
        assert_eq!(parse_citation_key(".invalid"), None);
        assert_eq!(parse_citation_key(":invalid"), None);
    }

    #[test]
    fn test_parse_citation_key_stops_at_space() {
        assert_eq!(parse_citation_key("key rest"), Some(3));
    }

    #[test]
    fn is_crossref_key_accepts_builtin_without_custom() {
        assert!(is_crossref_key("fig-plot", &[]));
        assert!(!is_crossref_key("algo-cd", &[]));
    }

    #[test]
    fn is_crossref_key_accepts_custom_prefix() {
        let custom = vec!["algo".to_string()];
        assert!(is_crossref_key("algo-cd", &custom));
        assert!(is_crossref_key("ALGO-cd", &custom));
        assert!(is_crossref_key("tbl-x", &custom));
        assert!(!is_crossref_key("algo", &custom));
        assert!(!is_crossref_key("doe99", &custom));
    }

    #[test]
    fn test_parse_bare_citation_simple() {
        let result = try_parse_bare_citation("@doe99", 0);
        assert_eq!(result, Some((6, "doe99", false)));
    }

    #[test]
    fn test_parse_bare_citation_with_suppress() {
        let result = try_parse_bare_citation("-@smith04", 0);
        assert_eq!(result, Some((9, "smith04", true)));
    }

    #[test]
    fn test_parse_bare_citation_with_trailing_text() {
        let result = try_parse_bare_citation("@doe99 says", 0);
        assert_eq!(result, Some((6, "doe99", false)));
    }

    #[test]
    fn test_parse_bare_citation_braced_key() {
        let result = try_parse_bare_citation("@{https://example.com}", 0);
        assert_eq!(result, Some((22, "{https://example.com}", false)));
    }

    #[test]
    fn test_parse_bare_citation_not_citation() {
        assert_eq!(try_parse_bare_citation("not a citation", 0), None);
        assert_eq!(try_parse_bare_citation("@", 0), None);
    }

    #[test]
    fn test_bare_citation_suppressed_after_word() {
        assert_eq!(try_parse_bare_citation("word@key", 4), None);
        assert_eq!(try_parse_bare_citation("1@key", 1), None);
        assert_eq!(try_parse_bare_citation("user@example.com", 4), None);
    }

    #[test]
    fn test_bare_citation_suppressed_after_cjk() {
        let text = "違法編訂@jzkhl";
        let at = text.find('@').unwrap();
        assert_eq!(try_parse_bare_citation(text, at), None);
    }

    #[test]
    fn test_bare_citation_suppressed_after_period() {
        assert_eq!(try_parse_bare_citation("x.@key", 2), None);
    }

    #[test]
    fn test_bare_citation_allowed_after_non_word_punct() {
        assert_eq!(
            try_parse_bare_citation("x)@key", 2),
            Some((4, "key", false))
        );
        assert_eq!(try_parse_bare_citation("_@key", 1), Some((4, "key", false)));
    }

    #[test]
    fn test_bare_citation_allowed_after_space() {
        assert_eq!(
            try_parse_bare_citation("says @doe99", 5),
            Some((6, "doe99", false))
        );
    }

    #[test]
    fn test_suppress_author_citation_allowed_after_word() {
        assert_eq!(
            try_parse_bare_citation("word-@key", 4),
            Some((5, "key", true))
        );
    }

    #[test]
    fn test_bare_citation_at_buffer_start() {
        assert_eq!(
            try_parse_bare_citation("@doe99", 0),
            Some((6, "doe99", false))
        );
    }

    #[test]
    fn test_suppressed_bare_citation_after_word() {
        assert_eq!(
            suppressed_bare_citation("word@doe99", 4),
            Some((6, "doe99", false))
        );
        let text = "違法編訂@jzkhl";
        let at = text.find('@').unwrap();
        assert_eq!(
            suppressed_bare_citation(text, at),
            Some((6, "jzkhl", false))
        );
    }

    #[test]
    fn test_suppressed_bare_citation_none_when_recognized() {
        assert_eq!(suppressed_bare_citation("says @doe99", 5), None);
        assert_eq!(suppressed_bare_citation("@doe99", 0), None);
        assert_eq!(suppressed_bare_citation("word-@key", 4), None);
        assert_eq!(suppressed_bare_citation("word key", 4), None);
    }

    #[test]
    fn test_parse_bracketed_citation_simple() {
        let result = try_parse_bracketed_citation("[@doe99]");
        assert_eq!(result, Some((8, "@doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_multiple() {
        let result = try_parse_bracketed_citation("[@doe99; @smith2000]");
        assert_eq!(result, Some((20, "@doe99; @smith2000")));
    }

    #[test]
    fn test_parse_bracketed_citation_with_prefix() {
        let result = try_parse_bracketed_citation("[see @doe99]");
        assert_eq!(result, Some((12, "see @doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_with_locator() {
        let result = try_parse_bracketed_citation("[@doe99, pp. 33-35]");
        assert_eq!(result, Some((19, "@doe99, pp. 33-35")));
    }

    #[test]
    fn test_parse_bracketed_citation_complex() {
        let result = try_parse_bracketed_citation("[see @doe99, pp. 33-35 and *passim*]");
        assert_eq!(result, Some((36, "see @doe99, pp. 33-35 and *passim*")));
    }

    #[test]
    fn test_parse_bracketed_citation_with_suppress() {
        let result = try_parse_bracketed_citation("[-@doe99]");
        assert_eq!(result, Some((9, "-@doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_not_citation() {
        assert_eq!(try_parse_bracketed_citation("[text](url)"), None);
        assert_eq!(try_parse_bracketed_citation("[just text]"), None);
    }

    #[test]
    fn test_bracketed_email_is_not_citation() {
        assert_eq!(try_parse_bracketed_citation("[jola@math.ku.dk]"), None);
    }

    #[test]
    fn test_invalid_marker_prevents_bracketed_citation() {
        assert_eq!(
            try_parse_bracketed_citation("[email jola@math.ku.dk; see @doe99]"),
            None
        );
    }

    #[test]
    fn test_inline_link_owns_brackets_containing_citation() {
        assert_eq!(
            try_parse_bracketed_citation("[see @doe99](https://example.com)"),
            None
        );
    }

    #[test]
    fn test_parse_bracketed_citation_nested_brackets() {
        let result = try_parse_bracketed_citation("[see [nested] @doe99]");
        assert_eq!(result, Some((21, "see [nested] @doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_escaped_bracket() {
        let result = try_parse_bracketed_citation(r"[@doe99 with \] escaped]");
        assert_eq!(result, Some((24, r"@doe99 with \] escaped")));
    }

    #[test]
    fn test_parse_bracketed_citation_paren_in_prefix() {
        let result = try_parse_bracketed_citation("[see (Smith 1999) and @doe99]");
        assert_eq!(result, Some((29, "see (Smith 1999) and @doe99")));
    }

    #[test]
    fn test_bracketed_citation_ignores_at_in_code_span() {
        assert_eq!(try_parse_bracketed_citation("[`@foo`]"), None);
    }

    #[test]
    fn test_bracketed_citation_code_span_in_prefix() {
        assert_eq!(
            try_parse_bracketed_citation("[`x@y` @doe99]"),
            Some((14, "`x@y` @doe99"))
        );
    }

    #[test]
    fn test_bracketed_citation_bracket_in_code_span() {
        assert_eq!(
            try_parse_bracketed_citation("[`a]b` @doe99]"),
            Some((14, "`a]b` @doe99"))
        );
    }

    #[test]
    fn test_bracketed_citation_unterminated_backtick() {
        assert_eq!(
            try_parse_bracketed_citation("[`@foo bar]"),
            Some((11, "`@foo bar"))
        );
    }

    #[test]
    fn test_bracketed_citation_ignores_at_in_nested_image_url() {
        assert_eq!(
            try_parse_bracketed_citation(
                "[![npm version](https://badge.fury.io/js/@arity-cli%2Farity-cli.svg?icon=si%3Anpm)]"
            ),
            None
        );
    }

    #[test]
    fn test_bracketed_citation_ignores_at_in_nested_link_url() {
        assert_eq!(
            try_parse_bracketed_citation("[a [link](https://x.io/@scope) here]"),
            None
        );
    }

    #[test]
    fn test_bracketed_citation_marker_after_nested_link_url() {
        assert_eq!(
            try_parse_bracketed_citation("[see [foo](url@x) and @bar]"),
            Some((27, "see [foo](url@x) and @bar"))
        );
    }

    #[test]
    fn test_parse_bracketed_citation_escaped_at_in_prefix() {
        let result =
            try_parse_bracketed_citation(r"[see also \@ref(svm) and @bischl_applied_2024]");
        assert_eq!(
            result,
            Some((46, r"see also \@ref(svm) and @bischl_applied_2024"))
        );
    }
}
