//! A guarded drawing folder: the font search asks the guard about every file it would open there,
//! before opening it (the automation roots use this to keep a font link from leading out of them).
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,guard\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

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

/// The lower-case file name of `file`, so a guard never reads the test folder's own name.
fn name(file: &Path) -> String {
    file.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Every candidate the search opens in a guarded folder goes to the guard first, whatever its name
/// or how the search finds it, and a font the guard refuses is skipped while the others are used.
#[test]
fn every_file_the_search_opens_goes_to_the_guard_first() {
    let bytes = shx::compile(FONT).unwrap();
    let owner = fresh_dir("guard");
    let dir = owner.0.clone();
    std::fs::write(dir.join("cadcraft-guard-ok.shx"), &bytes).unwrap();
    std::fs::write(dir.join("cadcraft-guard-bad.shx"), &bytes).unwrap();
    // Found only by looking through the folder, as a link under another spelling would be.
    std::fs::write(dir.join("CadCraft-Guard-Mixed.SHX"), &bytes).unwrap();
    std::fs::write(dir.join("CadCraft-Guard-Badmixed.SHX"), &bytes).unwrap();
    let asked: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let log = asked.clone();
    shx::add_drawing_dir_guarded(
        &dir,
        "first",
        Arc::new(move |file: &Path| {
            log.lock().unwrap().push(file.to_path_buf());
            !name(file).contains("bad")
        }),
    );
    assert!(shx::find("cadcraft-guard-ok.shx").is_some());
    assert!(shx::find("cadcraft-guard-mixed.shx").is_some(), "a name found by looking through the folder");
    assert!(shx::find("cadcraft-guard-bad.shx").is_none(), "a font the guard refuses is skipped");
    assert!(shx::find("cadcraft-guard-badmixed.shx").is_none(), "also one found by looking through the folder");
    let asked = asked.lock().unwrap();
    // The guard saw each font's file under the spelling the search tried (a file system that
    // ignores case answers the first one, so only the letters are compared).
    for file in ["cadcraft-guard-ok.shx", "cadcraft-guard-bad.shx", "cadcraft-guard-mixed.shx", "cadcraft-guard-badmixed.shx"] {
        let want = dir.join(file).to_string_lossy().to_ascii_lowercase();
        assert!(asked.iter().any(|a| a.to_string_lossy().to_ascii_lowercase() == want), "the guard was asked about {file}: {asked:?}");
    }
    drop(asked);
    // The folder added again without a guard (another session in the process) keeps its guard.
    shx::add_drawing_dir(&dir);
    assert!(shx::find("cadcraft-guard-bad.shx").is_none());
    // Another owner's guard is required as well: a font is opened only when every owner allows it.
    shx::add_drawing_dir_guarded(&dir, "second", Arc::new(|file: &Path| !name(file).contains("ok")));
    assert!(shx::find("cadcraft-guard-ok.shx").is_none(), "refused by the second owner");
    assert!(shx::find("cadcraft-guard-mixed.shx").is_some(), "allowed by both");
    // An owner giving its guard again replaces its own, and the other's stays.
    shx::add_drawing_dir_guarded(&dir, "second", Arc::new(|_: &Path| true));
    assert!(shx::find("cadcraft-guard-ok.shx").is_some());
    assert!(shx::find("cadcraft-guard-bad.shx").is_none(), "the first owner's guard still holds");
    // The same folder named as a font folder or on the search path is held to its guards too.
    shx::set_font_dirs(vec![dir.clone()]);
    shx::set_search_path(&dir.to_string_lossy());
    assert!(shx::find("cadcraft-guard-bad.shx").is_none(), "a font folder or the search path doesn't lift the guard");
    assert!(shx::find("cadcraft-guard-ok.shx").is_some());
    // The folder falling off the list of drawing folders (the list is short) keeps its guards while a
    // font folder or the search path still reaches it.
    for n in 0..40 {
        shx::add_drawing_dir(&dir.join(format!("other-{n}")));
    }
    assert!(!shx::drawing_dirs().contains(&dir));
    assert!(shx::find("cadcraft-guard-bad.shx").is_none(), "the outside font stays rejected after the eviction");
    assert!(shx::find("cadcraft-guard-ok.shx").is_some());
}
