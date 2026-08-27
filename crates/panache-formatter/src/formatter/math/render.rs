//! Rendering pipeline for the math content formatter.
//!
//! Operates on a freshly re-parsed `MATH_CONTENT` tree (see the parent module).
//! The transforms are structural and each is independently idempotent — see
//! `STYLE.md` for the rules and the alignment idempotency argument. The short
//! version: every cell is *trimmed before its width is measured* and padding is
//! *trailing only*, so a second pass measures the same content widths and emits
//! identical bytes.

use rowan::NodeOrToken;

use super::ir::Ir;
use super::printer::Printer;
use super::{MathContext, MathFormatOptions, lower};
use crate::syntax::{
    AstNode, MathCommand, MathContent, MathLineBreak, MathScripted, SyntaxElement, SyntaxKind,
    SyntaxNode, SyntaxToken,
};
use panache_parser::semantic::math::{
    DelimiterRole, MathClass, SemanticMathAtom, math_atoms, semantic_math_atoms_in,
};

const INDENT: &str = "  ";

/// Entry point: dispatch on context. Returns delimiter-free content.
pub(super) fn render(tree: &SyntaxNode, opts: &MathFormatOptions) -> Option<String> {
    let top: Vec<SyntaxElement> = tree.children_with_tokens().collect();
    match opts.context {
        MathContext::Inline => render_inline_content(tree, &top, opts),
        MathContext::Display => render_display(tree, &top, opts),
        MathContext::EnvironmentBody => render_environment_body(tree, &top, opts),
    }
}

fn render_environment_body(
    tree: &SyntaxNode,
    elements: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_delimited_environment(&content, opts))
    {
        let mut output = Printer::new(opts.line_width, INDENT.len()).print(&document, INDENT.len());
        if ends_in_math_newline(elements) {
            output.push('\n');
        }
        return Some(output);
    }
    if !elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_ALIGN)
        && let Some(document) = MathContent::cast(tree.clone()).and_then(|content| {
            lower::try_lower_environment_content(&content, &opts.signature_scope)
        })
    {
        return Some(trimmed_body(
            &Printer::new(opts.line_width, INDENT.len()),
            &document,
            INDENT.len(),
        ));
    }
    if let Some(document) = lower::try_lower_environment_body(elements, opts) {
        return Some(trimmed_body(
            &Printer::new(opts.line_width, INDENT.len()),
            &document,
            INDENT.len(),
        ));
    }
    let has_comment = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
    let has_authored_break = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK);
    let has_top_level_authored_break = elements.iter().any(|element| {
        element
            .as_node()
            .cloned()
            .and_then(MathLineBreak::cast)
            .and_then(|line_break| line_break.marker_token())
            .is_some_and(|marker| marker.text() == r"\\")
    });
    if !elements.iter().any(contains_environment)
        && ((has_comment && !has_authored_break)
            || (has_authored_break && (!has_comment || has_top_level_authored_break)))
        && !tree
            .descendants_with_tokens()
            .any(|element| element.kind() == SyntaxKind::MATH_ALIGN)
        && let Some(document) = MathContent::cast(tree.clone()).and_then(|content| {
            lower::try_lower_environment_content(&content, &opts.signature_scope)
        })
    {
        return Some(trimmed_body(
            &Printer::new(opts.line_width, INDENT.len()),
            &document,
            INDENT.len(),
        ));
    }
    None
}

/// Inline math shares its line with the host paragraph or table cell, so a
/// lowered body is printed flat: a newline here would end the paragraph or
/// split the cell across rows. The exception is a body carrying a `%` comment,
/// which runs to end of line and therefore keeps its hard breaks.
fn render_inline_content(
    tree: &SyntaxNode,
    elements: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    let printer = Printer::new(opts.line_width, INDENT.len());
    let keeps_breaks = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
    let print = |document: &Ir| {
        if keeps_breaks {
            printer.print(document, 0)
        } else {
            printer.print_flat(document).trim().to_string()
        }
    };

    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_delimited_environment(&content, opts))
    {
        return Some(print(&document));
    }
    if elements.iter().any(contains_environment) {
        if let Some(document) = lower::try_lower_inline_environment(elements.to_vec(), opts) {
            return Some(print(&document));
        }
        let hanging_offset = keeps_breaks.then_some(1);
        if let Some(document) = mixed_segment_doc(elements, opts, hanging_offset) {
            return Some(print(&document));
        }
    }
    MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_content(&content, &opts.signature_scope))
        .map(|document| print(&document))
}

/// Print a delimited body without its own trailing break. A body that ends in
/// a comment or a `\\` row lowers to a trailing hard line so nothing follows it
/// on that line; the caller already puts the closing delimiter on a line of its
/// own, so keeping the break would leave a blank line between them.
fn trimmed_body(printer: &Printer, document: &Ir, indent: usize) -> String {
    printer.print(document, indent).trim_end().to_string()
}

