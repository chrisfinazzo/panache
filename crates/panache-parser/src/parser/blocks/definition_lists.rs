use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::container_stack::{leading_indent, leading_indent_from};
use crate::parser::utils::helpers::strip_newline;
use crate::parser::utils::inline_emission;

/// Tries to parse a definition list marker (`:` or `~`)
///
/// Returns Some((marker_char, indent_cols, spaces_after_cols, spaces_after_bytes)) if found, None otherwise.
/// The marker can be indented 0-3 spaces and must be followed by whitespace.
pub fn try_parse_definition_marker(line: &str) -> Option<(char, usize, usize, usize)> {
    // Cheap byte-level leading-byte gate: a definition marker is `:` or
    // `~` after up to 3 ASCII spaces. Avoid the `leading_indent`
    // (Unicode char walk + tab-aware column count) on the common
    // non-marker line.
    {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() && i < 3 && bytes[i] == b' ' {
            i += 1;
        }
        match bytes.get(i) {
            Some(&b':') | Some(&b'~') => {}
            _ => return None,
        }
    }

    // Count leading whitespace in columns (0-3 allowed)
    let (indent_cols, indent_bytes) = leading_indent(line);
    if indent_cols > 3 {
        return None;
    }

    let after_indent = &line[indent_bytes..];

    // Check for : or ~ marker
    let marker = after_indent.chars().next()?;
    if !matches!(marker, ':' | '~') {
        return None;
    }

    let after_marker = &after_indent[1..];

    // Must be followed by whitespace, a line ending, or end of line. A bare
    // marker whose definition body starts on the next line arrives as ":\n"
    // (lines keep their trailing newline via `split_inclusive`); the newline is
    // end-of-line, not content, so it's still a valid marker (matches pandoc's
    // lazy `:`-on-its-own-line definition form).
    if !after_marker.starts_with(' ')
        && !after_marker.starts_with('\t')
        && !after_marker.starts_with('\n')
        && !after_marker.starts_with('\r')
        && !after_marker.is_empty()
    {
        return None;
    }

    // Seed the column counter past the marker's own column, so a tab after
    // the marker expands to the stop it actually reaches: `:\td` puts the tab
    // at column 1, reaching column 4 (a content column of 4), not column 5.
    let (spaces_after_cols, spaces_after_bytes) =
        leading_indent_from(after_marker, indent_cols + 1);

    Some((marker, indent_cols, spaces_after_cols, spaces_after_bytes))
}

/// Emit a term line into the syntax tree
pub(crate) fn emit_term(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::TERM.into());
    // Strip trailing newline from line (it will be emitted separately)
    let (text, newline_str) = strip_newline(line);
    let trimmed_text = text.trim_end();

    if !trimmed_text.is_empty() {
        inline_emission::emit_inlines(builder, trimmed_text, config, false);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
    builder.finish_node(); // Term
}

/// Emit a definition marker
pub(crate) fn emit_definition_marker(
    builder: &mut GreenNodeBuilder<'static>,
    marker: char,
    indent_cols: usize,
) {
    if indent_cols > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &" ".repeat(indent_cols));
    }
    builder.token(SyntaxKind::DEFINITION_MARKER.into(), &marker.to_string());
}

// Helper functions for definition list management in Parser

use crate::parser::blocks::tables::{LineView, is_caption_followed_by_table};
use crate::parser::utils::container_stack::{Container, ContainerStack};

/// Check if we're in a definition list.
pub(in crate::parser) fn in_definition_list(containers: &ContainerStack) -> bool {
    containers
        .stack
        .iter()
        .any(|c| matches!(c, Container::DefinitionList { .. }))
}

/// Look ahead past blank lines to find a definition marker.
/// Returns Some(blank_line_count) if found, None otherwise.
///
/// `lines` is absolute-indexed. Pass a [`StrippedLines`] rather than a raw
/// slice whenever a container prefix is open: the marker test only accepts
/// 0-3 leading spaces, so a raw `> : b` never matches and the term line above
/// it would silently lose its definition body.
pub(in crate::parser) fn next_line_is_definition_marker(
    lines: &(impl LineView + ?Sized),
    pos: usize,
) -> Option<usize> {
    let mut check_pos = pos + 1;
    let mut blank_count = 0;
    while check_pos < lines.line_count() {
        let line = lines.line(check_pos);
        if line.trim().is_empty() {
            blank_count += 1;
            check_pos += 1;
            continue;
        }
        if let Some((marker, ..)) = try_parse_definition_marker(line) {
            // A marker on a line that does not carry the enclosing item's
            // content indent is not inside the item: the strip faked that
            // indent by eating content characters, so the `:` it landed on is
            // preceded by text on its own line. Pandoc folds such a line into
            // the paragraph above (`- a\nb\n\nc :` is `BulletList` + `Para "c
            // :"`), so it must not promote the term line above it.
            if !lines.carries_container_prefix(check_pos) {
                return None;
            }
            if marker == ':' && is_caption_followed_by_table(lines, check_pos) {
                return None;
            }
            return Some(blank_count);
        } else {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::blocks::tables::is_caption_followed_by_table;

    #[test]
    fn test_parse_definition_marker_colon() {
        assert_eq!(
            try_parse_definition_marker(":   Definition"),
            Some((':', 0, 3, 3))
        );
    }

    #[test]
    fn test_parse_definition_marker_tilde() {
        assert_eq!(
            try_parse_definition_marker("~   Definition"),
            Some(('~', 0, 3, 3))
        );
    }

    #[test]
    fn test_parse_definition_marker_indented() {
        assert_eq!(
            try_parse_definition_marker("  : Definition"),
            Some((':', 2, 1, 1))
        );
        assert_eq!(
            try_parse_definition_marker("   ~ Definition"),
            Some(('~', 3, 1, 1))
        );
        assert_eq!(try_parse_definition_marker("\t: Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_too_indented() {
        assert_eq!(try_parse_definition_marker("    : Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_no_space_after() {
        assert_eq!(try_parse_definition_marker(":Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_at_eol() {
        assert_eq!(try_parse_definition_marker(":"), Some((':', 0, 0, 0)));
    }

    #[test]
    fn test_parse_definition_marker_bare_with_newline() {
        // Lines retain their trailing newline (`split_inclusive`), so a bare
        // marker whose body starts on the next line arrives as ":\n". The
        // trailing newline is end-of-line, not content, so it's still a marker.
        assert_eq!(try_parse_definition_marker(":\n"), Some((':', 0, 0, 0)));
        assert_eq!(try_parse_definition_marker("~\n"), Some(('~', 0, 0, 0)));
        assert_eq!(try_parse_definition_marker(":\r\n"), Some((':', 0, 0, 0)));
    }

    #[test]
    fn next_line_marker_ignores_colon_table_caption() {
        let lines = [
            "Here's a table with a reference:",
            "",
            ": (\\#tab:mytable) A table with a reference.",
            "",
            "| A   | B   | C   |",
            "| --- | --- | --- |",
            "| 1   | 2   | 3   |",
        ];
        assert!(is_caption_followed_by_table(&lines[..], 2));
        assert_eq!(next_line_is_definition_marker(&lines[..], 0), None);
    }

    #[test]
    fn test_definition_list_preserves_first_content_line_losslessly() {
        let input = "[`--reference-doc=`*FILE*]{#option--reference-doc}\n\n:   Use the specified file as a style reference in producing a\n    docx or ODT file.\n\n    Docx\n\n    :   For best results, the reference docx should be a modified\n        version of a docx file produced using pandoc.\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }
}
