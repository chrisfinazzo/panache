//! Tests for citation, reference label, and path completion.

use super::helpers::*;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit, Documentation,
    Position, Range, Uri,
};
use std::fs;
use tempfile::TempDir;

fn reference_completions(marked: &str, config: &str) -> Vec<CompletionItem> {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join(".git")).unwrap();
    fs::write(tmp.path().join("panache.toml"), config).unwrap();
    let uri = Uri::from_file_path(tmp.path().join("doc.md")).unwrap();
    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(tmp.path()).unwrap().as_str());
    let (before, after) = marked.split_once('|').expect("cursor marker");
    server.open_document(uri.as_str(), &format!("{before}{after}"), "markdown");
    let line = before.bytes().filter(|b| *b == b'\n').count() as u32;
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count() as u32;
    match server.completion(uri.as_str(), line, character) {
        Some(CompletionResponse::Array(items)) => items,
        None => Vec::new(),
        other => panic!("unexpected completion response: {other:?}"),
    }
}

#[test]
fn reference_completion_replaces_second_label() {
    for flavor in ["pandoc", "commonmark"] {
        for (usage, start, end, replacement) in [
            ("[text][|]", 7, 7, "Reference"),
            ("[text][re|]", 7, 9, "Reference"),
            ("[text][re|mainder]", 7, 16, "Reference"),
            ("[text][Reference|]", 7, 16, "Reference"),
            ("[Reference][re|]", 12, 14, "Reference"),
            ("[text][|", 7, 7, "Reference]"),
            ("[text][re|", 7, 9, "Reference]"),
            ("![alt][re|]", 7, 9, "Reference"),
            ("![alt][re|", 7, 9, "Reference]"),
            ("[a [nested] text][re|]", 18, 20, "Reference"),
            ("[a `]` text][re|]", 13, 15, "Reference"),
            ("[a \\] text][re|]", 12, 14, "Reference"),
        ] {
            let items = reference_completions(
                &format!("[Reference]: https://example.com\n\n{usage}"),
                &format!("flavor = \"{flavor}\"\n"),
            );
            assert_eq!(items.len(), 1, "{flavor}: {usage}");
            let item = &items[0];
            assert_eq!(item.label, "Reference");
            assert_eq!(item.kind, Some(CompletionItemKind::REFERENCE));
            assert_eq!(item.detail.as_deref(), Some("https://example.com"));
            let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
                panic!("expected a text edit: {usage}");
            };
            assert_eq!(
                edit.range,
                Range::new(Position::new(2, start), Position::new(2, end)),
                "{usage}"
            );
            assert_eq!(edit.new_text, replacement, "{usage}");
        }
    }
}

#[test]
fn reference_completion_normalizes_and_deduplicates_labels() {
    let definitions = "[Zebra]: /z\n[Research  Notes]: /first\n[research notes]: /duplicate\n[Résumé]: /unicode\n\n";
    let items = reference_completions(&format!("{definitions}[text][|]"), "");
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["Research  Notes", "Résumé", "Zebra"]
    );
    assert_eq!(items[0].detail.as_deref(), Some("/first"));
    for (prefix, expected) in [("RESEARCH\t n", "Research  Notes"), ("RÉ", "Résumé")] {
        let items = reference_completions(&format!("{definitions}[text][{prefix}|]"), "");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, expected);
    }
    assert!(reference_completions(&format!("{definitions}[text][absent|]"), "").is_empty());
}

#[test]
fn reference_completion_uses_utf16_edit_ranges() {
    let items = reference_completions("[Résumé]: /url\n\n🦀 [é][RÉ|sumé]", "");
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(
        edit.range,
        Range::new(Position::new(2, 7), Position::new(2, 13))
    );
    assert_eq!(edit.new_text, "Résumé");
}

#[test]
fn reference_completion_preserves_text_after_unfinished_label() {
    let items = reference_completions("[ref]: /url\n\n[text][re| and [another]", "");
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(
        edit.range,
        Range::new(Position::new(2, 7), Position::new(2, 9))
    );
    assert_eq!(edit.new_text, "ref]");
}

