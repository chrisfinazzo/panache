use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::project_index::{DefinitionLabels, extend_labels_from_tree};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{
    AstNode, Crossref, FootnoteReference, Link, SyntaxKind, SyntaxNode, UnresolvedReference,
};
use crate::utils::{crossref_resolution_labels, normalize_anchor_label, normalize_label};

pub struct UndefinedReferencesRule;

impl Rule for UndefinedReferencesRule {
    fn name(&self) -> &str {
        "undefined-references"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "undefined-references",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const {
                &[
                    DiagnosticCode::warning("undefined-reference-label"),
                    DiagnosticCode::warning("undefined-footnote-id"),
                ]
            },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[
            SyntaxKind::LINK,
            SyntaxKind::UNRESOLVED_REFERENCE,
            SyntaxKind::FOOTNOTE_REFERENCE,
            SyntaxKind::CROSSREF,
        ]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let (input, config) = (cx.input, cx.config);
        let mut diagnostics = Vec::new();

        // Build only the *current* document's definitions (cheap, O(doc)); check
        // cross-file resolution against the shared project aggregate by
        // membership rather than copying it into a local set per file (that copy
        // was O(project) per file — an O(n^2) hot spot on large projects).
        let doc_symbols = cx.symbol_index();
        let mut labels = DefinitionLabels::default();
        extend_labels_from_tree(&mut labels, cx.tree, config, &doc_symbols);
        let project = cx.project_symbol_index();
        let project = project.as_deref();

        let reference_defined = |normalized: &str| {
            labels.reference_labels.contains(normalized)
                || labels.heading_text_labels.contains(normalized)
                || project.is_some_and(|p| {
                    p.definitions.reference_labels.contains(normalized)
                        || p.definitions.heading_text_labels.contains(normalized)
                })
        };
        let footnote_defined = |normalized: &str| {
            labels.footnote_ids.contains(normalized)
                || project.is_some_and(|p| p.definitions.footnote_ids.contains(normalized))
        };
        let crossref_defined = |candidate: &str| {
            labels.crossref_labels.contains(candidate)
                || project.is_some_and(|p| p.definitions.crossref_labels.contains(candidate))
        };

        for link in cx
            .nodes(SyntaxKind::LINK)
            .iter()
            .cloned()
            .filter_map(Link::cast)
        {
            if link.dest().is_some() {
                continue;
            }

            let Some((label_text, location_node)) = extract_reference_label_and_node(&link) else {
                continue;
            };
            let normalized_label = normalize_label(&label_text);
            if normalized_label.is_empty() || reference_defined(&normalized_label) {
                continue;
            }

            diagnostics.push(Diagnostic::warning(
                Location::from_node(&location_node, input),
                "undefined-reference-label",
                format!("Reference label '[{}]' not found", label_text),
            ));
        }

        // Bracket-shape patterns whose label didn't resolve at parse
        // time emit `UNRESOLVED_REFERENCE` (Pandoc dialect; see
        // `crates/panache-parser/src/parser/inlines/inline_ir.rs`).
        // Image-shape variants (`![alt][missing]`) flow through the
        // same wrapper; flag them too.
        for unresolved in cx
            .nodes(SyntaxKind::UNRESOLVED_REFERENCE)
            .iter()
            .cloned()
            .filter_map(UnresolvedReference::cast)
        {
            // A `[^`-opened bracket is a reversed inline-footnote marker, not a
            // reference label; `reversed-footnote-marker` says so precisely.
            // Same flavor scoping as the footnote branch below.
            let start = usize::from(unresolved.syntax().text_range().start());
            if config.extensions.inline_footnotes && input[start..].starts_with("[^") {
                continue;
            }

            let Some((label_text, location_node)) = extract_unresolved_label_and_node(&unresolved)
            else {
                continue;
            };
            let normalized_label = normalize_label(&label_text);
            if normalized_label.is_empty() || reference_defined(&normalized_label) {
                continue;
            }
            let prefix = if unresolved.is_image() { "![" } else { "[" };
            diagnostics.push(Diagnostic::warning(
                Location::from_node(&location_node, input),
                "undefined-reference-label",
                format!("Reference label '{prefix}{label_text}]' not found"),
            ));
        }

        for footnote_ref in cx
            .nodes(SyntaxKind::FOOTNOTE_REFERENCE)
            .iter()
            .cloned()
            .filter_map(FootnoteReference::cast)
        {
            let id = footnote_ref.id();

            // Under pandoc a whitespace-bearing label never reads as a footnote
            // reference at all --- the bracket degrades to text, a link, or a
            // citation --- so "footnote not found" would be the wrong claim.
            // `reversed-footnote-marker` owns that shape. The guard is scoped to
            // flavors with inline footnotes because GFM and MyST route `[^id]`
            // through markdown-it, which does accept spaces in the label.
            if config.extensions.inline_footnotes && id.chars().any(char::is_whitespace) {
                continue;
            }

            let normalized = normalize_label(&id);
            if normalized.is_empty() || footnote_defined(&normalized) {
                continue;
            }

            diagnostics.push(Diagnostic::warning(
                Location::from_node(footnote_ref.syntax(), input),
                "undefined-footnote-id",
                format!("Footnote '[^{}]' not found", id),
            ));
        }

        for crossref in cx
            .nodes(SyntaxKind::CROSSREF)
            .iter()
            .cloned()
            .filter_map(Crossref::cast)
        {
            for key in crossref.keys() {
                let label = key.text();
                let normalized = normalize_anchor_label(&label);
                if normalized.is_empty() {
                    continue;
                }

                // Custom (extension-injected) crossref prefixes are managed by
                // an extension panache can't model — e.g. pseudocode's `@algo-`,
                // whose `#| label:` lives in a non-executable fence we don't
                // parse as a chunk option. Treat the reference as a crossref but
                // don't try to validate its target, which we'd never resolve.
                if crate::parser::inlines::citations::has_custom_crossref_prefix(
                    &label,
                    &config.crossref_prefixes,
                ) {
                    continue;
                }

                let candidates =
                    crossref_resolution_labels(&normalized, config.extensions.bookdown_references);
                if candidates
                    .iter()
                    .any(|candidate| crossref_defined(candidate))
                {
                    continue;
                }
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(key.text_range(), input),
                    "undefined-reference-label",
                    format!("Cross-reference label '@{}' not found", label),
                ));
            }
        }

        diagnostics
    }
}

