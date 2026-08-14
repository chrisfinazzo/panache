use super::helpers::{
    assert_block_kinds, count_children, find_all, find_first, parse_blocks,
    parse_blocks_pandoc_3_9, parse_blocks_with_config,
};
use crate::options::{Extensions, Flavor, ParserOptions};
use crate::syntax::{SyntaxKind, SyntaxNode};

#[test]
fn simple_bullet_list() {
    let input = "* one\n* two\n* three\n";
    let config = ParserOptions {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    assert!(
        config.extensions.fenced_divs,
        "fenced_divs should be enabled for this test"
    );
    let tree = parse_blocks_with_config(input, &config);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn bullet_list_requires_space_after_marker() {
    let input = "*one\n*two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn bullet_list_with_different_markers() {
    let input = "* item\n+ item\n- item\n";
    let tree = parse_blocks(input);
    // Should create ONE list (bullet markers are all equivalent per Pandoc)
    let lists = find_all(&tree, SyntaxKind::LIST);
    assert_eq!(lists.len(), 1);
}

#[test]
fn bullet_list_indented_1_to_3_spaces() {
    let input = " * one space\n  * two spaces\n   * three spaces\n";
    let tree = parse_blocks(input);
    // All should be valid list items
    let list_items = find_all(&tree, SyntaxKind::LIST_ITEM);
    assert_eq!(list_items.len(), 3);
}

#[test]
fn bullet_list_indented_4_spaces_is_code() {
    let input = "    * not a list\n";
    let tree = parse_blocks(input);
    // Should be code block, not list
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn bullet_list_with_continuation() {
    let input = "* here is my first\n  list item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn bullet_list_lazy_continuation() {
    let input = "* here is my first\nlist item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn list_item_can_start_with_atx_heading() {
    let input = "- # Heading\n";
    let tree = parse_blocks(input);

    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    let list_item = list
        .children()
        .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .expect("should find list item");

    assert!(
        find_first(&list_item, SyntaxKind::HEADING).is_some(),
        "list item should contain HEADING"
    );
    assert!(
        find_first(&list_item, SyntaxKind::PLAIN).is_none(),
        "heading-only list item should not be parsed as PLAIN"
    );
}

#[test]
fn nested_bullet_lists() {
    let input = "* fruits\n  + apples\n  + pears\n* vegetables\n";
    let tree = parse_blocks(input);
    let outer_list = find_first(&tree, SyntaxKind::LIST).expect("should find outer list");
    assert_eq!(count_children(&outer_list, SyntaxKind::LIST_ITEM), 2);

    // Should have nested list inside first item
    let nested_lists = find_all(&tree, SyntaxKind::LIST);
    assert!(
        nested_lists.len() >= 2,
        "should have at least 2 lists (outer + nested)"
    );
}

#[test]
fn outdented_item_after_nested_list_returns_to_outer_level() {
    let input = "* Item 1\n  + Nested item\n      *  Deeply nested\n +  Item 2\n";
    let tree = parse_blocks(input);
    let lists = find_all(&tree, SyntaxKind::LIST);

    let outer_list = lists.first().expect("should have an outer list");
    assert_eq!(count_children(outer_list, SyntaxKind::LIST_ITEM), 2);

    let top_level_items: Vec<_> = outer_list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    let first_item = top_level_items
        .first()
        .expect("should have first list item");
    let second_item = top_level_items
        .get(1)
        .expect("should have second list item");

    assert!(
        find_first(first_item, SyntaxKind::LIST).is_some(),
        "first item should keep nested list"
    );
    assert!(
        find_first(second_item, SyntaxKind::LIST).is_none(),
        "second item should be at outer level, not nested"
    );
}

/// Pinned to the pandoc 3.9 target: the nested lists here start at `iv.` and
/// `(A)`, which pandoc 3.10 reads as paragraph text (jgm/pandoc#11735). The
/// indented-code question this case guards only arises once they nest.
#[test]
fn fancy_list_continuation_with_nested_list_is_not_indented_code() {
    use crate::options::{Extensions, PandocCompat, ParserOptions};

    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        pandoc_compat: PandocCompat::V3_9,
        ..Default::default()
    };
    let input = "(2) begins with 2\n(3) and now 3\n\n    with a continuation\n\n    iv. sublist with roman numerals,\n        starting with 4\n    v.  more items\n        (A)  a subsublist\n        (B)  a subsublist\n";

    let tree = crate::parser::Parser::new(input, &config).parse();

    assert!(
        find_first(&tree, SyntaxKind::CODE_BLOCK).is_none(),
        "continuation content should not parse as indented code"
    );

    let lists = find_all(&tree, SyntaxKind::LIST);
    assert!(
        lists.len() >= 3,
        "should contain outer, nested roman, and nested alpha lists"
    );
}

#[test]
fn loose_list_with_blank_lines() {
    let input = "* one\n\n* two\n\n* three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn simple_ordered_list() {
    let input = "1. one\n2. two\n3. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn ordered_list_numbers_ignored() {
    let input = "5. one\n7. two\n1. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn ordered_list_with_hash_marker() {
    let input = "#. one\n#. two\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn ordered_list_requires_space_after_marker() {
    let input = "1.one\n2.two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn mixed_markers_create_separate_lists() {
    let input = "(2) Two\n(5) Three\n1. Four\n* Five\n";
    let tree = parse_blocks(input);
    // Should create separate lists for each marker type
    let lists = find_all(&tree, SyntaxKind::LIST);
    assert!(lists.len() >= 3, "should have at least 3 separate lists");
}

#[test]
fn parenthesized_decimal_with_only_closer_text_does_not_interrupt_list_item() {
    let input = "- outer\n  4) )\n  continued\n";
    let tree = parse_blocks(input);

    let lists = find_all(&tree, SyntaxKind::LIST);
    assert_eq!(lists.len(), 1, "should keep a single outer list");
    let outer = lists.first().expect("outer list");
    assert_eq!(count_children(outer, SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn task_list_unchecked() {
    let input = "- [ ] unchecked task\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn task_list_checked() {
    let input = "- [x] checked task\n- [X] also checked\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn list_with_multiple_paragraphs() {
    let input = "* First paragraph.\n\n  Continued.\n\n* Second item.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn list_after_blank_line() {
    let input = "\n* item\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list after blank");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn list_after_paragraph() {
    let input = "Not a list.\n\n* Now a list\n";
    assert_block_kinds(
        input,
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::LIST,
        ],
    );
}

#[test]
fn list_item_with_valid_fenced_divs_parses_as_fenced_div_nodes() {
    let input = "2.  Once your repository is created, clone it to your local computer.\n\n    ::: {.content-visible unless-meta=\"tool.is_rstudio\"}\n    You can do this any way you are comfortable, for instance in the Terminal, it might look like:\n\n    ``` {.bash filename=\"Terminal\"}\n    git clone git@github.com:<username>/<repo-name>.git\n    ```\n\n    Where you use your own user name and repo name.\n    :::\n\n    ::: {.content-visible when-meta=\"tool.is_rstudio\"}\n    You can do this any way you are comfortable, but one approach is to use **File** > **New Project**. In the **New Project** dialog, select **From Version Control**, then **Git**, and copy and paste the repo URL from GitHub.\n    :::\n";
    let tree = parse_blocks(input);
    let list_item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
    let fenced_divs = find_all(&list_item, SyntaxKind::FENCED_DIV);
    assert_eq!(
        fenced_divs.len(),
        2,
        "expected two fenced divs inside list item"
    );
}

// Fancy lists tests - require fancy_lists extension

#[test]
fn fancy_list_lower_alpha_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a. first\nb. second\nc. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_alpha_right_paren() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a) first\nb) second\nc) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_alpha_parens() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(a) first\n(b) second\n(c) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_alpha_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "A.  first\nB.  second\nC.  third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_alpha_period_requires_two_spaces() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // One space should NOT parse as list (to avoid false positives like "B. Russell")
    let input = "A. first\nB. second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());

    // Two spaces SHOULD parse as list
    let input_valid = "A.  first\nB.  second\n";
    let tree_valid = crate::parser::Parser::new(input_valid, &config).parse();
    let list = find_first(&tree_valid, SyntaxKind::LIST).expect("should find list with 2 spaces");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn fancy_list_lower_roman_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "i. first\nii. second\niii. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_roman_right_paren() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "i) first\nii) second\niii) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_roman_parens() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(i) first\n(ii) second\n(iii) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_roman_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // Pandoc requires 2 spaces after a single-character upper Roman + period
    // (avoids confusion with initials like "I. M. Pei"). Multi-character
    // markers like `II.` only need 1 space.
    let input = "I.  first\nII. second\nIII. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_roman_period_single_char_one_space_rejected() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "I. first\nII. second\nIII. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(
        find_first(&tree, SyntaxKind::LIST).is_none(),
        "single-character upper Roman + period needs 2 spaces; should fall back to paragraph"
    );
}

