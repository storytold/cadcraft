//! OPEN typed at the command line with the file to open reads it in the background, as
//! File ▸ Open does, so a large drawing doesn't freeze the window. Programmatic calls (the control
//! channel, MCP, scripts, LISP) still get their answer at once.

use std::time::{Duration, Instant};

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, cmdline};
use serde_json::json;

#[path = "../support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// A reader that accepts any file (the same one other tests in this binary install, so whichever
/// is installed first, opening works the same).
fn install_reader() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |_, _| Ok(cadcraft_engine::doc::Drawing::new_metric()),
        write: |_, _| Ok(b"0\nSECTION\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: None,
    });
}

/// Type `line` and press Enter, as the person at the keyboard does.
fn type_line(app: &mut CadApp, line: &str) {
    app.cmd.buffer = line.to_string();
    cmdline::submit(app);
}

/// Wait for the drawings being read in the background to be added (or reported).
fn settle(app: &mut CadApp) {
    let t = Instant::now();
    while app.opening.busy() && t.elapsed() < Duration::from_secs(10) {
        app.finish_opening();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.finish_opening();
}

fn title(app: &CadApp) -> Option<String> {
    app.session.state().ok().map(|s| s.title.clone())
}

#[test]
fn typed_open_with_a_path_reads_in_the_background() {
    install_reader();
    let dir = TempDir::new("typed-open");
    let plan = dir.file("plan.dxf", b"0\nEOF\n");
    let section = dir.file("section.dxf", b"0\nEOF\n");
    let mut app = CadApp::new(Session::new(), Services::default());
    let before = app.session.docs.len();

    type_line(&mut app, &format!("OPEN {plan}"));
    assert!(app.opening.busy(), "the read runs on the background reader: {:?}", app.session.log);
    assert_eq!(app.session.docs.len(), before, "nothing is added until the read finishes");
    settle(&mut app);
    assert_eq!(app.session.docs.len(), before + 1);
    assert_eq!(title(&app).as_deref(), Some("plan.dxf"), "the opened drawing is the active tab");
    assert_eq!(app.ui.recent_files.first(), Some(&plan));

    // The JSON form with only a path, typed: the same.
    type_line(&mut app, &format!("open {}", json!({ "path": section })));
    assert!(app.opening.busy());
    settle(&mut app);
    assert_eq!(app.session.docs.len(), before + 2);
    assert_eq!(title(&app).as_deref(), Some("section.dxf"));
}

#[test]
fn programmatic_open_with_a_path_still_answers_at_once() {
    install_reader();
    let dir = TempDir::new("programmatic-open");
    let plan = dir.file("plan.dxf", b"0\nEOF\n");
    let mut app = CadApp::new(Session::new(), Services::default());
    let before = app.session.docs.len();

    // engine.execute / app.open on the control channel, and MCP.
    let r = app.run("open", json!({ "path": plan }));
    assert!(!app.opening.busy());
    let index = r.ok().and_then(|v| v.get("index").and_then(serde_json::Value::as_u64));
    assert_eq!(index, Some(before as u64), "the result names the opened drawing");
    assert_eq!(title(&app).as_deref(), Some("plan.dxf"));

    // `cmdline.input` on the control channel and a script line: the drawing is open when they return.
    assert_eq!(app.cmdline(&format!("open {}", json!({ "path": plan }))), None);
    assert!(!app.opening.busy());
    assert_eq!(app.session.docs.len(), before + 2);
    app.session.script(&format!("open {}\n", json!({ "path": plan }))).ok();
    assert!(!app.opening.busy());
    assert_eq!(app.session.docs.len(), before + 3);
}

#[test]
fn typed_open_of_a_missing_file_reports_the_same_error() {
    install_reader();
    let dir = TempDir::new("typed-open-missing");
    let missing = dir.path_str("gone.dxf");
    let mut app = CadApp::new(Session::new(), Services::default());
    let before = app.session.docs.len();
    let expected = app.run("open", json!({ "path": missing })).err().unwrap_or_default();
    assert!(expected.contains("gone.dxf"), "{expected}");

    type_line(&mut app, &format!("OPEN {missing}"));
    settle(&mut app);
    assert_eq!(app.session.docs.len(), before);
    assert_eq!(app.session.log.last(), Some(&expected), "{:?}", app.session.log);
    assert_eq!(app.status.as_ref().map(|(s, _)| s.as_str()), Some(expected.as_str()));
}
