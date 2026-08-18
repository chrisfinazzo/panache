//! YAML parser core: orchestrator + streaming-token-to-CST builder.
//!
//! Two layers live in this module:
//!
//! 1. **Orchestrator** — [`parse_yaml_tree`] and [`parse_yaml_report`].
//!    These drive [`parse_stream`] for a pure-YAML parse rooted at
//!    `YAML_STREAM`, run the structural
//!    [`super::validator::validate_yaml`] pass, and surface diagnostics.
//!    Host envelope wrappers (`DOCUMENT`, `YAML_METADATA_CONTENT`,
//!    `HASHPIPE_YAML_CONTENT`) are added by the host parser at embedding
//!    sites and are not concerns of the standalone YAML parse path.
//!
//! 2. **Streaming parser** — [`parse_stream`] drives
//!    [`super::scanner::Scanner`] and emits the rowan green tree. Each
//!    contiguous run of body content is wrapped in a `YAML_DOCUMENT`
//!    node (with `---` / `...` markers consumed inside the document
//!    they delimit); block-context content nests under `YAML_BLOCK_MAP`
//!    / `YAML_BLOCK_SEQUENCE` containers driven by the scanner's
//!    synthetic `BlockMappingStart` / `BlockSequenceStart` / `BlockEnd`
//!    markers; each key-value pair is wrapped in
//!    `YAML_BLOCK_MAP_ENTRY`, each `-` entry in
//!    `YAML_BLOCK_SEQUENCE_ITEM`, and each map entry splits into
//!    `YAML_BLOCK_MAP_KEY` (everything up to and including the `:`) and
//!    `YAML_BLOCK_MAP_VALUE` (everything after). Flow contexts mirror
//!    the same shape: `YAML_FLOW_MAP` / `YAML_FLOW_MAP_ENTRY` /
//!    `YAML_FLOW_MAP_KEY` / `YAML_FLOW_MAP_VALUE` and
//!    `YAML_FLOW_SEQUENCE` / `YAML_FLOW_SEQUENCE_ITEM`. Source-backed
//!    `[` / `]` / `{` / `}` / `,` are emitted at the container level
//!    (siblings of items), with item/entry sub-wrappers closing on `,`
//!    and the matching closer.

#![allow(dead_code)]

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::model::{YamlDiagnostic, YamlParseReport};
use super::profile::YamlValidationContext;
use super::scanner::{Scanner, TokenKind, TriviaKind};

