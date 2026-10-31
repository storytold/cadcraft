//! The engine refuses file writes and closes a [`consent`] scope doesn't allow, wherever the call
//! comes from: a command, a script or a LISP routine built at run time.

use serde_json::json;

use crate::Session;
use crate::consent::{self, Consent};

const WRITES: Consent = Consent { write_files: true, close_drawings: false };

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-consent-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_shared_writer_refuses_without_consent() {
    let dir = scratch("atomic");
    let path = dir.join("notes.txt");
    std::fs::write(&path, "the user's notes").unwrap();
    let e = consent::with(Consent::NONE, || crate::atomic_write::write_atomic(&path, b"0\nSECTION")).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the user's notes");
    consent::with(WRITES, || crate::atomic_write::write_atomic(&path, b"new")).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    std::fs::remove_dir_all(&dir).ok();
}

/// A LISP routine assembles the file name and the mode at run time: nothing the call names. Its
/// `open` for writing gets nil, and the file stays as it was.
#[test]
fn a_lisp_routine_cant_write_a_file_without_consent() {
    let dir = scratch("lisp");
    let path = dir.join("notes.txt");
    std::fs::write(&path, "the user's notes").unwrap();
    let p = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        "(setq f (open (strcat \"{}\" \"{}\") (chr 119))) (if f (progn (write-line \"gone\" f) (close f) \"wrote\") \"refused\")",
        &p[..p.len() - 4],
        &p[p.len() - 4..]
    );
    let mut s = Session::new();
    let r = consent::with(Consent::NONE, || s.execute("lisp", &json!({ "code": code }))).unwrap();
    assert_eq!(r["value"], "\"refused\"", "{r}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the user's notes");
    let r = consent::with(WRITES, || s.execute("lisp", &json!({ "code": code }))).unwrap();
    assert_eq!(r["value"], "\"wrote\"", "{r}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "gone\n");
    std::fs::remove_dir_all(&dir).ok();
}

/// A file opened while writing was allowed and closed while it isn't is not written.
#[test]
fn a_lisp_file_closed_without_consent_is_not_written() {
    let dir = scratch("lisp-close");
    let path = dir.join("out.txt");
    let p = path.to_string_lossy().replace('\\', "/");
    let mut s = Session::new();
    consent::with(WRITES, || s.execute("lisp", &json!({ "code": format!("(setq f (open \"{p}\" \"w\")) (write-line \"x\" f)") }))).unwrap();
    consent::with(Consent::NONE, || s.execute("lisp", &json!({ "code": "(close f)" }))).unwrap();
    assert!(!path.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tableexport_refuses_without_consent() {
    let dir = scratch("table");
    let path = dir.join("t.csv");
    let mut s = Session::new();
    let t = s.execute("table", &json!({ "at": [0, 0], "rows": 2, "cols": 2 })).unwrap();
    let handle = t["handle"].as_str().unwrap().to_string();
    let call = json!({ "handle": handle, "path": path.to_string_lossy() });
    let e = consent::with(Consent::NONE, || s.execute("tableexport", &call)).unwrap_err();
    assert!(e.to_string().contains("not allowed"), "{e}");
    assert!(!path.exists());
    consent::with(WRITES, || s.execute("tableexport", &call)).unwrap();
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// CLOSE and CLOSEALL lose what wasn't saved: they need their own consent, also from a script.
#[test]
fn close_and_closeall_refuse_without_consent() {
    let mut s = Session::new();
    s.new_drawing(true);
    let n = s.docs.len();
    for cmd in ["close", "closeall"] {
        let e = consent::with(WRITES, || s.execute(cmd, &json!({}))).unwrap_err();
        assert!(e.to_string().contains("not allowed"), "{e}");
        assert_eq!(s.docs.len(), n);
    }
    consent::with(WRITES, || s.script("close\n")).ok();
    assert_eq!(s.docs.len(), n);
    consent::with(Consent { write_files: false, close_drawings: true }, || s.execute("close", &json!({}))).unwrap();
    assert_eq!(s.docs.len(), n - 1);
}
