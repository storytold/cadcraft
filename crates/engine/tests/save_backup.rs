//! Saving over a drawing keeps the previous file as `<name>.bak` (ISAVEBAK, #540), and the
//! save itself replaces the file atomically: no temporary files are left behind.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, backup_path, looks_complete, looks_like_drawing, save_keeps_current, set_io};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    // Saves at a chosen version (a DWG 2000), as the app's.
    cadcraft_engine::cmd::file::set_write_as(|d, n, o| {
        cadcraft_io::write_with(d, n, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cadcraft-bak-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn circles(path: &std::path::Path) -> usize {
    let d = cadcraft_io::read(&std::fs::read(path).unwrap(), &*path.to_string_lossy()).unwrap();
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
    assert_eq!(files(&dir), ["Plan.dxf", "Plan.dxf.bak"]);
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

    // Cut off with a drawing's start kept (half a DXF, a DWG's version tag alone): the backup
    // stays too.
    let whole = std::fs::read(&path).unwrap();
    std::fs::write(&path, &whole[..whole.len() / 2]).unwrap();
    add_circle(&mut s, 11.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(circles(&backup_path(&path)), 1, "half a DXF");
    std::fs::write(&path, b"AC1032").unwrap();
    add_circle(&mut s, 12.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(circles(&backup_path(&path)), 1, "a DWG's tag alone");

    // ISAVEBAK 0: the backup is left as it was.
    s.execute("setvar", &json!({ "name": "ISAVEBAK", "value": 0 })).unwrap();
    add_circle(&mut s, 15.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!((circles(&path), circles(&backup_path(&path))), (6, 1));
    assert_eq!(files(&dir), ["Plan.dxf", "Plan.dxf.bak"]);
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

/// A drawing looks whole from its end: an ASCII DXF ends with 0/EOF, a binary one with EOF and
/// its NUL, and a DWG is not a few bytes long.
#[test]
fn whole_drawings_are_told_from_cut_off_ones() {
    let ascii = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
    assert!(looks_complete(ascii, ascii, ascii.len() as u64));
    let cut = &ascii[..30];
    assert!(!looks_complete(cut, cut, cut.len() as u64));
    let mut binary = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    binary.extend_from_slice(b"\0\0SECTION\0\0\0EOF\0");
    assert!(looks_complete(&binary, &binary, binary.len() as u64));
    assert!(!looks_complete(&binary, &binary[..binary.len() - 5], binary.len() as u64 - 5));
    assert!(!looks_complete(b"AC1032", b"AC1032", 6));
    assert!(!looks_complete(b"AC1032", b"", 30_000), "a DWG tag without its header is not whole");
}

/// A DWG reaches the end its own header gives: cut off anywhere (not only right after its start),
/// in a 2018 and a 2000 file, it doesn't replace the good backup; whole, it does.
#[test]
fn a_dwg_cut_off_anywhere_keeps_the_good_backup() {
    let _saving = saving();
    hooks();
    for version in [2018, 2000] {
        let dir = temp_dir(&format!("dwg{version}"));
        let path = dir.join("Plan.dwg");
        let p = path.to_string_lossy().to_string();
        let mut s = Session::new();
        add_circle(&mut s, 0.0);
        s.execute("saveas", &json!({ "path": p, "version": version })).unwrap();
        add_circle(&mut s, 5.0);
        s.execute("qsave", &json!({})).unwrap();
        assert_eq!(circles(&backup_path(&path)), 1, "{version}: the first save is backed up");
        let whole = std::fs::read(&path).unwrap();
        let head = &whole[..whole.len().min(4096)];
        assert!(looks_complete(head, &whole[whole.len().saturating_sub(256)..], whole.len() as u64), "{version}: whole");
        for keep in [whole.len() / 2, whole.len() * 9 / 10, whole.len() - 1] {
            assert!(!looks_complete(head, &[], keep as u64), "{version}: cut to {keep} of {}", whole.len());
        }
        // Another program leaves it cut off at 90%: the next save keeps the good backup.
        std::fs::write(&path, &whole[..whole.len() * 9 / 10]).unwrap();
        add_circle(&mut s, 10.0);
        s.execute("qsave", &json!({})).unwrap();
        assert_eq!(circles(&backup_path(&path)), 1, "{version}: the cut-off file didn't replace it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The checks can be wrong about a real file. With no backup yet, a file that doesn't look whole
/// is still kept, because the save is about to replace the only copy of it.
#[test]
fn a_file_that_looks_damaged_is_kept_when_there_is_no_backup_yet() {
    let _saving = saving();
    hooks();
    let dir = temp_dir("first");
    let path = dir.join("Plan.dxf");
    let p = path.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    let odd = b"AC1032 but not as this header expects it".to_vec();
    std::fs::write(&path, &odd).unwrap();
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(std::fs::read(backup_path(&path)).unwrap(), odd);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the warning promises and what the save does agree. A 2007 DWG, whose completeness can't
/// be told, replaces an older backup; a file known to be cut off doesn't.
#[test]
fn the_backup_a_save_promises_is_the_one_it_writes() {
    let _saving = saving();
    hooks();
    let dir = temp_dir("promise");
    let path = dir.join("Plan.dxf");
    let p = path.to_string_lossy().to_string();
    let mut s = Session::new();
    add_circle(&mut s, 0.0);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    add_circle(&mut s, 5.0);
    s.execute("qsave", &json!({})).unwrap();
    let old = std::fs::read(backup_path(&path)).unwrap();

    let mut dwg2007 = b"AC1021".to_vec();
    dwg2007.resize(4096, 7);
    std::fs::write(&path, &dwg2007).unwrap();
    assert!(save_keeps_current(&path));
    add_circle(&mut s, 10.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(std::fs::read(backup_path(&path)).unwrap(), dwg2007, "the promise was kept");

    std::fs::write(backup_path(&path), &old).unwrap();
    std::fs::write(&path, b"AC1032").unwrap();
    assert!(!save_keeps_current(&path));
    add_circle(&mut s, 15.0);
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(std::fs::read(backup_path(&path)).unwrap(), old, "cut off: nothing was promised");

    cadcraft_engine::autosave::set_isavebak(false);
    assert!(!save_keeps_current(&path), "backups off");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `plan.dwg` and `plan.dxf` in one folder keep their own backups, in the DWG's letter case.
#[test]
fn drawings_sharing_a_name_keep_their_own_backups() {
    let name = |p: &str| backup_path(std::path::Path::new(p)).to_string_lossy().into_owned();
    assert_eq!(name("x/plan.dwg"), "x/plan.bak");
    assert_eq!(name("x/plan.DWG"), "x/plan.BAK");
    assert_eq!(name("x/plan.dxf"), "x/plan.dxf.bak");
    assert_eq!(name("x/plan.dwt"), "x/plan.dwt.bak");
}
