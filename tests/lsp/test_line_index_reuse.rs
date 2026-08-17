//! The write phase's line-index reuse.
//!
//! `did_change` patches the index the previous edit left behind instead of
//! rebuilding one from the salsa memo (which every keystroke invalidates, so
//! during a typing burst it is always cold). Reuse changes nothing a client can
//! observe, so these tests assert it through the two counters the harness
//! exposes --- otherwise a regression to rebuilding every keystroke would leave
//! the whole suite green.
//!
//! The correctness half is the opposite claim: a cached index is used *only*
//! while it still names the text allocation salsa holds, so any other writer of
//! the text discards it. `a_text_write_outside_did_change_discards_the_cached_index`
//! is the test that fails loudly when that guard goes.
//!
//! The last two tests pin what the write phase's cache does *not* cover: the
//! reader. They are the measurement `TODO.md`'s reader-side item asks for, in
//! counter form --- how *often* a reader rebuilds, the half no bench answers
//! (`benches/lsp_write_phase.rs` times the write phase only). They assert the
//! status quo, so unifying the two caches will invert them, deliberately.

use super::helpers::{TestLspServer, UriExt, full_document_change, incremental_change};
use lsp_types::{FileChangeType, FileEvent, Uri};
use std::fs;
use tempfile::TempDir;

/// Only the first edit of a burst builds an index; every later one patches what
/// its predecessor left behind. The salsa memo cannot serve them: each keystroke
/// invalidates it and no reader runs inside the debounce window.
#[test]
fn a_typing_burst_rebuilds_the_line_index_once() {
    let mut server = TestLspServer::new();
    let uri = "file:///burst.qmd";
    server.open_document(uri, "# Title\n\nabcde\n", "quarto");

    let before = server.line_index_rebuilds();
    for (index, letter) in ["v", "w", "x", "y", "z"].iter().enumerate() {
        let at = index as u32;
        server.edit_document(uri, vec![incremental_change(2, at, 2, at + 1, letter)]);
    }

    assert_eq!(
        server.line_index_rebuilds() - before,
        1,
        "a five-keystroke burst must build one index, not five"
    );
    assert_eq!(
        server.get_document_content(uri),
        Some("# Title\n\nvwxyz\n".to_string())
    );
    assert_eq!(server.cached_line_indexes(), 1);
}

/// The correctness pin. A watcher event rewrites an open document's text input
/// behind the write phase's back; the cached index still describes the *old*
/// bytes, and using it would splice the next edit into them --- silently
/// throwing the on-disk change away. Validating by allocation identity turns
/// that into a rebuild.
#[test]
fn a_text_write_outside_did_change_discards_the_cached_index() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let path = root.join("doc.qmd");
    fs::write(&path, "# Title\n\nOne\n").unwrap();

    let mut server = TestLspServer::new();
    server.initialize(&Uri::from_file_path(root).unwrap().to_string());
    let uri = Uri::from_file_path(&path).unwrap().to_string();
    server.open_document(&uri, "# Title\n\nOne\n", "quarto");

    server.edit_document(&uri, vec![incremental_change(2, 0, 2, 3, "Two")]);
    let after_first = server.line_index_rebuilds();
    assert_eq!(server.cached_line_indexes(), 1);

    // The document's own file changes on disk, and the watcher syncs it into
    // salsa: a *different* allocation carrying different bytes.
    fs::write(&path, "# Title\n\nAlpha\nBeta\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&path).unwrap(),
        typ: FileChangeType::CHANGED,
    }]);

    server.edit_document(&uri, vec![incremental_change(3, 0, 3, 4, "Gamma")]);

    assert_eq!(
        server.get_document_content(&uri),
        Some("# Title\n\nAlpha\nGamma\n".to_string()),
        "the edit must be spliced into the text salsa holds, not into the \
         stale text the cached index was built for"
    );
    assert_eq!(
        server.line_index_rebuilds() - after_first,
        1,
        "the out-of-band write must force a rebuild"
    );
}

/// An edit that reproduces the current bytes is skipped by
/// `set_text_if_changed`, so salsa keeps the older (equal, but distinct)
/// allocation while the write phase holds an index built for the new one. The
/// store must notice and keep nothing rather than cache a claim that is already
/// false.
#[test]
fn an_identical_write_leaves_no_stale_index() {
    let mut server = TestLspServer::new();
    let uri = "file:///identical.qmd";
    server.open_document(uri, "# Title\n\nOne\n", "quarto");

    server.edit_document(uri, vec![incremental_change(2, 0, 2, 3, "Two")]);
    assert_eq!(server.cached_line_indexes(), 1);

    server.edit_document(uri, vec![full_document_change("# Title\n\nTwo\n")]);
    assert_eq!(
        server.cached_line_indexes(),
        0,
        "an index salsa's text does not back must not be cached"
    );

    server.edit_document(uri, vec![incremental_change(2, 0, 2, 3, "Six")]);
    assert_eq!(
        server.get_document_content(uri),
        Some("# Title\n\nSix\n".to_string())
    );
}

/// A whole-document replacement has no span to resolve, so it rebuilds by
/// construction --- but the index it produces is the one the *next* notification
/// patches, including a ranged change later in the same notification.
#[test]
fn a_full_replacement_reseeds_the_reused_index() {
    let mut server = TestLspServer::new();
    let uri = "file:///replaced.qmd";
    server.open_document(uri, "# Title\n\nOne\n", "quarto");

    server.edit_document(
        uri,
        vec![
            full_document_change("# Other\n\nalpha\n"),
            incremental_change(2, 0, 2, 5, "beta"),
        ],
    );
    assert_eq!(
        server.get_document_content(uri),
        Some("# Other\n\nbeta\n".to_string())
    );

    let after_replace = server.line_index_rebuilds();
    server.edit_document(uri, vec![incremental_change(2, 0, 2, 4, "gamma")]);

    assert_eq!(
        server.line_index_rebuilds(),
        after_replace,
        "the replacement's index must serve the next notification"
    );
    assert_eq!(
        server.get_document_content(uri),
        Some("# Other\n\ngamma\n".to_string())
    );
}

