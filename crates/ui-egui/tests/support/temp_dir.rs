//! A test's own folder in the system temp directory, removed when the test ends, even when an
//! assertion fails. Shared by the integration tests through `#[path]`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// `cadcraft-<tag>-<pid>-<n>`: the process id keeps parallel test binaries apart and the counter
/// keeps the tests of one binary apart, so no two tests ever share (or delete) one folder.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cadcraft-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test's temp folder");
        Self(dir)
    }

    /// Write `contents` to `name` in the folder; returns the file's path as text.
    pub fn file(&self, name: &str, contents: impl AsRef<[u8]>) -> String {
        let p = self.0.join(name);
        std::fs::write(&p, contents).expect("write a test file");
        p.to_string_lossy().into_owned()
    }

    /// The path of `name` in the folder as text, without creating it.
    pub fn path_str(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
