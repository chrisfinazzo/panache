//! Implicit-figure promotion for standalone images.
//!
//! Pandoc's `implicit_figures` extension turns a paragraph into a `Figure`
//! when---and only when---the image is *alone* in that paragraph. The
//! decision therefore cannot be made when the image line is first seen: a
//! lazy continuation line on the next row keeps the whole thing a single
//! `Para [Image, SoftBreak, ...]`. Promotion happens at paragraph close,
//! where the buffered paragraph text is already known, the same way setext
//! headings and `PLAIN` retagging are resolved.

use crate::options::ParserOptions;

use crate::parser::inlines::links::{LinkScanContext, try_parse_inline_image};

/// Whether a closing paragraph's buffered text is a standalone image, i.e.
/// whether it should be wrapped as `FIGURE` rather than `PARAGRAPH`.
///
/// `text` is the paragraph's accumulated content with container markers
/// stripped (see `ParagraphBuffer::get_text_for_parsing`).
pub(in crate::parser) fn paragraph_is_standalone_image(text: &str, config: &ParserOptions) -> bool {
    // Pandoc-only behavior; CommonMark/GFM keep the image inline within the
    // paragraph and do not promote it to a figure block.
    if !config.extensions.implicit_figures {
        return false;
    }

    let trimmed = text.trim();
    if !trimmed.starts_with("![") {
        return false;
    }

    let Some((len, _alt, _dest, _attrs)) =
        try_parse_inline_image(trimmed, LinkScanContext::from_options(config))
    else {
        return false;
    };

    trimmed[len..].trim().is_empty()
}
