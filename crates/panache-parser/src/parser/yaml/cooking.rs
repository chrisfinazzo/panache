#![allow(dead_code)]

//! Pure YAML 1.2 scalar cooking — text transformations that turn the
//! raw source bytes of a scalar token (with its quote wrappers /
//! block-scalar header / line breaks intact) into the canonical
//! decoded string the spec defines.
//!
//! These helpers are shared between event projection and the future
//! formatter so the cooking pipeline has a single home. Block-scalar
//! cooking still requires CST context (parent indent for content-indent
//! inference) and stays in [`super::events`] for now; the quoted and
//! plain primitives live here.

use super::scanner::ScalarStyle;

/// Cook a raw scalar source slice into its canonical YAML 1.2 string,
/// dispatching on `style`. Plain / single-quoted / double-quoted are
/// fully handled; literal and folded block scalars return the raw
/// text because their cooking depends on parent content-indent which
/// is not derivable from `raw` alone.
pub(crate) fn cook(style: ScalarStyle, raw: &str) -> String {
    match style {
        ScalarStyle::Plain => cook_plain(raw),
        ScalarStyle::SingleQuoted => cook_single_quoted(raw),
        ScalarStyle::DoubleQuoted => cook_double_quoted(raw),
        ScalarStyle::Literal | ScalarStyle::Folded => raw.to_string(),
    }
}

/// Cook a plain (unquoted) scalar. Multi-line plain scalars fold
/// non-blank lines with single spaces, ignoring blank lines and
/// embedded `#`-comment lines (these appear in multi-line flow
/// continuations where the scanner currently embeds them in the
/// scalar token's text).
pub(crate) fn cook_plain(raw: &str) -> String {
    if !raw.contains('\n') {
        return raw.trim().to_string();
    }
    let mut pieces = Vec::new();
    for line in raw.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        pieces.push(trimmed.to_string());
    }
    pieces.join(" ")
}

/// Cook a single-quoted scalar. Auto-detects single-line vs multi-line
/// by inspecting `raw` for a newline character.
pub(crate) fn cook_single_quoted(raw: &str) -> String {
    if raw.contains('\n') {
        cook_single_quoted_multi_line(raw)
    } else {
        cook_single_quoted_single_line(raw)
    }
}

/// Cook a double-quoted scalar. Auto-detects single-line vs multi-line
/// by inspecting `raw` for a newline character.
pub(crate) fn cook_double_quoted(raw: &str) -> String {
    if raw.contains('\n') {
        cook_double_quoted_multi_line(raw)
    } else {
        cook_double_quoted_single_line(raw)
    }
}

/// Multi-line single-quoted: YAML §7.3.2 line folding, then `''` → `'`
/// for the YAML 1.2 single-quote-escape rule.
pub(crate) fn cook_single_quoted_multi_line(raw: &str) -> String {
    let trimmed = raw.trim_start_matches([' ', '\t', '\n']);
    let inner = strip_quoted_wrapper(trimmed, '\'');
    let folded = fold_quoted_inner(&inner, false);
    folded.replace("''", "'")
}

/// Multi-line double-quoted: YAML §7.3.3 line folding (with escaped
/// line-break support per §7.5), then `\x..` / `\u....` / `\n` / `\t`
/// / etc. escape decoding per §5.7.
pub(crate) fn cook_double_quoted_multi_line(raw: &str) -> String {
    let trimmed = raw.trim_start_matches([' ', '\t', '\n']);
    let inner = strip_quoted_wrapper(trimmed, '"');
    let folded = fold_quoted_inner(&inner, true);
    decode_double_quoted_inner(&folded)
}

/// Strip surrounding `'…'` and unescape `''` → `'` for a single-line
/// single-quoted scalar.
pub(crate) fn cook_single_quoted_single_line(raw: &str) -> String {
    let body = raw.strip_prefix('\'').unwrap_or(raw);
    let body = body.strip_suffix('\'').unwrap_or(body);
    body.replace("''", "'")
}

/// Strip surrounding `"…"` and decode escape sequences for a
/// single-line double-quoted scalar. Unknown escapes are kept verbatim
/// (`\?` stays `\?`) so the event harness can surface them as
/// bare backslash-prefixed text.
pub(crate) fn cook_double_quoted_single_line(raw: &str) -> String {
    let body = raw.strip_prefix('"').unwrap_or(raw);
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            break;
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        decode_double_quoted_escape(next, &mut chars, &mut out);
    }
    out
}

