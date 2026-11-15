//! When even the overflow guards run out, every font is refused, whatever name the owners gave.
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,closed\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

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

/// Past every limit, a font that no guard objects to is still refused, also when an owner is called
/// `closed`.
#[test]
fn guards_that_ran_out_refuse_every_font() {
    let bytes = shx::compile(FONT).unwrap();
    let owner = fresh_dir("closed");
    let dir = owner.0.clone();
    std::fs::write(dir.join("cadcraft-closed-ok.shx"), &bytes).unwrap();
    shx::set_font_dirs(vec![dir.clone()]);
    assert!(shx::find("cadcraft-closed-ok.shx").is_some(), "found while nothing guards the folder");
    for n in 0..6000 {
        shx::add_drawing_dir_guarded(&dir.join(format!("drawings-{n}")), "closed", Arc::new(|_: &Path| true));
    }
    assert!(shx::find("cadcraft-closed-ok.shx").is_none(), "refused once the guards ran out");
}