fn strip_line_prefix(input: &str, prefix: &str) -> String {
    input
        .lines()
        .map(|line| match line.strip_prefix(prefix) {
            Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Structural validation for embedded (prefixed) YAML. Strips the
/// per-line `prefix` to the plain-YAML baseline and runs the standard
/// [`super::validator::validate_yaml`] pass. The verdict matches the
/// stripped baseline; diagnostic offsets refer to the stripped text
/// (host-offset remapping is a later concern). An empty `prefix` is
/// plain validation.
pub fn validate_yaml_with_prefix(input: &str, prefix: &str) -> Option<YamlDiagnostic> {
    if prefix.is_empty() {
        return super::validator::validate_yaml(input);
    }
    super::validator::validate_yaml(&strip_line_prefix(input, prefix))
}

/// Strip the per-line `prefix` exactly as [`strip_line_prefix`] does, but also
/// record — for every byte of the stripped output, plus a trailing EOF entry —
/// the byte offset it came from within `input`. Building the text and the map in
/// lockstep guarantees they cannot drift, so a diagnostic offset (in stripped
/// coordinates) can be located back in the original prefixed region. `prefix`
/// must be non-empty (empty-prefix validation never strips).
fn strip_line_prefix_with_offsets(input: &str, prefix: &str) -> (String, Vec<usize>) {
    let mut stripped = String::new();
    let mut offsets = Vec::new();
    let base = input.as_ptr() as usize;
    for (line_idx, line) in input.lines().enumerate() {
        let line_off = line.as_ptr() as usize - base;
        if line_idx > 0 {
            offsets.push(line_off.saturating_sub(1));
            stripped.push('\n');
        }
        let (payload, payload_off) = match line.strip_prefix(prefix) {
            Some(rest) => {
                let after = rest.strip_prefix(' ').unwrap_or(rest);
                (after, line.len() - after.len())
            }
            None => (line, 0),
        };
        offsets.extend((0..payload.len()).map(|i| line_off + payload_off + i));
        stripped.push_str(payload);
    }
    offsets.push(input.len());
    (stripped, offsets)
}

/// Locate a structural YAML diagnostic in `input` (raw, possibly `prefix`-marked),
/// returning the diagnostic plus the byte range **within `input`** it covers
/// (start, end). An empty `prefix` is plain YAML with identity offsets. Returns
/// `None` when `input` is valid — the verdict matches
/// [`validate_yaml_with_prefix`]. The host parser adds the region's document
/// start to emit a host-ranged `SyntaxError` for malformed embedded YAML.
pub fn locate_yaml_diagnostic(input: &str, prefix: &str) -> Option<(YamlDiagnostic, usize, usize)> {
    locate_yaml_diagnostic_ctx(input, prefix, YamlValidationContext::substrate())
}

/// Like [`locate_yaml_diagnostic`], but validates under `ctx` — the
/// (flavor, location) consumer profile for this YAML region. The host parser
/// builds `ctx` from the document flavor and whether the region is frontmatter
/// or a hashpipe `#|` body, so consumer-only rejections (implicit empty keys,
/// duplicate keys) surface for the parsers that actually read the YAML.
pub fn locate_yaml_diagnostic_ctx(
    input: &str,
    prefix: &str,
    ctx: YamlValidationContext,
) -> Option<(YamlDiagnostic, usize, usize)> {
    if prefix.is_empty() {
        let diag = super::validator::validate_yaml_with_context(input, ctx)?;
        let start = diag.byte_start.min(input.len());
        let end = diag.byte_end.min(input.len()).max(start);
        return Some((diag, start, end));
    }
    let diag =
        super::validator::validate_yaml_with_context(&strip_line_prefix(input, prefix), ctx)?;
    let (_stripped, offsets) = strip_line_prefix_with_offsets(input, prefix);
    let start = offsets.get(diag.byte_start).copied().unwrap_or(input.len());
    let end = offsets
        .get(diag.byte_end)
        .copied()
        .unwrap_or(input.len())
        .max(start);
    Some((diag, start, end))
}

/// Parse YAML tree structure from input, or `None` if it fails to parse.
pub fn parse_yaml_tree(input: &str) -> Option<SyntaxNode> {
    parse_yaml_report(input).tree
}

/// Parse YAML tree structure and include diagnostics on failure.
///
/// Diagnostics flow through the structural
/// [`super::validator::validate_yaml`] pass, which composes per-cluster
/// `check_*` functions covering directive ordering, structural shape
/// (unterminated flow, trailing content, invalid keys, indent
/// anomalies, block-scalar header, etc.), and lex-level checks like
/// `LEX_INVALID_DOUBLE_QUOTED_ESCAPE`.
///
/// The returned tree, when present, comes from the streaming scanner
/// and builder.
pub fn parse_yaml_report(input: &str) -> YamlParseReport {
    if let Some(err) = super::validator::validate_yaml(input) {
        return YamlParseReport {
            tree: None,
            diagnostics: vec![err],
        };
    }

    YamlParseReport {
        tree: Some(parse_stream(input)),
        diagnostics: Vec::new(),
    }
}

/// Drive the scanner over `input` and build a CST. Always returns a
/// `SyntaxNode` — the scanner is permissive and the builder preserves
/// bytes regardless of well-formedness.
pub fn parse_stream(input: &str) -> SyntaxNode {
    parse_stream_inner(input, None)
}

/// Like [`parse_stream`], but treats `prefix` (e.g. hashpipe `"#|"`) as
/// an embedded-YAML per-line marker: the scanner excludes it from
/// column/indent accounting and the builder peels it into
/// `YAML_LINE_PREFIX` leaves, so the resulting CST's token ranges are
/// host ranges directly (prefix bytes included as trivia, no offset
/// remapping). An empty `prefix` behaves like [`parse_stream`].
///
/// `prefix` may be a *composite* marker — a container prefix prepended to
/// `#|` (e.g. `"   #|"` for a list-indented cell, `"> #|"` for a blockquoted
/// one). Within a hashpipe preamble the container prefix is uniform per line,
/// so matching the whole composite marker via `strip_prefix` parses a nested
/// cell identically to a top-level one, peeling the entire prefix into one
/// `YAML_LINE_PREFIX` leaf. The host computes this marker (see
/// `parse_fenced_code_block`).
pub fn parse_stream_with_prefix(input: &str, prefix: &str) -> SyntaxNode {
    parse_stream_inner(input, (!prefix.is_empty()).then_some(prefix))
}

fn parse_stream_inner(input: &str, line_prefix: Option<&str>) -> SyntaxNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::YAML_STREAM.into());
    let mut scanner = match line_prefix {
        Some(prefix) => Scanner::with_prefix(input, prefix),
        None => Scanner::new(input),
    };
    let mut doc_open = false;
    let mut doc_only_has_directives = false;
    let mut block_stack: Vec<BlockFrame> = Vec::new();
    let mut prev_significant: Option<TokenKind> = None;
    let mut decoration_col_floor: Option<usize> = None;
    while let Some(tok) = scanner.next_token() {
        let last_significant = prev_significant;
        let decorations_so_far = decoration_col_floor;
        let is_decoration = matches!(
            tok.kind,
            TokenKind::Anchor | TokenKind::Tag | TokenKind::Alias
        );
        if !matches!(
            tok.kind,
            TokenKind::Trivia(_) | TokenKind::StreamStart | TokenKind::StreamEnd
        ) {
            if is_decoration {
                decoration_col_floor = Some(
                    decoration_col_floor.map_or(tok.start.column, |c| c.min(tok.start.column)),
                );
            } else {
                prev_significant = Some(tok.kind);
                decoration_col_floor = None;
            }
        }
        match tok.kind {
            TokenKind::StreamStart | TokenKind::StreamEnd => continue,
            TokenKind::BlockMappingStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                doc_only_has_directives = false;
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP.into());
                block_stack.push(BlockFrame::BlockMap {
                    entry_open: false,
                    in_value: false,
                });
                continue;
            }
            TokenKind::BlockSequenceStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                doc_only_has_directives = false;
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE.into());
                block_stack.push(BlockFrame::BlockSequence {
                    item_open: false,
                    indentless: false,
                });
                continue;
            }
            TokenKind::BlockEnd => {
                close_indentless_sequences(&mut builder, &mut block_stack);
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                if block_stack.pop().is_some() {
                    builder.finish_node();
                }
                continue;
            }
            TokenKind::FlowSequenceStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                doc_only_has_directives = false;
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE.into());
                block_stack.push(BlockFrame::FlowSequence { item_open: false });
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_FLOW_INDICATOR.into(), text);
                continue;
            }
            TokenKind::FlowSequenceEnd => {
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_FLOW_INDICATOR.into(), text);
                if matches!(
                    block_stack.last(),
                    Some(BlockFrame::FlowSequence { .. } | BlockFrame::FlowMap { .. })
                ) {
                    block_stack.pop();
                    builder.finish_node();
                }
                continue;
            }
            TokenKind::FlowMappingStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                doc_only_has_directives = false;
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_FLOW_MAP.into());
                block_stack.push(BlockFrame::FlowMap {
                    entry_open: false,
                    in_value: false,
                });
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_FLOW_INDICATOR.into(), text);
                continue;
            }
            TokenKind::FlowMappingEnd => {
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_FLOW_INDICATOR.into(), text);
                if matches!(
                    block_stack.last(),
                    Some(BlockFrame::FlowMap { .. } | BlockFrame::FlowSequence { .. })
                ) {
                    block_stack.pop();
                    builder.finish_node();
                }
                continue;
            }
            TokenKind::FlowEntry => {
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_FLOW_INDICATOR.into(), text);
                continue;
            }
            TokenKind::Key => {
                close_indentless_sequences(&mut builder, &mut block_stack);
                if matches!(
                    block_stack.last(),
                    Some(BlockFrame::BlockMap { .. } | BlockFrame::FlowMap { .. })
                ) {
                    open_map_entry_with_key(&mut builder, &mut block_stack);
                }
                if tok.start.index == tok.end.index {
                    continue;
                }
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
            }
            TokenKind::Value => {
                close_indentless_sequences(&mut builder, &mut block_stack);
                let map_state = match block_stack.last().copied() {
                    Some(BlockFrame::BlockMap {
                        entry_open,
                        in_value,
                    }) => Some((false, entry_open, in_value)),
                    Some(BlockFrame::FlowMap {
                        entry_open,
                        in_value,
                    }) => Some((true, entry_open, in_value)),
                    _ => None,
                };
                if let Some((is_flow, mut entry_open, mut in_value)) = map_state {
                    if !is_flow && entry_open && in_value {
                        close_open_sub_wrapper(&mut builder, &mut block_stack);
                        entry_open = false;
                        in_value = false;
                    }
                    if !entry_open {
                        open_map_entry_with_key(&mut builder, &mut block_stack);
                    }
                    if !in_value {
                        let text = &input[tok.start.index..tok.end.index];
                        if !text.is_empty() {
                            builder.token(SyntaxKind::YAML_COLON.into(), text);
                        }
                        builder.finish_node(); // close KEY
                        let value_kind = if is_flow {
                            SyntaxKind::YAML_FLOW_MAP_VALUE
                        } else {
                            SyntaxKind::YAML_BLOCK_MAP_VALUE
                        };
                        builder.start_node(value_kind.into());
                        if let Some(
                            BlockFrame::BlockMap { in_value, .. }
                            | BlockFrame::FlowMap { in_value, .. },
                        ) = block_stack.last_mut()
                        {
                            *in_value = true;
                        }
                        continue;
                    }
                }
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
            }
            TokenKind::BlockEntry => {
                let decorations_inside_value =
                    decorations_so_far.is_none_or(|c| c > tok.start.column);
                let indentless_value = last_significant == Some(TokenKind::Value)
                    && matches!(
                        block_stack.last(),
                        Some(BlockFrame::BlockMap { in_value: true, .. })
                    )
                    && decorations_inside_value;
                let indentless_key = last_significant == Some(TokenKind::Key)
                    && matches!(
                        block_stack.last(),
                        Some(BlockFrame::BlockMap {
                            entry_open: true,
                            in_value: false,
                        })
                    )
                    && decorations_inside_value;
                if indentless_value || indentless_key {
                    builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE.into());
                    block_stack.push(BlockFrame::BlockSequence {
                        item_open: false,
                        indentless: true,
                    });
                }
                if matches!(block_stack.last(), Some(BlockFrame::BlockSequence { .. })) {
                    close_open_sub_wrapper(&mut builder, &mut block_stack);
                    builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM.into());
                    if let Some(BlockFrame::BlockSequence { item_open, .. }) =
                        block_stack.last_mut()
                    {
                        *item_open = true;
                    }
                }
            }
            TokenKind::Trivia(_) => {}
            _ => {
                if !matches!(tok.kind, TokenKind::DocumentStart | TokenKind::DocumentEnd) {
                    ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                }
            }
        }
        let text = &input[tok.start.index..tok.end.index];
        if text.is_empty() {
            continue;
        }
        let kind = map_token_to_syntax_kind(tok.kind);
        match tok.kind {
            TokenKind::DocumentStart => {
                if doc_open && doc_only_has_directives {
                    builder.token(kind.into(), text);
                    doc_only_has_directives = false;
                } else {
                    close_block_containers(&mut builder, &mut block_stack);
                    if doc_open {
                        builder.finish_node();
                    }
                    builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
                    doc_open = true;
                    doc_only_has_directives = false;
                    builder.token(kind.into(), text);
                }
            }
            TokenKind::DocumentEnd => {
                close_block_containers(&mut builder, &mut block_stack);
                if !doc_open {
                    builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
                }
                builder.token(kind.into(), text);
                builder.finish_node();
                doc_open = false;
                doc_only_has_directives = false;
            }
            TokenKind::Trivia(_) => {
                builder.token(kind.into(), text);
            }
            TokenKind::Directive => {
                let was_open = doc_open;
                ensure_doc_open(&mut builder, &mut doc_open);
                if !was_open {
                    doc_only_has_directives = true;
                }
                builder.token(kind.into(), text);
            }
            TokenKind::Scalar(_) => {
                ensure_doc_open(&mut builder, &mut doc_open);
                doc_only_has_directives = false;
                emit_scalar_node(&mut builder, text, line_prefix);
            }
            _ => {
                ensure_doc_open(&mut builder, &mut doc_open);
                doc_only_has_directives = false;
                builder.token(kind.into(), text);
            }
        }
    }
    close_block_containers(&mut builder, &mut block_stack);
    if doc_open {
        builder.finish_node();
    }
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

