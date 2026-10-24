//! Saving over a drawing keeps the previous file as `<name>.bak` (ISAVEBAK, #540), and the
//! save itself replaces the file atomically: no temporary files are left behind.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, backup_path, looks_like_drawing, set_io};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cadcraft-bak-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn circles(path: &std::path::Path) -> usize {
    let d = cadcraft_io::read(&std::fs::read(path).unwrap(), &path.to_string_lossy()).unwrap();
    d.model.iter().filter(|e| matches!(e.kind, cadcraft_engine::doc::EntityKind::Circle(_))).count()
}

/// Only the drawing and its backup are in the folder: atomic saves leave no staging files.
fn files(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    v.sort();
    v
}

/// ISAVEBAK is process-wide: the tests that save hold this lock, so one turning it off can't
/// race another's save. Dropping the guard turns it back on, also when a test fails.
static SAVES: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Saving(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for Saving {
    fn drop(&mut self) {
        cadcraft_engine::autosave::set_isavebak(true);
    }
}

fn saving() -> Saving {
    let g = SAVES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cadcraft_engine::autosave::set_isavebak(true);
    Saving(g)
}

fn add_circle(s: &mut Session, x: f64) {
    s.execute("circle", &json!({ "center": [x, 0], "radius": 1 })).unwrap();
}

#[test]
fn each_save_keeps_the_previous_file_as_bak() {
    let _saving = saving();
    hooks();
    let dir = temp_dir("rotate");
    let path = dir.join("Plan.dxf");
    let p = path.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    assert_eq!(files(&dir), ["Plan.dxf"], "a first save has nothing to back up");

    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(files(&dir), ["Plan.bak", "Plan.dxf"]);
    assert_eq!((circles(&path), circles(&backup_path(&path))), (2, 1), "the .bak is the previous save");

    add_circle(&mut s, 10.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!((circles(&path), circles(&backup_path(&path))), (3, 2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_file_never_replaces_a_good_backup_and_isavebak_0_keeps_none() {
    let _saving = saving();
    hooks();
    let dir = temp_dir("damaged");
    let path = dir.join("Plan.dxf");
    let p = path.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(circles(&backup_path(&path)), 1);

    // Another program truncates the drawing; the next save keeps the good backup.
    std::fs::write(&path, b"").unwrap();
    add_circle(&mut s, 10.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!((circles(&path), circles(&backup_path(&path))), (3, 1));

    // ISAVEBAK 0: the backup is left as it was.
    s.execute("setvar", &json!({ "name": "ISAVEBAK", "value": 0 })).unwrap();
    add_circle(&mut s, 15.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!((circles(&path), circles(&backup_path(&path))), (4, 1));
    assert_eq!(files(&dir), ["Plan.bak", "Plan.dxf"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drawings_are_recognised_by_their_start() {
    assert!(looks_like_drawing(b"AC1032\0\0\0\0\0"));
    assert!(looks_like_drawing(b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n"));
    assert!(looks_like_drawing(b"999\ndxfrw\n0\nSECTION\n"));
    assert!(looks_like_drawing(b"AutoCAD Binary DXF\r\n\x1a\0"));
    for junk in [&b""[..], b"\0\0\0\0", b"0\nEOF\n", b"<svg>", b"hello"] {
        assert!(!looks_like_drawing(junk), "{junk:?}");
    }
}

/// A private drawing (0600) keeps a private backup: a new .bak, and a .bak already there that
/// others could read, both take the drawing's mode.
#[cfg(unix)]
#[test]
fn the_backup_is_no_more_readable_than_the_drawing() {
    use std::os::unix::fs::PermissionsExt;
    let _saving = saving();
    hooks();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = temp_dir("mode");
    let path = dir.join("Plan.dxf");
    let p = path.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(mode(&backup_path(&path)), 0o600, "a new .bak");

    std::fs::set_permissions(backup_path(&path), std::fs::Permissions::from_mode(0o644)).unwrap();
    add_circle(&mut s, 10.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(mode(&backup_path(&path)), 0o600, "a .bak already there");
    assert_eq!(circles(&backup_path(&path)), 2);

    // A .bak kept more private than the drawing is never widened.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(backup_path(&path), std::fs::Permissions::from_mode(0o600)).unwrap();
    add_circle(&mut s, 15.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(mode(&backup_path(&path)), 0o600, "a more private .bak");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A symlink planted at the .bak is replaced by the backup, never written through: the file it
/// pointed at is left as it was.
#[cfg(unix)]
#[test]
fn a_symlink_at_the_backup_is_not_followed() {
    let _saving = saving();
    hooks();
    let dir = temp_dir("link");
    let path = dir.join("Plan.dxf");
    let p = path.to_string_lossy().to_string();
    let other = dir.join("other.txt");
    std::fs::write(&other, b"someone else's file").unwrap();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    std::os::unix::fs::symlink(&other, backup_path(&path)).unwrap();
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(std::fs::read(&other).unwrap(), b"someone else's file");
    assert!(std::fs::symlink_metadata(backup_path(&path)).unwrap().is_file(), "the .bak is a file now");
    assert_eq!(circles(&backup_path(&path)), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