#[test]
fn reference_completion_flattens_multiline_definition_labels() {
    let items = reference_completions(
        "[Research\nNotes]: /url\n\n[text][re|]",
        "flavor = \"commonmark\"\n",
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Research Notes");
}

#[test]
fn reference_completion_finds_later_definitions_from_prose_containers() {
    for usage in [
        "# [text][re|]",
        "> [text][re|]",
        "- [text][re|]",
        "*[text][re|]*",
    ] {
        let items = reference_completions(&format!("{usage}\n\n[ref]: /url\n"), "");
        assert_eq!(items.len(), 1, "{usage}");
        assert_eq!(items[0].label, "ref");
    }
}

#[test]
fn reference_completion_excludes_other_contexts() {
    for flavor in ["pandoc", "commonmark"] {
        for usage in [
            "plain re|",
            "[re|]",
            "![re|]",
            "[text|][ref]",
            "![alt|][ref]",
            "[ref|]: /destination",
            "`[text][re|]`",
            "```\n[text][re|]\n```",
            "    [text][re|]",
            "<script>\n[text][re|]\n</script>",
            "<span title=\"[text][re|]\">",
            "[text](https://example.com/[text][re|])",
            "[outer [text][re|]](url)",
            "\\[text][re|]",
            "[text]\\[re|]",
            "`[text]`[re|]",
            "[text][ref][re|]",
            "[@cite][re|]",
            "[^note][re|]",
        ] {
            let items = reference_completions(
                &format!("[ref]: /url\n\n{usage}"),
                &format!("flavor = \"{flavor}\"\n"),
            );
            assert!(items.is_empty(), "{flavor}: {usage}: {items:?}");
        }
    }
    for usage in [
        "$[text][re|]$",
        "$$\n[text][re|]\n$$",
        "[@cite [text][re|]]",
    ] {
        assert!(
            reference_completions(&format!("[ref]: /url\n\n{usage}"), "").is_empty(),
            "{usage}"
        );
    }
    assert!(
        reference_completions("---\ntitle: '[text][re|]'\n---\n\n[ref]: /url\n", "").is_empty()
    );
}

#[test]
fn reference_completion_respects_extension_settings() {
    let marked = "[ref]: /url\n\n[text][re|]";
    assert!(reference_completions(marked, "[extensions]\nreference-links = false\n").is_empty());
    for gap in [" ", "\t", "\n"] {
        let marked = format!("[ref]: /url\n\n[text]{gap}[re|]");
        assert!(
            reference_completions(&marked, "[extensions]\nspaced-reference-links = false\n")
                .is_empty()
        );
        assert_eq!(
            reference_completions(&marked, "[extensions]\nspaced-reference-links = true\n").len(),
            1
        );
    }
}

#[test]
fn reference_completion_tracks_unsaved_definitions_in_current_document() {
    let mut server = TestLspServer::new();
    let uri = "untitled:reference-completion";
    server.open_document(
        "untitled:another-document",
        "[elsewhere]: /other",
        "markdown",
    );
    server.open_document(uri, "[first]: /first\n\n[text][", "markdown");
    let Some(CompletionResponse::Array(items)) = server.completion(uri, 2, 7) else {
        panic!("expected reference completion in an untitled document");
    };
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["first"]
    );
    server.edit_document(
        uri,
        vec![full_document_change("[second]: /second\n\n[text][")],
    );
    let Some(CompletionResponse::Array(items)) = server.completion(uri, 2, 7) else {
        panic!("expected updated reference completion");
    };
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["second"]
    );
}

#[test]
fn test_completion_without_citation_context() {
    let mut server = TestLspServer::new();

    // Open a document without citation context
    let content = "Just plain text.";
    server.open_document("file:///test.md", content, "markdown");

    // Request completion in plain text
    let result = server.completion("file:///test.md", 0, 5);

    // Should return None when not in citation context
    assert!(
        result.is_none(),
        "Should not provide completions outside citation context"
    );
}

