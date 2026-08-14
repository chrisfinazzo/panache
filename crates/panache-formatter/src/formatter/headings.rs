use panache_parser::parser::utils::attributes::try_parse_trailing_attributes;
use rowan::NodeOrToken;

use super::core::normalize_attribute_text;
use super::inline::format_inline_node;
use super::smart::normalize_smart_punctuation;
use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};

/// Render a single heading line (`### content {attrs}`), formatting inline
/// elements and normalizing attributes. Surrounding blank lines and the
/// trailing newline are the caller's responsibility.
///
/// This is the single source of truth for heading content: both the
/// document-body `HEADING` branch and the list-nested heading path call it, so
/// inline content (code spans, links, emphasis) and attributes are reformatted
/// identically regardless of where the heading appears. The `#` prefix means
/// the rendered line can never collide with a thematic break.
pub(super) fn format_heading(node: &SyntaxNode, config: &Config) -> String {
    let mut level = 1;
    let mut attributes = String::new();
    let mut content = String::new();

    for child in node.children() {
        match child.kind() {
            SyntaxKind::ATX_HEADING_MARKER => {
                let t = child.text().to_string();
                level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
            }
            SyntaxKind::SETEXT_HEADING_UNDERLINE => {
                let t = child.text().to_string();
                level = if t.trim().starts_with('=') { 1 } else { 2 };
            }
            SyntaxKind::HEADING_CONTENT => {
                for element in child.children_with_tokens() {
                    match element {
                        NodeOrToken::Token(t) => {
                            if t.kind() == SyntaxKind::NEWLINE {
                                // Collapse internal newlines (multi-line setext
                                // content like `Foo\nBar\n---`) to a single
                                // space so the heading re-emits as one ATX line.
                                if !content.ends_with(' ') {
                                    content.push(' ');
                                }
                            } else {
                                content.push_str(
                                    normalize_smart_punctuation(
                                        t.text(),
                                        config.formatter_extensions.smart,
                                        config.formatter_extensions.smart_quotes,
                                    )
                                    .as_ref(),
                                );
                            }
                        }
                        NodeOrToken::Node(n) => {
                            content.push_str(&format_inline_node(&n, config));
                        }
                    }
                }
            }
            SyntaxKind::ATTRIBUTE => {
                attributes = normalize_attribute_text(&child.text().to_string());
            }
            _ => {}
        }
    }

    // Trim trailing closing hashes and surrounding whitespace (`# Title #`).
    let content = content
        .trim_end_matches(|c: char| c == '#' || c.is_whitespace())
        .trim_start();

    let mut out = "#".repeat(level);
    if !content.is_empty() {
        out.push(' ');
        out.push_str(content);
    }
    // A closing run is decoration and normally dropped --- except when it is
    // load-bearing. pandoc reads a heading's attribute block only *after* the
    // run, so the run is what keeps a trailing `{...}` in the content out of
    // the attributes: `# foo {#id} #` is `Header 1 (foo-id) [Str "foo", Space,
    // Str "{#id}"]`, and dropping the run would rewrite the id to `id`.
    if try_parse_trailing_attributes(content).is_some() {
        out.push_str(" #");
    }
    if !attributes.is_empty() {
        out.push(' ');
        out.push_str(&attributes);
    }
    out
}
