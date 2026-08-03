//! Client configuration handling: the `workspace/didChangeConfiguration` push
//! notification and the `workspace/configuration` pull reply.
//!
//! Live-applies a client configuration change without a server restart: pushed
//! or pulled runtime settings (currently `experimental.incrementalParsing`)
//! update in place, on-disk `panache.toml` config is re-read for every open
//! document, and the debounced settle re-publishes diagnostics over the fresh
//! state.

use lsp_types::DidChangeConfigurationParams;
use serde_json::Value;

use crate::lsp::dispatch::runtime_incremental_parsing_from_value;
use crate::lsp::documents;
use crate::lsp::global_state::GlobalState;

pub(crate) fn did_change_configuration(gs: &mut GlobalState, params: DidChangeConfigurationParams) {
    // The push payload is optional: clients using the pull model send `null`.
    // Either way we still reload on-disk config below, so a bare notification is
    // a useful "reload config" signal.
    if !params.settings.is_null() {
        apply_runtime_settings(gs, &params.settings);
    }

    // Pull-model clients send a (possibly bare) notification to signal
    // "re-pull"; fetch the fresh `panache` settings, applied asynchronously via
    // `apply_pulled_configuration`. A no-op for clients that only push (they
    // don't advertise `workspace.configuration`).
    gs.pull_configuration();

    reload_and_settle(gs);
}

/// Apply the reply to a `workspace/configuration` pull. The response is a JSON
/// array with one element per requested item; we request a single `panache`
/// section, so element `[0]` is that section object (or `null` when the client
/// has no such settings). An empty or null reply is a no-op.
pub(crate) fn apply_pulled_configuration(gs: &mut GlobalState, value: Value) {
    let Some(section) = value.as_array().and_then(|items| items.first()) else {
        return;
    };
    if section.is_null() {
        return;
    }
    apply_runtime_settings(gs, section);
    reload_and_settle(gs);
}

/// Update the in-memory runtime settings from a client settings/section value,
/// tolerating the nesting differences between the push payload
/// (`settings.panache.*`) and a section-scoped pull reply (bare `experimental.*`)
/// — both handled by [`runtime_incremental_parsing_from_value`].
fn apply_runtime_settings(gs: &mut GlobalState, value: &Value) {
    if let Some(incremental) = runtime_incremental_parsing_from_value(value)
        && gs.runtime_settings.experimental_incremental_parsing != incremental
    {
        log::debug!(
            "lsp runtime setting experimental.incrementalParsing={incremental} (client settings)"
        );
        gs.runtime_settings.experimental_incremental_parsing = incremental;
    }
}

/// Re-read on-disk config for every open document and re-publish diagnostics
/// over the fresh state.
fn reload_and_settle(gs: &mut GlobalState) {
    documents::reload_open_documents_config(gs);
    gs.arm_settle();
}