#[test]
fn test_completion_in_citation_without_bibliography() {
    let mut server = TestLspServer::new();

    // Open a document with citation syntax but no bibliography configured
    let content = "Text with [@] citation.";
    server.open_document("file:///test.md", content, "markdown");

    // Request completion at @ position
    let result = server.completion(
        "file:///test.md",
        0,
        12, // Position after [@
    );

    // Should return None when no bibliography is configured
    assert!(
        result.is_none(),
        "Should not provide completions without bibliography"
    );
}

#[test]
fn test_completion_with_project_bibliography() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    fs::write(root.join("refs.bib"), "@book{known,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(doc_path).expect("doc uri");
    let content = "Text [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 0, 7);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "known"),
        "Expected bibliography key completion"
    );
}

#[test]
fn test_completion_preserves_bibliography_key_case() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    fs::write(root.join("refs.bib"), "@article{Eddelbuettel:2011,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(doc_path).expect("doc uri");
    let content = "Text [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 0, 7);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "Eddelbuettel:2011"
            && item.insert_text.as_deref() == Some("Eddelbuettel:2011")),
        "Expected completion to preserve original bibliography key casing"
    );
}

#[test]
fn test_completion_with_inline_references() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nreferences:\n  - id: inline\n    title: Inline\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 6, 7);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "inline"),
        "Expected inline reference completion"
    );
}

#[test]
fn test_completion_with_csl_yaml_bibliography() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("refs.yaml"), "- id: cslkey\n  title: Sample\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.yaml\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 4, 7);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "cslkey"),
        "Expected CSL YAML bibliography completion"
    );
}

#[test]
fn test_completion_with_csl_json_bibliography() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(
        root.join("refs.json"),
        "[{\"id\":\"cslkey\",\"title\":\"Sample\"}]",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.json\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 4, 7);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "cslkey"),
        "Expected CSL JSON bibliography completion"
    );
}

#[test]
fn test_completion_with_ris_bibliography() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("refs.ris"), "TY  - JOUR\nID  - riskey\nER  - \n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.ris\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 4, 7);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "riskey"),
        "Expected RIS bibliography completion"
    );
}

#[test]
fn test_completion_returns_none_for_invalid_yaml_frontmatter() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("refs.yaml"), "- id: cslkey\n  title: Sample\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: [\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 4, 7);
    assert!(
        result.is_none(),
        "Expected no completion when YAML frontmatter is invalid"
    );
}

#[test]
fn test_completion_returns_none_inside_yaml_frontmatter() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("refs.bib"), "@book{known,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\ntitle: \"@\"\nbibliography: refs.bib\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let result = server.completion(doc_uri.as_str(), 1, 9);
    assert!(
        result.is_none(),
        "Expected no citation completion when cursor is inside YAML frontmatter"
    );
}

#[test]
fn test_completion_includes_only_crossrefable_chunk_labels() {
    let mut server = TestLspServer::new();

    let content = "```{r}\n#| label: setup\n1 + 1\n```\n\n```{r}\n#| label: fig-plot\n#| fig-cap: \"Plot\"\nplot(1:10)\n```\n\nSee @\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let result = server.completion("file:///test.qmd", 11, 6);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "fig-plot"),
        "Expected Quarto figure crossref label completion"
    );
    assert!(
        !items.iter().any(|item| item.label == "setup"),
        "Expected non-crossrefable chunk labels to be excluded"
    );
}

// --- Path completion in `![](…)` and `[](…)` destinations ---

fn open_doc_with_files(_server: &TestLspServer, files: &[(&str, &str)]) -> (TempDir, Uri) {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    for (rel, contents) in files {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, contents).unwrap();
    }
    let doc_uri = Uri::from_file_path(root.join("doc.md")).expect("doc uri");
    (temp_dir, doc_uri)
}

#[test]
fn test_image_path_completion_lists_image_files_only() {
    let mut server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(
        &server,
        &[
            ("images/foo.png", ""),
            ("images/bar.jpg", ""),
            ("images/notes.txt", ""),
        ],
    );

    let content = "![](images/)\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    // Cursor between `images/` and `)`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "foo.png"), "labels: {labels:?}");
    assert!(labels.iter().any(|l| l == "bar.jpg"), "labels: {labels:?}");
    assert!(
        !labels.iter().any(|l| l == "notes.txt"),
        "txt files should be excluded in image context: {labels:?}"
    );
}