#[test]
fn fancy_list_upper_roman_right_paren() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "I) first\nII) second\nIII) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_disabled_when_extension_off() {
    // With fancy_lists disabled, alphabetic markers should not parse as lists
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a. first\nb. second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_hash_marker_disabled_when_extension_off() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "#. first\n#. second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_right_paren_decimal_disabled_when_extension_off() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "1) first\n2) second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_parens_decimal_disabled_when_extension_off() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(1) first\n(2) second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_complex_roman() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input =
        "iv. fourth\nv. fifth\nvi. sixth\nvii. seventh\nviii. eighth\nix. ninth\nx. tenth\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 7);
}

// Example lists tests - require example_lists extension

#[test]
fn example_list_basic() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) First example\n(@) Second example\n(@) Third example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn example_list_with_labels() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@good) This is a good example\n(@bad) This is a bad example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn example_list_mixed_labeled_unlabeled() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) First example\n(@foo) Labeled example\n(@) Another example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn example_list_separated_by_text() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // According to spec, example lists can be separated and continue numbering
    let input = "(@) First example\n\nSome text.\n\n(@) Second example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let lists = find_all(&tree, SyntaxKind::LIST);
    // Should have 2 separate lists
    assert_eq!(lists.len(), 2);
    // Each should have 1 item
    assert_eq!(count_children(&lists[0], SyntaxKind::LIST_ITEM), 1);
    assert_eq!(count_children(&lists[1], SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn example_list_disabled_when_extension_off() {
    // With example_lists disabled, (@) should not parse as a list
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn example_list_with_underscores_and_hyphens() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@my_label) Example with underscore\n(@my-label) Example with hyphen\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn nested_lower_roman_with_uneven_marker_width_stays_single_nested_list() {
    let input = "a. retain.\n\n     i. short;\n    ii. short;\n   iii. short;\n";
    let tree = parse_blocks(input);

    let outer = find_first(&tree, SyntaxKind::LIST).expect("should find outer list");
    assert_eq!(count_children(&outer, SyntaxKind::LIST_ITEM), 1);

    let outer_item = outer
        .children()
        .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .expect("outer list should contain one item");

    let nested_lists: Vec<_> = outer_item
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST)
        .collect();
    assert_eq!(
        nested_lists.len(),
        1,
        "nested roman items should stay in one nested list"
    );
    assert_eq!(count_children(&nested_lists[0], SyntaxKind::LIST_ITEM), 3);
}

/// Count LIST_ITEM ancestors of a node (nesting depth of item content).
fn list_item_depth(node: &SyntaxNode) -> usize {
    node.ancestors()
        .filter(|a| a.kind() == SyntaxKind::LIST_ITEM)
        .count()
}

#[test]
fn horizontal_rule_in_depth_two_list_item() {
    // pandoc (both -f markdown and -f commonmark): HorizontalRule nested in
    // the inner item. The item indent (4 columns) must not trip the
    // CommonMark 4-space guard in `try_parse_horizontal_rule`.
    let input = "- outer\n\n  - inner\n\n    ***\n\n    deep\n";
    let tree = parse_blocks(input);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("nested rule should parse as HORIZONTAL_RULE");
    assert_eq!(list_item_depth(&hr), 2, "rule should sit in the inner item");
}

#[test]
fn spaced_dash_rule_after_list_is_sibling() {
    // pandoc (both dialects): a spaced dash run after a blank line is a
    // thematic break, never a new list item (`bulletListStart` runs
    // `notFollowedBy' hrule`; CommonMark 4.1 gives the break precedence).
    // The list must close so the rule is the document's block, not the
    // list's child.
    let input = "- a\n- b\n\n- - - -\n";
    let tree = parse_blocks(input);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("spaced dash run should parse as HORIZONTAL_RULE");
    assert_eq!(list_item_depth(&hr), 0, "rule must be a list sibling");
    assert!(
        hr.ancestors().all(|a| a.kind() != SyntaxKind::LIST),
        "rule must not nest inside the LIST"
    );
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn spaced_star_rule_after_list_is_sibling() {
    // Same precedence rule for the `*` bullet/rule collision.
    let input = "* a\n* b\n\n* * *\n";
    let tree = parse_blocks(input);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("spaced star run should parse as HORIZONTAL_RULE");
    assert!(
        hr.ancestors().all(|a| a.kind() != SyntaxKind::LIST),
        "rule must not nest inside the LIST"
    );
}

#[test]
fn spaced_dash_rule_at_item_content_col_stays_in_item() {
    // pandoc (both dialects): `BulletList [[Para "a", HorizontalRule]]` --- a
    // rule indented to the item's content column belongs to the item.
    let input = "- a\n\n  - - - -\n";
    let tree = parse_blocks(input);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("indented dash run should parse as HORIZONTAL_RULE");
    assert_eq!(list_item_depth(&hr), 1, "rule should sit in the item");
}

#[test]
fn deeply_indented_spaced_dash_rule_stays_in_item_without_sublist() {
    // pandoc (both dialects): still `BulletList [[Para "a", HorizontalRule]]`.
    // Four raw columns are two effective columns inside the item, so the line
    // is a rule there --- not a sublist whose first item swallows a `- - -`.
    let input = "- a\n\n    - - - -\n";
    let tree = parse_blocks(input);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("indented dash run should parse as HORIZONTAL_RULE");
    assert_eq!(list_item_depth(&hr), 1, "rule should sit in the item");
    assert_eq!(
        find_all(&tree, SyntaxKind::LIST).len(),
        1,
        "no sublist must open for the rule line"
    );
}

#[test]
fn spaced_dash_rule_without_blank_line_is_lazy_text() {
    // pandoc -f markdown: a rule cannot interrupt a paragraph, and the line
    // is not a sibling item either, so it folds into item b's text
    // (`Plain [Str "b", SoftBreak, Str "-", ...]`).
    let input = "- a\n- b\n- - - -\n";
    let tree = parse_blocks(input);
    assert!(
        find_first(&tree, SyntaxKind::HORIZONTAL_RULE).is_none(),
        "rule must not interrupt the item's paragraph under pandoc"
    );
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(
        count_children(&list, SyntaxKind::LIST_ITEM),
        2,
        "dash run must not open a third item"
    );
}

#[test]
fn spaced_dash_rule_without_blank_line_interrupts_under_commonmark() {
    // pandoc -f commonmark: a thematic break interrupts the paragraph and
    // ends the list --- the dialect counterpart of the lazy-text test above.
    let input = "- a\n- b\n- - - -\n";
    let config = ParserOptions {
        flavor: Flavor::CommonMark,
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        dialect: crate::Dialect::CommonMark,
        ..Default::default()
    };
    let tree = parse_blocks_with_config(input, &config);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("spaced dash run should interrupt the paragraph under CommonMark");
    assert!(
        hr.ancestors().all(|a| a.kind() != SyntaxKind::LIST),
        "rule must be a sibling of the closed LIST"
    );
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn atx_heading_in_depth_two_list_item() {
    // pandoc (both dialects): Header nested in the inner item.
    let input = "- outer\n\n  - inner\n\n    # head\n";
    let tree = parse_blocks(input);
    let heading =
        find_first(&tree, SyntaxKind::HEADING).expect("nested ATX heading should parse as HEADING");
    assert_eq!(list_item_depth(&heading), 2);
}

#[test]
fn atx_heading_then_text_in_quoted_list_item() {
    // pandoc (both dialects): BlockQuote [BulletList [[Header 1, Plain]]].
    // The item's buffer holds the continuation line's `>` marker as a
    // structural segment; the multi-line ATX split must see past it and
    // re-inject the `>` bytes into the trailing block.
    let input = "> - # h\n>   text\n";
    let tree = parse_blocks(input);
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("should find list item");
    let heading = find_first(&item, SyntaxKind::HEADING)
        .expect("quoted item's ATX heading should parse as HEADING");
    let trailing = heading
        .next_sibling()
        .expect("heading should have trailing sibling block");
    assert!(
        matches!(trailing.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH),
        "trailing text should be a separate block, got {:?}",
        trailing.kind()
    );
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn html_block_then_text_in_quoted_list_item() {
    // pandoc: BlockQuote [BulletList [[Div [Plain "foo"], Plain "after"]]].
    // Same marker-segment shape as the ATX case, through the HTML lift.
    let input = "> - <div>foo</div>\n>   after\n";
    let tree = parse_blocks(input);
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("should find list item");
    let div = find_first(&item, SyntaxKind::HTML_BLOCK_DIV)
        .expect("quoted item's matched-pair div should lift to HTML_BLOCK_DIV");
    let trailing = div
        .next_sibling()
        .expect("div should have trailing sibling block");
    assert!(
        matches!(trailing.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH),
        "trailing text should be a separate block, got {:?}",
        trailing.kind()
    );
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn atx_heading_then_text_in_depth_two_list_item() {
    // pandoc (both dialects): Header followed by a separate Para/Plain,
    // both inside the inner item (multi-line buffer chunk).
    let input = "- outer\n\n  - inner\n\n    # head\n    more\n";
    let tree = parse_blocks(input);
    let heading =
        find_first(&tree, SyntaxKind::HEADING).expect("nested ATX heading should parse as HEADING");
    assert_eq!(list_item_depth(&heading), 2);
    let trailing = heading
        .next_sibling()
        .expect("heading should have trailing sibling block");
    assert!(
        matches!(trailing.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH),
        "trailing text should be a separate block, got {:?}",
        trailing.kind()
    );
}

#[test]
fn horizontal_rule_three_extra_spaces_in_list_item() {
    // Content column + up to 3 extra spaces is still a thematic break in
    // pandoc (both dialects): `- item` with a rule indented 5 columns.
    let input = "- item\n\n     ---\n\n  text\n";
    let tree = parse_blocks(input);
    let hr = find_first(&tree, SyntaxKind::HORIZONTAL_RULE)
        .expect("rule at content_col + 3 should parse as HORIZONTAL_RULE");
    assert_eq!(list_item_depth(&hr), 1);
}

#[test]
fn horizontal_rule_four_extra_spaces_in_list_item_is_not_a_rule() {
    // Content column + 4 spaces is indented-code territory in pandoc, not a
    // thematic break; detection must not claim it after indent stripping.
    let input = "- item\n\n      ---\n\n  text\n";
    let tree = parse_blocks(input);
    assert!(
        find_first(&tree, SyntaxKind::HORIZONTAL_RULE).is_none(),
        "rule at content_col + 4 must not parse as HORIZONTAL_RULE"
    );
}

#[test]
fn continuation_indent_is_stripped_from_inline_code_content() {
    // pandoc's `listLine` gobbles the item's content column off every
    // continuation line, so the code span reads `x  y` (one space from the
    // newline, one left over from the 3-column indent), not `x    y`.
    let input = "- a\n   `x\n   y`\n";
    let tree = parse_blocks(input);
    let code = find_first(&tree, SyntaxKind::INLINE_CODE).expect("should find inline code");
    let content: String = code
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::INLINE_CODE_CONTENT)
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(content, "x\n y");
    assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
}

#[test]
fn ordered_marker_at_content_column_opens_nested_list_under_pandoc_3_9() {
    // Under the 3.9 target `pandoc -f markdown -t native` nests every one of
    // these as an `OrderedList` inside the outer item; only the outer item's
    // content column matters, not the marker's number or delimiter style.
    for input in [
        "1.  a\n    10.  b\n",
        "1.  a\n    2.  b\n",
        "1.  a\n    (b)  b\n",
        "1.  a\n    (2)  b\n",
    ] {
        let tree = parse_blocks_pandoc_3_9(input);
        let lists = find_all(&tree, SyntaxKind::LIST);
        assert_eq!(
            lists.len(),
            2,
            "marker at the outer item's content column should nest: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    }
}

/// Pandoc 3.10.1 requires an ordered sublist to start at 1 — or its
/// equivalent in the marker's own numbering — so every shape above is now
/// paragraph text instead (jgm/pandoc#11735). Each string below was checked
/// against `pandoc -f markdown -t native` 3.10.2.
#[test]
fn ordered_sublist_must_start_at_one_under_pandoc_3_10() {
    for input in [
        "1.  a\n    10.  b\n",
        "1.  a\n    2.  b\n",
        "1.  a\n    (b)  b\n",
        "1.  a\n    (2)  b\n",
        "-   a\n\n    iv. b\n",
        "-   a\n\n    C.  b\n",
    ] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::LIST).len(),
            1,
            "sublist starting past 1 must stay paragraph text: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    }
}

/// The restriction is about *starting* a sublist, not about the numbers a
/// list may run through: a marker that continues an already-open list is
/// still a sibling item whatever its number.
#[test]
fn ordered_sublist_start_rule_leaves_sibling_items_alone() {
    for (input, expected_lists) in [
        ("1. a\n2. b\n", 1),
        ("- item\n  1. a\n  2. b\n", 2),
        ("2. top level\n3. still fine\n", 1),
        ("> 2. quoted top level\n> 3. fine\n", 1),
    ] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::LIST).len(),
            expected_lists,
            "sibling continuation must keep its list: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    }
}

/// Start-1 equivalents in every numbering style still open a sublist, as do
/// the auto-numbered markers, which pandoc always reports as starting at 1.
#[test]
fn ordered_sublist_starting_at_one_still_nests() {
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    for input in [
        "-   a\n\n    1. b\n",
        "-   a\n\n    i. b\n",
        "-   a\n\n    a. b\n",
        "-   a\n\n    A.  b\n",
        "-   a\n\n    (1) b\n",
        "-   a\n\n    #. b\n",
        "-   a\n\n    (@) b\n",
    ] {
        let tree = parse_blocks_with_config(input, &config);
        assert_eq!(
            find_all(&tree, SyntaxKind::LIST).len(),
            2,
            "a sublist starting at 1 must still nest: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    }
}

/// A marker indented past its list's base column but short of the open item's
/// content column is a sibling item of that list, not a new nested list.
/// The blank line has already closed the item, so there is nothing left to
/// nest into — emitting a `LIST` there would make it a direct child of a
/// `LIST`, a shape the pandoc-ast projector drops wholesale. Checked against
/// `pandoc -f markdown -t native`, which reads `Drifted` as a second item of
/// the inner list.
#[test]
fn drifted_marker_short_of_content_col_is_a_sibling_item() {
    for input in [
        "a. Grant\n\n   1. One\n\n    2. Drifted\n",
        "- Grant\n\n  - One\n\n   - Drifted\n",
    ] {
        let tree = parse_blocks(input);
        let lists = find_all(&tree, SyntaxKind::LIST);
        assert_eq!(
            lists.len(),
            2,
            "a drifted marker must not open a third list: {input:?}"
        );
        assert_eq!(
            count_children(&lists[1], SyntaxKind::LIST_ITEM),
            2,
            "the drifted marker belongs to the inner list: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    }
}

/// A `LIST` is never a direct child of a `LIST`; every list nests through a
/// `LIST_ITEM`. Guards the whole drift band around a closed item's list.
#[test]
fn no_list_is_a_direct_child_of_a_list() {
    for input in [
        "a. Grant\n\n   1. One\n\n    2. Drifted\n",
        "a. Grant\n\n   1. One\n\n     2. Drifted\n",
        "a. Grant\n\n   1. One\n\n      1. Deeper\n",
        "- Grant\n\n  - One\n\n   - Drifted\n",
        "- Grant\n\n  - One\n\n    - Deeper\n",
    ] {
        let tree = parse_blocks(input);
        for list in find_all(&tree, SyntaxKind::LIST) {
            assert!(
                list.parent().is_none_or(|p| p.kind() != SyntaxKind::LIST),
                "LIST directly inside LIST: {input:?}"
            );
        }
        assert_eq!(tree.text().to_string(), input, "parse must stay lossless");
    }
}
