use rowan::{TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{
    AstNode, BlockNode, DisplayMath, InlineFootnote, InlineNode, SyntaxKind, SyntaxNode,
    SyntaxToken, TableCaption, TableCell, YamlMetadata,
};

pub struct SpaceBeforePunctuationRule;

impl Rule for SpaceBeforePunctuationRule {
    fn name(&self) -> &str {
        "space-before-punctuation"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "space-before-punctuation",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("space-before-punctuation")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::YAML_METADATA]
    }

    fn wants_text_tokens(&self) -> bool {
        true
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let preserve_semicolon = document_is_french(cx);
        let mut diagnostics = Vec::new();
        for token in cx.text_tokens() {
            let Some(paragraph) = prose_paragraph(token) else {
                continue;
            };
            for (offset, punctuation) in token.text().char_indices() {
                if !matches!(punctuation, '.' | ',' | ';')
                    || (punctuation == ';' && preserve_semicolon)
                {
                    continue;
                }
                let Some(range) = suspicious_gap(token, offset, &paragraph, cx) else {
                    continue;
                };
                diagnostics.push(
                    Diagnostic::warning(
                        Location::from_range(range, cx.input),
                        self.name(),
                        format!("Space before '{punctuation}' may be unintended"),
                    )
                    .with_fix(Fix::unsafe_fix(
                        format!("Remove space before '{punctuation}'"),
                        vec![Edit {
                            range,
                            replacement: String::new(),
                        }],
                    )),
                );
            }
        }
        diagnostics
    }
}

fn document_is_french(cx: &LintContext) -> bool {
    let lang = cx
        .nodes(SyntaxKind::YAML_METADATA)
        .iter()
        .filter_map(|node| YamlMetadata::cast(node.clone()))
        .filter_map(|yaml| yaml.document())
        .find_map(|document| {
            document
                .block_map()
                .and_then(|map| map.value_of("lang")?.as_scalar())
                .or_else(|| document.flow_map()?.value_of("lang")?.as_scalar())
                .map(|scalar| scalar.value())
        });
    lang.as_deref()
        .or(cx.config.lang.as_deref())
        .is_some_and(|lang| {
            lang.trim()
                .split(['-', '_'])
                .next()
                .is_some_and(|primary| primary.eq_ignore_ascii_case("fr"))
        })
}

fn prose_paragraph(token: &SyntaxToken) -> Option<SyntaxNode> {
    let mut paragraph = None;
    for ancestor in token.parent_ancestors() {
        if paragraph.is_none() {
            match BlockNode::cast(ancestor.clone().into()) {
                BlockNode::Paragraph(_) | BlockNode::Plain(_) => {
                    paragraph = Some(ancestor);
                    continue;
                }
                _ => {}
            }
            if matches!(
                InlineNode::cast(ancestor.clone().into()),
                InlineNode::Emphasis(_) | InlineNode::Strong(_)
            ) || InlineFootnote::can_cast(ancestor.kind())
            {
                continue;
            }
            return None;
        }
        if TableCell::can_cast(ancestor.kind())
            || TableCaption::can_cast(ancestor.kind())
            || matches!(
                ancestor.kind(),
                SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_RAW | SyntaxKind::HTML_BLOCK_DIV
            )
            || matches!(
                BlockNode::cast(ancestor.into()),
                BlockNode::Heading(_)
                    | BlockNode::Table(_)
                    | BlockNode::Figure(_)
                    | BlockNode::ReferenceDefinition(_)
                    | BlockNode::YamlMetadata(_)
                    | BlockNode::LineBlock(_)
            )
        {
            return None;
        }
    }
    paragraph
}

