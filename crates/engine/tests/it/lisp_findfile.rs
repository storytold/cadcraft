//! LISP `findfile` finds folders as well as files (#2985): routines test whether an optional
//! module is installed with `(findfile "module/.")`.

use cadcraft_engine::Session;
use serde_json::json;

#[path = "../support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// `(findfile name)` as LISP prints it.
fn findfile(name: &str) -> String {
    let mut s = Session::new();
    let r = s.execute("lisp", &json!({ "code": format!("(findfile \"{name}\")") })).unwrap();
    r["value"].as_str().unwrap().to_string()
}

#[test]
fn findfile_finds_a_folder_with_and_without_a_trailing_dot() {
    let dir = TempDir::new("findfile-folder");
    std::fs::create_dir_all(dir.join("module")).unwrap();
    // Forward slashes, so the name needs no escaping in the LISP string on Windows.
    let folder = dir.join("module").to_string_lossy().replace('\\', "/");
    assert_eq!(findfile(&folder), format!("\"{folder}\""));
    assert_eq!(findfile(&format!("{folder}/.")), format!("\"{folder}/.\""));
    // A folder that isn't there is still nil.
    assert_eq!(findfile(&format!("{folder}-missing")), "nil");
    assert_eq!(findfile(&format!("{folder}-missing/.")), "nil");
}

#[test]
fn findfile_still_finds_a_file() {
    let dir = TempDir::new("findfile-file");
    let file = dir.file("routine.lsp", "(princ)").replace('\\', "/");
    assert_eq!(findfile(&file), format!("\"{file}\""));
}
