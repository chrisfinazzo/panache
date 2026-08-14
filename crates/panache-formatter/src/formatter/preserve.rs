//! Line emission for `wrap = preserve`.
//!
//! Every other wrap mode re-renders inlines and so drops the padding at the
//! end of a source line as a side effect of rebuilding it. Preserve mode
//! copies bytes instead, which is what keeps the line breaks intact -- but it
//! also carried the trailing whitespace straight into the output (issue #496).
//! This module is the one place that turns a block's tokens into preserve-mode
//! lines, so the two concerns stay separated: line breaks preserved, padding
//! dropped.

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Split a block into the lines `wrap = preserve` should emit, with trailing
/// whitespace removed and hard breaks normalized.
///
/// Only `NEWLINE` and `HARD_LINE_BREAK` tokens end a line. Newlines inside
/// verbatim constructs never reach us as either kind -- a code span keeps its
/// whole body in one `INLINE_CODE_CONTENT` token, math uses `MATH_NEWLINE`, and
/// inline HTML uses `WHITESPACE` -- so their bytes are copied untouched and
/// this stays a prose-only trim.
///
/// A trailing run that genuinely *is* a hard break becomes `\` when
/// `escaped_line_breaks` is on, matching what the inline formatter emits for
/// every other wrap mode. Otherwise the original whitespace run is kept, since
/// it is the only thing carrying the break.
///
/// Container `LINE_PREFIX` tokens are kept, since most callers emit the
/// block's own source indentation. Callers that rebuild the prefix themselves
/// want [`preserve_lines_unprefixed`] instead.
pub(super) fn preserve_lines(node: &SyntaxNode, escaped_line_breaks: bool) -> Vec<String> {
    lines_with_prefix(node, escaped_line_breaks, LinePrefix::Keep)
}

/// Like [`preserve_lines`], but drops the container `LINE_PREFIX` tokens. They
/// are in the tree for losslessness; a caller that writes its own prefix on
/// every line would otherwise emit them twice.
pub(super) fn preserve_lines_unprefixed(
    node: &SyntaxNode,
    escaped_line_breaks: bool,
) -> Vec<String> {
    lines_with_prefix(node, escaped_line_breaks, LinePrefix::Drop)
}

#[derive(Clone, Copy, PartialEq)]
enum LinePrefix {
    Keep,
    Drop,
}

fn lines_with_prefix(
    node: &SyntaxNode,
    escaped_line_breaks: bool,
    prefix: LinePrefix,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    collect(node, escaped_line_breaks, prefix, &mut lines, &mut current);

    // Mirror `str::lines`: a block ending in a newline has no empty final line.
    if !current.is_empty() {
        lines.push(trim_padding(&current));
    }
    lines
}

fn collect(
    node: &SyntaxNode,
    escaped_line_breaks: bool,
    prefix: LinePrefix,
    lines: &mut Vec<String>,
    current: &mut String,
) {
    for item in node.children_with_tokens() {
        match item {
            NodeOrToken::Node(child) => {
                collect(&child, escaped_line_breaks, prefix, lines, current)
            }
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::LINE_PREFIX if prefix == LinePrefix::Drop => {}
                SyntaxKind::NEWLINE => {
                    lines.push(trim_padding(current));
                    current.clear();
                }
                SyntaxKind::HARD_LINE_BREAK => {
                    if escaped_line_breaks {
                        current.push('\\');
                    } else {
                        current.push_str(token.text().trim_end_matches(['\r', '\n']));
                    }
                    lines.push(std::mem::take(current));
                }
                _ => current.push_str(token.text()),
            },
        }
    }
}

/// Trim only ASCII spaces and tabs. `str::trim_end` would also eat a
/// non-breaking space, which is content.
fn trim_padding(line: &str) -> String {
    line.trim_end_matches([' ', '\t']).to_string()
}
