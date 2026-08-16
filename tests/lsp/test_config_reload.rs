//! Tests for live config reload via `workspace/didChangeConfiguration` and
//! `panache.toml` watcher events: client-pushed runtime settings update without
//! a restart, and on-disk config edits refresh every open document.

use super::helpers::*;
use lsp_types::{FileChangeType, FileEvent, Uri};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

/// A pushed `didChangeConfiguration` flips the experimental incremental-parsing
/// runtime setting live (it was previously only read once at `initialize`).
#[test]
fn did_change_configuration_updates_runtime_setting() {
    // This asserts the client-settings plumbing, which the environment
    // override deliberately bypasses.
    if incremental_parsing_forced_by_env() {
        return;
    }
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");
    server.open_document("file:///workspace/doc.qmd", "# Title\n", "quarto");
    assert!(
        !server.experimental_incremental_parsing_enabled(),
        "incremental parsing defaults off"
    );

    server.did_change_configuration(json!({
        "settings": { "panache": { "experimental": { "incrementalParsing": true } } }
    }));

    assert!(
        server.experimental_incremental_parsing_enabled(),
        "didChangeConfiguration should enable incremental parsing without a restart"
    );
}

/// Whether the built-in lint plan for `uri` carries a `heading-hierarchy`
/// diagnostic.
fn has_heading_hierarchy(server: &TestLspServer, uri: &str) -> bool {
    server
        .get_built_in_diagnostics(uri)
        .unwrap_or_default()
        .iter()
        .any(|d| d.code == "heading-hierarchy")
}

/// Rewriting `panache.toml` to re-enable a rule and sending
/// `didChangeConfiguration` re-reads disk config for the already-open document.
#[test]
fn did_change_configuration_reloads_disk_config() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let config_path = root.join("panache.toml");
    fs::write(&config_path, "[lint.rules]\nheading-hierarchy = false\n").unwrap();

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(doc_uri.as_str(), "# H1\n\n### H3 skip\n", "quarto");
    assert!(
        !has_heading_hierarchy(&server, doc_uri.as_str()),
        "rule disabled by initial config"
    );

    // Re-enable the rule on disk, then notify the server.
    fs::write(&config_path, "[lint.rules]\nheading-hierarchy = true\n").unwrap();
    server.did_change_configuration(json!(null));

    assert!(
        has_heading_hierarchy(&server, doc_uri.as_str()),
        "didChangeConfiguration should re-read panache.toml and re-enable the rule"
    );
}

/// A watcher event for `panache.toml` reloads disk config the same way, without
/// any client settings push.
#[test]
fn panache_toml_watcher_reloads_disk_config() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let config_path = root.join("panache.toml");
    fs::write(&config_path, "[lint.rules]\nheading-hierarchy = false\n").unwrap();

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(doc_uri.as_str(), "# H1\n\n### H3 skip\n", "quarto");
    assert!(!has_heading_hierarchy(&server, doc_uri.as_str()));
    let before = server.document_salsa_config(doc_uri.as_str());

    fs::write(&config_path, "[lint.rules]\nheading-hierarchy = true\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&config_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }]);

    assert!(
        has_heading_hierarchy(&server, doc_uri.as_str()),
        "a panache.toml watcher event should reload disk config"
    );
    // The handle itself must move, not just the value behind it: the reload is
    // the only path that re-points a document at a new `FileConfig`, and the
    // incremental reparse base is re-admitted under exactly that condition.
    assert!(
        before != server.document_salsa_config(doc_uri.as_str()),
        "a reloaded config should re-point the document at a new interned handle"
    );
}

/// A config reload re-points the document at a new `FileConfig` handle, and the
/// incremental reparse base is recorded per `(file, config)` pair --- so the
/// reload must re-admit under the new handle or the document silently
/// full-parses every keystroke from then on.
#[test]
fn reloading_config_re_admits_the_reparse_base() {
    if incremental_parsing_forced_off() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let config_path = root.join("panache.toml");
    fs::write(&config_path, "[lint.rules]\nheading-hierarchy = false\n").unwrap();

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize_with_options(
        root_uri.as_str(),
        Some(json!({
            "settings": { "panache": { "experimental": { "incrementalParsing": true } } }
        })),
    );
    server.open_document(doc_uri.as_str(), "# H1\n\n### H3 skip\n", "quarto");
    assert!(
        server.document_reparse_admitted(doc_uri.as_str()),
        "did_open admits the document"
    );

    fs::write(&config_path, "[lint.rules]\nheading-hierarchy = true\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&config_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }]);

    assert!(
        server.document_reparse_admitted(doc_uri.as_str()),
        "a config reload must re-admit the document under its new handle"
    );
}

/// A watcher event for a differently-named base config reached via `extend`
/// reloads open documents. The config-name globs only match `panache.toml`, so
/// this exercises the extend-chain tracking that watches arbitrary base files.
#[test]
fn extended_base_config_watcher_reloads_disk_config() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let base_path = root.join("base.toml");
    fs::write(&base_path, "[lint.rules]\nheading-hierarchy = false\n").unwrap();
    // The discovered config extends the base and adds nothing of its own.
    let config_path = root.join("panache.toml");
    fs::write(&config_path, "extend = \"base.toml\"\n").unwrap();

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(doc_uri.as_str(), "# H1\n\n### H3 skip\n", "quarto");
    assert!(
        !has_heading_hierarchy(&server, doc_uri.as_str()),
        "rule disabled by the extended base config"
    );

    // Flip the rule on in the *base* file (not panache.toml) and notify.
    fs::write(&base_path, "[lint.rules]\nheading-hierarchy = true\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&base_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }]);

    assert!(
        has_heading_hierarchy(&server, doc_uri.as_str()),
        "editing an `extend`ed base config should reload dependent documents"
    );
}
