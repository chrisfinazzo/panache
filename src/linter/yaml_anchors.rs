//! Shared extraction of YAML anchor declarations and alias references from a
//! document's embedded YAML regions (frontmatter and hashpipe `#|` options).
//!
//! Both the `duplicate-yaml-anchor` and `unused-yaml-anchor` lint rules walk the
//! same anchor/alias token stream, so the walk lives here once. YAML resets
//! anchor scope at each document boundary (`---`), so results are grouped by
//! embedded `YAML_DOCUMENT` — mirroring the validator's per-document
//! `check_undeclared_alias` pass.

use std::collections::HashSet;

use rowan::TextRange;

use crate::syntax::{SyntaxKind, SyntaxNode};

/// One `&name` anchor declaration and the source range of its token.
pub(crate) struct AnchorOccurrence {
    pub name: String,
    pub range: TextRange,
}

/// Anchors and alias references collected from a single embedded YAML document.
pub(crate) struct DocumentAnchors {
    /// `&name` declarations in document order.
    pub anchors: Vec<AnchorOccurrence>,
    /// Names referenced by a `*name` alias somewhere in the document.
    pub used: HashSet<String>,
}

/// Collect anchor declarations and alias uses for every embedded YAML document
/// under `regions` — the frontmatter `YAML_METADATA` and hashpipe
/// `HASHPIPE_YAML_PREAMBLE` nodes the shared lint walk bucketed. Ranges are
/// host-aligned, so they map directly onto the host document source. Documents
/// with no anchor declarations are omitted (neither rule cares about them).
pub(crate) fn collect_document_anchors(regions: &[&SyntaxNode]) -> Vec<DocumentAnchors> {
    let mut docs = Vec::new();
    for region in regions {
        for document in region
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
        {
            let mut anchors = Vec::new();
            let mut used = HashSet::new();
            for token in document
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
            {
                match token.kind() {
                    SyntaxKind::YAML_ANCHOR => anchors.push(AnchorOccurrence {
                        name: strip_sigil(token.text()).to_string(),
                        range: token.text_range(),
                    }),
                    SyntaxKind::YAML_ALIAS => {
                        used.insert(strip_sigil(token.text()).to_string());
                    }
                    _ => {}
                }
            }
            if !anchors.is_empty() {
                docs.push(DocumentAnchors { anchors, used });
            }
        }
    }
    docs
}

/// Drop the leading `&`/`*` indicator from an anchor/alias token's text.
fn strip_sigil(text: &str) -> &str {
    text.strip_prefix('&')
        .or_else(|| text.strip_prefix('*'))
        .unwrap_or(text)
}