fn suspicious_gap(
    token: &SyntaxToken,
    offset: usize,
    paragraph: &SyntaxNode,
    cx: &LintContext,
) -> Option<TextRange> {
    let at = usize::from(token.text_range().start()) + offset;
    let start = usize::from(paragraph.text_range().start());
    let before = &cx.input[start..at];
    let previous = before.trim_end_matches([' ', '\t']);
    if previous.len() == before.len() {
        return None;
    }
    let gap_start = start + previous.len();
    let previous_token = paragraph
        .token_at_offset(TextSize::from(gap_start as u32))
        .left_biased()?;
    // Escaped and nonbreaking spaces are deliberate source syntax.
    if !matches!(previous_token.kind(), SyntaxKind::TEXT)
        && previous_token.text_range().end() > TextSize::from(gap_start as u32)
    {
        return None;
    }
    let last = previous.chars().next_back()?;
    if !(last.is_alphanumeric()
        || matches!(last, ')' | ']' | '}' | '\'' | '"' | '’' | '”')
        || ends_inline(&previous_token, gap_start))
    {
        return None;
    }
    // A suffix such as `.md` or a decimal fragment such as `.5` is a word.
    let after = &token.text()[offset + 1..];
    if after.chars().next().is_some_and(|ch| !ch.is_whitespace()) {
        return None;
    }
    // A text token can end at a prefix such as the dot in .`column-margin`.
    // Only whitespace or a closing delimiter makes it standalone punctuation.
    if after.is_empty()
        && token.next_sibling_or_token().is_some_and(|next| {
            !matches!(
                InlineNode::cast(next.clone()),
                InlineNode::Space(_)
                    | InlineNode::SoftBreak(_)
                    | InlineNode::HardBreak(_)
                    | InlineNode::NonbreakingSpace(_)
            ) && !matches!(
                next.kind(),
                SyntaxKind::EMPHASIS_MARKER
                    | SyntaxKind::STRONG_MARKER
                    | SyntaxKind::INLINE_FOOTNOTE_END
            )
        })
    {
        return None;
    }
    // Spaced ellipses can also be Pandoc presentation pauses.
    if cx.input[at + 1..]
        .trim_start_matches(|ch: char| ch.is_ascii_whitespace())
        .starts_with(['.', ',', ';'])
    {
        return None;
    }
    let word = previous.split_whitespace().next_back()?;
    if (cx.input.as_bytes()[at] == b';' && word.contains('&'))
        || word.contains("://")
        || word.contains('@')
    {
        return None;
    }
    if cx.input.as_bytes()[at] == b'.'
        && could_be_list_marker(word, cx.config.extensions.fancy_lists)
    {
        let prefix = &previous[..previous.len() - word.len()];
        let line_prefix = prefix.rsplit('\n').next().unwrap_or(prefix);
        if line_prefix.trim_matches([' ', '\t', '>']).is_empty() {
            return None;
        }
        // Formatting puts display math on its own lines; the following word
        // could then become a list marker even if it shares a source line now.
        let end = start + prefix.trim_end_matches([' ', '\t']).len();
        if paragraph
            .token_at_offset(TextSize::from(end as u32))
            .left_biased()
            .is_some_and(|token| {
                token
                    .parent_ancestors()
                    .any(|node| DisplayMath::can_cast(node.kind()))
            })
        {
            return None;
        }
    }
    Some(TextRange::new(
        TextSize::from(gap_start as u32),
        TextSize::from(at as u32),
    ))
}

fn ends_inline(token: &SyntaxToken, end: usize) -> bool {
    token.parent_ancestors().any(|node| {
        usize::from(node.text_range().end()) == end
            && matches!(
                InlineNode::cast(node.into()),
                InlineNode::Emphasis(_)
                    | InlineNode::Strong(_)
                    | InlineNode::Code(_)
                    | InlineNode::Math(_)
                    | InlineNode::Link(_)
                    | InlineNode::Image(_)
            )
    })
}

