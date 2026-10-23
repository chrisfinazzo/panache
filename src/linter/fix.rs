//! Apply diagnostic fixes consistently in the CLI and documentation examples.

/// The result of rewriting a document with its diagnostics' fixes.
pub struct AppliedFixes {
    pub output: String,
    /// Fixes skipped because they overlapped an already-applied fix.
    pub conflicted: usize,
}

/// Apply the fixes attached to `diagnostics` to `input`.
///
/// Fixes can genuinely overlap: an external linter may report two rules for
/// the same span (ruff emits both `I001` and `F401` for an unused import,
/// each rewriting the whole import line). Overlapping edits cannot both be
/// applied in one pass, so a fix whose edits land inside already-rewritten
/// text is skipped wholesale --- never partially, since a fix's edits are
/// only meaningful together --- and counted for the caller to report. The
/// skipped fixes apply on a subsequent run, once the first round settled.
pub fn apply_fixes(
    input: &str,
    diagnostics: &[crate::linter::Diagnostic],
    allow_unsafe: bool,
) -> AppliedFixes {
    use crate::linter::FixSafety;
    use crate::linter::diagnostics::Edit;

    let mut fixes: Vec<Vec<&Edit>> = diagnostics
        .iter()
        .filter_map(|d| d.fix.as_ref())
        .filter(|f| allow_unsafe || f.safety == FixSafety::Safe)
        .map(|f| {
            let mut edits: Vec<&Edit> = f.edits.iter().collect();
            edits.sort_by_key(|e| e.range.start());
            edits
        })
        .filter(|edits| !edits.is_empty())
        .collect();

    // Order by where each fix first touches the document so the greedy pass
    // below walks forward. Ties keep the order the rules produced them in.
    fixes.sort_by_key(|edits| edits[0].range.start());

    let mut accepted: Vec<&Edit> = Vec::new();
    let mut covered_end = 0usize;
    let mut conflicted = 0usize;

    for edits in fixes {
        let first_start: usize = edits[0].range.start().into();
        if first_start < covered_end {
            conflicted += 1;
            continue;
        }
        let fix_end: usize = edits
            .iter()
            .map(|e| usize::from(e.range.end()))
            .max()
            .unwrap_or(first_start);
        covered_end = fix_end.max(covered_end);
        accepted.extend(edits);
    }

    accepted.sort_by_key(|e| e.range.start());

    let mut output = String::new();
    let mut last_end = 0usize;

    for edit in accepted {
        let start: usize = edit.range.start().into();
        let end: usize = edit.range.end().into();

        // Edits within a single fix are expected to be disjoint; guard anyway
        // so a malformed fix cannot panic the whole run.
        if start < last_end {
            continue;
        }

        output.push_str(&input[last_end..start]);
        output.push_str(&edit.replacement);
        last_end = end;
    }

    output.push_str(&input[last_end..]);
    AppliedFixes { output, conflicted }
}
