//! Past the number of guarded folders kept, a new folder's guards apply to every folder searched, so
//! no font folder or search path entry is left unguarded.
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,overflow\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

/// With far more guarded folders than are kept, a font folder holding a font the guards refuse still
/// has it refused, and one the guards allow is still found.
#[test]
fn guards_past_the_limit_apply_to_every_folder() {
    let bytes = shx::compile(FONT).unwrap();
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-overflow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cadcraft-overflow-bad.shx"), &bytes).unwrap();
    std::fs::write(dir.join("cadcraft-overflow-ok.shx"), &bytes).unwrap();
    for n in 0..1100 {
        let folder = dir.join(format!("drawings-{n}"));
        shx::add_drawing_dir_guarded(
            &folder,
            "session",
            Arc::new(|file: &Path| !file.file_name().is_some_and(|n| n.to_string_lossy().contains("bad"))),
        );
    }
    // A later folder with the same owner and a guard that allows everything doesn't lift the others.
    shx::add_drawing_dir_guarded(&dir.join("last-drawings"), "session", Arc::new(|_: &Path| true));
    shx::set_font_dirs(vec![dir.clone()]);
    assert!(shx::find("cadcraft-overflow-bad.shx").is_none(), "refused by a guard given past the limit");
    assert!(shx::find("cadcraft-overflow-ok.shx").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}
