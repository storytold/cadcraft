//! Past the number of guarded folders kept, a new folder's guards apply to every folder searched, so
//! no font folder or search path entry is left unguarded.
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,overflow\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

/// A folder of this test's own, made here so nothing else put it there first, and removed when the
/// test ends, also when it fails.
struct TestDir(std::path::PathBuf);

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

/// With far more guarded folders than are kept, a font folder holding a font the guards refuse still
/// has it refused, and one the guards allow is still found.
#[test]
fn guards_past_the_limit_apply_to_every_folder() {
    let bytes = shx::compile(FONT).unwrap();
    let owner = fresh_dir("overflow");
    let dir = owner.0.clone();
    std::fs::write(dir.join("cadcraft-overflow-bad.shx"), &bytes).unwrap();
    std::fs::write(dir.join("cadcraft-overflow-ok.shx"), &bytes).unwrap();
    // The guard reads the name without its case: the search also asks for the upper-case spelling,
    // which a file system that ignores case opens as the same file.
    for n in 0..1100 {
        let folder = dir.join(format!("drawings-{n}"));
        shx::add_drawing_dir_guarded(
            &folder,
            "session",
            Arc::new(|file: &Path| !file.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("bad"))),
        );
    }
    // A later folder with the same owner and a guard that allows everything doesn't lift the others.
    shx::add_drawing_dir_guarded(&dir.join("last-drawings"), "session", Arc::new(|_: &Path| true));
    shx::set_font_dirs(vec![dir.clone()]);
    assert!(shx::find("cadcraft-overflow-bad.shx").is_none(), "refused by a guard given past the limit");
    assert!(shx::find("cadcraft-overflow-ok.shx").is_some());
}