#[test]
fn test_image_path_completion_includes_video_files() {
    let mut server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(
        &server,
        &[
            ("media/clip.mp4", ""),
            ("media/clip.webm", ""),
            ("media/notes.txt", ""),
        ],
    );

    let content = "![](media/)\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    // Cursor between `media/` and `)`: line 0, char 10.
    let result = server.completion(doc_uri.as_str(), 0, 10);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "clip.mp4"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "clip.webm"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "notes.txt"),
        "txt files should be excluded in image context: {labels:?}"
    );
}

#[test]
fn test_image_path_completion_includes_subdirectory() {
    let mut server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(&server, &[("images/nested/keep.png", "")]);

    let content = "![](images/)\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    let result = server.completion(doc_uri.as_str(), 0, 11);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let folder = items
        .iter()
        .find(|i| i.label == "nested/")
        .expect("nested/ directory");
    assert_eq!(folder.kind, Some(CompletionItemKind::FOLDER));
}

#[test]
fn test_image_path_completion_filters_by_typed_prefix() {
    let mut server = TestLspServer::new();
    let (_tmp, doc_uri) =
        open_doc_with_files(&server, &[("images/foo.png", ""), ("images/bar.png", "")]);

    let content = "![](images/f)\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    // Cursor between `f` and `)`: line 0, char 12.
    let result = server.completion(doc_uri.as_str(), 0, 12);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "foo.png"), "labels: {labels:?}");
    assert!(
        !labels.iter().any(|l| l == "bar.png"),
        "prefix `f` must exclude bar.png: {labels:?}"
    );
}

#[test]
fn test_link_path_completion_includes_all_files() {
    let mut server = TestLspServer::new();
    let (_tmp, doc_uri) =
        open_doc_with_files(&server, &[("docs/intro.md", ""), ("docs/notes.txt", "")]);

    let content = "[see](docs/)\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    // Cursor between `docs/` and `)`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "intro.md"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "notes.txt"),
        "labels: {labels:?}"
    );
}

#[test]
fn test_no_path_completion_inside_image_alt_text() {
    let mut server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(&server, &[("images/foo.png", "")]);

    let content = "![images/](images/)\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    // Cursor inside the alt text `![images/]`, between `/` and `]`: line 0, char 9.
    let result = server.completion(doc_uri.as_str(), 0, 9);
    assert!(
        result.is_none(),
        "alt-text region must not trigger path completion"
    );
}

#[test]
fn test_completion_capability_registers_path_trigger_characters() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("temp dir absolute");
    let init = server.initialize_result(root_uri.as_str());
    let triggers = init
        .capabilities
        .completion_provider
        .expect("completion provider")
        .trigger_characters
        .expect("trigger_characters");
    assert!(triggers.iter().any(|t| t == "/"), "triggers: {triggers:?}");
    assert!(triggers.iter().any(|t| t == "("), "triggers: {triggers:?}");
    assert!(triggers.iter().any(|t| t == "<"), "triggers: {triggers:?}");
    assert!(triggers.iter().any(|t| t == "["), "triggers: {triggers:?}");
}

// --- Path completion inside Quarto shortcodes ---

fn open_quarto_doc_with_files(
    _server: &TestLspServer,
    files: &[(&str, &str)],
    doc_rel: &str,
) -> (TempDir, Uri) {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    for (rel, contents) in files {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, contents).unwrap();
    }
    let doc_uri = Uri::from_file_path(root.join(doc_rel)).expect("doc uri");
    (temp_dir, doc_uri)
}

#[test]
fn test_shortcode_include_completes_quarto_files() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("_intro.qmd", ""), ("_setup.R", ""), ("scratch.txt", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< include _ >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor between `_` and ` `: line 0, char 13.
    let result = server.completion(doc_uri.as_str(), 0, 13);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        labels.iter().any(|l| l == "_intro.qmd"),
        "labels: {labels:?}"
    );
    assert!(labels.iter().any(|l| l == "_setup.R"), "labels: {labels:?}");
    assert!(
        !labels.iter().any(|l| l == "scratch.txt"),
        ".txt should be filtered out for include: {labels:?}"
    );
}

