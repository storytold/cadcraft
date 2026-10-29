//! Pluggable save targets (storytold/wordcraft#518, Phase 0-1).
//!
//! [`StorageBackend`] abstracts where document bytes live: a local or NAS folder today, an
//! rclone transport or native S3 later. Every action on a backend goes through this trait, so
//! the typing path never touches the network: callers serialise first (pure bytes), then call
//! one blocking method.
//!
//! The contract per method:
//! - [`StorageBackend::read`] / [`StorageBackend::write`] / [`StorageBackend::stat`] /
//!   [`StorageBackend::list`] move bytes and metadata.
//! - [`StorageBackend::capabilities`] reports what the backend can do without any I/O, so
//!   callers can adapt (e.g. skip `If-Match` where conditional writes are unsupported).
//! - [`StorageBackend::probe`] optionally talks to the server (an `OPTIONS` round trip on
//!   WebDAV) and refines the capabilities.
//! - Writes that set [`WriteOptions::atomic`] must be all-or-nothing as seen by a later `read`:
//!   the local backend stages a sibling temporary file and renames; WebDAV relies on the
//!   server's single-`PUT` semantics.
//! - [`StorageBackend::recover`] replays or clears the [`journal`] left by an interrupted write,
//!   so the next startup lands in a known state. Phase 3 (lease files) builds on the journal.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod journal;
pub mod local;
pub mod webdav;

use thiserror::Error;

/// What a backend knows about one stored file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMeta {
    /// Byte length.
    pub size: u64,
    /// Opaque version tag (a WebDAV `ETag`, stripped of quotes); `None` where untracked.
    pub etag: Option<String>,
    /// Last-modified stamp as the backend reports it (HTTP date, file mtime); display only.
    pub modified: Option<String>,
    /// `true` for a collection (folder); `false` for a file.
    pub is_dir: bool,
}

/// One entry of a [`StorageBackend::list`] result: the backend-relative path plus its metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Backend-relative path with `/` separators (no leading slash).
    pub path: String,
    /// What the backend knows about it.
    pub meta: FileMeta,
}

/// What a backend can do, without any I/O. Callers read this before choosing a strategy (e.g.
/// whether a conditional write with `If-Match` is honoured or silently ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// An `atomic` write is all-or-nothing as seen by a later `read`.
    pub atomic_write: bool,
    /// [`WriteOptions::if_match`] is honoured (a stale tag fails with [`StorageError::Conflict`]).
    pub conditional_write: bool,
    /// [`StorageBackend::list`] is implemented (else it reports [`StorageError::Unsupported`]).
    pub list: bool,
    /// The backend has ETags, so optimistic concurrency is possible: [`StorageBackend::stat`] and
    /// [`StorageBackend::write`] return one. [`StorageBackend::list`] entries may omit it (a local
    /// folder would have to read every file); `stat` the entry when the tag is needed.
    pub etag: bool,
    /// Bytes survive a process restart on the backend itself (as opposed to a pure cache).
    pub persistent: bool,
}

/// How [`StorageBackend::write`] should behave.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Only write when the stored version still carries this tag (an `ETag` from [`stat`][`StorageBackend::stat`]);
    /// a mismatch fails with [`StorageError::Conflict`]. `None` writes unconditionally.
    pub if_match: Option<String>,
    /// Stage-then-commit so a crash mid-write never leaves a half-written file visible.
    /// Backends without atomic support return [`StorageError::Unsupported`] when this is set.
    pub atomic: bool,
}

/// What [`StorageBackend::probe`] learned from the far end (a WebDAV `OPTIONS` round trip).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    /// Capabilities after talking to the server (never weaker than [`StorageBackend::capabilities`]
    /// where the probe failed; adapted where it succeeded).
    pub capabilities: Capabilities,
    /// Raw `DAV` header classes the server advertises (e.g. `1, 2, ordered-collections`).
    pub dav_classes: Vec<String>,
    /// Anything worth telling the user (fallbacks taken, features missing).
    pub notes: Vec<String>,
}

/// What [`StorageBackend::recover`] did with the [`journal`] left by interrupted writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Journal entries completed (a staged write committed) or confirmed already committed.
    pub completed: usize,
    /// Journal entries dropped because neither the staged nor the final file survived.
    pub dropped: usize,
    /// Orphan staging files removed (no journal entry names them).
    pub removed_temp: usize,
}

