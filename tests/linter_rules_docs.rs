//! Keep the generated reference complete, reproducible, and true to behavior.

use std::collections::BTreeSet;
use std::path::Path;

use panache::linter::docs::{lint_example, render_catalog};
use panache::linter::fix::apply_fixes;
use panache::linter::{FixSafety, all_rules};

#[test]
fn every_rule_has_a_description_and_executable_examples() {
    for rule in all_rules() {
        assert!(
            !rule.description().trim().is_empty(),
            "{} has no description",
            rule.name()
        );
        assert!(
            !rule.examples().is_empty(),
            "{} has no examples",
            rule.name()
        );
    }
}

#[test]
fn documented_examples_emit_the_expected_codes() {
    for rule in all_rules() {
        let meta = rule.metadata();
        let mut covered = BTreeSet::new();
        let mut demonstrated_fix = false;
        for example in rule.examples() {
            assert!(
                !example.expected_codes.is_empty(),
                "{} has no expected codes",
                rule.name()
            );
            let diagnostics = lint_example(rule.as_ref(), example)
                .unwrap_or_else(|error| panic!("{}: {error}", rule.name()));
            let actual: BTreeSet<_> = diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect();
            let expected: BTreeSet<_> = example.expected_codes.iter().copied().collect();
            assert_eq!(
                actual,
                expected,
                "example for {}: {}",
                rule.name(),
                example.source
            );
            covered.extend(expected);
            for diagnostic in &diagnostics {
                let code = meta
                    .codes
                    .iter()
                    .find(|code| code.code == diagnostic.code)
                    .unwrap_or_else(|| {
                        panic!(
                            "{} emitted undeclared code {}",
                            rule.name(),
                            diagnostic.code
                        )
                    });
                assert_eq!(diagnostic.severity, code.severity, "{}", diagnostic.code);
                if diagnostic.fix.is_some() {
                    assert!(meta.auto_fix, "{} offers an undeclared fix", rule.name());
                    demonstrated_fix = true;
                }
            }
        }
        let declared: BTreeSet<_> = meta.codes.iter().map(|code| code.code).collect();
        assert_eq!(
            covered,
            declared,
            "{} has codes without examples",
            rule.name()
        );
        assert_eq!(
            demonstrated_fix,
            meta.auto_fix,
            "{} has no example of its fix",
            rule.name()
        );
    }
}

#[test]
fn fix_examples_respect_safety_and_resolve_the_demonstrated_issue() {
    for rule in all_rules() {
        for example in rule.examples() {
            let diagnostics = lint_example(rule.as_ref(), example).unwrap();
            let safe = apply_fixes(example.source, &diagnostics, false);
            if diagnostics
                .iter()
                .any(|diagnostic| diagnostic.fix.is_some())
                && diagnostics
                    .iter()
                    .filter_map(|diagnostic| diagnostic.fix.as_ref())
                    .all(|fix| fix.safety == FixSafety::Unsafe)
            {
                assert_eq!(
                    safe.output,
                    example.source,
                    "{} applied an unsafe fix",
                    rule.name()
                );
            }
            let fixed = apply_fixes(example.source, &diagnostics, example.allow_unsafe_fixes);
            if fixed.output == example.source {
                continue;
            }
            assert_eq!(
                fixed.conflicted,
                0,
                "{} has conflicting example fixes",
                rule.name()
            );
            let source = fixed.output;
            let fixed_diagnostics =
                panache::linter::docs::lint_example_source(rule.as_ref(), example, &source)
                    .unwrap();
            assert!(
                fixed_diagnostics.is_empty(),
                "{} remains after its fix: {fixed_diagnostics:?}",
                rule.name()
            );
        }
    }
}

#[test]
fn every_rule_and_diagnostic_code_has_a_stable_anchor() {
    let catalog = render_catalog().unwrap();
    for rule in all_rules() {
        let meta = rule.metadata();
        for name in std::iter::once(meta.name).chain(meta.codes.iter().map(|code| code.code)) {
            assert_eq!(
                catalog.matches(&format!("{{#{name}}}")).count(),
                1,
                "missing or duplicate anchor for {name}"
            );
        }
    }
}

#[test]
fn committed_catalog_matches_the_generator() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/reference/_linter-rules.qmd");
    let committed =
        std::fs::read_to_string(path).expect("run `task docs:rules` to generate the catalog");
    assert_eq!(
        committed,
        render_catalog().unwrap(),
        "rule reference is stale; run `task docs:rules`"
    );
    let reference = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/reference/linter-rules.qmd"),
    )
    .unwrap();
    assert!(reference.contains("{{< include _linter-rules.qmd >}}"));
}
