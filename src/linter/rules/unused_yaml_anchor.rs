use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::linter::yaml_anchors::collect_document_anchors;
use crate::syntax::SyntaxKind;

/// Warns when a YAML anchor is declared but never referenced by any alias within
/// the same embedded YAML document. An unused anchor is valid YAML but dead
/// weight — mirrors yamllint's `anchors: forbid-unused-anchors`.
pub struct UnusedYamlAnchorRule;

impl Rule for UnusedYamlAnchorRule {
    fn name(&self) -> &str {
        "unused-yaml-anchor"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "unused-yaml-anchor",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("unused-yaml-anchor")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[
            SyntaxKind::YAML_METADATA,
            SyntaxKind::HASHPIPE_YAML_PREAMBLE,
        ]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let regions: Vec<&_> = cx
            .nodes(SyntaxKind::YAML_METADATA)
            .iter()
            .chain(cx.nodes(SyntaxKind::HASHPIPE_YAML_PREAMBLE).iter())
            .collect();

        let mut diagnostics = Vec::new();
        for doc in collect_document_anchors(&regions) {
            for anchor in &doc.anchors {
                if !doc.used.contains(anchor.name.as_str()) {
                    diagnostics.push(Diagnostic::warning(
                        Location::from_range(anchor.range, cx.input),
                        "unused-yaml-anchor",
                        format!("YAML anchor `&{}` is never used", anchor.name),
                    ));
                }
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Extensions, Flavor};

    fn lint(input: &str, config: &Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        UnusedYamlAnchorRule.check_tree(&tree, input, config, None)
    }

    fn quarto_config() -> Config {
        Config {
            flavor: Flavor::Quarto,
            extensions: Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        }
    }

    #[test]
    fn flags_unused_anchor_in_frontmatter() {
        let input = "---\nx: &a 1\n---\n";
        let diagnostics = lint(input, &Config::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-yaml-anchor");
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "&a");
    }

    #[test]
    fn accepts_referenced_anchor() {
        let input = "---\nx: &a 1\ny: *a\n---\n";
        assert!(lint(input, &Config::default()).is_empty());
    }

    #[test]
    fn flags_unused_anchor_in_hashpipe() {
        let input = "```{r}\n#| a: &x 1\n1 + 1\n```\n";
        let diagnostics = lint(input, &quarto_config());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-yaml-anchor");
        assert!(diagnostics[0].message.contains("`&x`"));
    }

    #[test]
    fn ignores_document_without_frontmatter() {
        assert!(lint("# Title\n\nBody.\n", &Config::default()).is_empty());
    }
}