#[test]
fn test_shortcode_include_resolves_absolute_path_against_workspace_root() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("chapters/_intro.qmd", ""), ("subdir/doc.qmd", "")],
        "subdir/doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< include /chapters/ >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor between `/chapters/` and ` `: line 0, char 22.
    let result = server.completion(doc_uri.as_str(), 0, 22);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        labels.iter().any(|l| l == "_intro.qmd"),
        "expected workspace-rooted match: {labels:?}"
    );
}

#[test]
fn test_shortcode_embed_filters_to_notebooks() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("nb.ipynb", ""), ("sibling.qmd", ""), ("notes.md", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< embed  >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor between `embed ` and ` >}}`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "nb.ipynb"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "sibling.qmd"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "notes.md"),
        ".md should be filtered out for embed: {labels:?}"
    );
}

#[test]
fn test_shortcode_embed_returns_none_after_hash() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("nb.ipynb", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< embed nb.ipynb# >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor after `#`: line 0, char 19.
    let result = server.completion(doc_uri.as_str(), 0, 19);
    assert!(
        result.is_none(),
        "cell-id completion is out of scope for v1"
    );
}

#[test]
fn test_shortcode_video_filters_to_video_extensions() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("clip.mp4", ""), ("clip.webm", ""), ("thumb.png", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< video  >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor between `video ` and ` >}}`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "clip.mp4"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "clip.webm"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "thumb.png"),
        "images should be filtered out for video: {labels:?}"
    );
}

#[test]
fn test_shortcode_video_returns_none_for_url_prefix() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("clip.mp4", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< video https:// >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor after `https://`: line 0, char 18.
    let result = server.completion(doc_uri.as_str(), 0, 18);
    assert!(
        result.is_none(),
        "URL prefixes should not produce filesystem suggestions"
    );
}

#[test]
fn test_shortcode_placeholder_filters_to_images() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("pic.png", ""), ("vector.svg", ""), ("clip.mp4", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< placeholder  >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor between `placeholder ` and ` >}}`: line 0, char 17.
    let result = server.completion(doc_uri.as_str(), 0, 17);
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "pic.png"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "vector.svg"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "clip.mp4"),
        "video files should be filtered out for placeholder: {labels:?}"
    );
}

#[test]
fn test_shortcode_unknown_name_returns_none() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("a.qmd", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< lipsum  >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor between `lipsum ` and ` >}}`: line 0, char 12.
    let result = server.completion(doc_uri.as_str(), 0, 12);
    assert!(
        result.is_none(),
        "unknown shortcodes should not trigger path completion"
    );
}

#[test]
fn test_shortcode_completion_skipped_in_plain_markdown() {
    let mut server = TestLspServer::new();
    // `.git/HEAD` anchors the project boundary so config discovery doesn't
    // leak in from an ancestor `panache.toml` on the host.
    let (tmp, doc_uri) =
        open_quarto_doc_with_files(&server, &[(".git/HEAD", ""), ("_intro.qmd", "")], "doc.md");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< include _ >}}\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    let result = server.completion(doc_uri.as_str(), 0, 13);
    assert!(
        result.is_none(),
        "shortcode completion should be Quarto-only"
    );
}

#[test]
fn test_shortcode_completion_skipped_on_named_arg() {
    let mut server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("nb.ipynb", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str());

    let content = "{{< embed nb.ipynb echo=t >}}\n";
    server.open_document(doc_uri.as_str(), content, "quarto");

    // Cursor inside `echo=t|`: line 0, char 25 (just after the `t`).
    let result = server.completion(doc_uri.as_str(), 0, 25);
    assert!(
        result.is_none(),
        "named args should not trigger path completion"
    );
}

/// Pull the markdown string out of a resolved item's documentation field.
fn documentation_markdown(item: &CompletionItem) -> Option<String> {
    match item.documentation.as_ref()? {
        Documentation::MarkupContent(markup) => Some(markup.value.clone()),
        Documentation::String(value) => Some(value.clone()),
    }
}