/// Strip the surrounding quote characters from a multi-line quoted
/// scalar's raw source. Walks until the first un-escaped (for `"`) or
/// non-doubled (for `'`) closing quote so embedded `\"` / `''` don't
/// terminate early.
pub(crate) fn strip_quoted_wrapper(text: &str, quote: char) -> String {
    let body = text.strip_prefix(quote).unwrap_or(text);
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if quote == '"' {
            if ch == '\\' {
                out.push(ch);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            if ch == '"' {
                break;
            }
        } else if ch == '\'' {
            if chars.peek() == Some(&'\'') {
                out.push('\'');
                out.push('\'');
                chars.next();
                continue;
            }
            break;
        }
        out.push(ch);
    }
    out
}

/// Fold the inner body of a multi-line quoted scalar per YAML §7.3:
/// - On the first line, leading whitespace is preserved as-is.
/// - On continuation lines, leading whitespace is stripped.
/// - Trailing whitespace from the running output is dropped before folding.
/// - A run of `n` consecutive empty lines folds to `n` `\n` chars.
/// - A single line break (no blank between) folds to a single space.
/// - Trailing whitespace of the final line is stripped (matching
///   yaml-test-suite event expectations for multi-line quoted scalars).
///
/// `escaped_breaks` enables YAML §7.5 double-quoted escaped line breaks:
/// a continuation line whose predecessor ends in an unescaped (odd-count)
/// backslash joins directly with no folded space, and the escaping
/// backslash is dropped. Pass `false` for single-quoted and plain
/// scalars, where a trailing backslash is literal content.
pub(crate) fn fold_quoted_inner(inner: &str, escaped_breaks: bool) -> String {
    let mut out = String::new();
    let mut blanks = 0usize;
    let mut have_first = false;
    for (idx, line) in inner.split('\n').enumerate() {
        if idx == 0 {
            out.push_str(line);
            have_first = true;
            continue;
        }
        let stripped = line.trim_start_matches([' ', '\t']);
        if stripped.is_empty() {
            blanks += 1;
            continue;
        }
        trim_trailing_ws_respecting_escape(&mut out, escaped_breaks);
        if escaped_breaks && blanks == 0 && have_first && ends_with_odd_backslashes(&out) {
            out.pop();
            out.push_str(stripped);
            blanks = 0;
            continue;
        }
        if !have_first {
        } else if blanks == 0 {
            out.push(' ');
        } else {
            for _ in 0..blanks {
                out.push('\n');
            }
        }
        out.push_str(stripped);
        blanks = 0;
        have_first = true;
    }
    if blanks > 0 {
        trim_trailing_ws_respecting_escape(&mut out, escaped_breaks);
        if blanks == 1 {
            out.push(' ');
        } else {
            for _ in 0..blanks - 1 {
                out.push('\n');
            }
        }
    }
    out
}

/// Strip trailing space/tab chars from a double-quoted folding buffer,
/// preserving the first whitespace char of a `\<ws>` escape sequence.
///
/// YAML 1.2 §5.7 includes escapes `\<TAB>` (literal tab) and `\<SPACE>`
/// (literal space) — the whitespace after the backslash is the
/// escape's argument and must survive the trailing-whitespace strip
/// that fold rules apply on continuation. Without this, inputs like
/// `"x\<TAB> \n y"` (DE56/02) lose the tab and the trailing `\` is
/// mis-detected as a line-continuation marker, collapsing the value
/// to `xy`.
///
/// For single-quoted / plain scalars (`escaped_breaks == false`), `\`
/// is literal content and the function degrades to a plain whitespace
/// strip.
pub(crate) fn trim_trailing_ws_respecting_escape(out: &mut String, escaped_breaks: bool) {
    let bytes = out.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t') {
        end -= 1;
    }
    if !escaped_breaks || end == bytes.len() || end == 0 || bytes[end - 1] != b'\\' {
        out.truncate(end);
        return;
    }
    let mut bs_start = end - 1;
    while bs_start > 0 && bytes[bs_start - 1] == b'\\' {
        bs_start -= 1;
    }
    let bs_count = end - bs_start;
    if bs_count % 2 == 1 {
        out.truncate(end + 1);
    } else {
        out.truncate(end);
    }
}

