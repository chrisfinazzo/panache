use std::collections::HashMap;

use rowan::{TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{
    AstNode, BlockNode, Citation, ImageLink, Link, LinkText, SyntaxKind, SyntaxNode, SyntaxToken,
    UnresolvedReference,
};

pub struct MalformedCitationRule;

impl Rule for MalformedCitationRule {
    fn description(&self) -> &'static str {
        include_str!("docs/malformed-citation.md")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn examples(&self) -> &'static [crate::linter::docs::Example] {
        use crate::linter::docs::Example;
        const { &[Example::new("See [@].\n", &["malformed-citation"])] }
    }

    fn name(&self) -> &str {
        "malformed-citation"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "malformed-citation",
            default_on: true,
            requires: Requirement::Citations,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("malformed-citation")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::CITATION]
    }

    fn wants_text_tokens(&self) -> bool {
        true
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let mut closing_brackets = HashMap::new();
        for token in cx.text_tokens() {
            let Some(parent) = token.parent() else {
                continue;
            };
            let bracketed = LinkText::cast(parent.clone())
                .and_then(|text| text.syntax().parent())
                .and_then(UnresolvedReference::cast)
                .is_some_and(|reference| reference.label().is_none());
            if !bracketed && !is_prose(&parent) {
                continue;
            }
            diagnostics.extend(
                malformed_markers(token, bracketed)
                    .into_iter()
                    .map(|(range, message)| diagnostic(range, cx.input, message)),
            );
        }

        for citation in cx
            .nodes(SyntaxKind::CITATION)
            .iter()
            .cloned()
            .filter_map(Citation::cast)
        {
            if citation
                .syntax()
                .ancestors()
                .any(|node| Link::can_cast(node.kind()) || ImageLink::can_cast(node.kind()))
            {
                continue;
            }
            for marker in citation.markers() {
                if marker.key().is_none_or(|key| key.text().is_empty()) {
                    diagnostics.push(diagnostic(
                        marker.text_range(),
                        cx.input,
                        "Citation marker has no key; add a citation key after '@'",
                    ));
                }
            }
            if let Some(range) = unclosed_bracket(&citation, &mut closing_brackets) {
                diagnostics.push(diagnostic(
                    range,
                    cx.input,
                    "Bracketed citation has no closing ']'; close the citation bracket",
                ));
            }
        }
        diagnostics.sort_by_key(|diagnostic| diagnostic.location.range.start());
        diagnostics
    }
}

fn diagnostic(range: TextRange, input: &str, message: &str) -> Diagnostic {
    Diagnostic::warning(
        Location::from_range(range, input),
        "malformed-citation",
        message,
    )
}

fn is_prose(node: &SyntaxNode) -> bool {
    matches!(
        BlockNode::cast(node.clone().into()),
        BlockNode::Paragraph(_) | BlockNode::Plain(_) | BlockNode::Heading(_)
    )
}

/// Share the classification with undefined references so one broken bracket
/// gets the specific citation diagnostic instead of a missing-link diagnosis.
pub(super) fn has_malformed_marker(reference: &UnresolvedReference) -> bool {
    if reference.label().is_some() {
        return false;
    }
    reference.text_node().is_some_and(|text| {
        text.syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .any(|token| !malformed_markers(&token, true).is_empty())
    })
}

fn malformed_markers(token: &SyntaxToken, bracketed: bool) -> Vec<(TextRange, &'static str)> {
    let text = token.text();
    let mut markers = Vec::new();
    for (at, _) in text.match_indices('@') {
        let start = if text[..at].ends_with('-') {
            at - 1
        } else {
            at
        };
        let previous = text[..start].chars().next_back().or_else(|| {
            token
                .prev_sibling_or_token()
                .and_then(|element| element.to_string().chars().next_back())
        });
        // Emails and mentions attached to a word do not establish citation intent.
        if previous.is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, ';' | '(')) {
            continue;
        }
        let next = text[at + 1..].chars().next();
        let (end, message) = match next {
            Some('{') => (
                at + 2,
                "Braced citation key has no closing '}'; close the key brace",
            ),
            _ if !bracketed => continue,
            None | Some(';' | ']') => (
                at + 1,
                "Citation marker has no key; add a citation key after '@'",
            ),
            Some(ch) if ch.is_whitespace() => (
                at + 1,
                "Citation key must immediately follow '@' without whitespace",
            ),
            Some(ch) if !ch.is_alphanumeric() && ch != '_' => (
                at + 1 + ch.len_utf8(),
                "Citation key must start with a letter, digit, or '_'; use '@{...}' for other keys",
            ),
            _ => continue,
        };
        let base = token.text_range().start();
        markers.push((
            TextRange::new(
                base + TextSize::from(start as u32),
                base + TextSize::from(end as u32),
            ),
            message,
        ));
    }
    markers
}

