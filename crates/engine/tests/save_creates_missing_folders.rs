//! A save to a folder that doesn't exist writes the file anyway and says what it created, so a
//! mistyped path costs a stray folder, not the drawing (#2215).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-new-folder-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn session() -> Session {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let mut s = Session::empty();
    s.open_drawing(Drawing::new_imperial(), "a.dxf", None);
    s
}

#[test]
fn saveas_and_export_write_the_file_and_report_the_new_folder() {
    let dir = scratch("cmds");
    let mut s = session();
    for (cmd, name) in [("saveas", "one"), ("export", "two")] {
        let top = dir.join(name);
        let path = top.join("deeper").join("a.dxf");
        let r = s.execute(cmd, &json!({ "path": path.to_string_lossy() })).unwrap();
        assert!(path.is_file(), "{cmd} wrote no file");
        assert_eq!(r["createdFolder"], json!(top.to_string_lossy()), "{cmd}");
        let said = top.display().to_string();
        assert!(s.log.iter().any(|l| l.contains(&said) && l.starts_with("Created the folder")), "{cmd}: {:?}", s.log);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_into_an_existing_folder_reports_nothing() {
    let dir = scratch("quiet");
    let mut s = session();
    let path = dir.join("a.dxf");
    let r = s.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap();
    assert!(r.get("createdFolder").is_none());
    assert!(!s.log.iter().any(|l| l.starts_with("Created the folder")), "{:?}", s.log);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_that_cannot_be_made_fails_and_says_so() {
    let dir = scratch("blocked");
    let blocker = dir.join("a_file");
    std::fs::write(&blocker, b"x").unwrap();
    let mut s = session();
    let path = blocker.join("sub").join("a.dxf");
    let e = s.execute("saveas", &json!({ "path": path.to_string_lossy() })).unwrap_err().to_string();
    assert!(e.contains("could not be created"), "{e}");
    assert!(!s.log.iter().any(|l| l.starts_with("Created the folder")), "{:?}", s.log);
    let _ = std::fs::remove_dir_all(&dir);
}
