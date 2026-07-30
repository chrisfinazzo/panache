//! Regression tests locking in the structural-sharing property the incremental
//! reparser relies on: retained top-level blocks keep their `Arc` identity
//! across an edit, and unchanged blocks compare equal whether the document was
//! reparsed incrementally or from scratch.
//!
//! This property is what lets the downstream salsa analysis pipeline memoize
//! per-block: unchanged blocks are content-addressed cache hits. If a refactor
//! silently deep-copies subtrees (losing identity) or perturbs unchanged blocks,
//! these tests fail before the perf regression reaches salsa.

use panache_parser::SyntaxNode;
use panache_parser::parser::{parse, parse_incremental_suffix};
use rowan::{GreenNode, GreenNodeData, NodeOrToken};

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

/// Owned green subtrees of the top-level `DOCUMENT` children (nodes only).
fn blocks(tree: &SyntaxNode) -> Vec<GreenNode> {
    tree.green()
        .children()
        .filter_map(|child| match child.to_owned() {
            NodeOrToken::Node(node) => Some(node),
            NodeOrToken::Token(_) => None,
        })
        .collect()
}

/// The address of a green node's shared allocation. Two green handles pointing
/// at the same allocation (i.e. sharing structure) report the same address.
fn green_addr(node: &GreenNode) -> usize {
    let data: &GreenNodeData = node;
    data as *const GreenNodeData as usize
}

#[test]
fn incremental_suffix_retains_prefix_block_identity() {
    // No headings -> the suffix-window strategy, which retains a genuine prefix.
    let input = "para one\n\npara two\n\npara three\n\npara four\n\npara five\n";
    let old_tree = parse(input, None);
    let old_blocks = blocks(&old_tree);

    let start = input.find("five").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "FIVE");
    let new_edit = (start, start + 4);

    let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit);
    assert_eq!(
        inc.strategy, "suffix_window",
        "expected the suffix strategy"
    );

    let new_blocks = blocks(&inc.tree);
    let old_addrs: std::collections::HashSet<usize> = old_blocks.iter().map(green_addr).collect();
    let shared = new_blocks
        .iter()
        .filter(|block| old_addrs.contains(&green_addr(block)))
        .count();

    // Every block except the edited trailing paragraph must be pointer-shared.
    assert!(
        shared >= new_blocks.len() - 1,
        "expected all-but-one prefix blocks to share Arc identity, got {shared}/{}",
        new_blocks.len()
    );
    assert!(shared > 0, "no blocks shared identity");
}

#[test]
fn section_window_retains_surrounding_block_identity() {
    let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
    let old_tree = parse(input, None);
    let old_blocks = blocks(&old_tree);

    let start = input.find("beta").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "BETA");
    let new_edit = (start, start + 4);

    let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit);
    assert_eq!(
        inc.strategy, "section_window",
        "expected the section strategy"
    );

    let new_blocks = blocks(&inc.tree);
    let old_addrs: std::collections::HashSet<usize> = old_blocks.iter().map(green_addr).collect();

    // The section window reparses the whole edited section (its heading, blank
    // lines, and body), so those blocks are rebuilt. The guarantee is that
    // blocks in *other* sections keep their `Arc` identity: the leading `Intro`
    // section and the trailing `End` section are untouched.
    assert!(
        old_addrs.contains(&green_addr(&new_blocks[0])),
        "the first block (Intro heading) should be pointer-shared"
    );
    let last = new_blocks.last().expect("non-empty document");
    assert!(
        old_addrs.contains(&green_addr(last)),
        "the last block (End section body) should be pointer-shared"
    );
    let shared = new_blocks
        .iter()
        .filter(|block| old_addrs.contains(&green_addr(block)))
        .count();
    assert!(
        shared >= 7,
        "sections outside the edit should be pointer-shared, got {shared}/{}",
        new_blocks.len()
    );
}

#[test]
fn incremental_and_full_reparse_agree_block_for_block() {
    let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
    let old_tree = parse(input, None);

    let start = input.find("beta").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "BETA");
    let new_edit = (start, start + 4);

    let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit);
    let full = parse(&updated, None);

    let inc_blocks = blocks(&inc.tree);
    let full_blocks = blocks(&full);

    // Incremental reparse must produce a tree structurally identical to a full
    // reparse of the same text, block for block. This is the guarantee that
    // content-addressed salsa memoization stays correct under either strategy.
    assert_eq!(
        inc_blocks.len(),
        full_blocks.len(),
        "block count diverged between incremental and full reparse"
    );
    for (i, (a, b)) in inc_blocks.iter().zip(&full_blocks).enumerate() {
        assert_eq!(
            a, b,
            "block {i} diverged between incremental and full reparse"
        );
    }
    assert_eq!(inc.tree.to_string(), full.to_string());
}

#[test]
fn unchanged_blocks_compare_equal_across_edit() {
    // The load-bearing salsa property: editing one block leaves every other
    // block `==` to its pre-edit counterpart (so per-block memos hit).
    let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
    let old_tree = parse(input, None);
    let old_blocks = blocks(&old_tree);

    let start = input.find("beta").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "BETA");

    let full = parse(&updated, None);
    let new_blocks = blocks(&full);

    assert_eq!(old_blocks.len(), new_blocks.len());
    let differing = old_blocks
        .iter()
        .zip(&new_blocks)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 1,
        "exactly one block should differ after a single-block edit"
    );
}

#[test]
fn incremental_reparse_is_lossless() {
    let input = "para one\n\npara two\n\npara three\n\npara four\n\npara five\n";
    let old_tree = parse(input, None);

    let start = input.find("three").expect("marker present");
    let old_edit = (start, start + 5);
    let updated = apply_edit(input, old_edit, "THREE!!");
    let new_edit = (start, start + 7);

    let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit);
    assert_eq!(
        inc.tree.text().to_string(),
        updated,
        "incremental reparse must round-trip to the edited text"
    );
}