#[derive(Debug, Clone, Copy)]
enum BlockFrame {
    BlockMap {
        entry_open: bool,
        in_value: bool,
    },
    /// `indentless` marks a sequence opened as a block-map value whose
    /// `-` entries sit at the same column as the parent key (YAML's
    /// "indentless sequence", spec 8.2.1). The scanner never pushes an
    /// indent level for it, so it emits no matching `BlockEnd`; the
    /// builder must close the frame itself when the parent map's next
    /// `Key` / `Value` / `BlockEnd` arrives.
    BlockSequence {
        item_open: bool,
        indentless: bool,
    },
    FlowMap {
        entry_open: bool,
        in_value: bool,
    },
    FlowSequence {
        item_open: bool,
    },
}

fn ensure_doc_open(builder: &mut GreenNodeBuilder<'_>, doc_open: &mut bool) {
    if !*doc_open {
        builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
        *doc_open = true;
    }
}

fn ensure_flow_seq_item_open(builder: &mut GreenNodeBuilder<'_>, stack: &mut [BlockFrame]) {
    if let Some(BlockFrame::FlowSequence { item_open }) = stack.last_mut()
        && !*item_open
    {
        builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE_ITEM.into());
        *item_open = true;
    }
}

/// Open `<MAP>_ENTRY` > `<MAP>_KEY` for the next entry, closing any
/// previously-open entry on the same Map frame. Caller must have
/// verified the top frame is a Map (Block or Flow).
fn open_map_entry_with_key(builder: &mut GreenNodeBuilder<'_>, stack: &mut [BlockFrame]) {
    close_open_sub_wrapper(builder, stack);
    let (entry_kind, key_kind) = match stack.last() {
        Some(BlockFrame::BlockMap { .. }) => (
            SyntaxKind::YAML_BLOCK_MAP_ENTRY,
            SyntaxKind::YAML_BLOCK_MAP_KEY,
        ),
        Some(BlockFrame::FlowMap { .. }) => (
            SyntaxKind::YAML_FLOW_MAP_ENTRY,
            SyntaxKind::YAML_FLOW_MAP_KEY,
        ),
        _ => return,
    };
    builder.start_node(entry_kind.into());
    builder.start_node(key_kind.into());
    if let Some(
        BlockFrame::BlockMap {
            entry_open,
            in_value,
        }
        | BlockFrame::FlowMap {
            entry_open,
            in_value,
        },
    ) = stack.last_mut()
    {
        *entry_open = true;
        *in_value = false;
    }
}

