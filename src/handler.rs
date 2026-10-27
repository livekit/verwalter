//! Spec: contract VersionedFileHandler. One implementation per file format,
//! compiled in. Handlers work on file content, not paths, so they stay pure.

use semver::Version;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub trait VersionedFileHandler {
    /// Whether this handler owns the file at `path` (by name or extension).
    fn handles(&self, path: &str) -> bool;

    fn read_version(&self, content: &str) -> Result<Version, Error>;

    /// Invariant MinimalEdit: only the version field changes; formatting
    /// and unrelated content are preserved.
    fn write_version(&self, content: &str, version: &Version) -> Result<String, Error>;

    /// Names of in-repo packages this file depends on.
    fn internal_dependencies(&self, content: &str) -> Vec<String>;

    /// Update the pin on `dependency` to `version`. Content without such a
    /// pin is returned unchanged.
    fn write_dependency_version(
        &self,
        content: &str,
        dependency: &str,
        version: &Version,
    ) -> Result<String, Error>;
}

/// Pick the handler for a path. The first match wins.
pub fn handler_for<'a>(
    handlers: &'a [Box<dyn VersionedFileHandler>],
    path: &str,
) -> Option<&'a dyn VersionedFileHandler> {
    handlers.iter().find(|h| h.handles(path)).map(|h| h.as_ref())
}

#[cfg(test)]
pub(crate) mod contract {
    //! Obligation contract-signature.VersionedFileHandler.*: every handler
    //! must pass this. Call it from the handler's own tests with a sample
    //! file and a fresh version.

    use super::*;

    pub fn check(h: &dyn VersionedFileHandler, sample: &str, next: &Version) {
        let current = h.read_version(sample).expect("read_version on sample");
        assert_ne!(&current, next, "pick a `next` that differs from the sample");

        let written = h.write_version(sample, next).expect("write_version");
        // Invariant Roundtrip
        assert_eq!(&h.read_version(&written).unwrap(), next);
        // Invariant MinimalEdit: exactly one line differs.
        let changed = sample.lines().zip(written.lines()).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 1, "write_version must change exactly one line");
        assert_eq!(sample.lines().count(), written.lines().count());
    }
}