fn render_display(
    tree: &SyntaxNode,
    top: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_delimited_environment(&content, opts))
    {
        let mut output =
            Printer::new(opts.line_width, INDENT.len()).print(&document, opts.math_indent);
        if ends_in_math_newline(top) {
            output.push('\n');
        }
        return Some(output);
    }
    if !top.iter().any(contains_environment)
        && let Some(document) = MathContent::cast(tree.clone()).and_then(|content| {
            lower::try_lower_display_content(
                &content,
                &opts.signature_scope,
                opts.line_width.saturating_sub(opts.math_indent),
            )
        })
    {
        return Some(trimmed_body(
            &Printer::new(opts.line_width, INDENT.len()),
            &document,
            opts.math_indent,
        ));
    }

    // An ordinary delimiter enclosing a block environment must itself take
    // the broken layout. Handle that structure before the generic atom
    // composition path, which correctly aligns the environment but cannot
    // make the surrounding delimiters participate in its forced breaks.
    if has_mixed_environment_content(top)
        && let Some(output) = render_typed_mixed_delimited_display(top, opts)
    {
        return Some(output);
    }

    if top.iter().any(contains_environment)
        && ordinary_delimiters_balanced(top)
        && let Some(document) = lower::try_lower_display_environment(
            top.to_vec(),
            opts,
            opts.line_width.saturating_sub(opts.math_indent),
        )
    {
        let significant = top
            .iter()
            .filter(|element| !is_layout_whitespace(element))
            .collect::<Vec<_>>();
        let base_indent = if significant.len() == 1
            && significant[0].as_node().is_some_and(|node| {
                node.kind() == SyntaxKind::MATH_ENVIRONMENT
                    || MathScripted::cast(node.clone()).is_some_and(|scripted| {
                        scripted
                            .base()
                            .and_then(SyntaxElement::into_node)
                            .is_some_and(|base| base.kind() == SyntaxKind::MATH_ENVIRONMENT)
                    })
            }) {
            0
        } else {
            opts.math_indent
        };
        return Some(Printer::new(opts.line_width, INDENT.len()).print(&document, base_indent));
    }

    if has_mixed_environment_content(top) {
        if let Some(output) = render_top_level_mixed_environment(top, opts) {
            return Some(output);
        }
        return None;
    }
    None
}

fn ends_in_math_newline(elements: &[SyntaxElement]) -> bool {
    elements
        .last()
        .is_some_and(|element| element.kind() == SyntaxKind::MATH_NEWLINE)
}

fn has_mixed_environment_content(elems: &[SyntaxElement]) -> bool {
    let has_environment = elems.iter().any(contains_environment);
    let has_free_content = elems.iter().any(|element| {
        element.kind() != SyntaxKind::MATH_ENVIRONMENT && !is_layout_whitespace(element)
    });
    has_environment && has_free_content
}

fn contains_environment(element: &SyntaxElement) -> bool {
    element.kind() == SyntaxKind::MATH_ENVIRONMENT
        || element.as_node().is_some_and(|node| {
            node.descendants()
                .any(|descendant| descendant.kind() == SyntaxKind::MATH_ENVIRONMENT)
        })
}

/// An element that lays out as an environment block: a bare
/// `MATH_ENVIRONMENT`, or a `MATH_SCRIPTED` node with an environment base,
/// whose script elements glue onto the block's closing line
/// (`\end{pmatrix}^T`).
fn environment_block(element: &SyntaxElement) -> Option<(SyntaxNode, Vec<SyntaxElement>)> {
    let node = element.as_node()?;
    if node.kind() == SyntaxKind::MATH_ENVIRONMENT {
        return Some((node.clone(), Vec::new()));
    }
    let scripted = MathScripted::cast(node.clone())?;
    let base = scripted.base()?.into_node()?;
    if base.kind() != SyntaxKind::MATH_ENVIRONMENT {
        return None;
    }
    let scripts = node
        .children_with_tokens()
        .filter(|child| {
            matches!(
                child.kind(),
                SyntaxKind::MATH_SUBSCRIPT | SyntaxKind::MATH_SUPERSCRIPT
            )
        })
        .collect();
    Some((base, scripts))
}

fn render_typed_mixed_delimited_display(
    elements: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    if elements.iter().any(contains_comment) {
        return None;
    }
    let environments = elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| environment_block(element).is_some().then_some(index))
        .collect::<Vec<_>>();
    let (open, close) = enclosing_delimiters(elements, &environments)?;
    let prefix = lower::try_lower_elements(elements[..=open].to_vec(), &opts.signature_scope)?;
    let suffix = lower::try_lower_elements(elements[close..].to_vec(), &opts.signature_scope)?;
    let body = delimited_body_doc_configured(&elements[open + 1..close], opts, true)?;
    let document = Ir::group(Ir::concat([
        prefix,
        Ir::indent(Ir::concat([Ir::SoftLine, body])),
        Ir::SoftLine,
        suffix,
    ]));
    Some(Printer::new(opts.line_width, INDENT.len()).print(&document, opts.math_indent))
}

