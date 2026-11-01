//! APPLOAD (#1820): typed or chosen from the menu it picks a `.lsp` file in the desktop's file
//! dialog; a JSON call carries its path and never opens one; a load that fails reports an error
//! on the command line.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-appload-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    dir
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("a test file");
    path.to_string_lossy().to_string()
}

/// An app whose `.lsp` picker answers `answer` and records the directory it was given.
fn app_with_picker(answer: Option<String>) -> (CadApp, Rc<RefCell<Vec<String>>>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&asked);
    let services = Services {
        pick_lisp: Some(Box::new(move |dir: &str| {
            log.borrow_mut().push(dir.to_string());
            answer.clone()
        })),
        ..Services::default()
    };
    (CadApp::new(Session::new(), services), asked)
}

#[test]
fn a_json_path_loads_the_file_and_lists_its_commands_and_never_picks() {
    let dir = temp_dir("json");
    let path = write(&dir, "tools.lsp", "(defun c:hi () (princ \"hi\") (princ)) (defun c:bye () (princ))");
    let (mut app, asked) = app_with_picker(Some(path.clone()));

    let r = app.run("appload", json!({ "path": path })).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(r["commands"], json!(["BYE", "HI"]));
    assert!(asked.borrow().is_empty(), "a JSON call never opens a file dialog");
    assert!(app.session.log.iter().any(|l| l == "tools.lsp successfully loaded."), "{:?}", app.session.log);
    app.session.cmdline("HI").unwrap();
    assert_eq!(app.session.log.last().map(String::as_str), Some("hi"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refused_and_failed_loads_are_ordinary_errors_and_never_pick() {
    let dir = temp_dir("errors");
    let (mut app, asked) = app_with_picker(Some(write(&dir, "picked.lsp", "")));

    // A file that is not a LISP routine is refused before it is read.
    let text = write(&dir, "notes.txt", "(defun c:no () (princ))");
    let e = app.run("appload", json!({ "path": text })).unwrap_err();
    assert!(e.contains(".lsp"), "{e}");
    assert!(app.session.log.iter().any(|l| l.contains(".lsp")), "reported on the command line: {:?}", app.session.log);

    // A missing file is an ordinary error, not a crash.
    let missing = dir.join("missing.lsp").to_string_lossy().to_string();
    let e = app.run("appload", json!({ "path": missing })).unwrap_err();
    assert!(e.contains("can't read"), "{e}");

    // A file that does not parse is reported too.
    let broken = write(&dir, "broken.lsp", "(defun c:oops (");
    let e = app.run("appload", json!({ "path": broken })).unwrap_err();
    assert!(!e.is_empty());
    assert!(app.session.log.iter().any(|l| l.contains("error")), "{:?}", app.session.log);

    assert!(asked.borrow().is_empty(), "failed loads never open a file dialog");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typed_appload_opens_the_picker_and_remembers_the_directory() {
    let dir = temp_dir("pick");
    let path = write(&dir, "tools.lsp", "(defun c:hi () (princ \"hi\") (princ))");
    let (mut app, asked) = app_with_picker(Some(path));

    app.cmdline("appload");
    assert_eq!(*asked.borrow(), vec![String::new()], "the first pick starts in the default directory");
    assert!(app.session.log.iter().any(|l| l == "tools.lsp successfully loaded."), "{:?}", app.session.log);
    app.session.cmdline("HI").unwrap();
    assert_eq!(app.session.log.last().map(String::as_str), Some("hi"));
    let dir_text = dir.to_string_lossy().to_string();
    assert_eq!(app.ui.last_lisp_dir.as_deref(), Some(dir_text.as_str()));

    // The directory is kept in the preferences and given to the picker next time (its alias too).
    let prefs = app.prefs_json();
    let (mut restarted, asked_again) = app_with_picker(None);
    restarted.load_prefs(&prefs);
    restarted.start("ap");
    assert_eq!(*asked_again.borrow(), vec![dir_text]);
    assert!(!restarted.session.prompting(), "a cancelled picker does nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picked_file_that_asks_for_a_point_waits_for_the_user() {
    let dir = temp_dir("interactive");
    // Loading asks for a point: a picked file runs as a typed APPLOAD does, so the routine
    // waits for the answer instead of being handed nil.
    let path = write(&dir, "ask.lsp", "(setq picked (getpoint \"\\nPick a point: \")) (princ picked)");
    let (mut app, asked) = app_with_picker(Some(path));

    app.cmdline("appload");
    assert_eq!(asked.borrow().len(), 1);
    assert!(app.session.lisp_running(), "the load waits for the point instead of getting nil");
    assert_eq!(app.session.prompt_text(), "Pick a point: ");
    app.cmdline("3,4");
    assert!(!app.session.lisp_running());
    assert!(app.session.lisp_var("PICKED").as_point().is_some(), "the picked point reached the routine");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn where_there_is_no_file_system_appload_says_to_drop_the_file() {
    // The web has no file system, so a name typed at the command line could never be read:
    // APPLOAD says what works there (dropping the file on the window) instead of asking.
    let mut app = CadApp::new(Session::new(), Services { no_file_system: true, ..Services::default() });
    app.cmdline("appload");
    assert!(!app.session.prompting(), "it never asks for a name it cannot read");
    assert!(app.session.log.iter().any(|l| l.contains("drop a .lsp file")), "{:?}", app.session.log);
    assert!(app.status.as_ref().is_some_and(|(s, _)| s.contains("drop a .lsp file")), "{:?}", app.status);
}

#[test]
fn a_cancelled_picker_is_a_no_op() {
    let (mut app, asked) = app_with_picker(None);
    let before = app.session.log.len();
    app.start("appload");
    assert_eq!(asked.borrow().len(), 1);
    assert_eq!(app.session.log.len(), before, "a cancelled picker says nothing");
    assert!(!app.session.prompting());
    assert!(app.ui.last_lisp_dir.is_none());
}

#[test]
fn without_a_picker_appload_asks_for_the_name_on_the_command_line() {
    let dir = temp_dir("prompt");
    let path = write(&dir, "tools.lsp", "(defun c:hi () (princ \"hi\") (princ))");
    // A desktop host with no file picker: the engine's prompt is the fallback (the web says to
    // drop the file instead, since it cannot read a path).
    let mut app = CadApp::new(Session::new(), Services::default());
    app.start("appload");
    assert!(app.session.prompting());
    assert!(app.session.prompt_text().contains("Enter name of LISP file to load"), "{}", app.session.prompt_text());
    app.cmdline(&path);
    assert!(!app.session.prompting());
    assert!(app.session.log.iter().any(|l| l == "tools.lsp successfully loaded."), "{:?}", app.session.log);
    app.session.cmdline("HI").unwrap();
    assert_eq!(app.session.log.last().map(String::as_str), Some("hi"));
    let _ = std::fs::remove_dir_all(&dir);
}
