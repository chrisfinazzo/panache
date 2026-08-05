//! `unsupported-metadata-key`: flag a frontmatter mapping key that pandoc's
//! metadata layer refuses to convert.
//!
//! Pandoc reads YAML frontmatter with libyaml and then converts the result to a
//! `Meta` value, where every mapping key must be a string. libyaml itself is
//! happy with a collection key (`[a, b]: v`, `{x: 1}: v`, `? - a`⏎`  - b`⏎`: v`)
//! or an alias key (`*anchor: v`), so no YAML parse error fires --- pandoc
//! aborts a step later with
//!
//! ```text
//! Error parsing YAML metadata at (line 1, column 1):
//! Error in $: Non-string keys are not supported
//! ```
//!
//! which points at the start of the metadata block, not at the offending key,
//! and takes the whole document with it. This rule points at the key instead.
//!
//! Scope, measured against pandoc 3.9.0.2 through the markdown reader (see
//! `crates/panache-parser/tests/yaml/consumer-matrix.md`):
//!
//! - Non-string *scalar* keys are fine --- `1: one`, `no: nope`,
//!   `2024-01-01: launch` all convert, because pandoc stringifies scalar keys.
//!   So this is not a YAML 1.1 typing rule and it never overlaps
//!   `consumer-divergence`.
//! - Collection keys are rejected at **any** depth, including inside a
//!   top-level sequence.
//! - An alias key is rejected even when its anchor holds a plain scalar
//!   (`a: &x 1`⏎`*x : y` fails with `Non-string key alias`), so the rule flags
//!   every alias key without resolving the anchor.
//! - Frontmatter only. Hashpipe `#|` options are read by js-yaml/knitr, which
//!   have no such restriction, and never reach pandoc's metadata layer.
//! - A top-level frontmatter *scalar* or *sequence* is not this error: pandoc
//!   silently declines to treat the block as metadata and re-reads it as
//!   content. Panache already parses those as content too, so there is nothing
//!   to flag.
//!
//! No auto-fix: quoting the key (`'[a, b]': v`) keeps pandoc happy but invents
//! a key the author never wrote, and the common real-world trigger --- a link
//! reference definition (`[1]: https://example.com`) pasted inside the
//! frontmatter --- wants the line moved out of the header instead.

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxKind, SyntaxNode};

pub const UNSUPPORTED_METADATA_KEY: &str = "unsupported-metadata-key";

pub struct UnsupportedMetadataKeyRule;

impl Rule for UnsupportedMetadataKeyRule {
    fn name(&self) -> &str {
        UNSUPPORTED_METADATA_KEY
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: UNSUPPORTED_METADATA_KEY,
            default_on: true,
            requires: Requirement::PandocMetadata,
            auto_fix: false,
            codes: const { &[DiagnosticCode::error(UNSUPPORTED_METADATA_KEY)] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::YAML_METADATA]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for region in cx.nodes(SyntaxKind::YAML_METADATA) {
            for key in region.descendants().filter(|n| {
                matches!(
                    n.kind(),
                    SyntaxKind::YAML_BLOCK_MAP_KEY | SyntaxKind::YAML_FLOW_MAP_KEY
                )
            }) {
                if let Some(diagnostic) = classify(&key, cx.input) {
                    diagnostics.push(diagnostic);
                }
            }
        }
        diagnostics
    }
}

/// The diagnostic for a key pandoc's metadata layer rejects, or `None` for the
/// scalar keys it accepts.
fn classify(key: &SyntaxNode, input: &str) -> Option<Diagnostic> {
    if let Some(collection) = key.children().find(|child| is_collection(child.kind())) {
        let shape = describe(collection.kind());
        return Some(
            Diagnostic::error(
                Location::from_node(&collection, input),
                UNSUPPORTED_METADATA_KEY,
                format!("{shape} used as a metadata key is not supported by pandoc"),
            )
            .with_note(
                DiagnosticNoteKind::Note,
                "pandoc converts frontmatter to metadata, where every key must be a string; \
                 it fails the whole document with `Non-string keys are not supported`",
            )
            .with_note(
                DiagnosticNoteKind::Help,
                "quote the key to make it a string, or move the line out of the frontmatter \
                 if it was not meant to be metadata",
            ),
        );
    }

    // An alias key (`*anchor: v`) is rejected whatever the anchor holds, so the
    // anchor is never resolved. Aliases nested inside a collection key are
    // already covered by the branch above.
    let alias = key
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == SyntaxKind::YAML_ALIAS)?;

    Some(
        Diagnostic::error(
            Location::from_range(alias.text_range(), input),
            UNSUPPORTED_METADATA_KEY,
            format!(
                "YAML alias `{}` used as a metadata key is not supported by pandoc",
                alias.text()
            ),
        )
        .with_note(
            DiagnosticNoteKind::Note,
            "pandoc rejects an alias key even when the anchor holds a plain scalar, failing \
             the whole document with `Non-string key alias`",
        )
        .with_note(
            DiagnosticNoteKind::Help,
            "write the key out literally instead of aliasing it",
        ),
    )
}