fn extract_reference_label_and_node(link: &Link) -> Option<(String, SyntaxNode)> {
    if let Some(link_ref) = link.reference() {
        let label = link_ref.label();
        if !label.trim().is_empty() {
            return Some((label, link_ref.syntax().clone()));
        }
    }

    link.text()
        .map(|text| (text.text_content(), link.syntax().clone()))
}

/// Mirror of [`extract_reference_label_and_node`] for the
/// `UnresolvedReference` wrapper. Full / collapsed forms expose the
/// label via `LinkRef`; shortcut form has no `LinkRef` and uses the
/// inner text as the label.
fn extract_unresolved_label_and_node(
    unresolved: &UnresolvedReference,
) -> Option<(String, SyntaxNode)> {
    if let Some(label) = unresolved.label()
        && !label.trim().is_empty()
    {
        // Locate the LINK_REF child for the diagnostic location.
        let link_ref_node = unresolved
            .syntax()
            .children()
            .find(|c| c.kind() == crate::syntax::SyntaxKind::LINK_REF)
            .unwrap_or_else(|| unresolved.syntax().clone());
        return Some((label, link_ref_node));
    }
    let text = unresolved.text();
    if text.trim().is_empty() {
        return None;
    }
    Some((text, unresolved.syntax().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};
    use std::fs;
    use tempfile::TempDir;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        rule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn reports_missing_reference_labels() {
        let input = "Text with [link][missing].\n\n[ok]: https://example.com\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-reference-label");
        assert!(diagnostics[0].message.contains("[missing]"));
    }

    #[test]
    fn reports_missing_footnotes() {
        let input = "Text with footnote[^missing].\n\n[^ok]: Defined.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-footnote-id");
        assert!(diagnostics[0].message.contains("[^missing]"));
    }

    #[test]
    fn accepts_collapsed_and_shortcut_reference_links() {
        let input = "Collapsed [GitHub][] and shortcut [Wiki].\n\n[GitHub]: https://github.com\n[Wiki]: https://wikipedia.org\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_implicit_heading_references() {
        let input = "# Heading Name\n\nSee [Heading Name].\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn implicit_heading_references_require_auto_identifiers() {
        let input = "# Heading Name\n\nSee [Heading Name].\n";
        let mut config = Config::default();
        config.extensions.implicit_header_references = true;
        config.extensions.auto_identifiers = false;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-reference-label");
    }

    #[test]
    fn accepts_quarto_crossref_to_table_caption_attribute() {
        // Pandoc's `+caption_attributes` lifts a trailing `{#tbl-id}` from the
        // caption text into the Table's outer attribute, so `@tbl-id` resolves.
        let input = "@tbl-glm\n\n  | Model |\n  | :---- |\n  | A     |\n\n  : {#tbl-glm}\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "undefined-reference-label"),
            "@tbl-glm should resolve via the caption's {{#tbl-glm}} attribute, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn accepts_quarto_crossref_to_display_math_attribute_no_blank_line() {
        // `$$...$$ {#eq-id}\n@eq-id` (no blank line after the closing fence)
        // should still register the equation id and resolve the reference.
        let input = "$$\na = b\n$$ {#eq-primal-problem}\n@eq-primal-problem\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "undefined-reference-label"),
            "@eq-primal-problem should resolve via $$...$${{#eq-id}} on the same line, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn accepts_quarto_crossref_to_chunk_label() {
        let input = "See @fig-plot.\n\n```{r}\n#| label: fig-plot\nplot(1:10)\n```\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_missing_quarto_crossref_label() {
        let input = "See @fig-missing.\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-reference-label");
        assert!(diagnostics[0].message.contains("@fig-missing"));
    }

    #[test]
    fn custom_crossref_prefix_target_not_validated() {
        // An extension-injected crossref prefix (`@algo-`) is recognized as a
        // crossref, but its target lives in a mechanism panache can't model
        // (a pseudocode fence's `#| label:`), so the rule must not flag it as
        // an undefined reference.
        let input = "See @algo-cd.\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        config.crossref_prefixes = vec!["algo".to_string()];
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_prefixed_crossref_to_chunk_label() {
        let input = "See \\@ref(fig:plot).\n\n```{r}\n#| label: plot\n#| fig-cap: \"Plot\"\nplot(1:10)\n```\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_table_caption_label_declaration() {
        // Bookdown registers `(\#tab:label)` inside a pipe-table caption as a
        // crossref target — same shape as the equation form, but for any
        // bookdown prefix. Mirrors the user-reported false positive on .Rmd.
        let input = "\\@ref(tab:moth-phenotype)).\n\n  | a   | b   |\n  | :-: | :-: |\n  |  c  |  d  |\n\n  : (\\#tab:moth-phenotype)\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(
            diagnostics.is_empty(),
            "tab:moth-phenotype should resolve via the table caption's (\\#tab:moth-phenotype) declaration, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn accepts_bookdown_theorem_environment_crossref() {
        let input = "Exercise \\@ref(exr:mu)\n\n::: {#mu .exercise}\nfoobar\n:::\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_equation_crossref_with_mixed_case_label() {
        let input =
            "\\begin{equation}\n  1 = 1\n  (\\#eq:solveG)\n\\end{equation}\n\n\\@ref(eq:solveG)\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        config.extensions.bookdown_equation_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_section_crossref_with_hyphenated_slug() {
        let input = "# Heading\n\nA ref to \\@ref(heading).\n\n## Heading 2\n\nA ref to \\@ref(heading-2).\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn resolves_bookdown_crossref_with_empty_bookdown_yml() {
        // When _bookdown.yml exists but is empty, bookdown auto-discovers all .Rmd files.
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(
            root.join("1-one.Rmd"),
            "---\ntitle: Test\n---\n# One {#one}\n",
        )
        .expect("write 1-one.Rmd");
        fs::write(root.join("2-two.Rmd"), "\\@ref(one)\n").expect("write 2-two.Rmd");

        let input = fs::read_to_string(root.join("2-two.Rmd")).expect("read 2-two.Rmd");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;

        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &root.join("2-two.Rmd"))
            .expect("metadata");
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "undefined-reference-label"),
            "empty _bookdown.yml should auto-discover .Rmd files in the project"
        );
    }

    #[test]
    fn resolves_reference_defined_in_quarto_project_document() {
        // A shortcut reference whose definition lives in a sibling Quarto
        // project document should resolve cross-file, mirroring bookdown.
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("_quarto.yml"), "project:\n  type: book\n").expect("write _quarto.yml");
        fs::write(root.join("defs.qmd"), "[shared]: https://example.com\n")
            .expect("write defs.qmd");
        fs::write(root.join("body.qmd"), "See [shared].\n").expect("write body.qmd");

        let path = root.join("body.qmd");
        let input = fs::read_to_string(&path).expect("read body.qmd");
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &path).expect("metadata");
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "undefined-reference-label"),
            "reference defined in a sibling Quarto document should resolve: {:?}",
            diagnostics
        );
    }
}