/// Keyed per document, not a single slot: alternating between two documents (a
/// split view, or a rename's per-document `didChange` burst) must not make every
/// edit a miss.
#[test]
fn two_documents_each_keep_their_own_line_index() {
    let mut server = TestLspServer::new();
    let first = "file:///first.qmd";
    let second = "file:///second.qmd";
    server.open_document(first, "# First\n\naaa\n", "quarto");
    server.open_document(second, "# Second\n\nbbb\n", "quarto");

    // Each edit must move the bytes: an edit that reproduces the current text
    // is skipped by `set_text_if_changed`, which is a different path (see
    // `an_identical_write_leaves_no_stale_index`).
    let before = server.line_index_rebuilds();
    for (uri, replacement) in [
        (first, "xyz"),
        (second, "xyz"),
        (first, "pqr"),
        (second, "pqr"),
    ] {
        server.edit_document(uri, vec![incremental_change(2, 0, 2, 3, replacement)]);
    }

    assert_eq!(
        server.line_index_rebuilds() - before,
        2,
        "each document rebuilds once; the second round must hit"
    );
    assert_eq!(server.cached_line_indexes(), 2);
}

#[test]
fn closing_a_document_retires_its_line_index() {
    let mut server = TestLspServer::new();
    let uri = "file:///closing.qmd";
    server.open_document(uri, "# Title\n\nOne\n", "quarto");
    server.edit_document(uri, vec![incremental_change(2, 0, 2, 3, "Two")]);
    assert_eq!(server.cached_line_indexes(), 1);

    server.close_document(uri);
    assert_eq!(server.cached_line_indexes(), 0);
}

/// An untitled buffer has no backing path, so its text goes through
/// `update_input_text` rather than `update_file_text` --- a different setter, and
/// the one place the write could reach an input other than the one the cache is
/// keyed on.
#[test]
fn an_untitled_buffer_reuses_its_line_index() {
    let mut server = TestLspServer::new();
    let uri = "untitled:Untitled-1";
    server.open_document(uri, "# Draft\n\nOne\n", "quarto");

    server.edit_document(uri, vec![incremental_change(2, 0, 2, 3, "Two")]);
    let after_first = server.line_index_rebuilds();
    server.edit_document(uri, vec![incremental_change(2, 0, 2, 3, "Six")]);

    assert_eq!(
        server.line_index_rebuilds(),
        after_first,
        "an untitled buffer must reuse its index like any other document"
    );
    assert_eq!(
        server.get_document_content(uri),
        Some("# Draft\n\nSix\n".to_string())
    );
}

/// Reads at one revision share one index: the salsa memo does its job as long as
/// the text is not moving. Establishes that the gap the next test measures is
/// specifically *per revision*, not per read --- so it is the keystroke that
/// costs a rebuild, and a cursor sitting still costs nothing.
#[test]
fn repeated_reads_at_one_revision_build_one_index() {
    let mut server = TestLspServer::new();
    let uri = "file:///still.qmd";
    server.open_document(uri, "# Title\n\nalpha beta gamma\n", "quarto");

    let before = server.line_index_read_rebuilds();
    for character in 0..5 {
        server.document_highlight(uri, 2, character);
    }

    assert_eq!(
        server.line_index_read_rebuilds() - before,
        1,
        "five reads at one revision must share one index"
    );
}

/// The reader-side gap, measured. An editor issues a request per typed character
/// (completion, highlight, semantic tokens), and each lands on a revision the
/// keystroke before it just invalidated --- so every one re-executes the
/// `line_index` memo over the whole document, on a pool thread, while
/// `GlobalState::line_index_cache` is holding an index for those very bytes.
///
/// One rebuild per keystroke is therefore the number, and it is what makes the
/// per-rebuild cost worth paying attention to rather than amortized away: on the
/// 297 KB fixture a rebuild is the ~100 us that commit `56beaf52` took off the
/// write phase and left here.
///
/// When the two caches are unified this must become 0, and the assertion below
/// is written to be inverted rather than deleted.
#[test]
fn every_keystroke_costs_a_reader_one_rebuild() {
    let mut server = TestLspServer::new();
    let uri = "file:///burst_with_reads.qmd";
    server.open_document(uri, "# Title\n\nabcde\n", "quarto");

    let keystrokes = ["v", "w", "x", "y", "z"];
    let before = server.line_index_read_rebuilds();
    for (index, letter) in keystrokes.iter().enumerate() {
        let at = index as u32;
        server.edit_document(uri, vec![incremental_change(2, at, 2, at + 1, letter)]);
        // What a real client does between keystrokes, and the only thing that
        // matters about it here: it resolves a position, so it needs the index.
        server.document_highlight(uri, 2, at);
    }

    assert_eq!(
        server.line_index_read_rebuilds() - before,
        keystrokes.len() as u64,
        "each keystroke invalidates the memo, so each following read rebuilds; \
         a shared cache would make this 0"
    );
    assert_eq!(
        server.get_document_content(uri),
        Some("# Title\n\nvwxyz\n".to_string())
    );
}
