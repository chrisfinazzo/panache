use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::project_index::collect_usage_labels;
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::SyntaxKind;

pub struct UnusedDefinitionsRule;

impl Rule for UnusedDefinitionsRule {
    fn name(&self) -> &str {
        "unused-definitions"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "unused-definitions",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const {
                &[
                    DiagnosticCode::warning("unused-definition-label"),
                    DiagnosticCode::warning("unused-footnote-id"),
                ]
            },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[
            SyntaxKind::LINK,
            SyntaxKind::IMAGE_LINK,
            SyntaxKind::UNRESOLVED_REFERENCE,
            SyntaxKind::FOOTNOTE_REFERENCE,
        ]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let index = cx.symbol_index();
        // Local usages (this document only); a definition used in any sibling
        // document of the same project also counts. Check the shared project
        // usage aggregate by membership rather than copying it into `used` per
        // file (that copy was an O(n^2) hot spot on large projects).
        let used = collect_usage_labels(
            cx.nodes(SyntaxKind::LINK)
                .iter()
                .chain(cx.nodes(SyntaxKind::IMAGE_LINK).iter())
                .chain(cx.nodes(SyntaxKind::UNRESOLVED_REFERENCE).iter())
                .chain(cx.nodes(SyntaxKind::FOOTNOTE_REFERENCE).iter())
                .cloned(),
        );
        let project = cx.project_symbol_index();
        let project = project.as_deref();
        let reference_used = |label: &str| {
            used.reference_labels.contains(label)
                || project.is_some_and(|p| p.usage.reference_labels.contains(label))
        };
        let footnote_used = |id: &str| {
            used.footnote_ids.contains(id)
                || project.is_some_and(|p| p.usage.footnote_ids.contains(id))
        };

        let mut diagnostics = Vec::new();
        for (label, ranges) in index.reference_definition_entries() {
            if reference_used(label) {
                continue;
            }
            for range in ranges {
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(label_bracket_range(input, *range), input),
                    "unused-definition-label",
                    format!("Reference definition '[{}]' is never used", label),
                ));
            }
        }

        for (id, ranges) in index.footnote_definition_entries() {
            if footnote_used(id) {
                continue;
            }
            for range in ranges {
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(label_bracket_range(input, *range), input),
                    "unused-footnote-id",
                    format!("Footnote '[^{}]' is never used", id),
                ));
            }
        }

        diagnostics
    }
}

/// Narrow a definition's full-node range down to just its `[label]` (or
/// `[^id]`) bracket span, so the diagnostic underlines the label rather than
/// the whole definition line (destination URL or footnote body included).
///
/// Falls back to the original range if no bracket pair is found. Honors
/// backslash escapes so an escaped `\]` inside the label doesn't close it.
fn label_bracket_range(input: &str, range: rowan::TextRange) -> rowan::TextRange {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    let Some(slice) = input.get(start..end) else {
        return range;
    };
    let Some(open) = slice.find('[') else {
        return range;
    };
    let bytes = slice.as_bytes();
    let mut i = open + 1;
    let mut escaped = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if !escaped => escaped = true,
            b']' if !escaped => {
                let abs_start = (start + open) as u32;
                let abs_end = (start + i + 1) as u32;
                return rowan::TextRange::new(abs_start.into(), abs_end.into());
            }
            _ => escaped = false,
        }
        i += 1;
    }
    range
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
        let rule = UnusedDefinitionsRule;
        rule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn reports_unused_reference_definition() {
        let input =
            "[used]: https://example.com\n[unused]: https://example.org\n\nSee [x][used].\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
        assert!(diagnostics[0].message.contains("[unused]"));
    }

    #[test]
    fn reports_unused_footnote_definition() {
        let input = "Text with footnote[^1].\n\n[^1]: Used.\n[^2]: Unused.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-footnote-id");
        assert!(diagnostics[0].message.contains("[^2]"));
    }

    #[test]
    fn accepts_definition_used_by_full_reference_image() {
        let input = "![This is an image][image-path]\n\n[image-path]: https://example.com/i.png\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "full reference image should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn accepts_definition_used_by_collapsed_reference_image() {
        let input = "![image-path][]\n\n[image-path]: https://example.com/i.png\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "collapsed reference image should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn accepts_definition_used_by_shortcut_reference_image() {
        let input = "![image-path]\n\n[image-path]: https://example.com/i.png\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "shortcut reference image should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn accepts_definitions_used_by_reference_image_inside_reference_link() {
        let input = "[![example][example-badge]][example-url]\n\n[example-badge]: https://example.com\n[example-url]: https://example.com\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "reference image nested inside a reference link should count both labels as used: {diagnostics:?}"
        );
    }

    #[test]
    fn still_reports_unused_definition_with_only_inline_image() {
        let input = "![alt](https://example.com/i.png)\n\n[unused]: https://example.org\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
        assert!(diagnostics[0].message.contains("[unused]"));
    }

    #[test]
    fn accepts_used_shortcut_reference_definition() {
        let input = "See [Label].\n\n[Label]: https://example.com\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_shortcut_reference_with_code_span_label() {
        // `[`insta`]` is a shortcut reference link whose label is a code span.
        // Its raw label matches the `[`insta`]:` definition, so the definition
        // is used, not unused. The usage side must compare on raw label text,
        // not rendered text (which would drop the code span and mismatch).
        let input = "[`insta`]\n\n[`insta`]: https://insta.rs/\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "code-span shortcut label should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn unused_definition_span_covers_only_the_label() {
        let input = "[unused]: https://example.org\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(
            &input[range], "[unused]",
            "span should cover only the label"
        );
    }

    #[test]
    fn unused_footnote_span_covers_only_the_label() {
        let input = "[^2]: Unused.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "[^2]", "span should cover only the label");
    }

    #[test]
    fn does_not_report_unused_definition_when_used_in_project_document() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        let doc1 = root.join("1-one.Rmd");
        let doc2 = root.join("2-two.Rmd");
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(&doc1, "[shared]: https://example.com\n").expect("write doc1");
        fs::write(&doc2, "See [x][shared].\n").expect("write doc2");

        let input = fs::read_to_string(&doc1).expect("read doc1");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &doc1).expect("metadata");

        let rule = UnusedDefinitionsRule;
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_unused_definition_when_not_used_in_project_document() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        let doc1 = root.join("1-one.Rmd");
        let doc2 = root.join("2-two.Rmd");
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(&doc1, "[shared]: https://example.com\n").expect("write doc1");
        fs::write(&doc2, "Plain text.\n").expect("write doc2");

        let input = fs::read_to_string(&doc1).expect("read doc1");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &doc1).expect("metadata");

        let rule = UnusedDefinitionsRule;
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
    }

    #[test]
    fn falls_back_to_local_behavior_without_project_root() {
        let temp = TempDir::new().expect("tempdir");
        let doc = temp.path().join("standalone.qmd");
        fs::write(&doc, "[alone]: https://example.com\n").expect("write doc");

        let input = fs::read_to_string(&doc).expect("read doc");
        let config = Config::default();
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &doc).expect("metadata");

        let rule = UnusedDefinitionsRule;
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
    }
}