fn could_be_list_marker(word: &str, fancy_lists: bool) -> bool {
    word.bytes().all(|byte| byte.is_ascii_digit())
        || (fancy_lists
            && ((word.len() == 1 && word.as_bytes()[0].is_ascii_alphabetic())
                || word.chars().all(|ch| "ivxlcdmIVXLCDM".contains(ch))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Extensions, Flavor};

    fn lint_with(input: &str, config: Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        SpaceBeforePunctuationRule.check_tree(&tree, input, &config, None)
    }

    fn fixed(input: &str, config: Config) -> String {
        let diagnostics = lint_with(input, config);
        let mut output = input.to_string();
        for diagnostic in diagnostics.iter().rev() {
            let fix = diagnostic.fix.as_ref().expect("spacing fix");
            assert_eq!(fix.safety, crate::linter::FixSafety::Unsafe);
            for edit in fix.edits.iter().rev() {
                output.replace_range(
                    usize::from(edit.range.start())..usize::from(edit.range.end()),
                    &edit.replacement,
                );
            }
        }
        output
    }

    #[test]
    fn fixes_horizontal_gaps_across_flavors() {
        for flavor in [
            Flavor::Pandoc,
            Flavor::Quarto,
            Flavor::RMarkdown,
            Flavor::CommonMark,
            Flavor::Gfm,
            Flavor::MultiMarkdown,
            Flavor::Mdsvex,
            Flavor::Myst,
        ] {
            let config = Config {
                flavor,
                extensions: Extensions::for_flavor(flavor),
                ..Config::default()
            };
            let input = "Café   , then β\t; finally words .\r\n";
            let output = fixed(input, config.clone());
            assert_eq!(output, "Café, then β; finally words.\r\n", "{flavor:?}");
            assert!(lint_with(&output, config).is_empty());
        }
    }

    #[test]
    fn handles_prose_in_containers_and_beside_inline_nodes() {
        for (input, expected) in [
            ("*Words , more* .\n", "*Words, more*.\n"),
            ("**Words ; more** .\n", "**Words; more**.\n"),
            ("- Words .\n", "- Words.\n"),
            ("> Words .\n", "> Words.\n"),
            ("[^note]: Words .\n", "[^note]: Words.\n"),
            ("Words ^[a note , more] .\n", "Words ^[a note, more].\n"),
            ("`code` , then $x$ ; end .\n", "`code`, then $x$; end.\n"),
            ("[label](url) , next .\n", "[label](url), next.\n"),
        ] {
            assert_eq!(fixed(input, Config::default()), expected, "{input:?}");
        }
    }

    #[test]
    fn excludes_nonprose_contexts() {
        for input in [
            "# Heading .\n",
            "[label ,](url)\n",
            "[A ,]\n\n[A ,]: /example\n",
            "![caption .](image.png)\n",
            "`literal .`\n",
            "$x ; y$\n",
            "```\ncode .\n```\n",
            "---\ntitle: Title .\n---\n",
            "<pre>literal .</pre>\n",
            "<div>\n\nWords .\n\n</div>\n",
            "| A | B |\n|---|---|\n| word . | word , |\n",
            "| Words .\n",
        ] {
            assert!(lint_with(input, Config::default()).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn preserves_deliberate_spacing_and_punctuation_runs() {
        for input in [
            "Words\u{00a0}; more\n",
            "Words\u{202f}; more\n",
            "Words\u{2009}; more\n",
            "Words\\ ; more\n",
            "Words \\, more\n",
            "Words\n.\n",
            "Words  \n.\n",
            "Words\\\n.\n",
            "Words . . .\n",
            "Words ...\n",
            "Words ; ; next\n",
            "Use .md files and .5 or ,000 fragments.\n",
        ] {
            assert!(lint_with(input, Config::default()).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn preserves_punctuation_prefixes_on_adjacent_inline_content() {
        for input in [
            "Use the .`column-body-outset` class.\n",
            "Use the .*column* class.\n",
            "Use the .**column** class.\n",
            "Use the .[column](url) class.\n",
            "Use the .\\* selector.\n",
        ] {
            assert!(lint_with(input, Config::default()).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn fixes_punctuation_before_closing_inline_delimiters() {
        for (input, expected) in [
            ("*Words .*\n", "*Words.*\n"),
            ("**Words ,** more\n", "**Words,** more\n"),
            ("Words ^[a note .]\n", "Words ^[a note.]\n"),
        ] {
            assert_eq!(fixed(input, Config::default()), expected, "{input:?}");
        }
    }

    #[test]
    fn avoids_creating_markdown_syntax() {
        for input in [
            "1 . item\n",
            "A .\n",
            "iv . item\n",
            "- 1 . item\n",
            "> 1 . item\n",
            "Words\n1 . item\n",
            "Words\\\n1 . item\n",
            "- Before $$x$$ A .\n",
            "&amp ; &#123 ; &#x3b ;\n",
            "https://example.com ;\n",
            "user@example.com ;\n",
        ] {
            assert!(lint_with(input, Config::default()).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn french_semicolons_follow_yaml_then_config_language() {
        let french = Config {
            lang: Some("fr-CA".into()),
            ..Config::default()
        };
        let english = Config {
            lang: Some("en".into()),
            ..Config::default()
        };
        assert_eq!(
            fixed("Bonjour ; salut , fin .\n", french.clone()),
            "Bonjour ; salut, fin.\n"
        );
        for yaml in ["lang: fr", "lang: 'FR-ca' # comment", "{lang: fr-FR}"] {
            let input = format!("---\n{yaml}\n---\n\nBonjour ; salut , fin .\n");
            let expected = format!("---\n{yaml}\n---\n\nBonjour ; salut, fin.\n");
            assert_eq!(fixed(&input, english.clone()), expected);
        }
        let input = "---\nlang: en\n---\n\nWords ; more\n";
        assert_eq!(fixed(input, french), "---\nlang: en\n---\n\nWords; more\n");
    }
}