/// Find the citation completion item with the given label.
fn citation_item(items: &[CompletionItem], label: &str) -> CompletionItem {
    items
        .iter()
        .find(|item| item.label == label)
        .cloned()
        .unwrap_or_else(|| panic!("expected completion item `{label}`"))
}

#[test]
fn test_completion_citation_item_defers_preview() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    std::fs::write(
        root.join("refs.bib"),
        "@article{known, author={Smith, J.}, year={2020}, title={On Things}}\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.open_document(doc_uri.as_str(), "Text [@] citation.", "quarto");

    let Some(CompletionResponse::Array(items)) = server.completion(doc_uri.as_str(), 0, 7) else {
        panic!("Expected completion items");
    };
    let item = citation_item(&items, "known");

    // Preview is deferred: no documentation eagerly attached.
    assert!(
        item.documentation.is_none(),
        "citation preview should be deferred to resolve"
    );

    let data = item.data.as_ref().expect("citation item should carry data");
    assert_eq!(data.get("kind").and_then(|v| v.as_str()), Some("citation"));
    assert_eq!(data.get("key").and_then(|v| v.as_str()), Some("known"));
    assert_eq!(
        data.get("uri").and_then(|v| v.as_str()),
        Some(doc_uri.as_str())
    );
}

#[test]
fn test_resolve_completion_item_attaches_bibtex_preview() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    std::fs::write(
        root.join("refs.bib"),
        "@article{known, author={Smith, J.}, year={2020}, title={On Things}, journal={J. Things}}\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.open_document(doc_uri.as_str(), "Text [@] citation.", "quarto");

    let Some(CompletionResponse::Array(items)) = server.completion(doc_uri.as_str(), 0, 7) else {
        panic!("Expected completion items");
    };
    let resolved = server.resolve_completion_item(citation_item(&items, "known"));

    let markdown = documentation_markdown(&resolved).expect("resolved item should have a preview");
    assert_eq!(markdown, "Smith, J. (2020). *On Things*. J. Things");
    assert_eq!(resolved.detail.as_deref(), Some("article"));
}

#[test]
fn test_resolve_completion_item_csl_yaml_preview() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(
        root.join("refs.yaml"),
        "- id: cslkey\n  author:\n    - family: Doe\n  issued: 2021\n  title: Sample\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.yaml\n---\n\nText [@] citation.";
    server.open_document(doc_uri.as_str(), content, "quarto");

    let Some(CompletionResponse::Array(items)) = server.completion(doc_uri.as_str(), 4, 7) else {
        panic!("Expected completion items");
    };
    let resolved = server.resolve_completion_item(citation_item(&items, "cslkey"));

    let markdown = documentation_markdown(&resolved).expect("resolved item should have a preview");
    assert!(
        markdown.contains("*Sample*"),
        "expected title in preview, got: {markdown}"
    );
}

#[test]
fn test_resolve_completion_item_unknown_key_is_unchanged() {
    let mut server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    std::fs::write(root.join("refs.bib"), "@book{known,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str());

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.open_document(doc_uri.as_str(), "Text [@] citation.", "quarto");

    // Fabricate an item whose key no longer exists in the bibliography.
    let stale = CompletionItem {
        label: "gone".to_string(),
        data: Some(serde_json::json!({
            "kind": "citation",
            "uri": doc_uri.as_str(),
            "key": "gone",
        })),
        ..Default::default()
    };
    let resolved = server.resolve_completion_item(stale);

    assert!(
        resolved.documentation.is_none(),
        "resolving an unknown key should not attach documentation"
    );
}

#[test]
fn test_resolve_completion_item_ignores_non_citation_items() {
    let server = TestLspServer::new();

    let plain = CompletionItem {
        label: "plain".to_string(),
        ..Default::default()
    };
    let resolved = server.resolve_completion_item(plain.clone());

    assert_eq!(resolved.documentation, None);
    assert_eq!(resolved.label, plain.label);
}
