//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, DelimiterRole, MathBreakPriority, MathClass, SemanticMathAtom,
    SignatureScope, match_arg_slot, math_atoms, semantic_math_atoms_in,
};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{
    MathArgument, MathCommand, MathContent, MathDelimited, MathEnvironment, MathGroup,
    MathLineBreak, MathScript, MathScripted, SyntaxElement, SyntaxKind, SyntaxToken,
};

use super::ir::Ir;
use super::{MathFormatOptions, render};

/// Lower a supported math content body into the shared document IR.
///
/// Returning `None` keeps every unsupported shape on the legacy renderer until
/// its own parity slice lands.
pub(super) fn try_lower_content(content: &MathContent, scope: &SignatureScope) -> Option<Ir> {
    lower_body(
        content.elements().collect(),
        scope,
        Spacing::Normal,
        true,
        false,
    )
}

/// Lower a free display body, including Panache's implicit alignment for
/// relation chains separated by authored `\\` row markers.
pub(super) fn try_lower_display_content(
    content: &MathContent,
    scope: &SignatureScope,
    line_width: usize,
) -> Option<Ir> {
    let elements = content.elements().collect::<Vec<_>>();
    try_lower_display_elements(elements, scope, line_width)
}

/// Lower a formatter-derived free-display segment without inventing a CST wrapper.
pub(super) fn try_lower_display_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    line_width: usize,
) -> Option<Ir> {
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK)
    {
        lower_authored_breaks(
            elements,
            semantic_atoms,
            scope,
            Spacing::Normal,
            AuthoredBreakOptions {
                preserve_comment_context: true,
                environment_rows: false,
                align_authored_relations: true,
                display_width: Some(line_width),
            },
        )
    } else if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT)
    {
        lower_body_with_atoms(
            elements,
            semantic_atoms,
            scope,
            Spacing::Normal,
            true,
            false,
        )
    } else {
        let pieces = lower_pieces_with_atoms(
            &elements,
            &semantic_atoms,
            scope,
            Spacing::Normal,
            true,
            false,
        )?;
        Some(layout_display_pieces(&pieces, line_width, Spacing::Normal))
    }
}

/// Lower a free display containing one comment-bearing top-level environment.
///
/// The environment is a semantic operand whose forced lines participate in
/// the same binary- and relation-led layout as every other typed atom. Its
/// closing-line width lets following atoms compose without flattening the
/// environment document.
pub(super) fn try_lower_display_environment(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
    line_width: usize,
) -> Option<Ir> {
    let environment_indices = elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| display_environment(element).map(|_| index))
        .collect::<Vec<_>>();
    let [environment_index] = environment_indices.as_slice() else {
        return None;
    };
    let environment_element = &elements[*environment_index];
    let (environment, scripted) = display_environment(environment_element)?;
    if !environment
        .syntax()
        .descendants_with_tokens()
        .any(|descendant| descendant.kind() == SyntaxKind::MATH_COMMENT)
    {
        return None;
    }
    let end = environment.end()?;
    let script_document = match scripted.as_ref() {
        Some(scripted) => {
            if !environment_scripted_is_supported(scripted, &environment, &opts.signature_scope) {
                return None;
            }
            Some(lower_script_suffix(
                scripted,
                &opts.signature_scope,
                true,
                false,
            )?)
        }
        None => None,
    };
    let script_width = script_document.as_ref().map_or(Some(0), Ir::flat_width)?;
    let end_width = end.syntax().text().to_string().chars().count() + script_width;
    let environment_document = Ir::concat([
        render::environment_document(environment.syntax(), opts)?,
        script_document.unwrap_or(Ir::Nil),
    ]);
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    let environment_atom = semantic_atoms.iter().copied().find(|atom| {
        atom.range.start() == environment_element.text_range().start()
            && atom.range.end() == environment_element.text_range().end()
    })?;

    let before = &elements[..*environment_index];
    let after = &elements[*environment_index + 1..];
    let before_atoms = semantic_atoms_for(before, &semantic_atoms);
    if !display_environment_has_supported_prefix(before, &before_atoms) {
        return None;
    }
    let mut pieces = lower_pieces_with_atoms(
        before,
        &before_atoms,
        &opts.signature_scope,
        Spacing::Normal,
        true,
        false,
    )?;
    pieces.push(Piece {
        role: Role::from(environment_atom.break_priority),
        delimiter: environment_atom.delimiter,
        assignment: false,
        definition: false,
        unary: environment_atom.coerced_unary,
        authored_space_before: before_atoms
            .last()
            .is_some_and(|atom| atom.range.end() < environment_atom.range.start()),
        slash: false,
        control_word_operator: false,
        starts_control_word_letter: false,
        ends_control_word: false,
        multiline_tail_width: Some(end_width),
        document: environment_document,
    });
    let after_pieces = lower_pieces_with_atoms(
        after,
        &semantic_atoms_for(after, &semantic_atoms),
        &opts.signature_scope,
        Spacing::Normal,
        true,
        false,
    )?;
    pieces.extend(after_pieces);

    Some(layout_display_pieces(&pieces, line_width, Spacing::Normal))
}

fn display_environment_has_supported_prefix(
    elements: &[SyntaxElement],
    atoms: &[SemanticMathAtom],
) -> bool {
    let Some((last, prefix)) = atoms.split_last() else {
        return false;
    };
    if matches!(
        last.break_priority,
        MathBreakPriority::Binary | MathBreakPriority::Relation
    ) {
        return !prefix.is_empty();
    }

    if !last.coerced_unary && last.class == MathClass::Ord {
        return true;
    }

    if last.class == MathClass::Inner && is_delimited_operand(elements, last.range) {
        return true;
    }

    last.coerced_unary
        && prefix.last().is_some_and(|atom| {
            matches!(
                atom.break_priority,
                MathBreakPriority::Binary | MathBreakPriority::Relation
            )
        })
        && elements.iter().any(|element| {
            let Some(token) = element.as_token().filter(|token| {
                token.kind() == SyntaxKind::MATH_WORD
                    && token.text_range().start() <= last.range.start()
                    && token.text_range().end() >= last.range.end()
            }) else {
                return false;
            };
            token_slice(last.range, token).is_some_and(|text| matches!(text.as_str(), "+" | "-"))
        })
}

fn is_delimited_operand(elements: &[SyntaxElement], range: TextRange) -> bool {
    elements.iter().any(|element| {
        if element.text_range() != range {
            return false;
        }
        let Some(node) = element.as_node() else {
            return false;
        };
        MathDelimited::cast(node.clone()).is_some()
            || MathScripted::cast(node.clone())
                .and_then(|scripted| scripted.base())
                .and_then(SyntaxElement::into_node)
                .and_then(MathDelimited::cast)
                .is_some()
    })
}

fn display_environment(element: &SyntaxElement) -> Option<(MathEnvironment, Option<MathScripted>)> {
    let node = element.as_node()?;
    if let Some(environment) = MathEnvironment::cast(node.clone()) {
        return Some((environment, None));
    }
    let scripted = MathScripted::cast(node.clone())?;
    let environment = scripted
        .base()?
        .into_node()
        .and_then(MathEnvironment::cast)?;
    Some((environment, Some(scripted)))
}

fn environment_scripted_is_supported(
    scripted: &MathScripted,
    environment: &MathEnvironment,
    scope: &SignatureScope,
) -> bool {
    let base_range = environment.syntax().text_range();
    scripted.syntax().children_with_tokens().all(|element| {
        element.text_range() == base_range
            || is_layout_trivia(&element)
            || element
                .into_node()
                .and_then(MathScript::cast)
                .is_some_and(|script| script_is_supported(&script, scope))
    })
}

/// Lower a closed paired delimiter whose body contains one well-formed environment.
///
/// This stays separate from ordinary atom lowering until environments become
/// first-class typed atom documents. The narrow shape lets the existing
/// environment-grid document compose without admitting unpunctuated multiple
/// environments or malformed delimiter bodies.
pub(super) fn try_lower_delimited_environment(
    content: &MathContent,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    let mut top = content
        .elements()
        .filter(|element| !is_layout_trivia(element));
    let delimited = top.next()?.into_node().and_then(MathDelimited::cast)?;
    if top.next().is_some() {
        return None;
    }

    let left = delimited.left_token()?;
    let open = delimited.opening_delimiter()?;
    let body = delimited.body()?;
    let right = delimited.right_token()?;
    let close = delimited.closing_delimiter()?;
    let body_elements = body
        .elements()
        .filter(|element| !is_layout_trivia(element))
        .collect::<Vec<_>>();
    let body_document = match body_elements.as_slice() {
        [element] => {
            let environment = element.as_node().cloned().and_then(MathEnvironment::cast)?;
            render::environment_document(environment.syntax(), opts)?
        }
        _ => render::mixed_delimited_environment_document(&body, opts)?,
    };
    let opening_width = left.text().chars().count() + open.text().chars().count();
    // Badness lays comment-pinned inline continuations out after the host `$`
    // opener, which the delimiter-free math entry point does not retain.
    let host_inline_offset = usize::from(opts.context == super::MathContext::Inline);
    Some(Ir::concat([
        Ir::verbatim(left.text()),
        Ir::verbatim(open.text()),
        Ir::text(" "),
        Ir::align(opening_width + 1 + host_inline_offset, body_document),
        Ir::text(" "),
        Ir::verbatim(right.text()),
        Ir::verbatim(close.text()),
    ]))
}

