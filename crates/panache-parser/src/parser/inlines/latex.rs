//! Raw inline LaTeX parsing.
//!
//! Supports the `raw_tex` extension, which preserves inline LaTeX commands and
//! math environments.
//!
//! Inline LaTeX commands: \cite{ref}, \textbf{text}, etc.
//! Inline math environments: \begin{equation}...\end{equation}

use super::sink::InlineSink;
use crate::parser::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};
use crate::syntax::SyntaxKind;

/// Return the byte length of a raw inline LaTeX math environment.
pub(crate) fn try_parse_raw_math_environment(text: &str) -> Option<usize> {
    let env_name = extract_environment_name(text)?;
    if !is_inline_math_environment(env_name) {
        return None;
    }

    let begin_marker_len = text.find('}')? + 1;
    let end_marker = format!("\\end{{{env_name}}}");
    let end_rel = text[begin_marker_len..].find(&end_marker)?;
    Some(begin_marker_len + end_rel + end_marker.len())
}

/// Try to parse an inline LaTeX command starting at the given position.
/// Returns the number of **bytes** consumed if successful, or None.
///
/// LaTeX command pattern: \commandname[optional]{required}
/// - Starts with backslash
/// - Command name: letters only (a-zA-Z)
/// - Optional arguments in square brackets: [...]
/// - Required arguments in curly braces: {...}
pub(crate) fn try_parse_latex_command(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.is_empty() || bytes[0] != b'\\' {
        return None;
    }

    if bytes.len() > 1 && bytes[1] == b'\\' {
        return None;
    }

    let mut pos = 1; // Skip initial backslash

    let command_start = pos;
    while pos < bytes.len() && bytes[pos].is_ascii_alphabetic() {
        pos += 1;
    }

    if pos == command_start {
        return None;
    }

    while pos < bytes.len() {
        match bytes[pos] {
            b'[' => {
                pos = skip_bracketed_arg(text, pos)?;
            }
            b'{' => {
                pos = skip_braced_arg(text, pos)?;
            }
            _ => {
                break;
            }
        }
    }

    if pos > 1 { Some(pos) } else { None }
}

fn skip_bracketed_arg(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.get(start)? != &b'[' {
        return None;
    }

    let mut pos = start + 1;
    let mut depth = 1;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b'\\' if pos + 1 < bytes.len() => {
                pos += 2;
                continue;
            }
            _ => {}
        }
        pos += 1;
    }

    if depth == 0 { Some(pos) } else { None }
}

fn skip_braced_arg(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.get(start)? != &b'{' {
        return None;
    }

    let mut pos = start + 1;
    let mut depth = 1;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\\' if pos + 1 < bytes.len() => {
                pos += 2;
                continue;
            }
            _ => {}
        }
        pos += 1;
    }

    if depth == 0 { Some(pos) } else { None }
}

/// Parse a LaTeX command and add it to the builder.
pub(crate) fn parse_latex_command(builder: &mut impl InlineSink, text: &str, len: usize) {
    builder.start_node(SyntaxKind::LATEX_COMMAND.into());
    builder.token(SyntaxKind::TEXT.into(), &text[..len]);
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_math_environment_excludes_trailing_line_bytes() {
        let input = "\\begin{equation}\nx = y\n\\end{equation} \n";
        let len = try_parse_raw_math_environment(input).expect("raw math environment");

        assert_eq!(&input[..len], "\\begin{equation}\nx = y\n\\end{equation}");
    }

    #[test]
    fn test_simple_command() {
        assert_eq!(try_parse_latex_command(r"\cite{ref}"), Some(10));
        assert_eq!(try_parse_latex_command(r"\textbf{bold}"), Some(13));
    }

    #[test]
    fn test_command_with_optional_arg() {
        assert_eq!(
            try_parse_latex_command(r"\includegraphics[width=5cm]{file.png}"),
            Some(37)
        );
    }

    #[test]
    fn test_multiple_arguments() {
        assert_eq!(try_parse_latex_command(r"\newcommand{\foo}{bar}"), Some(22));
    }

    #[test]
    fn test_nested_braces() {
        assert_eq!(
            try_parse_latex_command(r"\command{text with {nested} braces}"),
            Some(35)
        );
    }

    #[test]
    fn test_no_arguments() {
        assert_eq!(try_parse_latex_command(r"\LaTeX "), Some(6));
    }

    #[test]
    fn test_escaped_backslash() {
        assert_eq!(try_parse_latex_command(r"\\"), None);
    }

    #[test]
    fn test_not_latex() {
        assert_eq!(try_parse_latex_command(r"\123"), None); // Numbers not allowed
        assert_eq!(try_parse_latex_command(r"\ "), None); // No command name
        assert_eq!(try_parse_latex_command("no backslash"), None);
    }

    #[test]
    fn test_unclosed_braces() {
        assert_eq!(try_parse_latex_command(r"\cite{ref"), None);
    }

    #[test]
    fn test_unclosed_brackets() {
        assert_eq!(try_parse_latex_command(r"\command[opt"), None);
    }
}