/// Close any indentless `YAML_BLOCK_SEQUENCE` frames on top of the
/// stack. These have no matching scanner `BlockEnd`, so they're closed
/// here when the parent map's next `Key` / `Value` / `BlockEnd` arrives.
/// Closing the open ITEM, finishing the SEQUENCE node, and popping the
/// frame reveals the parent map for the incoming token. Loops because
/// the next token may close several levels, though in practice
/// indentless frames never stack directly (they're always separated by
/// a map frame).
fn close_indentless_sequences(builder: &mut GreenNodeBuilder<'_>, stack: &mut Vec<BlockFrame>) {
    while let Some(BlockFrame::BlockSequence {
        indentless: true, ..
    }) = stack.last()
    {
        close_open_sub_wrapper(builder, stack);
        stack.pop();
        builder.finish_node(); // close YAML_BLOCK_SEQUENCE
    }
}

/// Close the top-of-stack frame's entry/item sub-wrapper if still open
/// and clear the flag. For maps, this closes the inner KEY/VALUE
/// node and the surrounding ENTRY. If we're closing while the entry
/// is still in its KEY phase (i.e. the entry never received a `:`,
/// e.g. a `?`-only explicit-key entry), an empty VALUE wrapper is
/// inserted before the ENTRY closes so every ENTRY has the same
/// `KEY + VALUE` child shape — the projection layer relies on that
/// invariant. For sequences it closes the ITEM. Caller decides whether
/// to also pop the frame itself.
fn close_open_sub_wrapper(builder: &mut GreenNodeBuilder<'_>, stack: &mut [BlockFrame]) {
    let Some(frame) = stack.last_mut() else {
        return;
    };
    match frame {
        BlockFrame::BlockMap {
            entry_open: true,
            in_value,
        } => {
            if *in_value {
                builder.finish_node(); // close VALUE
            } else {
                builder.finish_node(); // close KEY
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_VALUE.into());
                builder.finish_node(); // empty VALUE for shape parity
            }
            builder.finish_node(); // close ENTRY
            *frame = BlockFrame::BlockMap {
                entry_open: false,
                in_value: false,
            };
        }
        BlockFrame::FlowMap {
            entry_open: true,
            in_value,
        } => {
            if *in_value {
                builder.finish_node();
            } else {
                builder.finish_node();
                builder.start_node(SyntaxKind::YAML_FLOW_MAP_VALUE.into());
                builder.finish_node();
            }
            builder.finish_node();
            *frame = BlockFrame::FlowMap {
                entry_open: false,
                in_value: false,
            };
        }
        BlockFrame::BlockSequence {
            item_open: true,
            indentless,
        } => {
            let indentless = *indentless;
            builder.finish_node();
            *frame = BlockFrame::BlockSequence {
                item_open: false,
                indentless,
            };
        }
        BlockFrame::FlowSequence { item_open: true } => {
            builder.finish_node();
            *frame = BlockFrame::FlowSequence { item_open: false };
        }
        _ => {}
    }
}

