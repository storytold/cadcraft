//! A save over the drawing's own file refuses when another program changed the file after it was
//! opened or last saved here (CADCraft keeps no file open, so nothing else would stop it), and
//! `overwrite: true` replaces it anyway. A separate test binary because it installs the
//! process-wide file hooks, as `save_backup.rs` does.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-disk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn add_circle(s: &mut Session, x: f64) {
    s.execute("circle", &json!({ "center": [x, 0], "radius": 1 })).unwrap();
}

#[test]
fn qsave_refuses_a_file_someone_else_saved_and_overwrite_replaces_it() {
    hooks();
    let dir = folder("qsave");
    let file = dir.join("plan.dxf");
    let path = file.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": path })).unwrap();

    // Untouched on disk: saves go through, each compared with what the one before wrote.
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    add_circle(&mut s, 10.0);
    s.execute("qsave", &json!({})).unwrap();

    // Another program saves the same file, here with the same length.
    let mut theirs = std::fs::read(&file).unwrap();
    let last = theirs.len() - 1;
    theirs[last] ^= 0x20;
    std::fs::write(&file, &theirs).unwrap();
    add_circle(&mut s, 15.0);
    let e = s.execute("qsave", &json!({})).unwrap_err().to_string();
    assert!(e.contains("changed on disk"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), theirs, "their work stays");
    assert!(s.state().unwrap().is_dirty(), "the drawing still has its unsaved changes");
    assert!(s.execute("saveas", &json!({ "path": path })).is_err(), "SAVEAS onto its own file checks too");

    // A copy under another name is always possible, and leaves their file alone.
    let copy = dir.join("plan-mine.dxf").to_string_lossy().to_string();
    s.execute("saveas", &json!({ "path": copy })).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), theirs);
    s.execute("qsave", &json!({})).unwrap();

    // Back onto the original with overwrite: replaced on purpose.
    s.execute("saveas", &json!({ "path": path, "overwrite": true })).unwrap();
    assert_ne!(std::fs::read(&file).unwrap(), theirs);
    assert!(s.execute("qsave", &json!({ "overwrite": "yes" })).is_err(), "overwrite must be a bool");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_opened_and_changed_elsewhere_is_not_saved_over() {
    hooks();
    let dir = folder("open");
    let file = dir.join("plan.dxf");
    let path = file.to_string_lossy().to_string();
    let mut first = Session::new();
    add_circle(&mut first, 0.0);
    first.execute("saveas", &json!({ "path": path })).unwrap();

    // Two sessions open the same file; the first saves, then the second tries to.
    let mut second = Session::new();
    second.execute("open", &json!({ "path": path })).unwrap();
    first.execute("open", &json!({ "path": path })).unwrap();
    add_circle(&mut first, 5.0);
    first.execute("qsave", &json!({})).unwrap();
    let saved_by_first = std::fs::read(&file).unwrap();
    add_circle(&mut second, 10.0);
    let e = second.execute("qsave", &json!({})).unwrap_err().to_string();
    assert!(e.contains("changed on disk"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), saved_by_first, "the first session's save stays");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_file_or_a_drawing_never_saved_has_nothing_to_check() {
    hooks();
    let dir = folder("gone");
    let file = dir.join("plan.dxf");
    let path = file.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": path })).unwrap();
    std::fs::remove_file(&file).unwrap();
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    assert!(file.exists());

    // A drawing that never came from this file has nothing to compare: SAVEAS over it behaves as before.
    let mut fresh = Session::new();
    add_circle(&mut fresh, 0.0);
    fresh.execute("saveas", &json!({ "path": path })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
