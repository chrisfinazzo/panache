use std::collections::HashSet;

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::linter::yaml_anchors::collect_document_anchors;
use crate::syntax::SyntaxKind;

/// Warns when the same YAML anchor name is declared more than once within a
/// single embedded YAML document. This is valid YAML 1.2 (the last definition
/// wins), but a repeated anchor is almost always an accident — mirrors
/// yamllint's `anchors: forbid-duplicated-anchors`.
pub struct DuplicateYamlAnchorRule;

impl Rule for DuplicateYamlAnchorRule {
    fn name(&self) -> &str {
        "duplicate-yaml-anchor"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "duplicate-yaml-anchor",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("duplicate-yaml-anchor")] },
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
            let mut seen: HashSet<&str> = HashSet::new();
            for anchor in &doc.anchors {
                // The first declaration is fine; only re-declarations are flagged.
                if !seen.insert(anchor.name.as_str()) {
                    diagnostics.push(Diagnostic::warning(
                        Location::from_range(anchor.range, cx.input),
                        "duplicate-yaml-anchor",
                        format!("YAML anchor `&{}` is defined more than once", anchor.name),
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
        DuplicateYamlAnchorRule.check_tree(&tree, input, config, None)
    }

    fn quarto_config() -> Config {
        Config {
            flavor: Flavor::Quarto,
            extensions: Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        }
    }

    #[test]
    fn flags_duplicate_anchor_in_frontmatter() {
        let input = "---\nx: &a 1\ny: &a 2\n---\n";
        let diagnostics = lint(input, &Config::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-yaml-anchor");
        // Span covers the second `&a`, not the first.
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "&a");
        assert_eq!(usize::from(range.start()), input.find("&a 2").unwrap());
    }

    #[test]
    fn accepts_distinct_anchor_names() {
        let input = "---\nx: &a 1\ny: &b 2\n---\n";
        assert!(lint(input, &Config::default()).is_empty());
    }

    #[test]
    fn accepts_single_anchor() {
        let input = "---\nx: &a 1\ny: *a\n---\n";
        assert!(lint(input, &Config::default()).is_empty());
    }

    #[test]
    fn flags_duplicate_anchor_in_hashpipe() {
        let input = "```{r}\n#| a: &x 1\n#| b: &x 2\n1 + 1\n```\n";
        let diagnostics = lint(input, &quarto_config());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-yaml-anchor");
        assert!(diagnostics[0].message.contains("`&x`"));
    }

    #[test]
    fn ignores_document_without_frontmatter() {
        assert!(lint("# Title\n\nBody.\n", &Config::default()).is_empty());
    }
}
