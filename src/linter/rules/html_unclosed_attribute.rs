use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::InlineNode;

pub struct HtmlUnclosedAttributeRule;

impl Rule for HtmlUnclosedAttributeRule {
    fn description(&self) -> &'static str {
        include_str!("docs/html-unclosed-attribute.md")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn examples(&self) -> &'static [crate::linter::docs::Example] {
        use crate::linter::docs::Example;
        const {
            &[Example::new(
                "<a id=\"goal></a>\n",
                &["html-unclosed-attribute"],
            )]
        }
    }

    fn name(&self) -> &str {
        "html-unclosed-attribute"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "html-unclosed-attribute",
            default_on: true,
            requires: Requirement::RawHtml,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("html-unclosed-attribute")] },
        }
    }

    fn wants_text_tokens(&self) -> bool {
        true
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for token in cx.text_tokens() {
            let InlineNode::Text(text) = InlineNode::cast(token.clone().into()) else {
                continue;
            };
            for error in text.html_attribute_diagnostics() {
                diagnostics.push(
                    Diagnostic::warning(
                        Location::from_range(error.range, cx.input),
                        "html-unclosed-attribute",
                        format!(
                            "Unclosed quoted value for HTML attribute '{}'",
                            error.attribute
                        ),
                    )
                    .with_note(
                        DiagnosticNoteKind::Help,
                        "add the matching closing quote at the intended end of the value",
                    ),
                );
            }
        }
        diagnostics.sort_by_key(|diagnostic| diagnostic.location.range.start());
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::linter::Severity;

    fn check(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        HtmlUnclosedAttributeRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn reports_the_attribute_name_at_its_opening_quote() {
        let input = "仍會面臨 <a id=\"goal></a>\n";
        let diagnostics = check(input);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, "html-unclosed-attribute");
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(
            diagnostic.message,
            "Unclosed quoted value for HTML attribute 'id'"
        );
        assert_eq!(&input[diagnostic.location.range], "\"");
        assert!(diagnostic.fix.is_none());
        assert!(
            diagnostic
                .notes
                .iter()
                .any(|note| note.kind == DiagnosticNoteKind::Help)
        );
    }

    #[test]
    fn skips_embedded_languages_and_literal_html_examples() {
        for input in [
            "---\ntitle: '<a id=\"literal>'\n---\n",
            "$x <a id=\"literal>$\n",
            "$$\n<a id=\"literal>\n$$\n",
            "`<a id=\"literal>`{=html}\n",
            "```{=html}\n<a id=\"literal>\n```\n",
            "[label](https://example.org \"<a id='literal>\")\n",
            "<pre>\n<a id=\"literal>\n</pre>\n",
        ] {
            assert!(check(input).is_empty(), "{input:?}");
        }
    }
}