fn unclosed_bracket(
    citation: &Citation,
    closing_brackets: &mut HashMap<TextRange, Option<TextSize>>,
) -> Option<TextRange> {
    let parent = citation.syntax().parent()?;
    if citation.is_bracketed() || !is_prose(&parent) {
        return None;
    }
    let previous = citation.syntax().prev_sibling_or_token()?.into_token()?;
    if previous.kind() != SyntaxKind::TEXT || !previous.text().ends_with('[') {
        return None;
    }
    // Cache per block so repeated unmatched openers cannot cause quadratic scans.
    let last_close = closing_brackets
        .entry(parent.text_range())
        .or_insert_with(|| {
            parent
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| token.kind() == SyntaxKind::TEXT)
                .filter_map(|token| {
                    token
                        .text()
                        .rfind(']')
                        .map(|offset| token.text_range().start() + TextSize::from(offset as u32))
                })
                .last()
        });
    if last_close.is_some_and(|close| close >= citation.syntax().text_range().end()) {
        return None;
    }
    let marker = citation.markers().next()?;
    Some(TextRange::new(
        previous.text_range().end() - TextSize::from(1),
        marker.marker_range().end(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        MalformedCitationRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn explains_each_malformation() {
        for (input, message, span) in [
            ("[@]\n", "no key", "@"),
            ("[@ key]\n", "without whitespace", "@"),
            ("é [@💡key]\n", "must start", "@💡"),
            ("[@{unfinished]\n", "closing '}'", "@{"),
            ("@{}\n", "no key", "@{}"),
            ("[-@doe\n", "closing ']'", "[-@"),
        ] {
            let diagnostics = lint(input);
            assert_eq!(diagnostics.len(), 1, "{input:?}: {diagnostics:#?}");
            assert!(diagnostics[0].message.contains(message));
            assert_eq!(&input[diagnostics[0].location.range], span);
        }
    }

    #[test]
    fn ignores_markers_in_verbatim_and_link_contexts() {
        for input in [
            "`@{unfinished` and `[@{}]`\n",
            "````text\n@{unfinished\n[@{}]\n````\n",
            "$@{unfinished$ and $[@{}]$\n",
            "\\@{unfinished and [\\@.bad]\n",
            "\\[@key and [\\@key\n",
            "[@{}](url)\n",
            "![@{}](url)\n",
            "[@.bad][missing] and ![@.bad]\n",
            "---\ntitle: '[@.bad] @{unfinished'\n---\n",
            "<script>\n@{unfinished\n</script>\n",
        ] {
            assert!(lint(input).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn accepts_pandoc_key_and_suffix_syntax() {
        for input in [
            "[@a; -@b, p. 3]\n",
            "[@_a] [@123] [@Émile]\n",
            "[@{.key}] @{a key} [@{a\\}b}]\n",
            "[@a..b] [@a: suffix] [@a!]\n",
            "[user@example.org] and user@example.org\n",
            "A literal @ and @. in prose\n",
        ] {
            assert!(lint(input).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn reports_every_empty_key_in_a_group() {
        let input = "[@a; -@{}; @{}]\n";
        let diagnostics = lint(input);
        let spans: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| &input[diagnostic.location.range])
            .collect();
        assert_eq!(spans, ["-@{}", "@{}"]);
    }

    #[test]
    fn preserves_spans_in_containers() {
        let input =
            "> See [@.bad].\n\n- See [@{unfinished].\n\n# See [@]\n\n<div>\n@{unfinished\n</div>\n";
        let diagnostics = lint(input);
        let spans: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| &input[diagnostic.location.range])
            .collect();
        assert_eq!(spans, ["@.", "@{", "@", "@{"]);
    }
}