fn enclosing_delimiters(
    elements: &[SyntaxElement],
    environments: &[usize],
) -> Option<(usize, usize)> {
    for open in 0..elements.len() {
        if element_word_class(&elements[open]) != Some(MathClass::Open) {
            continue;
        }
        let mut delimiters = vec![element_text(&elements[open])?];
        for (close, element) in elements.iter().enumerate().skip(open + 1) {
            match element_word_class(element) {
                Some(MathClass::Open) => delimiters.push(element_text(element)?),
                Some(MathClass::Close) => {
                    let opening = delimiters.pop()?;
                    if !delimiters_match(&opening, &element_text(element)?) {
                        break;
                    }
                    if delimiters.is_empty() {
                        if environments
                            .iter()
                            .all(|environment| open < *environment && *environment < close)
                        {
                            return Some((open, close));
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn contains_comment(element: &SyntaxElement) -> bool {
    element.kind() == SyntaxKind::MATH_COMMENT
        || element.as_node().is_some_and(|node| {
            node.descendants_with_tokens()
                .filter_map(|descendant| descendant.into_token())
                .any(|token| token.kind() == SyntaxKind::MATH_COMMENT)
        })
}

/// The token that carries an element's atom identity for the delimiter and
/// segment scans: the token itself, or the base token of a `MATH_SCRIPTED`
/// node — so a scripted closing delimiter (`)^2`) still closes its `(`.
/// Structured bases (groups, environments) return `None` and stay opaque.
fn element_atom_token(element: &SyntaxElement) -> Option<SyntaxToken> {
    match element {
        NodeOrToken::Token(token) => Some(token.clone()),
        NodeOrToken::Node(node) => MathCommand::cast(node.clone())
            .and_then(|command| command.name_token())
            .or_else(|| {
                let base = MathScripted::cast(node.clone())?.base()?;
                match base {
                    NodeOrToken::Token(token) => Some(token),
                    NodeOrToken::Node(node) => {
                        MathCommand::cast(node).and_then(|command| command.name_token())
                    }
                }
            }),
    }
}

fn element_text(element: &SyntaxElement) -> Option<String> {
    element_atom_token(element).map(|token| token.text().to_string())
}

fn element_word_class(element: &SyntaxElement) -> Option<MathClass> {
    let token = element_atom_token(element)?;
    if token.kind() != SyntaxKind::MATH_WORD {
        return None;
    }
    let element = SyntaxElement::Token(token);
    let mut atoms = math_atoms(&element);
    let atom = atoms.next()?;
    atoms.next().is_none().then_some(atom.class)
}

fn delimiters_match(open: &str, close: &str) -> bool {
    matches!((open, close), ("(", ")") | ("[", "]"))
}

fn render_top_level_mixed_environment(
    elems: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    if elems.iter().any(contains_comment) || !ordinary_delimiters_balanced(elems) {
        return None;
    }
    let doc = mixed_segment_doc(elems, opts, Some(0))
        .or_else(|| delimited_body_doc_configured(elems, opts, true))?;
    Some(Printer::new(opts.line_width, INDENT.len()).print(&doc, opts.math_indent))
}

pub(super) fn can_render_mixed_environment_comments(
    tree: &SyntaxNode,
    opts: &MathFormatOptions,
) -> bool {
    let elements = tree.children_with_tokens().collect::<Vec<_>>();
    if !elements.iter().any(contains_environment) {
        return false;
    }
    match opts.context {
        MathContext::Inline => mixed_segment_doc(&elements, opts, Some(1)).is_some(),
        MathContext::Display => lower::try_lower_display_environment(
            elements,
            opts,
            opts.line_width.saturating_sub(opts.math_indent),
        )
        .is_some(),
        MathContext::EnvironmentBody => false,
    }
}

fn ordinary_delimiters_balanced(elems: &[SyntaxElement]) -> bool {
    let mut openings = Vec::new();
    for atom in semantic_math_atoms_in(elems.iter().cloned()) {
        match atom.delimiter {
            Some(DelimiterRole::Open) => {
                let Some(text) = semantic_atom_text(atom, elems) else {
                    return false;
                };
                openings.push(text);
            }
            Some(DelimiterRole::Close) => {
                let Some(opening) = openings.pop() else {
                    return false;
                };
                let Some(closing) = semantic_atom_text(atom, elems) else {
                    return false;
                };
                if !delimiters_match(&opening, &closing) {
                    return false;
                }
            }
            Some(DelimiterRole::Fence) | None => {}
        }
    }
    openings.is_empty()
}

fn semantic_atom_text(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> Option<String> {
    let element = elements.iter().find(|element| {
        element.text_range().start() <= atom.range.start()
            && element.text_range().end() >= atom.range.end()
    })?;
    match element {
        SyntaxElement::Token(token) => {
            let start = usize::from(atom.range.start() - token.text_range().start());
            let end = usize::from(atom.range.end() - token.text_range().start());
            token.text().get(start..end).map(ToOwned::to_owned)
        }
        SyntaxElement::Node(_) => element_text(element),
    }
}

fn delimited_body_doc_configured(
    body: &[SyntaxElement],
    opts: &MathFormatOptions,
    break_at_punctuation: bool,
) -> Option<Ir> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;

    for (index, element) in body.iter().enumerate() {
        match element_word_class(element) {
            Some(MathClass::Open) => depth += 1,
            Some(MathClass::Close) => depth = depth.saturating_sub(1),
            Some(MathClass::Punct) if depth == 0 => {
                segments.push(Ir::concat([
                    mixed_segment_doc(&body[start..index], opts, Some(0))?,
                    Ir::text(element.to_string()),
                ]));
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(mixed_segment_doc(&body[start..], opts, Some(0))?);
    if break_at_punctuation {
        Some(Ir::join(Ir::Line, segments))
    } else {
        let printer = Printer::new(opts.line_width, INDENT.len());
        let mut offset = 0usize;
        let mut aligned = Vec::with_capacity(segments.len());
        for segment in segments {
            let final_line_width = printer
                .print(&segment, 0)
                .lines()
                .last()
                .unwrap_or_default()
                .chars()
                .count();
            aligned.push(Ir::align(offset, segment));
            offset += final_line_width;
        }
        Some(Ir::concat(aligned))
    }
}

fn mixed_segment_doc(
    segment: &[SyntaxElement],
    opts: &MathFormatOptions,
    hanging_offset: Option<usize>,
) -> Option<Ir> {
    if segment.iter().any(contains_unsafe_mixed_trivia) {
        return None;
    }
    if segment
        .iter()
        .any(|element| environment_block(element).is_none() && contains_environment(element))
    {
        return None;
    }
    let environment_indices: Vec<usize> = segment
        .iter()
        .enumerate()
        .filter_map(|(index, element)| environment_block(element).is_some().then_some(index))
        .collect();
    match environment_indices.as_slice() {
        [] => lower::try_lower_elements(segment.to_vec(), &opts.signature_scope),
        [_] => lower::try_lower_environment_composition(
            segment.to_vec(),
            opts,
            hanging_offset.unwrap_or(0),
        ),
        _ => None,
    }
}

/// Compose environments and surrounding free content inside `\left…\right`.
///
/// Comments or authored breaks in the surrounding expression, malformed or
/// unpunctuated multiple environments, and unbalanced ordinary delimiters
/// cross the verbatim preservation boundary. Each environment body retains its
/// normal row policy.
pub(super) fn mixed_delimited_environment_document(
    body: &MathContent,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    let elements = body.elements().collect::<Vec<_>>();
    if !ordinary_delimiters_balanced(&elements) {
        return None;
    }

    let environment_count = elements
        .iter()
        .filter(|element| environment_block(element).is_some())
        .count();
    let has_free_content = elements
        .iter()
        .any(|element| environment_block(element).is_none() && !is_layout_whitespace(element));
    if environment_count == 0 || !has_free_content {
        return None;
    }

    if environment_count == 1 {
        mixed_segment_doc(&elements, opts, Some(0))
    } else {
        // A top-level punctuation mark is the only boundary at which Badness
        // composes multiple environment documents. Each segment accepts at
        // most one, and unlike ordinary delimited lists, Badness glues the next
        // environment directly to the punctuation.
        delimited_body_doc_configured(&elements, opts, false)
    }
}

fn contains_unsafe_mixed_trivia(element: &SyntaxElement) -> bool {
    match environment_block(element) {
        // The environment interior is laid out row by row, where breaks and
        // comments are safe; only glued scripts must stay trivia-free.
        Some((_, scripts)) => scripts.iter().any(has_comment_or_line_break),
        None => has_comment_or_line_break(element),
    }
}

fn has_comment_or_line_break(element: &SyntaxElement) -> bool {
    fn is_marker(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::MATH_COMMENT | SyntaxKind::MATH_LINE_BREAK)
    }
    if is_marker(element.kind()) {
        return true;
    }
    element.as_node().is_some_and(|node| {
        node.descendants_with_tokens()
            .any(|descendant| is_marker(descendant.kind()))
    })
}

pub(super) fn is_layout_whitespace(el: &SyntaxElement) -> bool {
    matches!(el.kind(), SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE)
        && el.as_token().is_some()
}