/// Lower an environment body, where Badness canonicalizes a space before each
/// authored row marker.
pub(super) fn try_lower_environment_content(
    content: &MathContent,
    scope: &SignatureScope,
) -> Option<Ir> {
    lower_body(
        content.elements().collect(),
        scope,
        Spacing::Normal,
        true,
        true,
    )
}

/// Lower a formatter-derived row or cell without inventing a CST wrapper.
pub(super) fn try_lower_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
) -> Option<Ir> {
    lower_body(elements, scope, Spacing::Normal, true, false)
}

/// Lower a bracketed body, routing comment-bearing bodies through hard lines.
fn lower_body(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    lower_body_configured(
        elements,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
        false,
    )
}

fn lower_body_configured(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
    align_authored_relations: bool,
) -> Option<Ir> {
    // A row break changes layout, but Badness retains the preceding atom when
    // assigning the following operator's contextual role.
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK)
    {
        lower_authored_breaks(
            elements,
            semantic_atoms,
            scope,
            spacing,
            AuthoredBreakOptions {
                preserve_comment_context,
                environment_rows,
                align_authored_relations,
                display_width: None,
            },
        )
    } else {
        lower_body_with_atoms(
            elements,
            semantic_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    }
}

fn lower_body_with_atoms(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT)
    {
        lower_edge_comments(
            elements,
            semantic_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    } else {
        lower_elements_with_atoms(
            elements,
            semantic_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    }
}

#[derive(Clone, Copy)]
struct RelationLayout {
    column: usize,
    rhs_start: usize,
    assignment: bool,
    definition: bool,
}

#[derive(Clone, Copy)]
struct AuthoredBreakOptions {
    preserve_comment_context: bool,
    environment_rows: bool,
    align_authored_relations: bool,
    display_width: Option<usize>,
}

fn lower_authored_breaks(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    options: AuthoredBreakOptions,
) -> Option<Ir> {
    let AuthoredBreakOptions {
        preserve_comment_context,
        environment_rows,
        align_authored_relations,
        display_width,
    } = options;
    struct AuthoredRow {
        document: Ir,
        marker: Option<String>,
        authored_space: bool,
        adjacent_comment: Option<String>,
        first_relation: Option<RelationLayout>,
        starts_with_relation: bool,
        has_align: bool,
        breakable_pieces: Option<Vec<Piece>>,
    }

    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut elements = elements.into_iter().peekable();

    while let Some(element) = elements.next() {
        if element.kind() != SyntaxKind::MATH_LINE_BREAK {
            row.push(element);
            continue;
        }

        let line_break = element.into_node().and_then(MathLineBreak::cast)?;
        if line_break.marker_token().as_ref().map(SyntaxToken::text) != Some(r"\\")
            || line_break
                .modifier()
                .is_some_and(|modifier| !modifier.is_closed())
        {
            return None;
        }

        let authored_space = row.iter().any(|element| !is_layout_trivia(element))
            && row.last().is_some_and(is_layout_trivia);
        let row_atoms = if environment_rows {
            semantic_math_atoms_in(row.iter().cloned()).collect()
        } else {
            semantic_atoms_for(&row, &semantic_atoms)
        };
        let has_align = row
            .iter()
            .any(|element| element.kind() == SyntaxKind::MATH_ALIGN);
        let (first_relation, starts_with_relation) = if align_authored_relations && !has_align {
            relation_layout(
                &row,
                &row_atoms,
                scope,
                spacing,
                preserve_comment_context,
                environment_rows,
            )?
        } else {
            (None, false)
        };
        let row_elements = std::mem::take(&mut row);
        let breakable_pieces = (!has_align
            && !row_elements
                .iter()
                .any(|element| element.kind() == SyntaxKind::MATH_COMMENT))
        .then(|| {
            lower_pieces_with_atoms(
                &row_elements,
                &row_atoms,
                scope,
                spacing,
                preserve_comment_context,
                environment_rows,
            )
        })
        .flatten();
        let document = lower_authored_row(
            row_elements,
            &row_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?;
        // Look past the trivia after the marker for a comment on the same
        // logical row, buffering it rather than cloning the whole iterator.
        let mut skipped = Vec::new();
        while environment_rows && elements.peek().is_some_and(is_layout_trivia) {
            skipped.push(elements.next().expect("peeked element"));
        }
        let adjacent_comment = if environment_rows
            && elements
                .peek()
                .is_some_and(|element| element.kind() == SyntaxKind::MATH_COMMENT)
        {
            let comment = elements.next().expect("peeked comment");
            if elements
                .peek()
                .is_some_and(|element| element.kind() == SyntaxKind::MATH_NEWLINE)
            {
                elements.next();
            }
            Some(comment.to_string())
        } else {
            // No comment followed, so the trivia opens the next row.
            row.extend(skipped);
            None
        };
        rows.push(AuthoredRow {
            document,
            marker: Some(line_break.syntax().to_string()),
            authored_space,
            adjacent_comment,
            first_relation,
            starts_with_relation,
            has_align,
            breakable_pieces,
        });
    }

    let row_atoms = if environment_rows {
        semantic_math_atoms_in(row.iter().cloned()).collect()
    } else {
        semantic_atoms_for(&row, &semantic_atoms)
    };
    let has_align = row
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_ALIGN);
    let (first_relation, starts_with_relation) = if align_authored_relations && !has_align {
        relation_layout(
            &row,
            &row_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?
    } else {
        (None, false)
    };
    let breakable_pieces = (!has_align
        && !row
            .iter()
            .any(|element| element.kind() == SyntaxKind::MATH_COMMENT))
    .then(|| {
        lower_pieces_with_atoms(
            &row,
            &row_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    })
    .flatten();
    let document = lower_authored_row(
        row,
        &row_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    rows.push(AuthoredRow {
        document,
        marker: None,
        authored_space: false,
        adjacent_comment: None,
        first_relation,
        starts_with_relation,
        has_align,
        breakable_pieces,
    });

    // A final row with no content of its own would otherwise leave a trailing
    // hard break, printing as a blank line before the closing delimiter.
    if rows.len() > 1
        && rows
            .last()
            .is_some_and(|row| row.marker.is_none() && matches!(row.document, Ir::Nil))
    {
        rows.pop();
    }

    let max_row_width = if environment_rows {
        rows.iter()
            .filter_map(|row| row.document.flat_width())
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let mut relation_indents = vec![0usize; rows.len()];
    if align_authored_relations {
        let mut index = 0;
        while index < rows.len() {
            if rows[index].marker.is_some() {
                let mut end = index;
                while rows[end].marker.is_some()
                    && end + 1 < rows.len()
                    && rows[end + 1].starts_with_relation
                {
                    end += 1;
                }
                if end > index && !rows[index..=end].iter().any(|row| row.has_align) {
                    let head_relation = rows[index].first_relation;
                    let head_width = rows[index].document.flat_width()?;
                    for continuation in index + 1..=end {
                        let continuation_relation = rows[continuation].first_relation?;
                        relation_indents[continuation] = match head_relation {
                            Some(head) if head.definition => 0,
                            Some(head) if !head.assignment => head.column,
                            Some(head) if continuation_relation.assignment => head.column,
                            Some(head) => head.rhs_start,
                            None => head_width.checked_add(1)?,
                        };
                    }
                    index = end + 1;
                    continue;
                }
            }
            index += 1;
        }
    }
    let mut documents = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let row_width = row.document.flat_width();
        let document = match (display_width, row.breakable_pieces) {
            (Some(width), Some(pieces)) => layout_display_pieces(
                &pieces,
                width.saturating_sub(relation_indents[index]),
                spacing,
            ),
            _ => row.document,
        };
        let mut row_documents = vec![document];
        if let Some(marker) = row.marker {
            if environment_rows {
                let padding = row_width.map_or(1, |width| max_row_width.saturating_sub(width) + 1);
                row_documents.push(Ir::text(" ".repeat(padding)));
            } else if row.authored_space {
                row_documents.push(Ir::text(" "));
            }
            row_documents.push(Ir::verbatim(marker));
            if let Some(comment) = row.adjacent_comment {
                row_documents.extend([Ir::text(" "), Ir::verbatim(comment)]);
            }
        }
        let row_document = Ir::concat(row_documents);
        if index == 0 {
            documents.push(row_document);
        } else if relation_indents[index] > 0 {
            documents.push(Ir::align(
                relation_indents[index],
                Ir::concat([Ir::HardLine, row_document]),
            ));
        } else {
            documents.extend([Ir::HardLine, row_document]);
        }
    }
    Some(Ir::concat(documents))
}

fn lower_authored_row(
    elements: Vec<SyntaxElement>,
    semantic_atoms: &[SemanticMathAtom],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    if !elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_ALIGN)
    {
        return lower_body_with_atoms(
            elements,
            semantic_atoms.to_vec(),
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        );
    }

    let mut documents = Vec::new();
    let mut cell = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        if element.kind() != SyntaxKind::MATH_ALIGN {
            cell.push(element.clone());
            continue;
        }

        let spaced_before = cell.last().is_some_and(|element| {
            element.kind() == SyntaxKind::MATH_SPACE
                && cell.iter().any(|element| !is_layout_trivia(element))
        });
        let cell_atoms = semantic_atoms_for(&cell, semantic_atoms);
        let spaced_before = spaced_before
            || cell_atoms
                .last()
                .is_some_and(|atom| atom.break_priority != MathBreakPriority::None);
        documents.push(lower_body_with_atoms(
            std::mem::take(&mut cell),
            cell_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?);

        let next_separator = elements[index + 1..]
            .iter()
            .find(|element| element.kind() == SyntaxKind::MATH_ALIGN)
            .map(SyntaxElement::text_range);
        let spaced_after = elements[index + 1..]
            .first()
            .is_some_and(|element| element.kind() == SyntaxKind::MATH_SPACE)
            || semantic_atoms
                .iter()
                .find(|atom| {
                    atom.range.start() >= element.text_range().end()
                        && next_separator.is_none_or(|range| atom.range.end() <= range.start())
                })
                .is_some_and(|atom| atom.break_priority != MathBreakPriority::None);
        if spaced_before {
            documents.push(Ir::text(" "));
        }
        documents.push(Ir::verbatim(element.to_string()));
        if spaced_after {
            documents.push(Ir::text(" "));
        }
    }

    let cell_atoms = semantic_atoms_for(&cell, semantic_atoms);
    documents.push(lower_body_with_atoms(
        cell,
        cell_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?);
    Some(Ir::concat(documents))
}

/// Badness indents a comment-broken body by one column per bracket level,
/// including the closing delimiter after a trailing comment. Applying the
/// hanging indent only to broken bodies keeps every flat body byte-identical.
fn hanging(width: usize, body: Ir) -> Ir {
    if body.contains_forced_break() {
        Ir::align(width, body)
    } else {
        body
    }
}

fn lower_edge_comments(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let mut documents = Vec::new();
    let mut segment_start = 0;
    for (index, comment) in elements.iter().enumerate() {
        if comment.kind() != SyntaxKind::MATH_COMMENT {
            continue;
        }
        let segment = &elements[segment_start..index];
        let segment_atoms = if preserve_comment_context {
            semantic_atoms_for(segment, &semantic_atoms)
        } else {
            semantic_math_atoms_in(segment.iter().cloned()).collect()
        };
        let has_content = !segment_atoms.is_empty();
        documents.push(lower_elements_with_atoms(
            segment.to_vec(),
            segment_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?);
        if has_content {
            let trailing_trivia = segment
                .iter()
                .rev()
                .take_while(|element| is_layout_trivia(element));
            let mut has_space = false;
            let mut has_newline = false;
            for trivia in trailing_trivia {
                has_space = true;
                has_newline |= trivia.kind() == SyntaxKind::MATH_NEWLINE;
            }
            if has_newline {
                return None;
            } else if has_space {
                documents.push(Ir::text(" "));
            }
        }
        documents.push(Ir::verbatim(comment.to_string()));
        // A comment runs to end of line, so anything the caller emits after
        // this body -- a closing brace, `\end`, the next segment -- has to
        // start on a new line.
        if index + 1 < elements.len() {
            documents.push(Ir::HardLine);
        }
        segment_start = index + 1;
    }
    let segment = &elements[segment_start..];
    let trailing_atoms = if preserve_comment_context {
        semantic_atoms_for(segment, &semantic_atoms)
    } else {
        semantic_math_atoms_in(segment.iter().cloned()).collect()
    };
    documents.push(lower_elements_with_atoms(
        segment.to_vec(),
        trailing_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?);
    Some(Ir::concat(documents))
}

fn semantic_atoms_for(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
) -> Vec<SemanticMathAtom> {
    semantic_atoms
        .iter()
        .copied()
        .filter(|atom| {
            elements.iter().any(|element| {
                element.text_range().start() <= atom.range.start()
                    && element.text_range().end() >= atom.range.end()
            })
        })
        .collect()
}

fn lower_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect();
    lower_elements_with_atoms(
        elements,
        semantic_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )
}

fn lower_elements_with_atoms(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let pieces = lower_pieces_with_atoms(
        &elements,
        &semantic_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    Some(document_from_pieces(&pieces, spacing))
}

fn lower_pieces_with_atoms(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Vec<Piece>> {
    if has_unsupported_scripted_composite_relation(elements)
        || !elements
            .iter()
            .all(|element| is_supported_element(element, scope))
    {
        return None;
    }

    let semantic_atoms = coalesce_definition_relations(elements, semantic_atoms);
    let mut pieces = Vec::new();
    let mut previous_end = None;

    for atom in semantic_atoms {
        let atom_document = definition_relation_document(
            atom,
            elements,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
        .or_else(|| {
            atom_document(
                atom,
                elements,
                scope,
                spacing,
                preserve_comment_context,
                environment_rows,
            )
        })?;
        pieces.push(Piece {
            role: Role::from(atom.break_priority),
            delimiter: atom.delimiter,
            assignment: atom.break_priority == MathBreakPriority::Relation
                && relation_is_assignment(atom, elements),
            definition: atom.break_priority == MathBreakPriority::Relation
                && is_definition_relation(atom, elements),
            unary: atom.coerced_unary,
            authored_space_before: previous_end.is_some_and(|end| end < atom.range.start()),
            slash: atom_document.slash,
            control_word_operator: atom_document.control_word_operator,
            starts_control_word_letter: atom_document.starts_control_word_letter,
            ends_control_word: atom_document.ends_control_word,
            multiline_tail_width: None,
            document: atom_document.document,
        });
        previous_end = Some(atom.range.end());
    }

    Some(pieces)
}

/// Badness's semantic stream keeps each definition colon as punctuation, but
/// its formatter prints the contiguous colon run and following `=` relation as
/// one operator. Authored whitespace still separates the scalars; a scripted
/// equals may cross the CST boundary because its script belongs to the whole
/// definition relation.
fn coalesce_definition_relations(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
) -> Vec<SemanticMathAtom> {
    let mut coalesced = Vec::with_capacity(semantic_atoms.len());
    let mut index = 0;
    while let Some(&first) = semantic_atoms.get(index) {
        let Some(word) = elements.iter().find_map(|element| {
            element.as_token().filter(|token| {
                token.kind() == SyntaxKind::MATH_WORD
                    && token.text_range().start() <= first.range.start()
                    && token.text_range().end() >= first.range.end()
            })
        }) else {
            coalesced.push(first);
            index += 1;
            continue;
        };
        if first.class != MathClass::Punct || token_slice(first.range, word).as_deref() != Some(":")
        {
            coalesced.push(first);
            index += 1;
            continue;
        }

        let mut relation_index = index + 1;
        while semantic_atoms.get(relation_index).is_some_and(|atom| {
            atom.class == MathClass::Punct
                && semantic_atoms[relation_index - 1].range.end() == atom.range.start()
                && token_slice(atom.range, word).as_deref() == Some(":")
        }) {
            relation_index += 1;
        }
        let Some(&relation) = semantic_atoms.get(relation_index) else {
            coalesced.push(first);
            index += 1;
            continue;
        };
        if relation.class != MathClass::Rel
            || semantic_atoms[relation_index - 1].range.end() != relation.range.start()
            || !atom_starts_with_equals(relation, elements)
        {
            coalesced.push(first);
            index += 1;
            continue;
        }

        coalesced.push(SemanticMathAtom {
            range: TextRange::new(first.range.start(), relation.range.end()),
            class: MathClass::Rel,
            delimiter: None,
            break_priority: MathBreakPriority::Relation,
            coerced_unary: false,
        });
        index = relation_index + 1;
    }
    coalesced
}

fn atom_starts_with_equals(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> bool {
    elements.iter().any(|element| {
        if element.text_range().start() > atom.range.start()
            || element.text_range().end() < atom.range.end()
        {
            return false;
        }
        match element {
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD => {
                token_slice(atom.range, token).is_some_and(|text| text.starts_with('='))
            }
            SyntaxElement::Node(node) => MathScripted::cast(node.clone())
                .and_then(|scripted| scripted.base())
                .and_then(SyntaxElement::into_token)
                .is_some_and(|base| {
                    base.kind() == SyntaxKind::MATH_WORD && base.text().starts_with('=')
                }),
            _ => false,
        }
    })
}

fn document_from_pieces(pieces: &[Piece], spacing: Spacing) -> Ir {
    let mut documents = Vec::new();
    let mut column = 0usize;
    for (index, piece) in pieces.iter().enumerate() {
        if piece_gap_before(pieces, index, spacing) {
            documents.push(Ir::text(" "));
            column += 1;
        }
        if let Some(tail_width) = piece.multiline_tail_width {
            documents.push(Ir::align(column, piece.document.clone()));
            column += tail_width;
        } else {
            column += piece.document.flat_width().unwrap_or(0);
            documents.push(piece.document.clone());
        }
    }

    Ir::concat(documents)
}

fn piece_gap_before(pieces: &[Piece], index: usize, spacing: Spacing) -> bool {
    if index == 0 {
        return false;
    }
    let slash_is_spaced = |slash_index: usize| {
        pieces[slash_index].slash
            && (pieces[slash_index].authored_space_before
                || pieces
                    .get(slash_index + 1)
                    .is_some_and(|piece| piece.authored_space_before)
                || adjacent_operator(pieces, slash_index, spacing))
    };
    gap_before(pieces, index, spacing) || slash_is_spaced(index - 1) || slash_is_spaced(index)
}

fn piece_columns(pieces: &[Piece], spacing: Spacing) -> Option<Vec<(usize, usize)>> {
    let mut column = 0usize;
    let mut columns = Vec::with_capacity(pieces.len());
    for (index, piece) in pieces.iter().enumerate() {
        if piece_gap_before(pieces, index, spacing) {
            column = column.checked_add(1)?;
        }
        let start = column;
        column = column.checked_add(
            piece
                .multiline_tail_width
                .or_else(|| piece.document.flat_width())?,
        )?;
        columns.push((start, column));
    }
    Some(columns)
}

fn top_level_operators(pieces: &[Piece]) -> Vec<(usize, Role)> {
    let mut depth = 0usize;
    let mut operators = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if depth == 0 && piece.role != Role::Operand {
            operators.push((index, piece.role));
        }
        match piece.delimiter {
            Some(DelimiterRole::Open) => depth += 1,
            Some(DelimiterRole::Close) => depth = depth.saturating_sub(1),
            Some(DelimiterRole::Fence) | None => {}
        }
    }
    operators
}

/// Lay out a typed free-display row using the same relation-first hierarchy as
/// the legacy breaker: relation continuations align semantically, and an
/// over-width relation segment breaks again at each binary operator.
fn layout_display_pieces(pieces: &[Piece], line_width: usize, spacing: Spacing) -> Ir {
    let flat = document_from_pieces(pieces, spacing);
    if flat.flat_width().is_some_and(|width| width <= line_width) {
        return flat;
    }
    if !pieces
        .iter()
        .any(|piece| piece.multiline_tail_width.is_some())
        && flat.flat_width().is_none()
    {
        return flat;
    }

    let relations = top_level_operators(pieces)
        .into_iter()
        .filter_map(|(index, role)| (role == Role::Relation).then_some(index))
        .collect::<Vec<_>>();
    let mut lines = Vec::<(usize, Ir)>::new();

    if relations.len() < 2 {
        lines.extend(layout_display_segment(pieces, 0, line_width, spacing));
    } else {
        let Some(columns) = piece_columns(pieces, spacing) else {
            return flat;
        };
        let first = relations[0];
        let relation_column = columns.get(first).map_or(0, |&(start, _)| start);
        let rhs_start = columns
            .get(first)
            .map_or(relation_column, |&(_, end)| end.saturating_add(1));
        let first_assignment = pieces[first].assignment;
        let bounds = std::iter::once(0)
            .chain(relations.iter().skip(1).copied())
            .chain(std::iter::once(pieces.len()))
            .collect::<Vec<_>>();
        for segment in 0..bounds.len() - 1 {
            let start = bounds[segment];
            let end = bounds[segment + 1];
            let crosses_multiline_atom = pieces[..start]
                .iter()
                .any(|piece| piece.multiline_tail_width.is_some());
            let indent = if segment == 0 {
                0
            } else if !first_assignment || pieces[start].assignment || crosses_multiline_atom {
                relation_column
            } else {
                rhs_start
            };
            lines.extend(layout_display_segment(
                &pieces[start..end],
                indent,
                line_width,
                spacing,
            ));
        }
    }

    lines_document(lines)
}

fn layout_display_segment(
    pieces: &[Piece],
    base_indent: usize,
    line_width: usize,
    spacing: Spacing,
) -> Vec<(usize, Ir)> {
    let flat = document_from_pieces(pieces, spacing);
    if flat
        .flat_width()
        .is_some_and(|width| base_indent.saturating_add(width) <= line_width)
    {
        return vec![(base_indent, flat)];
    }
    if !pieces
        .iter()
        .any(|piece| piece.multiline_tail_width.is_some())
        && flat.flat_width().is_none()
    {
        return vec![(base_indent, flat)];
    }

    let operators = top_level_operators(pieces);
    let binaries = operators
        .iter()
        .filter_map(|&(index, role)| (role == Role::Binary).then_some(index))
        .collect::<Vec<_>>();
    let Some(&first_binary) = binaries.first() else {
        return vec![(base_indent, flat)];
    };
    let Some(columns) = piece_columns(pieces, spacing) else {
        return vec![(base_indent, flat)];
    };
    let rhs_offset = operators
        .iter()
        .find(|&&(index, role)| role == Role::Relation && index < first_binary)
        .and_then(|&(index, _)| columns.get(index))
        .map_or(0, |&(_, end)| end.saturating_add(1));

    let mut lines = Vec::new();
    let head = document_from_pieces(&pieces[..first_binary], spacing);
    if !matches!(head, Ir::Nil) {
        lines.push((base_indent, head));
    }
    for (position, &start) in binaries.iter().enumerate() {
        let end = binaries.get(position + 1).copied().unwrap_or(pieces.len());
        lines.push((
            base_indent.saturating_add(rhs_offset),
            document_from_pieces(&pieces[start..end], spacing),
        ));
    }
    lines
}

fn lines_document(mut lines: Vec<(usize, Ir)>) -> Ir {
    let Some((_, first)) = lines.first().cloned() else {
        return Ir::Nil;
    };
    let mut documents = vec![first];
    for (indent, document) in lines.drain(1..) {
        documents.push(Ir::align(indent, Ir::concat([Ir::HardLine, document])));
    }
    Ir::concat(documents)
}

fn relation_layout(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<(Option<RelationLayout>, bool)> {
    let pieces = lower_pieces_with_atoms(
        elements,
        semantic_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    let base_gaps = (0..pieces.len())
        .map(|index| gap_before(&pieces, index, spacing))
        .collect::<Vec<_>>();
    let spaced_slashes = (0..pieces.len())
        .map(|index| {
            pieces[index].slash
                && (pieces[index].authored_space_before
                    || pieces
                        .get(index + 1)
                        .is_some_and(|piece| piece.authored_space_before)
                    || adjacent_operator(&pieces, index, spacing))
        })
        .collect::<Vec<_>>();

    let starts_with_relation = pieces
        .first()
        .is_some_and(|piece| piece.role == Role::Relation);
    let mut column = 0;
    let mut delimiter_depth = 0usize;
    for (index, piece) in pieces.iter().enumerate() {
        if index > 0 && (base_gaps[index] || spaced_slashes[index - 1] || spaced_slashes[index]) {
            column += 1;
        }
        let width = piece.document.flat_width()?;
        if delimiter_depth == 0 && piece.role == Role::Relation {
            return Some((
                Some(RelationLayout {
                    column,
                    rhs_start: column + width + 1,
                    assignment: piece.assignment,
                    definition: piece.definition,
                }),
                starts_with_relation,
            ));
        }
        column += width;
        match piece.delimiter {
            Some(DelimiterRole::Open) => delimiter_depth += 1,
            Some(DelimiterRole::Close) => delimiter_depth = delimiter_depth.saturating_sub(1),
            Some(DelimiterRole::Fence) | None => {}
        }
    }
    Some((None, starts_with_relation))
}

fn relation_is_assignment(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> bool {
    elements
        .iter()
        .find(|element| {
            element.text_range().start() <= atom.range.start()
                && element.text_range().end() >= atom.range.end()
        })
        .is_some_and(assignment_element)
}

fn is_definition_relation(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> bool {
    elements.iter().any(|element| match element {
        SyntaxElement::Token(token)
            if token.kind() == SyntaxKind::MATH_WORD
                && token.text_range().start() <= atom.range.start()
                && token.text_range().end() >= atom.range.end() =>
        {
            token_slice(atom.range, token)
                .is_some_and(|text| text.starts_with(':') && text.ends_with('='))
        }
        _ => false,
    }) || definition_relation_parts(atom, elements).is_some()
}

fn assignment_element(element: &SyntaxElement) -> bool {
    match element {
        // Badness aligns definition relations with the equality/comparison
        // chain they introduce. Only assignment-arrow commands use the
        // right-hand-side continuation anchor in typed layout.
        SyntaxElement::Token(_) => false,
        SyntaxElement::Node(node) => {
            if let Some(command) = MathCommand::cast(node.clone()) {
                return command.name_token().is_some_and(|name| {
                    matches!(
                        name.text().strip_prefix('\\').unwrap_or(name.text()),
                        "gets" | "leftarrow" | "mapsto" | "coloneqq"
                    )
                });
            }
            MathScripted::cast(node.clone())
                .and_then(|scripted| scripted.base())
                .is_some_and(|base| assignment_element(&base))
        }
    }
}

fn has_unsupported_scripted_composite_relation(elements: &[SyntaxElement]) -> bool {
    // The legacy renderer fuses an adjacent relation head with the scalar that
    // owns the script (`x<` + `=_i`). Definition relations are supported by
    // typed lowering; keep the other seams on the fallback because the pinned
    // Badness formatter still splits them.
    elements.windows(2).any(|pair| {
        let [SyntaxElement::Token(head), SyntaxElement::Node(node)] = pair else {
            return false;
        };
        if head.kind() != SyntaxKind::MATH_WORD
            || head.text_range().end() != node.text_range().start()
        {
            return false;
        }
        let Some(base) = MathScripted::cast(node.clone())
            .and_then(|scripted| scripted.base())
            .and_then(SyntaxElement::into_token)
            .filter(|base| base.kind() == SyntaxKind::MATH_WORD)
        else {
            return false;
        };
        let (Some(head), Some(base)) = (head.text().chars().last(), base.text().chars().next())
        else {
            return false;
        };

        match head {
            ':' => base == ':',
            '=' | '<' | '>' => matches!(base, '=' | '<' | '>'),
            _ => false,
        }
    })
}

fn is_supported_element(element: &SyntaxElement, scope: &SignatureScope) -> bool {
    match element {
        SyntaxElement::Token(token) => matches!(
            token.kind(),
            SyntaxKind::MATH_WORD | SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
        ),
        SyntaxElement::Node(node) => {
            MathGroup::cast(node.clone()).is_some_and(|group| {
                group.is_closed()
                    && group.body_elements().all(|element| {
                        // A comment is not a semantic atom; `lower_body` decides
                        // whether this body's comments are safe to break at.
                        element.kind() == SyntaxKind::MATH_COMMENT
                            || is_supported_element(&element, scope)
                    })
            }) || MathCommand::cast(node.clone())
                .is_some_and(|command| command_is_supported(&command, scope))
                || MathDelimited::cast(node.clone())
                    .is_some_and(|delimited| delimited_is_supported(&delimited, scope))
                || MathScripted::cast(node.clone())
                    .is_some_and(|scripted| scripted_is_supported(&scripted, scope))
                || MathLineBreak::cast(node.clone()).is_some_and(|line_break| {
                    line_break.marker_token().as_ref().map(SyntaxToken::text) == Some(r"\\")
                        && line_break
                            .modifier()
                            .is_none_or(|modifier| modifier.is_closed())
                })
        }
    }
}

fn atom_document(
    atom: SemanticMathAtom,
    elements: &[SyntaxElement],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<AtomDocument> {
    let range = atom.range;
    let element = elements.iter().find(|element| {
        element.text_range().start() <= range.start() && element.text_range().end() >= range.end()
    })?;
    let (document, slash) = match element {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD => {
            let text = token_slice(range, token)?;
            let slash = text == "/";
            (Ir::verbatim(text), slash)
        }
        SyntaxElement::Node(node) => {
            if let Some(group) = MathGroup::cast(node.clone()) {
                let open = group.open_token()?;
                let close = group.close_token()?;
                let body = lower_body(
                    group.body_elements().collect(),
                    scope,
                    spacing,
                    preserve_comment_context,
                    false,
                )?;
                (
                    Ir::concat([
                        Ir::verbatim(open.text()),
                        hanging(1, body),
                        Ir::verbatim(close.text()),
                    ]),
                    false,
                )
            } else if let Some(command) = MathCommand::cast(node.clone()) {
                (
                    lower_command(
                        &command,
                        scope,
                        spacing,
                        preserve_comment_context,
                        environment_rows,
                    )?,
                    false,
                )
            } else if let Some(delimited) = MathDelimited::cast(node.clone()) {
                (
                    lower_delimited(
                        &delimited,
                        scope,
                        spacing,
                        preserve_comment_context,
                        environment_rows,
                    )?,
                    false,
                )
            } else {
                let scripted = MathScripted::cast(node.clone())?;
                (
                    lower_scripted(
                        &scripted,
                        scope,
                        spacing,
                        preserve_comment_context,
                        environment_rows,
                    )?,
                    false,
                )
            }
        }
        _ => return None,
    };
    let raw_class = math_atoms(element)
        .find(|raw| raw.range.start() == range.start())
        .map(|raw| raw.class)?;
    Some(AtomDocument {
        document,
        slash,
        control_word_operator: element_ends_control_word(element)
            && matches!(raw_class, MathClass::Bin | MathClass::Rel),
        starts_control_word_letter: element_starts_control_word_letter(element),
        ends_control_word: element_ends_control_word(element),
    })
}

fn definition_relation_document(
    atom: SemanticMathAtom,
    elements: &[SyntaxElement],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<AtomDocument> {
    let (prefix, scripted) = definition_relation_parts(atom, elements)?;
    let scripted = lower_scripted(
        &scripted,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    Some(AtomDocument {
        document: Ir::concat([Ir::verbatim(prefix), scripted]),
        slash: false,
        control_word_operator: false,
        starts_control_word_letter: false,
        ends_control_word: false,
    })
}

fn definition_relation_parts(
    atom: SemanticMathAtom,
    elements: &[SyntaxElement],
) -> Option<(String, MathScripted)> {
    elements.windows(2).find_map(|pair| {
        let [SyntaxElement::Token(head), SyntaxElement::Node(node)] = pair else {
            return None;
        };
        if head.kind() != SyntaxKind::MATH_WORD
            || head.text_range().start() > atom.range.start()
            || head.text_range().end() != node.text_range().start()
            || node.text_range().end() != atom.range.end()
        {
            return None;
        }
        let scripted = MathScripted::cast(node.clone())?;
        let base = scripted.base()?.into_token()?;
        if base.kind() != SyntaxKind::MATH_WORD || !base.text().starts_with('=') {
            return None;
        }
        let prefix_range = TextRange::new(atom.range.start(), head.text_range().end());
        let prefix = token_slice(prefix_range, head)?;
        (!prefix.is_empty() && prefix.chars().all(|character| character == ':'))
            .then_some((prefix, scripted))
    })
}

fn lower_delimited(
    delimited: &MathDelimited,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    _environment_rows: bool,
) -> Option<Ir> {
    if !delimited_is_supported(delimited, scope) {
        return None;
    }

    let left = delimited.left_token()?;
    let open = delimited.opening_delimiter()?;
    let body = delimited.body()?;
    let right = delimited.right_token()?;
    let close = delimited.closing_delimiter()?;
    let mut documents = vec![Ir::verbatim(left.text()), Ir::verbatim(open.text())];
    if !body.text().trim().is_empty() {
        let inner = lower_body(
            body.elements().collect(),
            scope,
            spacing,
            preserve_comment_context,
            false,
        )?;
        let opening_width = left.text().chars().count() + open.text().chars().count();
        documents.push(Ir::align(
            opening_width + 1,
            Ir::concat([Ir::text(" "), inner, Ir::text(" ")]),
        ));
    }
    documents.extend([Ir::verbatim(right.text()), Ir::verbatim(close.text())]);
    Some(Ir::concat(documents))
}

fn delimited_is_supported(delimited: &MathDelimited, scope: &SignatureScope) -> bool {
    let (Some(left), Some(open), Some(body), Some(right), Some(close)) = (
        delimited.left_token(),
        delimited.opening_delimiter(),
        delimited.body(),
        delimited.right_token(),
        delimited.closing_delimiter(),
    ) else {
        return false;
    };
    let structural_ranges = [
        left.text_range(),
        open.text_range(),
        body.syntax().text_range(),
        right.text_range(),
        close.text_range(),
    ];
    delimited.syntax().children_with_tokens().all(|element| {
        structural_ranges.contains(&element.text_range()) || is_layout_trivia(&element)
    }) && body.elements().all(|element| {
        // The body's own comments break through `lower_body`; a comment outside
        // it, such as one between `\left` and its delimiter, stays unsupported.
        element.kind() == SyntaxKind::MATH_COMMENT || is_supported_element(&element, scope)
    })
}

fn lower_scripted(
    scripted: &MathScripted,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let base = scripted.base()?;
    if scripted.scripts().next().is_none() || !scripted_is_supported(scripted, scope) {
        return None;
    }

    Some(Ir::concat([
        lower_elements(
            vec![base],
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?,
        lower_script_suffix(scripted, scope, preserve_comment_context, environment_rows)?,
    ]))
}

fn lower_script_suffix(
    scripted: &MathScripted,
    scope: &SignatureScope,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let mut documents = Vec::new();
    for script in scripted.scripts() {
        if !script_is_supported(&script, scope) {
            return None;
        }
        let marker = script.marker_token()?;
        let argument = script.argument()?;
        documents.push(Ir::verbatim(marker.text()));
        documents.push(lower_elements(
            vec![argument],
            scope,
            Spacing::Script,
            preserve_comment_context,
            environment_rows,
        )?);
    }
    (!documents.is_empty()).then(|| Ir::concat(documents))
}

fn scripted_is_supported(scripted: &MathScripted, scope: &SignatureScope) -> bool {
    let Some(base) = scripted.base() else {
        return false;
    };
    if !is_supported_element(&base, scope) {
        return false;
    }

    let base_range = base.text_range();
    scripted.syntax().children_with_tokens().all(|element| {
        element.text_range() == base_range
            || is_layout_trivia(&element)
            || element
                .into_node()
                .and_then(MathScript::cast)
                .is_some_and(|script| script_is_supported(&script, scope))
    })
}

fn script_is_supported(script: &MathScript, scope: &SignatureScope) -> bool {
    let (Some(marker), Some(argument)) = (script.marker_token(), script.argument()) else {
        return false;
    };
    if !is_supported_element(&argument, scope) {
        return false;
    }

    let argument_range = argument.text_range();
    script.syntax().children_with_tokens().all(|element| {
        element.text_range() == marker.text_range()
            || element.text_range() == argument_range
            || is_layout_trivia(&element)
    })
}

fn is_layout_trivia(element: &SyntaxElement) -> bool {
    matches!(
        element.kind(),
        SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
    )
}

fn lower_command(
    command: &MathCommand,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    _environment_rows: bool,
) -> Option<Ir> {
    let name = command.name_token()?;
    if is_supported_bare_command(command, scope) {
        return Some(Ir::verbatim(name.text()));
    }

    let arguments = matched_math_arguments(command, scope)?;
    let mut previous_end = name.text_range().end();
    let mut documents = vec![Ir::verbatim(name.text())];
    if let Some(star) = command.star_token() {
        documents.push(Ir::verbatim(star.text()));
        previous_end = star.text_range().end();
    }
    for argument in arguments {
        let open = argument.open_token()?;
        let close = argument.close_token()?;
        let elements = argument.body_elements().collect::<Vec<_>>();
        let body = hanging(
            1,
            lower_body(elements, scope, spacing, preserve_comment_context, false)?,
        );
        if previous_end < argument.syntax().text_range().start() {
            documents.push(Ir::text(" "));
        }
        documents.extend([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]);
        previous_end = argument.syntax().text_range().end();
    }
    Some(Ir::concat(documents))
}

fn command_is_supported(command: &MathCommand, scope: &SignatureScope) -> bool {
    is_supported_bare_command(command, scope) || matched_math_arguments(command, scope).is_some()
}

fn is_supported_bare_command(command: &MathCommand, scope: &SignatureScope) -> bool {
    let Some(name) = command.name() else {
        return false;
    };
    if matches!(name.as_str(), "left" | "right")
        || scope.is_redefined(&name)
        || command
            .syntax()
            .children_with_tokens()
            .any(|element| element.kind() != SyntaxKind::MATH_CONTROL_WORD)
    {
        return false;
    }

    scope.command_signature(&name).is_none_or(|signature| {
        signature
            .arguments
            .iter()
            .all(|argument| !argument.required)
    })
}

fn matched_math_arguments(
    command: &MathCommand,
    scope: &SignatureScope,
) -> Option<Vec<MathArgument>> {
    if !command
        .syntax()
        .children_with_tokens()
        .all(|element| match element {
            SyntaxElement::Token(token) => {
                matches!(
                    token.kind(),
                    SyntaxKind::MATH_CONTROL_WORD
                        | SyntaxKind::MATH_SPACE
                        | SyntaxKind::MATH_NEWLINE
                ) || token.kind() == SyntaxKind::MATH_WORD && token.text() == "*"
            }
            SyntaxElement::Node(node) => MathArgument::cast(node).is_some(),
        })
    {
        return None;
    }
    let signature = scope.command_signature(&command.name()?)?;
    let arguments = command.attached_arguments().collect::<Vec<_>>();
    let mut slot = 0;
    let mut matched = Vec::with_capacity(arguments.len());

    for argument in arguments {
        if !argument.is_closed() {
            return None;
        }
        let kind = match argument {
            MathArgument::Brace(_) => ArgKind::Brace,
            MathArgument::Bracket(_) => ArgKind::Bracket,
        };
        let spec = match_arg_slot(&signature.arguments, &mut slot, kind)?;
        if spec.domain != ArgumentDomain::Math {
            return None;
        }
        matched.push(argument);
    }

    if signature.arguments[slot..]
        .iter()
        .any(|argument| argument.required)
    {
        return None;
    }
    Some(matched)
}

struct Piece {
    role: Role,
    delimiter: Option<DelimiterRole>,
    assignment: bool,
    definition: bool,
    /// A `+`/`-` that TeX coerced to a unary sign. It binds to the operand
    /// beside it, so it strips the authored space on either side.
    unary: bool,
    authored_space_before: bool,
    slash: bool,
    control_word_operator: bool,
    starts_control_word_letter: bool,
    ends_control_word: bool,
    /// Width of the atom's final forced line, measured from its own start.
    multiline_tail_width: Option<usize>,
    document: Ir,
}

struct AtomDocument {
    document: Ir,
    slash: bool,
    control_word_operator: bool,
    starts_control_word_letter: bool,
    ends_control_word: bool,
}

fn gap_before(pieces: &[Piece], index: usize, spacing: Spacing) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| pieces.get(index)) else {
        return false;
    };
    let current = &pieces[index];
    if previous.ends_control_word && current.starts_control_word_letter {
        return true;
    }

    // A binary operator or relation always wins its space, even next to a
    // unary sign (`a - -b`); otherwise a unary sign strips the authored space
    // it would have kept as an ordinary atom (`f( - x)` -> `f(-x)`).
    let tight = previous.unary || current.unary;

    match spacing {
        Spacing::Normal => {
            if current.role != Role::Operand || previous.role != Role::Operand {
                true
            } else {
                !tight && current.authored_space_before
            }
        }
        Spacing::Script => {
            current.control_word_operator
                || previous.control_word_operator
                || current.role == Role::Operand
                    && previous.role == Role::Operand
                    && !tight
                    && current.authored_space_before
                    && !touches_delimiter(previous, current)
        }
    }
}

fn adjacent_operator(pieces: &[Piece], index: usize, spacing: Spacing) -> bool {
    let previous = index.checked_sub(1).and_then(|index| pieces.get(index));
    let next = pieces.get(index + 1);
    match spacing {
        Spacing::Normal => previous
            .into_iter()
            .chain(next)
            .any(|piece| piece.role != Role::Operand),
        Spacing::Script => previous
            .into_iter()
            .chain(next)
            .any(|piece| piece.control_word_operator),
    }
}

fn touches_delimiter(previous: &Piece, current: &Piece) -> bool {
    [previous.delimiter, current.delimiter]
        .into_iter()
        .any(|role| matches!(role, Some(DelimiterRole::Open | DelimiterRole::Close)))
}

fn element_starts_control_word_letter(element: &SyntaxElement) -> bool {
    element_boundary_token(element, true)
        .and_then(|token| token.text().chars().next())
        .is_some_and(is_control_word_letter)
}

fn element_ends_control_word(element: &SyntaxElement) -> bool {
    element_boundary_token(element, false)
        .is_some_and(|token| token.kind() == SyntaxKind::MATH_CONTROL_WORD)
}

fn element_boundary_token(element: &SyntaxElement, first: bool) -> Option<SyntaxToken> {
    match element {
        SyntaxElement::Token(token) => Some(token.clone()),
        SyntaxElement::Node(node) => {
            let mut tokens = node
                .descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| {
                    !matches!(
                        token.kind(),
                        SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
                    )
                });
            if first { tokens.next() } else { tokens.last() }
        }
    }
}

/// Whether `character` could extend a preceding control word, forcing a space
/// between them. The parser's control-word alphabet is `[A-Za-z@]`; non-ASCII
/// letters stay included here because gluing a command to a following Greek
/// letter reads as one word even though the parser would still split it.
/// Catcode-12 characters such as `:` and `_` never extend a control word.
fn is_control_word_letter(character: char) -> bool {
    character.is_alphabetic() || character == '@'
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Spacing {
    Normal,
    Script,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Operand,
    Binary,
    Relation,
}

impl From<MathBreakPriority> for Role {
    fn from(priority: MathBreakPriority) -> Self {
        match priority {
            MathBreakPriority::None => Self::Operand,
            MathBreakPriority::Binary => Self::Binary,
            MathBreakPriority::Relation => Self::Relation,
        }
    }
}

fn token_slice(range: TextRange, word: &SyntaxToken) -> Option<String> {
    let word_range = word.text_range();
    if range.start() < word_range.start() || range.end() > word_range.end() {
        return None;
    }
    let start = usize::from(range.start() - word_range.start());
    let end = usize::from(range.end() - word_range.start());
    Some(word.text()[start..end].to_owned())
}

#[cfg(test)]
mod tests {
    use panache_parser::parser::math::{MathParseOptions, parse_math_content};
    use panache_parser::parser::parse;
    use panache_parser::semantic::math::SignatureScope;
    use rowan::ast::AstNode;

    use super::*;
    use crate::formatter::math::printer::Printer;
    use crate::syntax::SyntaxNode;

    fn content(input: &str) -> MathContent {
        MathContent::cast(SyntaxNode::new_root(parse_math_content(
            input,
            MathParseOptions::default(),
        )))
        .expect("math content root")
    }

    /// Print with forced breaks intact — these tests assert the lowered layout,
    /// including the hard lines that comments and `\\` rows introduce. Inline
    /// math flattens them instead; see `Printer::print_flat`.
    fn lower(input: &str) -> Option<String> {
        try_lower_content(&content(input), &SignatureScope::default())
            .map(|document| Printer::new(80, 2).print(&document, 0))
    }

    fn lower_display(input: &str) -> Option<String> {
        lower_display_width(input, 80)
    }

    fn lower_display_width(input: &str, line_width: usize) -> Option<String> {
        try_lower_display_content(&content(input), &SignatureScope::default(), line_width)
            .map(|document| Printer::new(line_width, 2).print(&document, 0))
    }

    #[test]
    fn lowers_flat_words_and_trivia() {
        let cases = [
            ("  a  b\nc  ", "a b c"),
            ("α+β", "α + β"),
            ("x=-y", "x = -y"),
            ("a--b", "a - -b"),
            ("- x", "-x"),
            ("a<=b", "a <= b"),
            ("a/ b", "a / b"),
            ("a /b", "a / b"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_definition_relations_through_the_semantic_stream() {
        let cases = [
            ("x:=y", "x := y"),
            ("a::=b", "a ::= b"),
            ("a:=_ib", "a :=_i b"),
            ("a::=_ib", "a ::=_i b"),
            (r"\mu:=\nu", r"\mu := \nu"),
            (r"x\coloneqq y", r"x \coloneqq y"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
        assert_eq!(lower_display("x:=y").as_deref(), Some("x := y"));
        assert_eq!(
            lower_display_width("A := bbbbbbbbbb = cccccccccc", 20).as_deref(),
            Some("A := bbbbbbbbbb\n  = cccccccccc"),
        );
        let authored = concat!(r"A :=_i a \\", "\n", r":=_j b \\", "\n", "= c");
        assert_eq!(lower_display(authored).as_deref(), Some(authored));
    }

    #[test]
    fn lowers_ordinary_groups_recursively() {
        let cases = [
            ("{ a+b }", "{a + b}"),
            ("a+{b-c}", "a + {b - c}"),
            ("a {- b}", "a {-b}"),
            ("{{ α<=β }}", "{{α <= β}}"),
            ("{   }", "{}"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_signature_proven_math_arguments_and_command_shells() {
        let cases = [
            (r"\frac{ a+b }{ c-d }", r"\frac{a + b}{c - d}"),
            (r"\sqrt{ a+b }", r"\sqrt{a + b}"),
            (r"\sqrt[ a+b ]{ c-d }", r"\sqrt[a + b]{c - d}"),
            (r"\frac { a+b } { c-d }", r"\frac {a + b} {c - d}"),
            (r"x+\frac{{ a+b }}{c}", r"x + \frac{{a + b}}{c}"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_signature_proven_math_arguments() {
        let cases = [
            (
                "\\frac{% numerator\n a+b}{c}",
                "\\frac{% numerator\n a + b}{c}",
            ),
            (
                "\\frac{a+b % numerator\n}{c}",
                "\\frac{a + b % numerator\n }{c}",
            ),
            ("\\sqrt[% index\n n+1]{x}", "\\sqrt[% index\n n + 1]{x}"),
            (
                "\\frac{a % keep this comment\n+b}{c}",
                "\\frac{a % keep this comment\n + b}{c}",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_bare_commands_through_the_semantic_stream() {
        let cases = [
            (r"\alpha+\beta", r"\alpha + \beta"),
            (r"a\cdot b", r"a \cdot b"),
            (r"x\leq-y", r"x \leq -y"),
            (r"\sin x", r"\sin x"),
            (r"\unknown x", r"\unknown x"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_supported_scripts_through_the_semantic_stream() {
        let cases = [
            ("x^2", "x^2"),
            ("x _ i ^ { a+b }", "x_i^{a+b}"),
            (r"\alpha_i+\beta^2", r"\alpha_i + \beta^2"),
            (r"\frac{ a+b }{c}^2", r"\frac{a + b}{c}^2"),
            ("{ a+b }^2", "{a + b}^2"),
            ("e^{x_i^2}", "e^{x_i^2}"),
            (r"\sum_{i=1}^{n} i", r"\sum_{i=1}^{n} i"),
            (r"x^\alpha+y_\beta", r"x^\alpha + y_\beta"),
            (r"x^{a\in A}", r"x^{a \in A}"),
            (r"x^{\alpha b}", r"x^{\alpha b}"),
            ("x^{( a )}", "x^{(a)}"),
            ("x^{a/ b}", "x^{a / b}"),
            (r"x^{\frac{a+b}{c-d}}", r"x^{\frac{a+b}{c-d}}"),
            (r"a\leq_i-b", r"a \leq_i -b"),
            (r"e^{- t}", r"e^{-t}"),
            (r"a: =_ib", r"a: =_i b"),
            (r"x< =_iy", r"x < =_i y"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_closed_paired_delimiters_with_supported_plain_bodies() {
        let cases = [
            (r"\left (  a+b  \right )", r"\left( a + b \right)"),
            (
                r"x+\left[ \frac{ a+b }{c} \right]",
                r"x + \left[ \frac{a + b}{c} \right]",
            ),
            (r"\left.   \alpha   \right|", r"\left. \alpha \right|"),
            (
                r"\left\langle x \right\rangle",
                r"\left\langle x \right\rangle",
            ),
            (r"\left(   \right)", r"\left(\right)"),
            (r"\left x \right)", r"\leftx\right)"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_supported_scripts_inside_paired_delimiters() {
        let cases = [
            (
                r"\left( x _ i + y ^ { a+b } \right)",
                r"\left( x_i + y^{a+b} \right)",
            ),
            (
                r"\left[ \frac{ a+b }{c}^2 \right]",
                r"\left[ \frac{a + b}{c}^2 \right]",
            ),
            (r"x ^ { \left( a+b \right) }", r"x^{\left( a+b \right)}"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_scripted_paired_delimiter_bases() {
        let cases = [
            (r"\left( x \right) ^ 2", r"\left( x \right)^2"),
            (
                r"a + \left[ b+c \right] _ { i+j }",
                r"a + \left[ b + c \right]_{i+j}",
            ),
            (r"\left. x_i \right| _ 0 ^ 1", r"\left. x_i \right|_0^1"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_nested_paired_delimiters_recursively() {
        let cases = [
            (
                r"\left[ \left( a+b \right) + c \right]",
                r"\left[ \left( a + b \right) + c \right]",
            ),
            (
                r"x ^ { \left[ \left( a+b \right) \right] }",
                r"x^{\left[ \left( a+b \right) \right]}",
            ),
            (
                r"\left( \left[ x \right] ^ 2 \right) _ i",
                r"\left( \left[ x \right]^2 \right)_i",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_leading_and_trailing_top_level_comments() {
        let cases = [
            ("% leading comment\nx = 1\n", "% leading comment\nx = 1"),
            ("% base comment\nx^2", "% base comment\nx^2"),
            ("a + b % this is a comment\n", "a + b % this is a comment\n"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_ordinary_groups() {
        let cases = [
            ("{a+b % inner\n}", "{a + b % inner\n }"),
            ("{% inner\n a+b}", "{% inner\n a + b}"),
            ("{a % inner\n+b}", "{a % inner\n + b}"),
            ("{ % only\n }", "{% only\n }"),
            ("{{a % inner\n}}", "{{a % inner\n  }}"),
            ("{a+b % inner\n} + c", "{a + b % inner\n } + c"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_bracketed_bodies_one_column_per_level() {
        let cases = [
            ("\\frac{{a % inner\n}}{c}", "\\frac{{a % inner\n  }}{c}"),
            ("\\sqrt[{a % inner\n}]{b}", "\\sqrt[{a % inner\n  }]{b}"),
            ("{\\frac{a % inner\n}{b}}", "{\\frac{a % inner\n  }{b}}"),
            (
                "\\left( {a % inner\n} \\right)",
                "\\left( {a % inner\n        } \\right)",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_paired_delimiter_bodies() {
        let cases = [
            (
                "\\left( a % inner\n+ b \\right)",
                "\\left( a % inner\n       + b \\right)",
            ),
            (
                "\\left( % lead\n a+b \\right)",
                "\\left( % lead\n       a + b \\right)",
            ),
            (
                "\\left(a % inner\n\\right)",
                "\\left( a % inner\n        \\right)",
            ),
            (
                "\\left( % only\n \\right)",
                "\\left( % only\n        \\right)",
            ),
            (
                "\\left\\langle a % inner\n+b \\right\\rangle",
                "\\left\\langle a % inner\n             + b \\right\\rangle",
            ),
            (
                "\\left( \\left[ a % inner\n \\right] \\right)",
                "\\left( \\left[ a % inner\n               \\right] \\right)",
            ),
            (
                "\\left( a % inner\n \\right)^2",
                "\\left( a % inner\n        \\right)^2",
            ),
            (
                "\\frac{\\left( a % inner\n \\right)}{b}",
                "\\frac{\\left( a % inner\n         \\right)}{b}",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_supported_script_arguments() {
        let cases = [
            ("x^{a % inner\n+b}", "x^{a % inner\n +b}"),
            ("x^{% inner\n a}", "x^{% inner\n a}"),
            ("x^{a+b % inner\n}", "x^{a+b % inner\n }"),
            ("x_{a % inner\n}^2", "x_{a % inner\n }^2"),
            ("x^{{a % inner\n}}", "x^{{a % inner\n  }}"),
            (
                "x^{a % inner\n}_{b % other\n}",
                "x^{a % inner\n }_{b % other\n }",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn carries_operator_context_across_mid_expression_comments() {
        let cases = [
            (
                "a% operand before comment\n+b",
                "a% operand before comment\n+ b",
            ),
            (
                "a+% binary before comment\n-b",
                "a +% binary before comment\n-b",
            ),
            (
                "a=% relation before comment\n-b",
                "a =% relation before comment\n-b",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_authored_line_breaks() {
        let cases = [
            ("a\\\\b", "a\\\\\nb"),
            ("a \\\\*[2ex]\n-b", "a \\\\*[2ex]\n- b"),
            ("a+b\\\\c-d", "a + b\\\\\nc - d"),
            ("{a+b\\\\c-d}", "{a + b\\\\\n c - d}"),
            ("\\frac{a+b\\\\c-d}{e}", "\\frac{a + b\\\\\n c - d}{e}"),
            (
                "\\left( a+b\\\\c-d \\right)",
                "\\left( a + b\\\\\n       c - d \\right)",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_comments_after_authored_line_breaks() {
        let input = "a \\\\ % first row\nb";
        assert_eq!(lower(input).as_deref(), Some("a \\\\\n% first row\nb"));
    }

    #[test]
    fn authored_line_break_lowering_is_idempotent() {
        for input in [
            "a\\\\b",
            "a \\\\*[2ex]\n-b",
            "a+b\\\\c-d",
            "{a+b\\\\c-d}",
            "\\frac{a+b\\\\c-d}{e}",
            "\\left( a+b\\\\c-d \\right)",
            "a \\\\ % first row\nb",
        ] {
            let once = lower(input).expect("supported authored line break");
            let twice = lower(&once).expect("formatted authored line break");
            assert_eq!(once, twice, "not idempotent: {input:?}");
        }
    }

    #[test]
    fn aligns_supported_relation_chains_across_authored_breaks() {
        let cases = [
            ("x = a \\\\\n= b \\\\\n= c", "x = a \\\\\n  = b \\\\\n  = c"),
            (
                "x \\gets a \\\\\n= b \\\\\n= c",
                "x \\gets a \\\\\n        = b \\\\\n        = c",
            ),
            ("x =_i a \\\\\n=_j b", "x =_i a \\\\\n  =_j b"),
            (
                "x \\gets a \\\\\n\\leftarrow b \\\\\n= c",
                "x \\gets a \\\\\n  \\leftarrow b \\\\\n        = c",
            ),
            ("x \\\\\n= b", "x \\\\\n  = b"),
        ];

        for (input, expected) in cases {
            let once = lower_display(input).expect("supported authored relation chain");
            assert_eq!(once, expected, "{input:?}");
            assert_eq!(lower_display(&once).as_deref(), Some(once.as_str()));
        }

        assert_eq!(
            lower_display("{x = a \\\\\n= b}").as_deref(),
            Some("{x = a \\\\\n = b}"),
            "nested authored rows must not acquire top-level relation alignment",
        );
    }

    #[test]
    fn lowers_width_driven_relation_and_binary_breaks() {
        let cases = [
            (
                "A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd",
                "A = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd",
            ),
            (
                concat!(r"x = a \\", "\n", "= bbbbbbbb + cccccccc + dddddddd"),
                concat!(
                    r"x = a \\",
                    "\n  = bbbbbbbb\n    + cccccccc\n    + dddddddd"
                ),
            ),
        ];

        for (input, expected) in cases {
            let once = lower_display_width(input, 20).expect("supported display wrapping");
            assert_eq!(once, expected, "{input:?}");
            assert_eq!(
                lower_display_width(&once, 20).as_deref(),
                Some(once.as_str()),
            );
        }
    }

    #[test]
    fn rejects_malformed_and_unsupported_scripts() {
        for input in [
            "x^",
            "x^% argument comment\n2",
            r"\text{ a+b }^2",
            r"x<=_iy",
            r"x>=_iy",
            r"a==_kb",
        ] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn defers_paired_delimiter_recovery_and_shell_comments() {
        for input in [
            r"\left( x",
            r"\left( x \right",
            "\\left % keep\n( x \\right)",
        ] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn rejects_redefined_and_incomplete_bare_commands() {
        for input in [r"\frac", r"\sqrt", r"\text"] {
            assert_eq!(lower(input), None, "{input:?}");
        }

        let document = parse("\\newcommand{\\leq}{x}\n\n$\\leq$\n", None);
        let scope = SignatureScope::from_root(&document);
        assert!(scope.is_redefined("leq"));
        assert!(try_lower_content(&content(r"\leq"), &scope).is_none());
    }

    #[test]
    fn rejects_commands_without_a_complete_math_signature_match() {
        for input in [
            r"\text{ a+b }",
            r"\unknown{ a+b }",
            r"\frac{a}",
            r"\frac{a}{b}{c}",
            r"\frac{a}{b",
            "\\frac% keep\n{a}{b}",
        ] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn rejects_every_unsupported_shape_category() {
        let cases = [
            "a+b\n% own line",
            r"a\\[1ex",
            "a&b",
            r"\begin{matrix}x\end{matrix}",
        ];

        for input in cases {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }
}
