//! A guarded drawing folder: the font search asks the guard about every file it would open there,
//! before opening it (the automation roots use this to keep a font link from leading out of them).
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,guard\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

/// A new folder of this test's own.
fn fresh_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every candidate the search opens in a guarded folder goes to the guard first, whatever its name
/// or how the search finds it, and a font the guard refuses is skipped while the others are used.
#[test]
fn every_file_the_search_opens_goes_to_the_guard_first() {
    let bytes = shx::compile(FONT).unwrap();
    let dir = fresh_dir();
    std::fs::write(dir.join("cadcraft-guard-ok.shx"), &bytes).unwrap();
    std::fs::write(dir.join("cadcraft-guard-bad.shx"), &bytes).unwrap();
    // Found only by looking through the folder, as a link under another spelling would be.
    std::fs::write(dir.join("CadCraft-Guard-Mixed.SHX"), &bytes).unwrap();
    std::fs::write(dir.join("CadCraft-Guard-Badmixed.SHX"), &bytes).unwrap();
    let asked: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let log = asked.clone();
    shx::add_drawing_dir_guarded(
        &dir,
        Arc::new(move |file: &Path| {
            log.lock().unwrap().push(file.to_path_buf());
            !file.to_string_lossy().to_ascii_lowercase().contains("bad")
        }),
    );
    assert!(shx::find("cadcraft-guard-ok.shx").is_some());
    assert!(shx::find("cadcraft-guard-mixed.shx").is_some(), "a name found by looking through the folder");
    assert!(shx::find("cadcraft-guard-bad.shx").is_none(), "a font the guard refuses is skipped");
    assert!(shx::find("cadcraft-guard-badmixed.shx").is_none(), "also one found by looking through the folder");
    let asked = asked.lock().unwrap();
    for file in
        ["cadcraft-guard-ok.shx", "CADCRAFT-GUARD-OK.SHX", "cadcraft-guard-bad.shx", "CadCraft-Guard-Mixed.SHX", "CadCraft-Guard-Badmixed.SHX"]
    {
        assert!(asked.contains(&dir.join(file)), "the guard was asked about {file}: {asked:?}");
    }
    // The guard goes with the folder: added again without one, the folder is searched freely.
    shx::add_drawing_dir(&dir);
    assert!(shx::find("cadcraft-guard-bad.shx").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}
