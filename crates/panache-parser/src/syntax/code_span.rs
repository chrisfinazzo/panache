//! Column-aware code-span payload extraction.
//!
//! A code span's bytes are not its content: pandoc's `tabFilter` expands every
//! tab *before* the reader runs, so a tab inside a code span is worth however
//! many columns it takes to reach the next tab stop **from its column in the
//! original line**. Reading the `INLINE_CODE_CONTENT` tokens on their own
//! therefore loses information, which is why both the pandoc-native projector
//! and the formatter go through [`code_span_payload`] instead.

use crate::parser::utils::container_stack::FOOTNOTE_INDENT_COLUMNS;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::NodeOrToken;

/// The content of the `INLINE_CODE` node `node`, with tabs expanded the way
/// pandoc's `tabFilter` does — from each tab's column in the source line,
/// counting any container prefix, since none of it has been stripped yet.
///
/// A code span therefore cannot be read token-text-wise: `` a`x\ty`b `` is
/// `Code "x y"` (the tab starts at column 3) while `` `x\n\ty` `` is
/// `Code "x     y"` (it starts at column 0). Internal line endings are kept as
/// `\n`; collapsing them to spaces is the caller's job, because the projector
/// and the formatter disagree about the surrounding padding.
///
/// The gobble closes the one gap the CST cannot express. Pandoc's `listLine`
/// takes an item's content column off every continuation line, and the parser
/// mirrors that by holding those bytes out of the span as `WHITESPACE`. A tab
/// straddling the content column has no byte boundary to split on — the CST is
/// byte-lossless, so the parser must leave it whole in the payload — and the
/// columns it loses to the gobble are subtracted here instead.
pub fn code_span_payload(node: &SyntaxNode, tab_width: usize) -> String {
    let tab_width = tab_width.max(1);
    let Some(first) = node.first_token() else {
        return String::new();
    };
    let (mut col, mut in_indent) = token_line_context(&first, tab_width);
    let gobble = list_gobble_columns(node, tab_width);
    let mut out = String::new();
    for el in node.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => {
                let expanded =
                    expand_span_tabs(t.text(), &mut col, &mut in_indent, gobble, tab_width);
                if t.kind() == SyntaxKind::INLINE_CODE_CONTENT {
                    out.push_str(&expanded);
                }
                if t.kind() == SyntaxKind::LINE_PREFIX {
                    in_indent = true;
                }
            }
            NodeOrToken::Node(n) => {
                expand_span_tabs(
                    &n.text().to_string(),
                    &mut col,
                    &mut in_indent,
                    gobble,
                    tab_width,
                );
            }
        }
    }
    out
}

fn expand_span_tabs(
    text: &str,
    col: &mut usize,
    in_indent: &mut bool,
    gobble: usize,
    tab_width: usize,
) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' => {
                let next = (*col / tab_width + 1) * tab_width;
                let from = if *in_indent { (*col).max(gobble) } else { *col };
                for _ in from..next {
                    out.push(' ');
                }
                *col = next;
            }
            '\n' => {
                out.push(c);
                *col = 0;
                *in_indent = true;
            }
            '\r' => out.push(c),
            ' ' => {
                out.push(c);
                *col += 1;
            }
            _ => {
                out.push(c);
                *col += 1;
                *in_indent = false;
            }
        }
    }
    out
}

fn token_line_context(token: &SyntaxToken, tab_width: usize) -> (usize, bool) {
    let mut pieces: Vec<String> = Vec::new();
    let mut in_indent = true;
    let mut cur = token.prev_token();
    while let Some(t) = cur {
        let text = t.text();
        if let Some(idx) = text.rfind('\n') {
            let tail = &text[idx + 1..];
            if !is_indent_run(tail) {
                in_indent = false;
            }
            pieces.push(tail.to_string());
            break;
        }
        if !matches!(
            t.kind(),
            SyntaxKind::BLOCK_QUOTE_MARKER | SyntaxKind::LINE_PREFIX
        ) && !is_indent_run(text)
        {
            in_indent = false;
        }
        pieces.push(text.to_string());
        cur = t.prev_token();
    }
    let col = pieces
        .iter()
        .rev()
        .fold(0usize, |col, piece| advance_columns(piece, col, tab_width));
    (col, in_indent)
}

fn is_indent_run(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || c == '\t')
}

fn advance_columns(text: &str, mut col: usize, tab_width: usize) -> usize {
    for c in text.chars() {
        match c {
            '\t' => col = (col / tab_width + 1) * tab_width,
            '\n' => col = 0,
            '\r' => {}
            _ => col += 1,
        }
    }
    col
}

/// Columns pandoc gobbles off every continuation line of the innermost
/// marker-introduced container holding `node` — that container's content
/// column, measured in the source line so nesting and blockquote prefixes are
/// already counted. Zero when no such container encloses `node`.
///
/// A list item (`listLine`) and a definition body share the rule and the
/// shape: a marker token, optionally its trailing spaces, then the content
/// column. The innermost one wins, and because the column is absolute, an
/// enclosing container's own gobble is already folded into it.
///
/// A footnote definition gobbles by the same rule but computes its column
/// differently: `noteBlock` strips a fixed `indentSpaces` (4) from every
/// continuation line regardless of how wide the `[^label]:` marker is, so the
/// column is the definition's own start column plus 4 — not the marker width.
fn list_gobble_columns(node: &SyntaxNode, tab_width: usize) -> usize {
    let Some(item) = node.ancestors().find(|a| {
        matches!(
            a.kind(),
            SyntaxKind::LIST_ITEM | SyntaxKind::DEFINITION | SyntaxKind::FOOTNOTE_DEFINITION
        )
    }) else {
        return 0;
    };
    if item.kind() == SyntaxKind::FOOTNOTE_DEFINITION {
        let Some(first) = item.first_token() else {
            return 0;
        };
        return token_line_context(&first, tab_width).0 + FOOTNOTE_INDENT_COLUMNS;
    }
    let marker_kind = if item.kind() == SyntaxKind::DEFINITION {
        SyntaxKind::DEFINITION_MARKER
    } else {
        SyntaxKind::LIST_MARKER
    };
    let tokens: Vec<SyntaxToken> = item
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .collect();
    let Some(i) = tokens.iter().position(|t| t.kind() == marker_kind) else {
        return 0;
    };
    let marker = &tokens[i];
    let mut col = advance_columns(
        marker.text(),
        token_line_context(marker, tab_width).0,
        tab_width,
    );
    if let Some(ws) = tokens.get(i + 1)
        && ws.kind() == SyntaxKind::WHITESPACE
        && !ws.text().contains('\n')
    {
        col = advance_columns(ws.text(), col, tab_width);
    }
    col
}
