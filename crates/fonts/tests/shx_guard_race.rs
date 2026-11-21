//! A guard given to a folder while a font search reads it: the search never gives a font that the
//! new guard refuses.
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,race\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

/// A folder of this test's own, made here so nothing else put it there first, and removed when the
/// test ends, also when it fails.
struct TestDir(PathBuf);

impl Drop for TestDir {
    /// Removes the folder and all it holds.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A new folder for `tag`: creating it must not find one already there.
fn fresh_dir(tag: &str) -> TestDir {
    let mut n = 0u32;
    loop {
        let dir = std::env::temp_dir().join(format!("cadcraft-shx-{tag}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return TestDir(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => panic!("{}: {e}", dir.display()),
        }
    }
}

/// A provider with no fonts that, the first time it is asked, gives `dir` a guard refusing every
/// file. The search asks it after taking its view of the folders and before reading them, so the
/// guard arrives while the search is under way.
struct LateGuard {
    dir: PathBuf,
    fired: AtomicBool,
}

impl shx::FontProvider for LateGuard {
    fn load(&self, _file_name: &str) -> Option<Vec<u8>> {
        if !self.fired.swap(true, Ordering::SeqCst) {
            shx::add_drawing_dir_guarded(&self.dir, "late", Arc::new(|_: &Path| false));
        }
        None
    }
}

#[test]
fn a_guard_given_during_a_search_refuses_the_font_it_was_reading() {
    let bytes = shx::compile(FONT).unwrap();
    let owner = fresh_dir("race");
    let dir = owner.0.clone();
    std::fs::write(dir.join("cadcraft-race.shx"), &bytes).unwrap();
    shx::set_font_dirs(vec![dir.clone()]);
    assert!(shx::find("cadcraft-race.shx").is_some(), "found while nothing guards the folder");
    shx::set_provider(Some(Arc::new(LateGuard { dir: dir.clone(), fired: AtomicBool::new(false) })));
    assert!(shx::find("cadcraft-race.shx").is_none(), "a font the folder's new guard refuses was given");
    // Asked again, the guard still holds.
    assert!(shx::find("cadcraft-race.shx").is_none());
}
