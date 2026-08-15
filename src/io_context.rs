//! Attach the offending path to bare [`io::Error`]s.
//!
//! `std::fs` errors carry no path, so a failure propagated out of the CLI reads
//! `No such file or directory (os error 2)` with no hint at *which* file. The
//! [`IoResultExt::with_path`] adapter names it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// An [`io::Error`] payload naming the file the failed operation targeted.
///
/// The original error stays reachable through [`std::error::Error::source`],
/// and the wrapping [`io::Error`] keeps its [`io::ErrorKind`], so callers that
/// match on the kind (missing file, permission denied) are unaffected.
pub struct PathContext {
    path: PathBuf,
    source: io::Error,
}

impl PathContext {
    /// The file the failed operation targeted.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// Mirror `Display` so panic messages and `{:?}` logs stay readable instead of
// dumping the struct fields.
impl fmt::Debug for PathContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for PathContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for PathContext {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub trait IoResultExt<T> {
    /// Name `path` in the error, when the operation failed.
    ///
    /// An error that already names a path is left alone, so nesting two
    /// annotated helpers cannot produce `a: b: message`.
    fn with_path(self, path: impl AsRef<Path>) -> io::Result<T>;
}

impl<T> IoResultExt<T> for io::Result<T> {
    fn with_path(self, path: impl AsRef<Path>) -> io::Result<T> {
        self.map_err(|source| {
            if source.get_ref().is_some_and(|e| e.is::<PathContext>()) {
                return source;
            }
            io::Error::new(
                source.kind(),
                PathContext {
                    path: path.as_ref().to_path_buf(),
                    source,
                },
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn not_found() -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, "no such file")
    }

    #[test]
    fn ok_passes_through() {
        let value: io::Result<u8> = Ok(7);
        assert_eq!(value.with_path("doc.md").unwrap(), 7);
    }

    #[test]
    fn error_is_prefixed_with_the_path() {
        let err = io::Result::<()>::Err(not_found())
            .with_path("docs/doc.md")
            .unwrap_err();
        assert_eq!(err.to_string(), "docs/doc.md: no such file");
    }

    #[test]
    fn error_kind_is_preserved() {
        let err = io::Result::<()>::Err(not_found())
            .with_path("doc.md")
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn original_error_stays_reachable_as_the_source() {
        let err = io::Result::<()>::Err(not_found())
            .with_path("doc.md")
            .unwrap_err();
        let ctx = err
            .get_ref()
            .and_then(|e| e.downcast_ref::<PathContext>())
            .expect("payload is a PathContext");
        assert_eq!(ctx.path(), Path::new("doc.md"));
        assert_eq!(ctx.source().unwrap().to_string(), "no such file");
    }

    #[test]
    fn an_already_named_path_is_not_wrapped_twice() {
        let err = io::Result::<()>::Err(not_found())
            .with_path("inner.md")
            .with_path("outer.md")
            .unwrap_err();
        assert_eq!(err.to_string(), "inner.md: no such file");
    }
}
