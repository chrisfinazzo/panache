use super::list_item_buffer::ListItemBuffer;
use super::text_buffer::ParagraphBuffer;
use crate::parser::blocks::lists::ListMarker;
use rowan::Checkpoint;

/// Which multi-line display-math delimiter is currently open in a paragraph.
///
/// One field (rather than parallel per-delimiter flags) so that delimiters of
/// one kind occurring inside an open region of another kind cannot latch a
/// second "open" state: while a region is open, only its own closer matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenDisplayMath {
    /// `$$`-delimited; holds the opening run length (a closer needs
    /// `run_len >= open_len`).
    Dollars(usize),
    /// `\[ ... \]` (extension `tex_math_single_backslash`).
    SingleBrackets,
    /// `\\[ ... \\]` (extension `tex_math_double_backslash`).
    DoubleBrackets,
}

#[derive(Debug, Clone)]
pub(crate) enum Container {
    BlockQuote {
        // No special tracking needed
    },
    Alert {
        blockquote_depth: usize,
    },
    FencedDiv {
        /// Indentation (columns) of the opening fence, in the
        /// container-prefix-stripped frame. A closing fence more indented than
        /// this is not a closer at the top level (pandoc rule); see
        /// `FencedDivCloseParser::detect_prepared`.
        open_indent_cols: usize,
    },
    /// MyST directive container. Closed by a fence line matching the opener's
    /// `fence_char` with at least `fence_count` repeats. The fence info is
    /// tracked here (unlike `FencedDiv`, whose closer is always bare `:::`)
    /// because backtick directives must distinguish their closer from nested
    /// code fences of a shorter run.
    MystDirective {
        fence_char: u8,
        fence_count: usize,
    },
    /// python-markdown admonition / pymdownx details container. Content is
    /// indented by `content_col` (4) columns; closes on dedent like a
    /// footnote definition.
    Admonition {
        content_col: usize,
    },
    List {
        marker: ListMarker,
        base_indent_cols: usize,
        has_blank_between_items: bool, // Track if list is loose (blank lines between items)
    },
    ListItem {
        content_col: usize,
        buffer: ListItemBuffer, // Buffer for list item content
        /// True iff this list item has so far only seen its marker line, with
        /// no real content (text, nested list, etc.) — a marker-only item.
        /// Used by CommonMark to close empty list items at the first blank
        /// line, per spec §5.2 ("a list item can begin with at most one
        /// blank line"). Pandoc keeps the item open across the blank.
        marker_only: bool,
        /// True when the marker's required-1-col space was virtually absorbed
        /// from a tab in the post-marker text rather than consumed as a
        /// literal byte. In that case the buffered content's first byte is at
        /// source column `content_col - 1`, not `content_col`. Used by
        /// indented-code-from-marker-line detection to walk col-aware leading
        /// whitespace correctly.
        virtual_marker_space: bool,
    },
    DefinitionList {
        // Definition lists don't need special tracking
    },
    DefinitionItem {
        // No special tracking needed
    },
    Definition {
        content_col: usize,
        plain_open: bool,
        /// Buffer for accumulating PLAIN content. Interleaved (rather than a
        /// flat text buffer) so each continuation line's gobbled
        /// `content_col` indent can be held *out* of the text handed to the
        /// inline parser and re-injected as `WHITESPACE` at emission.
        plain_buffer: ParagraphBuffer,
    },
    Paragraph {
        buffer: ParagraphBuffer, // Interleaved buffer for paragraph content with markers
        open_inline_math_envs: Vec<String>,
        open_display_math: Option<OpenDisplayMath>,
        // Checkpoint at the position the paragraph started; used to retroactively
        // wrap buffered content as PARAGRAPH (or HEADING for multi-line setext)
        // when the paragraph is closed.
        start_checkpoint: Checkpoint,
    },
    FootnoteDefinition {
        content_col: usize,
    },
}

pub(crate) struct ContainerStack {
    pub(crate) stack: Vec<Container>,
}

const TAB_STOP: usize = 4;

impl ContainerStack {
    pub(crate) fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub(crate) fn depth(&self) -> usize {
        self.stack.len()
    }

    pub(crate) fn last(&self) -> Option<&Container> {
        self.stack.last()
    }

    pub(crate) fn push(&mut self, c: Container) {
        self.stack.push(c);
    }
}

/// Expand tabs to columns (tab stop = 4) and return (cols, byte_offset).
pub(crate) fn leading_indent(line: &str) -> (usize, usize) {
    leading_indent_from(line, 0)
}

/// Like [`leading_indent`] but seeds the column counter at `start_col` so tab
/// expansion honors source-column tab-stops. Use when the leading whitespace
/// being measured doesn't begin at source column 0 (e.g. the bytes after a
/// list marker, where the marker itself occupies columns
/// `[indent_cols, indent_cols + marker_len)`).
pub(crate) fn leading_indent_from(line: &str, start_col: usize) -> (usize, usize) {
    let mut cols = 0usize;
    let mut bytes = 0usize;
    for b in line.bytes() {
        match b {
            b' ' => {
                cols += 1;
                bytes += 1;
            }
            b'\t' => {
                let absolute = start_col + cols;
                cols += TAB_STOP - (absolute % TAB_STOP);
                bytes += 1;
            }
            _ => break,
        }
    }
    (cols, bytes)
}

/// Number of leading bytes of `line` covering up to `content_col` columns of
/// container indentation — spaces, or tabs landing on a stop at or before it.
///
/// This is the gobble pandoc applies to a container's continuation lines
/// (`listLine` for a list item, the definition-body indent for a definition),
/// expressed in bytes so the caller can hold exactly those out of the text it
/// hands to the inline parser.
///
/// Unlike [`byte_index_at_column`], a tab that would *overshoot* `content_col`
/// stops the walk instead of being consumed whole. The CST is byte-lossless,
/// so a tab straddling the content column has no boundary to split on; leaving
/// it in the payload keeps all its columns, and the ones it should have lost to
/// the gobble are subtracted downstream by column.
pub(crate) fn gobbled_indent_prefix_len(line: &str, content_col: usize) -> usize {
    let mut consumed = 0usize;
    let mut col = 0usize;
    for &b in line.as_bytes() {
        if col >= content_col {
            break;
        }
        match b {
            b' ' => {
                col += 1;
                consumed += 1;
            }
            b'\t' => {
                let next = (col / TAB_STOP + 1) * TAB_STOP;
                if next > content_col {
                    break;
                }
                col = next;
                consumed += 1;
            }
            _ => break,
        }
    }
    consumed
}

/// Return byte index at a given column (tabs = 4).
pub(crate) fn byte_index_at_column(line: &str, target_col: usize) -> usize {
    let mut col = 0usize;
    let mut idx = 0usize;
    for (i, b) in line.bytes().enumerate() {
        if col >= target_col {
            return idx;
        }
        match b {
            b' ' => {
                col += 1;
                idx = i + 1;
            }
            b'\t' => {
                col += TAB_STOP - (col % TAB_STOP);
                idx = i + 1;
            }
            _ => break,
        }
    }
    idx
}
