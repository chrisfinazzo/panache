use super::helpers::*;
use lsp_types::*;
use panache::syntax::{SyntaxKind, Table};

fn actions(server: &TestLspServer, uri: &str, line: u32, character: u32) -> Vec<CodeAction> {
    server
        .get_code_actions(uri, line, character, line, character)
        .unwrap()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Convert to simple table"
                    || action.title == "Convert to multiline table" =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect()
}

fn byte_offset(text: &str, position: Position) -> usize {
    let start: usize = text
        .split_inclusive('\n')
        .take(position.line as usize)
        .map(str::len)
        .sum();
    let mut utf16 = 0;
    for (offset, ch) in text[start..].char_indices() {
        if utf16 == position.character {
            return start + offset;
        }
        utf16 += ch.len_utf16() as u32;
    }
    text.len()
}

fn apply(text: &str, action: &CodeAction) -> String {
    let edits = action
        .edit
        .as_ref()
        .unwrap()
        .changes
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    let mut result = text.to_string();
    result.replace_range(
        byte_offset(text, edit.range.start)..byte_offset(text, edit.range.end),
        &edit.new_text,
    );
    result
}

#[test]
fn offers_two_refactors_and_preserves_surroundings() {
    let mut server = TestLspServer::new();
    let text = "Before 😀.\n\n| A | B |\n|---|---|\n| one | two |\n\nAfter.\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 4, 4);
    assert_eq!(actions.len(), 2);
    for action in actions {
        assert_eq!(
            action.kind.as_ref(),
            Some(&CodeActionKind::REFACTOR_REWRITE)
        );
        let result = apply(text, &action);
        assert!(result.starts_with("Before 😀.\n\n"));
        assert!(result.ends_with("\nAfter.\n"));
        let tree = panache::parse(&result, None);
        let table = tree.descendants().find_map(Table::cast).unwrap();
        assert_eq!(
            table.syntax().kind(),
            if action.title.contains("multiline") {
                SyntaxKind::MULTILINE_TABLE
            } else {
                SyntaxKind::SIMPLE_TABLE
            }
        );
    }
}

#[test]
fn preserves_nested_prefixes_and_crlf() {
    let mut server = TestLspServer::new();
    let text = "- > | A | B |\r\n  > |---|---|\r\n  > | 😀 | 界 |\r\n\nAfter.\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 2, 8);
    assert_eq!(actions.len(), 2);
    for action in actions {
        let result = apply(text, &action);
        assert!(result.starts_with("- > "));
        assert!(result.contains("\r\n  > "));
        assert!(result.ends_with("\nAfter.\n"));
        let tree = panache::parse(&result, None);
        let table = tree.descendants().find_map(Table::cast).unwrap();
        assert!(
            table
                .syntax()
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::LIST_ITEM)
        );
        assert!(
            table
                .syntax()
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::BLOCK_QUOTE)
        );
    }
}

#[test]
fn omits_unsupported_actions_and_current_style() {
    let mut server = TestLspServer::new();
    for (text, expected) in [
        ("| A | B |\n|---|---|\n| x |\n", 0),
        ("A     B\n----- -----\none   two\n", 1),
    ] {
        server.open_document("file:///table.qmd", text, "quarto");
        assert_eq!(actions(&server, "file:///table.qmd", 0, 1).len(), expected);
        server.close_document("file:///table.qmd");
    }
}

#[test]
fn no_actions_for_selection_spanning_tables_or_cursor_outside() {
    let mut server = TestLspServer::new();
    let text = "| A | B |\n|---|---|\n| one | two |\n\n| C | D |\n|---|---|\n| three | four |\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let response = server
        .get_code_actions("file:///table.qmd", 0, 0, 6, 0)
        .unwrap();
    assert!(response.iter().all(|action| !matches!(action, CodeActionOrCommand::CodeAction(action) if action.title.contains("table"))));
    assert!(actions(&server, "file:///table.qmd", 3, 0).is_empty());
}

#[test]
fn reports_unsupported_reasons_only_to_capable_clients() {
    let text = "| A | B |\n|---|---|\n| x |\n";
    let mut server = TestLspServer::new();
    server.initialize_disabled_code_actions("file:///workspace");
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 2, 1);
    assert_eq!(actions.len(), 2);
    for action in actions {
        assert!(action.edit.is_none());
        assert!(
            action
                .disabled
                .unwrap()
                .reason
                .contains("number of columns")
        );
    }
}

