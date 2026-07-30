//! A `vfs`-style path<->id<->input map (mirrors rust-analyzer's `vfs` crate).
//!
//! Owns the single source of truth for file *identity*: the path<->[`FileId`]
//! bimap and the id->[`FileText`] input table. Only the writer mutates it
//! (`alloc_id`/`insert`/`remove_path`); cloned worker handles share the same
//! `Arc<Mutex<_>>` and only read. The salsa [`crate::salsa::SalsaDb`] holds one
//! [`Vfs`] and delegates all path/id lookups to it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::salsa::FileText;

/// Opaque, process-stable identity for a file (mirrors rust-analyzer's
/// `vfs::FileId`). A plain newtype --- not a salsa interned struct --- because
/// the LSP boundary must convert URI -> `FileId` synchronously on the main
/// thread, outside any salsa query. Intra-query path interning still goes
/// through [`crate::salsa::InternedPath`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FileId(u32);

/// The interior of [`Vfs`]: the path<->id bimap plus the id->input table. Only
/// the writer mutates it (`alloc_id`/`insert`/`remove`); cloned worker handles
/// share the same `Arc<Mutex<_>>` and only read.
#[derive(Default)]
struct VfsInner {
    next_id: u32,
    path_to_id: HashMap<PathBuf, FileId>,
    /// Backing path for each id; `None` for an in-memory buffer with no file on
    /// disk (retires the `<memory>` sentinel).
    id_to_path: HashMap<FileId, Option<PathBuf>>,
    id_to_input: HashMap<FileId, FileText>,
    /// Reverse map: a [`FileText`] input back to its (immutable) backing path.
    /// Lets path-keyed queries resolve a document's path from its `FileText`
    /// identity rather than threading a `PathBuf` parameter (audit §3.3 / G3).
    /// `None` for an in-memory buffer.
    input_to_path: HashMap<FileText, Option<PathBuf>>,
}

/// A `vfs`-style path<->id map that subsumes the former `file_cache`
/// (audit §3.3 / G3). Owned by [`crate::salsa::SalsaDb`] behind an
/// `Arc<Mutex<_>>` so cloned worker handles observe the same table.
#[derive(Clone, Default)]
pub(crate) struct Vfs {
    inner: Arc<Mutex<VfsInner>>,
}

impl Vfs {
    fn lock(&self) -> std::sync::MutexGuard<'_, VfsInner> {
        self.inner.lock().expect("vfs lock poisoned")
    }

    pub(crate) fn id_for_path(&self, path: &Path) -> Option<FileId> {
        self.lock().path_to_id.get(path).copied()
    }

    pub(crate) fn input_for_id(&self, id: FileId) -> Option<FileText> {
        self.lock().id_to_input.get(&id).copied()
    }

    pub(crate) fn input_for_path(&self, path: &Path) -> Option<FileText> {
        let inner = self.lock();
        let id = inner.path_to_id.get(path)?;
        inner.id_to_input.get(id).copied()
    }

    pub(crate) fn path_for_id(&self, id: FileId) -> Option<PathBuf> {
        self.lock().id_to_path.get(&id).cloned().flatten()
    }

    /// The immutable backing path for a [`FileText`] input, or `None` for an
    /// in-memory buffer / unregistered input.
    pub(crate) fn path_for_input(&self, input: FileText) -> Option<PathBuf> {
        self.lock().input_to_path.get(&input).cloned().flatten()
    }

    pub(crate) fn cached_paths(&self) -> Vec<PathBuf> {
        self.lock().path_to_id.keys().cloned().collect()
    }

    /// Allocate a fresh id. Called only by the single writer.
    pub(crate) fn alloc_id(&self) -> FileId {
        let mut inner = self.lock();
        let id = FileId(inner.next_id);
        inner.next_id += 1;
        id
    }

    /// Register an id's path and salsa input. Called only by the writer.
    pub(crate) fn insert(&self, id: FileId, path: Option<PathBuf>, input: FileText) {
        let mut inner = self.lock();
        if let Some(path) = path.clone() {
            inner.path_to_id.insert(path, id);
        }
        inner.id_to_path.insert(id, path.clone());
        inner.id_to_input.insert(id, input);
        inner.input_to_path.insert(input, path);
    }

    /// Forget a path's id/input mapping. Returns the removed [`FileId`], if any.
    pub(crate) fn remove_path(&self, path: &Path) -> Option<FileId> {
        let mut inner = self.lock();
        let id = inner.path_to_id.remove(path)?;
        inner.id_to_path.remove(&id);
        if let Some(input) = inner.id_to_input.remove(&id) {
            inner.input_to_path.remove(&input);
        }
        Some(id)
    }
}
