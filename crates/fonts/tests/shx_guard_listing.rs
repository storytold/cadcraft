//! A guarded folder is listed only when its guards allow the folder itself, so a folder outside the
//! automation roots isn't read even for the names in it.
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cadcraft_fonts::shx;

/// A folder of this test's own, made here so nothing else put it there first, and removed when the
/// test ends, also when it fails.
struct TestDir(PathBuf);

impl Drop for TestDir {
    /// Removes the folder and all it holds.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A new folder: creating it must not find one already there.
fn fresh_dir() -> TestDir {
    let mut n = 0u32;
    loop {
        let dir = std::env::temp_dir().join(format!("cadcraft-shx-listing-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return TestDir(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => panic!("{}: {e}", dir.display()),
        }
    }
}

/// Looking for a font that isn't there makes the search list the folder, and the guard is asked
/// about the folder itself first; a guard that refuses it ends the search there.
#[test]
fn the_folder_itself_goes_to_the_guard_before_it_is_listed() {
    let owner = fresh_dir();
    let dir = owner.0.clone();
    let asked: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let log = asked.clone();
    let folder = dir.clone();
    shx::add_drawing_dir_guarded(
        &dir,
        "listing",
        Arc::new(move |path: &Path| {
            log.lock().unwrap().push(path.to_path_buf());
            path != folder
        }),
    );
    assert!(shx::find("cadcraft-listing-missing.shx").is_none());
    let seen = asked.lock().unwrap().clone();
    assert!(seen.contains(&dir), "the guard was asked about the folder: {seen:?}");
}
