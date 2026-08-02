use rowan::{TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::metadata::{DocumentMetadata, inline_reference_contains};
use crate::parser::inlines::citations::suppressed_bare_citation;

/// Warns when a bare `@key` is glued to the preceding word so pandoc leaves it
/// as literal text instead of a citation, *and* `key` is a defined citation key
/// (a bibliography entry or an inline YAML `references:` entry). Gating on the
/// document's reference list keeps the rule quiet for email addresses and other
/// `@` text that was never meant to be a citation. See issue #448.
pub struct UnspacedCitationRule;

impl Rule for UnspacedCitationRule {
    fn name(&self) -> &str {
        "unspaced-citation"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "unspaced-citation",
            default_on: true,
            requires: Requirement::Citations,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("unspaced-citation")] },
        }
    }

    fn wants_text_tokens(&self) -> bool {
        true
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        if !cx.config.extensions.citations {
            return Vec::new();
        }
        // The gate is the document's defined citation keys; with no metadata
        // there is no reference list to check against.
        let Some(metadata) = cx.metadata else {
            return Vec::new();
        };

        let input = cx.input;
        let mut diagnostics = Vec::new();

        for token in cx.text_tokens() {
            let token_start: usize = token.text_range().start().into();
            // `@` is ASCII, so the byte offset from `match_indices` is a valid
            // absolute position in `input`.
            for (offset, _) in token.text().match_indices('@') {
                let at = token_start + offset;
                // Only bare `@key` occurrences that pandoc's `notAfterString`
                // rule suppressed (glued to a preceding word char) are
                // candidates; a recognized citation is not text and never
                // reaches here.
                let Some((len, key, _)) = suppressed_bare_citation(input, at) else {
                    continue;
                };
                if !key_is_defined(metadata, key) {
                    continue;
                }

                let range = TextRange::at(TextSize::from(at as u32), TextSize::from(len as u32));
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(range, input),
                    "unspaced-citation",
                    format!(
                        "Citation '@{key}' is glued to the preceding text and will not be \
                         recognized; separate it with a space or wrap it in brackets '[@{key}]'"
                    ),
                ));
            }
        }

        diagnostics
    }
}

/// Whether `key` is a citation key the document defines: an entry in a loaded
/// bibliography or an inline YAML `references:` entry.
fn key_is_defined(metadata: &DocumentMetadata, key: &str) -> bool {
    if metadata
        .bibliography_parse
        .as_ref()
        .and_then(|parse| parse.index.get(key))
        .is_some()
    {
        return true;
    }
    inline_reference_contains(&metadata.inline_references, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::metadata::{CitationInfo, DocumentMetadata, InlineReference};
    use std::path::PathBuf;

    /// Document metadata whose only defined citation keys are the given inline
    /// `references:` ids.
    fn metadata_with_keys(keys: &[&str]) -> DocumentMetadata {
        DocumentMetadata {
            source_path: PathBuf::from("test.qmd"),
            bibliography: None,
            metadata_files: Vec::new(),
            bibliography_parse: None,
            inline_references: keys
                .iter()
                .map(|id| InlineReference {
                    id: (*id).to_string(),
                    range: TextRange::default(),
                    path: PathBuf::from("test.qmd"),
                })
                .collect(),
            citations: CitationInfo { keys: Vec::new() },
            title: None,
            raw_yaml: String::new(),
        }
    }

    fn lint(input: &str, metadata: &DocumentMetadata) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        UnspacedCitationRule.check_tree(&tree, input, &config, Some(metadata))
    }

    #[test]
    fn flags_glued_key_defined_in_references() {
        let input = "See work@doe99 for this.";
        let diagnostics = lint(input, &metadata_with_keys(&["doe99"]));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unspaced-citation");
        assert!(diagnostics[0].message.contains("@doe99"));
        // The span covers exactly the glued `@doe99`.
        let start: usize = diagnostics[0].location.range.start().into();
        let end: usize = diagnostics[0].location.range.end().into();
        assert_eq!(&input[start..end], "@doe99");
    }

    #[test]
    fn flags_glued_key_defined_in_bibliography() {
        use crate::bib::{BibEntry, BibFormat, BibIndex, Span};
        use std::collections::HashMap;

        let mut entries = HashMap::new();
        entries.insert(
            "doe99".to_string(),
            BibEntry {
                key: "doe99".to_string(),
                entry_type: Some("article".to_string()),
                fields: HashMap::new(),
                source_file: PathBuf::from("refs.bib"),
                span: Span { start: 0, end: 0 },
                format: BibFormat::BibTeX,
            },
        );
        let mut metadata = metadata_with_keys(&[]);
        metadata.bibliography_parse = Some(crate::metadata::BibliographyParse {
            index: BibIndex {
                entries,
                duplicates: Vec::new(),
                errors: Vec::new(),
                load_errors: Vec::new(),
            },
            parse_errors: Vec::new(),
        });

        let diagnostics = lint("built on work@doe99 here.", &metadata);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unspaced-citation");
    }

    #[test]
    fn ignores_email_with_undefined_key() {
        // `example.com` is not a defined key, so the email stays quiet even
        // though another key is defined.
        let input = "Contact me at user@example.com for details.";
        let diagnostics = lint(input, &metadata_with_keys(&["doe99"]));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_properly_spaced_citation() {
        // `@doe99` after a space is a real citation (not text), so it never
        // reaches the rule even though the key is defined.
        let input = "As shown in @doe99, this holds.";
        let diagnostics = lint(input, &metadata_with_keys(&["doe99"]));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_suppress_author_form_after_word() {
        // `word-@doe99` is a real citation (the `@` follows the `-`), so it is
        // not text and never reaches the rule.
        let input = "prefix-@doe99 stands.";
        let diagnostics = lint(input, &metadata_with_keys(&["doe99"]));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn no_metadata_means_no_warnings() {
        // Without a reference list there is nothing to resolve against.
        let config = Config::default();
        let input = "See work@doe99 here.";
        let tree = crate::parser::parse(input, Some(config.clone()));
        let diagnostics = UnspacedCitationRule.check_tree(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn flags_each_glued_occurrence() {
        let input = "a@doe99 then b@doe99.";
        let diagnostics = lint(input, &metadata_with_keys(&["doe99"]));
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|d| d.code == "unspaced-citation"));
    }
}