fn is_collection(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::YAML_FLOW_SEQUENCE
            | SyntaxKind::YAML_FLOW_MAP
            | SyntaxKind::YAML_BLOCK_SEQUENCE
            | SyntaxKind::YAML_BLOCK_MAP
    )
}

fn describe(kind: SyntaxKind) -> &'static str {
    match kind {
        SyntaxKind::YAML_FLOW_MAP | SyntaxKind::YAML_BLOCK_MAP => "mapping",
        _ => "sequence",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Extensions, Flavor};

    fn lint(input: &str, config: &Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        UnsupportedMetadataKeyRule.check_tree(&tree, input, config, None)
    }

    fn config_for(flavor: Flavor) -> Config {
        Config {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            ..Default::default()
        }
    }

    #[test]
    fn flags_flow_sequence_key() {
        let input = "---\n[flow]: block\n---\n\nx\n";
        let diagnostics = lint(input, &config_for(Flavor::Quarto));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, UNSUPPORTED_METADATA_KEY);
        assert_eq!(diagnostics[0].severity, crate::linter::Severity::Error);
        // The span covers the key itself, not the `:` or the whole entry.
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "[flow]");
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn flags_flow_mapping_key() {
        let input = "---\n{a: 1}: v\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Pandoc));
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.starts_with("mapping used as"),
            "got {:?}",
            diagnostics[0].message
        );
    }

    #[test]
    fn flags_explicit_collection_key() {
        let input = "---\ntitle: t\n? [a, b]\n: v\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Pandoc));
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "[a, b]");
    }

    #[test]
    fn flags_explicit_block_sequence_key() {
        let input = "---\n? - a\n  - b\n: v\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Pandoc));
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.starts_with("sequence used as"),
            "got {:?}",
            diagnostics[0].message
        );
    }

    #[test]
    fn flags_nested_collection_key() {
        // pandoc rejects at any depth (`Error in $.keys`).
        let input = "---\ntitle: t\nkeys:\n  ? [a, b]\n  : v\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Quarto));
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn flags_collection_key_inside_flow_mapping() {
        let input = "---\nouter: {a: [b, c], [d, e]: f}\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Pandoc));
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "[d, e]");
    }

    #[test]
    fn flags_alias_key_even_when_anchor_is_a_scalar() {
        let input = "---\na: &x 1\n*x : y\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Pandoc));
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "*x");
        assert!(diagnostics[0].message.contains("`*x`"));
    }

    #[test]
    fn reports_an_anchored_collection_key_once() {
        let input = "---\n&a [x]: y\n---\n";
        let diagnostics = lint(input, &config_for(Flavor::Pandoc));
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].location.range;
        assert_eq!(&input[range], "[x]");
    }

    #[test]
    fn accepts_string_and_scalar_keys() {
        // Numbers, YAML 1.1 booleans, and dates are all stringified by pandoc.
        let input = "---\ntitle: t\n1: one\nno: nope\n2024-01-01: launch\n\
                     nested:\n  \"[a]\": quoted\nseq:\n  - a\n  - b\n---\n";
        assert!(lint(input, &config_for(Flavor::Quarto)).is_empty());
    }

    #[test]
    fn accepts_collections_in_values() {
        let input = "---\nok: [1, 2]\nalso: {a: 1}\nalias: &x [1]\nuse: *x\n---\n";
        assert!(lint(input, &config_for(Flavor::Pandoc)).is_empty());
    }

    #[test]
    fn ignores_hashpipe_options() {
        // `#|` options are read by js-yaml/knitr, never by pandoc's metadata
        // layer, so a collection key there is not this error.
        let input = "```{r}\n#| \"[a]\": b\n1 + 1\n```\n";
        assert!(lint(input, &config_for(Flavor::Quarto)).is_empty());
    }

    #[test]
    fn ignores_document_without_frontmatter() {
        assert!(lint("# Title\n\nBody.\n", &config_for(Flavor::Pandoc)).is_empty());
    }
}