fn close_block_containers(builder: &mut GreenNodeBuilder<'_>, stack: &mut Vec<BlockFrame>) {
    while let Some(frame) = stack.pop() {
        match frame {
            BlockFrame::BlockMap {
                entry_open: true,
                in_value,
            } => {
                if in_value {
                    builder.finish_node(); // close VALUE
                } else {
                    builder.finish_node(); // close KEY
                    builder.start_node(SyntaxKind::YAML_BLOCK_MAP_VALUE.into());
                    builder.finish_node();
                }
                builder.finish_node(); // close ENTRY
            }
            BlockFrame::FlowMap {
                entry_open: true,
                in_value,
            } => {
                if in_value {
                    builder.finish_node();
                } else {
                    builder.finish_node();
                    builder.start_node(SyntaxKind::YAML_FLOW_MAP_VALUE.into());
                    builder.finish_node();
                }
                builder.finish_node();
            }
            BlockFrame::BlockSequence {
                item_open: true, ..
            }
            | BlockFrame::FlowSequence { item_open: true } => {
                builder.finish_node();
            }
            _ => {}
        }
        builder.finish_node();
    }
}

/// Emit a scalar token's bytes as a `YAML_SCALAR` node whose leaves are
/// the per-physical-line content fragments (`YAML_SCALAR_TEXT`)
/// interleaved with `NEWLINE` leaves for the line breaks. Concatenating
/// the leaves reproduces `text` exactly, so this is byte-lossless and the
/// node's text range is unchanged. The node wrapper plus per-line
/// fragmentation is what lets the formatter/LSP treat a scalar as real
/// structure and is the seam a later step uses to interleave hashpipe
/// line-prefix leaves (see the yaml-formatter cutover plan, step 2).
fn emit_scalar_node(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    line_prefix: Option<&str>,
) {
    builder.start_node(SyntaxKind::YAML_SCALAR.into());
    emit_scalar_fragments(builder, text, line_prefix);
    builder.finish_node();
}

/// Split a scalar's source `text` into per-physical-line leaves:
/// `YAML_SCALAR_TEXT` content interleaved with `NEWLINE` line breaks
/// (`\n`, `\r\n`, and lone `\r` each one `NEWLINE` leaf). When
/// `line_prefix` is set, an embedded prefix at the start of each
/// *continuation* line (the marker plus at most one trailing space,
/// mirroring the scanner) is peeled into a leading `YAML_LINE_PREFIX`
/// leaf. The first line never carries an embedded prefix — its line-start
/// prefix was emitted as a separate `Trivia(LinePrefix)` token by the
/// scanner before the scalar began. Empty content runs are skipped
/// (rowan rejects zero-width tokens). The concatenation of all leaves
/// equals `text` exactly, so the node stays byte-lossless.
fn emit_scalar_fragments(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    line_prefix: Option<&str>,
) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line_index = 0usize;
    while i < bytes.len() {
        if line_index > 0
            && let Some(prefix) = line_prefix
            && let Some(len) = prefix_match_len(&text[i..], prefix)
        {
            builder.token(SyntaxKind::YAML_LINE_PREFIX.into(), &text[i..i + len]);
            i += len;
        }
        let content_start = i;
        while i < bytes.len() && !matches!(bytes[i], b'\n' | b'\r') {
            i += 1;
        }
        if content_start < i {
            builder.token(SyntaxKind::YAML_SCALAR_TEXT.into(), &text[content_start..i]);
        }
        if i < bytes.len() {
            let nl_len = if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            builder.token(SyntaxKind::NEWLINE.into(), &text[i..i + nl_len]);
            i += nl_len;
            line_index += 1;
        }
    }
}

fn prefix_match_len(s: &str, marker: &str) -> Option<usize> {
    let after = s.strip_prefix(marker)?;
    Some(marker.len() + usize::from(after.starts_with(' ')))
}