/// Everything that can go wrong talking to storage. Variants carry backend, path-free messages:
/// callers add context (the CLI prints `path: message`).
#[derive(Debug, Error)]
pub enum StorageError {
    /// Nothing stored under that name.
    #[error("not found: {0}")]
    NotFound(String),
    /// A conditional write lost its race (`If-Match` mismatch), or the name is already taken.
    #[error("conflict: {0}")]
    Conflict(String),
    /// The server refused the credentials, or they were missing.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// The backend or build cannot do this (e.g. plain-HTTP WebDAV asked for `https://`,
    /// `atomic` on a backend without staging).
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The far end or the request was malformed (bad URL, bad XML, unexpected status).
    #[error("protocol: {0}")]
    Protocol(String),
    /// Local I/O or socket failure.
    #[error("io: {0}")]
    Io(String),
    /// The name is invalid (empty, absolute, escapes its root via `..`).
    #[error("bad path: {0}")]
    BadPath(String),
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e.to_string())
    }
}

/// Where document bytes live. Implementations must be usable from any thread; none of these
/// methods is called on the typing path (only explicit save / autosave / CLI commands call in).
pub trait StorageBackend: Send + Sync {
    /// Read the whole file.
    fn read(&self, path: &str) -> Result<Vec<u8>, StorageError>;
    /// Store `bytes` under `path`, creating parent collections as needed.
    fn write(&self, path: &str, bytes: &[u8], opts: &WriteOptions) -> Result<FileMeta, StorageError>;
    /// Metadata for one path.
    fn stat(&self, path: &str) -> Result<FileMeta, StorageError>;
    /// Direct children of `prefix` (empty `prefix` lists the root). Always shallow (depth 1).
    fn list(&self, prefix: &str) -> Result<Vec<Entry>, StorageError>;
    /// Remove one stored file. Removing a missing file succeeds (idempotent).
    fn remove(&self, path: &str) -> Result<(), StorageError>;
    /// What this backend can do, without any I/O.
    fn capabilities(&self) -> Capabilities;
    /// Talk to the far end (if any) and refine [`capabilities`][Self::capabilities]. The default
    /// just reports the static capabilities with no notes.
    fn probe(&self) -> Result<ProbeReport, StorageError> {
        Ok(ProbeReport { capabilities: self.capabilities(), dav_classes: Vec::new(), notes: Vec::new() })
    }
    /// Replay or clear the [`journal`] left by interrupted writes. Safe to call at every startup.
    fn recover(&self) -> Result<RecoveryReport, StorageError> {
        Ok(RecoveryReport::default())
    }
}

/// Reject names that could escape a backend's root. Returns the cleaned relative path
/// (`a//b` becomes `a/b`).
pub(crate) fn clean_path(path: &str) -> Result<String, StorageError> {
    if path.is_empty() {
        return Err(StorageError::BadPath("empty path".into()));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(StorageError::BadPath(format!("{path}: absolute paths are not storage paths")));
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(StorageError::BadPath(format!("{path}: `..` escapes the storage root"))),
            _ if part.chars().any(|c| c.is_control()) => {
                return Err(StorageError::BadPath(format!("{path}: control characters are not storage names")));
            }
            // `C:/x`, `C:x` and `name:stream` re-root or redirect a Windows join; storage names
            // must stay portable across the platforms a synced folder is opened on.
            _ if part.contains(':') => {
                return Err(StorageError::BadPath(format!("{path}: `:` is not allowed in storage names")));
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(StorageError::BadPath(format!("{path}: names a collection, not a file")));
    }
    if parts.join("/").len() > 1024 {
        return Err(StorageError::BadPath(format!("{path}: path is longer than 1024 characters")));
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_path_rejects_escapes() {
        assert!(clean_path("").is_err());
        assert!(clean_path("/abs").is_err());
        assert!(clean_path("../up").is_err());
        assert!(clean_path("a/../../b").is_err());
        assert!(clean_path("a/./b").unwrap() == "a/b");
        assert!(clean_path("C:/Windows/x.dxf").is_err());
        assert!(clean_path("C:x.dxf").is_err());
        assert!(clean_path("a/b.dxf:stream").is_err());
    }
}