/// Whether `s` ends with an odd-length run of `\` characters, i.e. the
/// final backslash is unescaped. Used to detect double-quoted escaped
/// line breaks.
pub(crate) fn ends_with_odd_backslashes(s: &str) -> bool {
    s.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

/// Inner-only double-quoted decoder: the input has no surrounding quote
/// characters and is consumed in full. Shares escape decoding
/// semantics with [`cook_double_quoted_single_line`].
pub(crate) fn decode_double_quoted_inner(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        decode_double_quoted_escape(next, &mut chars, &mut out);
    }
    out
}

fn decode_double_quoted_escape(next: char, chars: &mut std::str::Chars<'_>, out: &mut String) {
    match next {
        '0' => out.push('\0'),
        'a' => out.push('\u{07}'),
        'b' => out.push('\u{08}'),
        't' | '\t' => out.push('\t'),
        'n' => out.push('\n'),
        'v' => out.push('\u{0B}'),
        'f' => out.push('\u{0C}'),
        'r' => out.push('\r'),
        'e' => out.push('\u{1B}'),
        ' ' => out.push(' '),
        '"' => out.push('"'),
        '/' => out.push('/'),
        '\\' => out.push('\\'),
        'N' => out.push('\u{85}'),
        '_' => out.push('\u{A0}'),
        'L' => out.push('\u{2028}'),
        'P' => out.push('\u{2029}'),
        'x' => {
            if let Some(c) = take_hex_char(chars, 2) {
                out.push(c);
            }
        }
        'u' => {
            if let Some(c) = take_hex_char(chars, 4) {
                out.push(c);
            }
        }
        'U' => {
            if let Some(c) = take_hex_char(chars, 8) {
                out.push(c);
            }
        }
        other => {
            out.push('\\');
            out.push(other);
        }
    }
}

fn take_hex_char(chars: &mut std::str::Chars<'_>, n: usize) -> Option<char> {
    let hex: String = chars.take(n).collect();
    if hex.len() != n {
        return None;
    }
    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_single_line_trims_whitespace() {
        assert_eq!(cook_plain("  hello  "), "hello");
    }

    #[test]
    fn plain_multi_line_folds_with_single_space() {
        assert_eq!(cook_plain("hello\n  world"), "hello world");
    }

    #[test]
    fn plain_multi_line_skips_blank_lines() {
        assert_eq!(cook_plain("a\n\nb"), "a b");
    }

    #[test]
    fn plain_multi_line_skips_hash_comment_lines() {
        assert_eq!(cook_plain("a\n# comment\nb"), "a b");
    }

    #[test]
    fn single_quoted_single_line_unescapes_doubled_quote() {
        assert_eq!(cook_single_quoted("'it''s'"), "it's");
    }

    #[test]
    fn double_quoted_single_line_decodes_basic_escapes() {
        assert_eq!(cook_double_quoted("\"a\\nb\""), "a\nb");
    }

    #[test]
    fn double_quoted_decodes_hex_escape() {
        assert_eq!(cook_double_quoted("\"\\x41\""), "A");
    }

    #[test]
    fn double_quoted_decodes_unicode_escape() {
        assert_eq!(cook_double_quoted("\"\\u00e9\""), "é");
    }

    #[test]
    fn double_quoted_unknown_escape_kept_verbatim() {
        assert_eq!(cook_double_quoted("\"\\?\""), "\\?");
    }

    #[test]
    fn single_quoted_multi_line_folds_lines_and_unescapes() {
        assert_eq!(cook_single_quoted("'foo\n  bar''baz'"), "foo bar'baz");
    }

    #[test]
    fn double_quoted_multi_line_folds_and_decodes() {
        assert_eq!(cook_double_quoted("\"foo\n  bar\\n\""), "foo bar\n");
    }

    #[test]
    fn cook_dispatches_on_style() {
        assert_eq!(cook(ScalarStyle::Plain, "  x  "), "x");
        assert_eq!(cook(ScalarStyle::SingleQuoted, "'x'"), "x");
        assert_eq!(cook(ScalarStyle::DoubleQuoted, "\"x\""), "x");
        assert_eq!(cook(ScalarStyle::Literal, "|\n  x\n"), "|\n  x\n");
        assert_eq!(cook(ScalarStyle::Folded, ">\n  x\n"), ">\n  x\n");
    }
}
