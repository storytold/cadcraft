//! TrueType fonts in the user's font folders (Options ▸ Fonts): found by file and family name,
//! listed first, and gone again when the folder is removed. Font folders are process-wide and a
//! change looks for fonts again, so this is one test in its own binary (run beside other tests,
//! it would empty the font table under them).

use std::path::{Path, PathBuf};

use cadcraft_fonts::ttf;

/// A TrueType file from the system's fonts to copy, if there is one (a bounded walk).
fn a_system_ttf() -> Option<PathBuf> {
    let mut stack: Vec<PathBuf> =
        ["C:/Windows/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts", "/usr/share/fonts", "/usr/local/share/fonts"]
            .map(PathBuf::from)
            .to_vec();
    let mut seen = 0;
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            seen += 1;
            if seen > 5_000 {
                return None;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttf")) {
                return Some(p);
            }
        }
    }
    None
}

fn copy_into(src: &Path, dir: &Path) -> bool {
    std::fs::create_dir_all(dir).is_ok() && std::fs::copy(src, dir.join("CadcraftTestFace.ttf")).is_ok()
}

#[test]
fn a_font_in_a_user_folder_is_found_listed_and_dropped_with_the_folder() {
    let Some(src) = a_system_ttf() else { return };
    let dir = std::env::temp_dir().join(format!("cadcraft-fontdir-{}", std::process::id()));
    if !copy_into(&src, &dir) {
        return;
    }
    let before = ttf::generation();
    ttf::set_font_dirs(&[dir.to_string_lossy().to_string()]);
    assert!(ttf::generation() > before);
    assert!(ttf::find("CadcraftTestFace.ttf").is_some());
    let listed = ttf::installed();
    let entry = listed.iter().find(|f| f.name == "CadcraftTestFace.ttf");
    assert!(entry.is_some_and(|f| f.user && f.family.is_some()), "{entry:?}");
    assert!(listed.first().is_some_and(|f| f.user), "the user's fonts come first");
    if let Some(family) = entry.and_then(|f| f.family.clone()) {
        assert!(ttf::find(&family).is_some(), "found by its family name: {family}");
    }
    ttf::set_font_dirs::<&str>(&[]);
    assert!(ttf::find("CadcraftTestFace.ttf").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