#[test]
fn reports_disabled_extensions_in_gfm() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("panache.toml"), "flavor = \"gfm\"\n").unwrap();
    let uri = Uri::from_file_path(dir.path().join("table.md")).unwrap();
    let mut server = TestLspServer::new();
    server.initialize_disabled_code_actions(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.open_document(
        uri.as_str(),
        "| A | B |\n|---|---|\n| x | y |\n",
        "markdown",
    );
    let actions = actions(&server, uri.as_str(), 0, 2);
    assert_eq!(actions.len(), 2);
    for action in actions {
        assert!(action.edit.is_none());
        assert!(
            action
                .disabled
                .unwrap()
                .reason
                .contains("extension is disabled")
        );
    }
}

#[test]
fn converts_at_caption_and_document_boundaries() {
    let samples = [
        (
            ": Caption 😀 {#tbl-id}\n\n| A | B |\n|---|---|\n| one | two |\n",
            0,
            2,
        ),
        (
            "| A | B |\n|---|---|\n| one | two |\n\n: Caption 😀 {#tbl-id}",
            4,
            2,
        ),
        ("| A | B |\n|---|---|\n| one | two |", 0, 0),
    ];
    for (text, line, col) in samples {
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", text, "quarto");
        let actions = actions(&server, "file:///table.qmd", line, col);
        assert_eq!(actions.len(), 2, "{text}");
        for action in actions {
            let result = apply(text, &action);
            assert_eq!(result.ends_with('\n'), text.ends_with('\n'));
            if text.contains("Caption") {
                assert!(result.contains("Caption 😀 {#tbl-id}"));
            }
        }
    }
}

#[test]
fn keeps_tables_in_their_containers() {
    let samples = [
        "- Item.\n\n  | A | B |\n  |---|---|\n  | one | two |\n\n- Sibling.\n",
        "> | A | B |\n> |---|---|\n> | one | two |\n\nAfter.\n",
        "::: {.box}\n\n| A | B |\n|---|---|\n| one | two |\n\n:::\n",
        "[^note]:\n    | A | B |\n    |---|---|\n    | one | two |\n\nAfter.\n",
        "Term\n:   Description.\n\n    | A | B |\n    |---|---|\n    | one | two |\n\nAfter.\n",
    ];
    for text in samples {
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", text, "quarto");
        let (line, content) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("one"))
            .unwrap();
        let actions = actions(
            &server,
            "file:///table.qmd",
            line as u32,
            content.find("one").unwrap() as u32,
        );
        assert_eq!(actions.len(), 2, "{text}");
        for action in actions {
            let result = apply(text, &action);
            let original = panache::parse(text, None);
            let parsed = panache::parse(&result, None);
            let parent_kinds = |tree: &panache::syntax::SyntaxNode| {
                tree.descendants()
                    .find_map(Table::cast)
                    .unwrap()
                    .syntax()
                    .ancestors()
                    .skip(1)
                    .map(|node| node.kind())
                    .collect::<Vec<_>>()
            };
            assert_eq!(parent_kinds(&original), parent_kinds(&parsed), "{result}");
        }
    }
}

#[test]
fn honors_code_action_kind_filter() {
    let mut server = TestLspServer::new();
    server.open_document(
        "file:///table.qmd",
        "| A | B |\n|---|---|\n| x | y |\n",
        "quarto",
    );
    for (kind, expected) in [
        (CodeActionKind::QUICKFIX, 0),
        (CodeActionKind::REFACTOR, 2),
        (CodeActionKind::REFACTOR_REWRITE, 2),
    ] {
        let response = server
            .get_code_actions_with_context(
                "file:///table.qmd",
                Range::default(),
                CodeActionContext {
                    diagnostics: vec![],
                    only: Some(vec![kind]),
                    trigger_kind: None,
                },
            )
            .unwrap();
        assert_eq!(response.iter().filter(|action| matches!(action, CodeActionOrCommand::CodeAction(action) if action.title.contains("table"))).count(), expected);
    }
}

#[test]
fn declines_marker_line_layouts_that_cannot_preserve_the_container() {
    let mut server = TestLspServer::new();
    server.initialize_disabled_code_actions("file:///workspace");
    let text = "- | A | B |\n  |---|---|\n  | one | two |\n\n- Sibling.\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 0, 4);
    assert_eq!(actions.len(), 2);
    for action in actions {
        assert!(action.edit.is_none());
        assert!(action.disabled.unwrap().reason.contains("container"));
    }
}
