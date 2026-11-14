//! When even the overflow guards run out, every font is refused, whatever name the owners gave.
// Integration tests: unwrapping on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use cadcraft_fonts::shx;

const FONT: &str = "*0,4,closed\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

/// Past every limit, a font that no guard objects to is still refused, also when an owner is called
/// `closed`.
#[test]
fn guards_that_ran_out_refuse_every_font() {
    let bytes = shx::compile(FONT).unwrap();
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-closed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cadcraft-closed-ok.shx"), &bytes).unwrap();
    shx::set_font_dirs(vec![dir.clone()]);
    assert!(shx::find("cadcraft-closed-ok.shx").is_some(), "found while nothing guards the folder");
    for n in 0..6000 {
        shx::add_drawing_dir_guarded(&dir.join(format!("drawings-{n}")), "closed", Arc::new(|_: &Path| true));
    }
    assert!(shx::find("cadcraft-closed-ok.shx").is_none(), "refused once the guards ran out");
    std::fs::remove_dir_all(&dir).unwrap();
}