fn map_token_to_syntax_kind(kind: TokenKind) -> SyntaxKind {
    match kind {
        TokenKind::Trivia(TriviaKind::Whitespace) => SyntaxKind::WHITESPACE,
        TokenKind::Trivia(TriviaKind::Newline) => SyntaxKind::NEWLINE,
        TokenKind::Trivia(TriviaKind::Comment) => SyntaxKind::YAML_COMMENT,
        TokenKind::Trivia(TriviaKind::LinePrefix) => SyntaxKind::YAML_LINE_PREFIX,
        TokenKind::DocumentStart => SyntaxKind::YAML_DOCUMENT_START,
        TokenKind::DocumentEnd => SyntaxKind::YAML_DOCUMENT_END,
        TokenKind::Directive => SyntaxKind::YAML_DIRECTIVE,
        TokenKind::BlockEntry => SyntaxKind::YAML_BLOCK_SEQ_ENTRY,
        TokenKind::FlowEntry => SyntaxKind::YAML_FLOW_INDICATOR,
        TokenKind::FlowSequenceStart | TokenKind::FlowSequenceEnd => {
            SyntaxKind::YAML_FLOW_INDICATOR
        }
        TokenKind::FlowMappingStart | TokenKind::FlowMappingEnd => SyntaxKind::YAML_FLOW_INDICATOR,
        TokenKind::Value => SyntaxKind::YAML_COLON,
        TokenKind::Anchor => SyntaxKind::YAML_ANCHOR,
        TokenKind::Alias => SyntaxKind::YAML_ALIAS,
        TokenKind::Tag => SyntaxKind::YAML_TAG,
        TokenKind::Scalar(_) => SyntaxKind::YAML_SCALAR_TEXT,
        TokenKind::Key => SyntaxKind::YAML_KEY,
        TokenKind::StreamStart
        | TokenKind::StreamEnd
        | TokenKind::BlockSequenceStart
        | TokenKind::BlockMappingStart
        | TokenKind::BlockEnd => SyntaxKind::YAML_FLOW_INDICATOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxKind;

    /// `parse_stream` must reproduce its input byte-for-byte.
    fn assert_lossless(input: &str) {
        assert_eq!(
            parse_stream(input).text().to_string(),
            input,
            "input {input:?} not preserved"
        );
    }

    #[test]
    fn strip_with_offsets_matches_strip_line_prefix() {
        for input in [
            "#| a: 1\n",
            "#| a: 1\n#|   b\n",
            "  #| x: 1\n",
            "#| a\r\n#| b\r\n",
            "#| a",
        ] {
            let (text, offsets) = strip_line_prefix_with_offsets(input, "#|");
            assert_eq!(text, strip_line_prefix(input, "#|"), "text for {input:?}");
            assert_eq!(offsets.len(), text.len() + 1, "offset count for {input:?}");
            assert!(
                offsets.iter().all(|&o| o <= input.len()),
                "offsets in bounds for {input:?}"
            );
        }
    }

    #[test]
    fn locate_maps_hashpipe_error_to_region_offset() {
        let input = "#| echo: [\n";
        let (_diag, start, _end) = locate_yaml_diagnostic(input, "#|").expect("diagnostic");
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn locate_maps_composite_marker_error() {
        let input = "   #| echo: [\n";
        let (_diag, start, _end) = locate_yaml_diagnostic(input, "   #|").expect("diagnostic");
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn locate_maps_crlf_region_error() {
        let input = "#| ok: 1\r\n#| echo: [\r\n";
        let (_diag, start, _end) = locate_yaml_diagnostic(input, "#|").expect("diagnostic");
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn locate_frontmatter_uses_identity_offsets() {
        let input = "title: [\n";
        let (diag, start, _end) = locate_yaml_diagnostic(input, "").expect("diagnostic");
        assert_eq!(start, diag.byte_start);
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn locate_returns_none_for_valid_yaml() {
        assert!(locate_yaml_diagnostic("#| echo: false\n", "#|").is_none());
        assert!(locate_yaml_diagnostic("title: ok\n", "").is_none());
    }

    #[test]
    fn block_scalar_followed_by_option_is_not_swallowed_as_comment() {
        let input = "#| fig-cap: |\n#|   A caption\n#| echo: false\n";
        let tree = parse_stream_with_prefix(input, "#|");
        assert_eq!(tree.to_string(), input, "byte-lossless");
        let entries = tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .count();
        assert_eq!(entries, 2, "expected fig-cap and echo entries");
        assert!(
            !tree
                .descendants_with_tokens()
                .any(|element| element.kind() == SyntaxKind::YAML_COMMENT),
            "the option line must not be scanned as a comment"
        );
    }

    #[test]
    fn returns_byte_lossless_cst_for_empty_input() {
        assert_lossless("");
    }

    #[test]
    fn returns_byte_lossless_cst_for_simple_mapping() {
        assert_lossless("key: value\n");
    }

    #[test]
    fn returns_byte_lossless_cst_for_block_sequence() {
        assert_lossless("- a\n- b\n");
    }

    #[test]
    fn returns_byte_lossless_cst_for_flow_mapping() {
        assert_lossless("{a: b, c: d}\n");
    }

    #[test]
    fn returns_byte_lossless_cst_for_block_scalar() {
        assert_lossless("key: |\n  hello\n  world\n");
    }

    #[test]
    fn returns_byte_lossless_cst_for_quoted_scalar() {
        assert_lossless("\"key\": \"value\"\n");
    }

    #[test]
    fn returns_byte_lossless_cst_for_multi_line_plain_scalar() {
        assert_lossless("key: hello\n  world\n");
    }

    #[test]
    fn preserves_explicit_key_indicator_byte_in_flow_context() {
        assert_lossless("{ ?foo: bar }\n");
    }

    #[test]
    fn does_not_absorb_terminator_line_break_into_flow_scalar() {
        assert_lossless("{a: 42\n}\n");
    }

    fn document_count(tree: &SyntaxNode) -> usize {
        tree.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .count()
    }

    #[test]
    fn implicit_document_wraps_body_with_no_markers() {
        let input = "key: value\n";
        let tree = parse_stream(input);
        assert_eq!(document_count(&tree), 1);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_doc_start_opens_document_marker_lives_inside() {
        let input = "---\nkey: value\n";
        let tree = parse_stream(input);
        assert_eq!(document_count(&tree), 1);
        let doc = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .expect("document node");
        assert!(
            doc.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_START)),
            "`---` token should live inside YAML_DOCUMENT"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_doc_end_closes_document_marker_lives_inside() {
        let input = "key: value\n...\n";
        let tree = parse_stream(input);
        assert_eq!(document_count(&tree), 1);
        let doc = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .expect("document node");
        assert!(
            doc.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_END)),
            "`...` token should live inside YAML_DOCUMENT"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn consecutive_doc_starts_emit_two_documents() {
        let input = "---\na\n---\nb\n";
        let tree = parse_stream(input);
        assert_eq!(document_count(&tree), 2);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn pre_document_trivia_stays_at_stream_level() {
        let input = "\n---\nkey: value\n";
        let tree = parse_stream(input);
        let stream_token_kinds: Vec<SyntaxKind> = tree
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|t| t.kind())
            .collect();
        assert!(
            stream_token_kinds.contains(&SyntaxKind::NEWLINE),
            "leading newline should be a direct child of YAML_STREAM, got {stream_token_kinds:?}"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn bare_doc_end_at_stream_start_opens_synthetic_empty_document() {
        let input = "...\n";
        let tree = parse_stream(input);
        assert_eq!(document_count(&tree), 1);
        assert_eq!(tree.text().to_string(), input);
    }

    fn first_document(tree: &SyntaxNode) -> SyntaxNode {
        tree.children()
            .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .expect("at least one document")
    }

    fn block_map_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    }

    fn block_seq_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
    }

    fn block_map_entries(map: &SyntaxNode) -> Vec<SyntaxNode> {
        map.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .collect()
    }

    fn block_seq_items(seq: &SyntaxNode) -> Vec<SyntaxNode> {
        seq.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .collect()
    }

    fn entry_key(entry: &SyntaxNode) -> SyntaxNode {
        entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
            .expect("entry should have a YAML_BLOCK_MAP_KEY child")
    }

    fn entry_value(entry: &SyntaxNode) -> SyntaxNode {
        entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .expect("entry should have a YAML_BLOCK_MAP_VALUE child")
    }

    #[test]
    fn consecutive_empty_key_colons_open_separate_entries() {
        let input = ": a\n: b\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 2, "expected two empty-key ENTRY nodes");
        for (entry, scalar) in entries.iter().zip(["a", "b"]) {
            let key = entry_key(entry);
            assert!(
                !key.children().any(|n| n.kind() == SyntaxKind::YAML_SCALAR),
                "empty key should carry no scalar, got {key:?}",
            );
            let value = entry_value(entry);
            assert!(
                value
                    .children()
                    .any(|n| n.kind() == SyntaxKind::YAML_SCALAR && n.text() == scalar),
                "value should be {scalar:?}, got {value:?}",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_mapping_wraps_key_value_with_key_and_value_sub_wrappers() {
        let input = "key: value\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1, "expected one ENTRY for `key: value`");
        let key = entry_key(&entries[0]);
        let value = entry_value(&entries[0]);
        assert!(
            key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
            "colon should be the trailing token of YAML_BLOCK_MAP_KEY",
        );
        assert!(
            value
                .children()
                .any(|n| n.kind() == SyntaxKind::YAML_SCALAR),
            "scalar `value` should live inside YAML_BLOCK_MAP_VALUE",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_sequence_wraps_entries_in_yaml_block_sequence() {
        let input = "- a\n- b\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let seq = block_seq_under(&doc).expect("YAML_BLOCK_SEQUENCE child");
        let items = block_seq_items(&seq);
        assert_eq!(items.len(), 2, "expected 2 YAML_BLOCK_SEQUENCE_ITEM");
        for item in &items {
            let dash_count = item
                .children_with_tokens()
                .filter(|el| {
                    el.as_token()
                        .is_some_and(|t| t.kind() == SyntaxKind::YAML_BLOCK_SEQ_ENTRY)
                })
                .count();
            assert_eq!(dash_count, 1, "each item owns exactly one `-` token");
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn nested_block_mapping_nests_inner_block_map_inside_outer_value() {
        let input = "outer:\n  inner: x\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let outer = block_map_under(&doc).expect("outer YAML_BLOCK_MAP");
        let outer_entries = block_map_entries(&outer);
        assert_eq!(outer_entries.len(), 1);
        let outer_value = entry_value(&outer_entries[0]);
        let inner = outer_value
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("inner YAML_BLOCK_MAP nested under outer VALUE");
        let inner_entries = block_map_entries(&inner);
        assert_eq!(inner_entries.len(), 1);
        let inner_key = entry_key(&inner_entries[0]);
        assert!(
            inner_key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
            "inner key should own its colon",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_sequence_inside_mapping_nests_under_outer_map_value() {
        let input = "items:\n  - a\n  - b\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1, "one entry: `items: <seq>`");
        let value = entry_value(&entries[0]);
        let seq = value
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("YAML_BLOCK_SEQUENCE nested under map VALUE");
        let items = block_seq_items(&seq);
        assert_eq!(items.len(), 2);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn dedent_closes_inner_block_map_before_next_outer_key() {
        let input = "outer:\n  inner: x\nsibling: y\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let outer = block_map_under(&doc).expect("outer YAML_BLOCK_MAP");
        let entries = block_map_entries(&outer);
        assert_eq!(
            entries.len(),
            2,
            "outer map should have two entries (`outer:` and `sibling:`)",
        );
        let first_value = entry_value(&entries[0]);
        let nested_in_first = first_value
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .count();
        assert_eq!(nested_in_first, 1);
        let second_value = entry_value(&entries[1]);
        let nested_in_second = second_value
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .count();
        assert_eq!(nested_in_second, 0);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_map_with_two_top_level_entries_emits_two_entry_wrappers() {
        let input = "a: 1\nb: 2\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        assert_eq!(block_map_entries(&map).len(), 2);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_key_indicator_question_mark_lives_inside_key() {
        let input = "? a\n: b\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1);
        let key = entry_key(&entries[0]);
        let has_question = key.children_with_tokens().any(|el| {
            el.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_KEY)
        });
        assert!(has_question, "`?` should live inside YAML_BLOCK_MAP_KEY");
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_key_indentless_sequence_wraps_inside_key() {
        let input = "?\n- a\n- b\n:\n- c\n- d\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1);
        let key = entry_key(&entries[0]);
        assert!(
            key.children()
                .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE),
            "explicit-key block sequence should be wrapped in YAML_BLOCK_SEQUENCE inside KEY",
        );
        let value = entry_value(&entries[0]);
        assert!(
            value
                .children()
                .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE),
            "value-side block sequence should remain wrapped",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn empty_key_shorthand_opens_entry_with_empty_key() {
        let input = ": value\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1);
        let key = entry_key(&entries[0]);
        assert!(
            !key.children().any(|n| n.kind() == SyntaxKind::YAML_SCALAR),
            "empty-key shorthand has no scalar in KEY",
        );
        assert!(
            key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
            "empty-key KEY still owns the `:` token",
        );
        let value = entry_value(&entries[0]);
        assert!(
            value
                .children()
                .any(|n| n.kind() == SyntaxKind::YAML_SCALAR),
            "VALUE owns the `value` scalar",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn document_end_marker_lives_at_document_level_not_inside_block_map() {
        let input = "key: value\n...\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let has_doc_end = doc.children_with_tokens().any(|el| {
            el.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_END)
        });
        assert!(
            has_doc_end,
            "DOCUMENT_END should be a direct child of YAML_DOCUMENT"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    fn flow_map_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    }

    fn flow_seq_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
    }

    fn flow_map_entries(map: &SyntaxNode) -> Vec<SyntaxNode> {
        map.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY)
            .collect()
    }

    fn flow_seq_items(seq: &SyntaxNode) -> Vec<SyntaxNode> {
        seq.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
            .collect()
    }

    #[test]
    fn flow_sequence_wraps_each_item_in_flow_sequence_item() {
        let input = "[a, b, c]\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let seq = flow_seq_under(&doc).expect("YAML_FLOW_SEQUENCE child");
        let items = flow_seq_items(&seq);
        assert_eq!(items.len(), 3);
        let bracket_count = seq
            .children_with_tokens()
            .filter(|el| {
                el.as_token().map(|t| t.text()) == Some("[")
                    || el.as_token().map(|t| t.text()) == Some("]")
            })
            .count();
        assert_eq!(bracket_count, 2, "`[` and `]` at SEQUENCE level");
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_mapping_wraps_each_entry_with_key_and_value() {
        let input = "{a: 1, b: 2}\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = flow_map_under(&doc).expect("YAML_FLOW_MAP child");
        let entries = flow_map_entries(&map);
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            let key = entry
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_KEY)
                .expect("entry has YAML_FLOW_MAP_KEY");
            assert!(
                key.children_with_tokens().any(|el| el
                    .as_token()
                    .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
                "flow KEY owns trailing `:`",
            );
            let value = entry
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_VALUE)
                .expect("entry has YAML_FLOW_MAP_VALUE");
            assert!(
                value
                    .children()
                    .any(|n| n.kind() == SyntaxKind::YAML_SCALAR),
                "flow VALUE owns its scalar",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_sequence_inside_flow_sequence_nests_under_outer_item() {
        let input = "[[1, 2], [3, 4]]\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let outer = flow_seq_under(&doc).expect("outer YAML_FLOW_SEQUENCE");
        let outer_items = flow_seq_items(&outer);
        assert_eq!(outer_items.len(), 2);
        for item in &outer_items {
            assert!(
                item.children()
                    .any(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE),
                "outer item should contain a nested YAML_FLOW_SEQUENCE",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_mapping_inside_flow_sequence_nests_under_item() {
        let input = "[{a: 1}, {b: 2}]\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let seq = flow_seq_under(&doc).expect("YAML_FLOW_SEQUENCE child");
        let items = flow_seq_items(&seq);
        assert_eq!(items.len(), 2);
        for item in &items {
            assert!(
                item.children()
                    .any(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP),
                "each item should contain a nested YAML_FLOW_MAP",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_mapping_at_block_map_value_nests_under_block_map_value() {
        let input = "key: {a: 1, b: 2}\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let block_map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&block_map);
        assert_eq!(entries.len(), 1);
        let value = entry_value(&entries[0]);
        assert!(
            value
                .children()
                .any(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP),
            "flow map should be nested under outer block map's VALUE",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn directive_prelude_stays_inside_document_opened_by_marker() {
        let input = "%TAG !e! tag:example.com,2000:app/\n---\n!e!foo \"bar\"\n";
        let tree = parse_stream(input);
        assert_eq!(document_count(&tree), 1);
        let doc = first_document(&tree);
        let has_doc_start = doc.children_with_tokens().any(|el| {
            el.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_START)
        });
        assert!(has_doc_start, "the `---` should live inside the same doc");
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_key_without_value_emits_empty_value_for_shape_parity() {
        let input = "? a\n? b\n";
        let tree = parse_stream(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            assert!(
                entry
                    .children()
                    .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY),
                "ENTRY missing KEY child",
            );
            assert!(
                entry
                    .children()
                    .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE),
                "ENTRY missing VALUE child",
            );
        }
    }
}
