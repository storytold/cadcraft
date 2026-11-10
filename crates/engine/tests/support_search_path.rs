//! `findfile` and INSERT of a file named by a relative path look in the current folder, then the
//! drawing's folder, then the support search path (#3003). The search path is process-wide (it is
//! also the SHX font search path), so the tests take turns.

use std::sync::{Mutex, MutexGuard, PoisonError};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Drawing, EntityKind};
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// A test drawing file: any bytes, one line in model space.
fn read(_bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let mut d = Drawing::new_imperial();
    d.model.push(cadcraft_engine::doc::Entity::new(
        cadcraft_engine::doc::Handle(0x60),
        EntityKind::Line(cadcraft_engine::doc::Line {
            a: cadcraft_engine::doc::geom::Vec3::new(0.0, 0.0, 0.0),
            b: cadcraft_engine::doc::geom::Vec3::new(10.0, 0.0, 0.0),
        }),
    ));
    Ok(d)
}

/// Serialises the tests and clears the search path before and after.
struct Turn {
    _guard: MutexGuard<'static, ()>,
}

fn turn() -> Turn {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    cadcraft_engine::fonts::shx::set_search_path("");
    Turn { _guard: guard }
}

impl Drop for Turn {
    fn drop(&mut self) {
        cadcraft_engine::fonts::shx::set_search_path("");
    }
}

/// A session with a drawing opened from `host` (its file's folder is the drawing's folder).
fn opened(host: &str) -> Session {
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host })).unwrap();
    s
}

/// `(findfile name)` as the file it found, or `None` for nil.
fn findfile(s: &mut Session, name: &str) -> Option<String> {
    let r = s.execute("lisp", &json!({ "code": format!("(findfile \"{name}\")") })).unwrap();
    let v = r["value"].as_str().unwrap().to_string();
    (v != "nil").then(|| v.trim_matches('"').replace("\\\\", "\\"))
}

fn block_names(s: &Session) -> Vec<String> {
    s.doc().unwrap().blocks.keys().cloned().collect()
}

#[test]
fn findfile_looks_in_the_drawings_folder() {
    let _t = turn();
    let dir = TempDir::new("findfile-drawing");
    let host = dir.file("host.dxf", "x");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    let part = dir.file("lib/blk.dxf", "x");
    let mut s = opened(&host);
    assert_eq!(findfile(&mut s, "lib/blk.dxf"), Some(part));
    // A name that is nowhere is still nil.
    assert_eq!(findfile(&mut s, "lib/nothing.dxf"), None);
}

#[test]
fn findfile_and_insert_use_the_support_search_path() {
    let _t = turn();
    let dir = TempDir::new("support-path");
    let host = dir.file("host.dxf", "x");
    let support = dir.join("support");
    std::fs::create_dir_all(support.join("lib")).unwrap();
    std::fs::write(support.join("lib").join("blk.dxf"), "x").unwrap();
    let mut s = opened(&host);

    // Nothing is found while the folder is not on the path ...
    assert_eq!(findfile(&mut s, "lib/blk.dxf"), None);
    assert!(s.execute("insert", &json!({ "file": "lib/blk", "at": [0, 0] })).is_err());

    // ... then SUPPORTPATH puts it there, and ACADPREFIX reports it.
    let list = support.to_string_lossy().into_owned();
    s.execute("setvar", &json!({ "name": "SUPPORTPATH", "value": list })).unwrap();
    assert_eq!(cadcraft_engine::sysvars::get(&s, "ACADPREFIX"), Some(json!(list)));
    assert!(s.execute("setvar", &json!({ "name": "ACADPREFIX", "value": "x" })).is_err(), "ACADPREFIX is read-only");
    assert_eq!(findfile(&mut s, "lib/blk.dxf"), Some(support.join("lib/blk.dxf").to_string_lossy().into_owned()));
    s.execute("insert", &json!({ "file": "lib/blk", "at": [0, 0] })).unwrap();
    assert_eq!(block_names(&s), vec!["blk".to_string()]);
}

#[test]
fn insert_finds_a_relative_file_in_the_drawings_folder() {
    let _t = turn();
    let dir = TempDir::new("insert-drawing");
    let host = dir.file("host.dxf", "x");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    dir.file("lib/blk.dxf", "x");
    let mut s = opened(&host);
    s.execute("insert", &json!({ "file": "lib/blk", "at": [0, 0] })).unwrap();
    assert_eq!(block_names(&s), vec!["blk".to_string()]);
    // A file that is nowhere reports the name as given.
    let e = s.execute("insert", &json!({ "file": "lib/none", "at": [0, 0] })).unwrap_err().to_string();
    assert!(e.contains("lib/none"), "{e}");
}
