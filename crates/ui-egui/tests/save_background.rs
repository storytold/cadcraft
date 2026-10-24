//! A slow writer must leave the UI/session available, preserve edits after the snapshot, and
//! never overwrite an existing file on cancellation or failure.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io, set_write_as};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::closing::Choice;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

static HOLD: AtomicBool = AtomicBool::new(false);
static ENTERED: AtomicBool = AtomicBool::new(false);

fn slow_writer(d: &Drawing, _: &str) -> Result<Vec<u8>, String> {
    ENTERED.store(true, Ordering::Release);
    let start = Instant::now();
    while HOLD.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(d.entity_count().to_string().into_bytes())
}

fn finish(app: &mut CadApp) {
    let start = Instant::now();
    while app.saving.busy() && start.elapsed() < Duration::from_secs(10) {
        app.finish_saving();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!app.saving.busy(), "worker did not finish");
}

#[test]
fn save_uses_snapshot_and_keeps_existing_file_on_cancel_or_failure() {
    set_io(IoHooks { read: |_, _| Err("unused".into()), write: slow_writer, plot: None });
    set_write_as(|drawing, path, _| slow_writer(drawing, path));
    let dir = std::env::temp_dir().join(format!("cadcraft-background-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("drawing.dxf");
    std::fs::write(&path, "OLD").unwrap();
    let mut app = CadApp::new(Session::new(), Services::default());
    app.session.execute("circle", &json!({"center":[0,0], "radius":1})).unwrap();
    HOLD.store(true, Ordering::Release);
    let first = app.run("saveas", json!({"path":path.to_string_lossy()})).unwrap();
    assert_eq!(first["state"], "running");
    let wait = Instant::now();
    while !ENTERED.load(Ordering::Acquire) && wait.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(ENTERED.load(Ordering::Acquire));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "OLD");
    let (ping, _) = ControlRequest::new("ping", json!({}));
    let ping_started = Instant::now();
    let Outcome::Done(reply) = handle(&mut app, &egui::Context::default(), &ping) else { panic!("ping must reply") };
    assert_eq!(reply["result"], "pong");
    assert!(ping_started.elapsed() < Duration::from_millis(100), "control handler remains responsive while writer is held");
    assert!(app.run("saveas", json!({"path":path.to_string_lossy()})).is_err(), "one worker at a time");
    app.session.execute("circle", &json!({"center":[2,0], "radius":1})).unwrap();
    HOLD.store(false, Ordering::Release);
    finish(&mut app);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1", "file is the starting snapshot");
    assert!(app.session.state().unwrap().is_dirty(), "later edit remains unsaved");
    assert_eq!(app.run("save.status", json!({"jobId":first["jobId"]})).unwrap()["state"], "completed");

    app.run("qsave", json!({})).unwrap();
    finish(&mut app);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "2");
    assert!(!app.session.state().unwrap().is_dirty());

    HOLD.store(true, Ordering::Release);
    ENTERED.store(false, Ordering::Release);
    let third = app.run("qsave", json!({})).unwrap();
    let wait = Instant::now();
    while !ENTERED.load(Ordering::Acquire) && wait.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(ENTERED.load(Ordering::Acquire));
    app.run("save.cancel", json!({"jobId":third["jobId"]})).unwrap();
    HOLD.store(false, Ordering::Release);
    finish(&mut app);
    assert_eq!(app.run("save.status", json!({"jobId":third["jobId"]})).unwrap()["state"], "cancelled");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "2");
    assert!(!std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".cadcraft-save-")));

    let png = dir.join("drawing.png");
    app.run("saveas", json!({"path":png.to_string_lossy()})).unwrap();
    assert_eq!(std::fs::read_to_string(&png).unwrap(), "2", "other SAVEAS exports still use the synchronous engine path");
    assert_eq!(app.session.state().unwrap().path.as_deref(), Some(path.to_string_lossy().as_ref()));

    let close_path = dir.join("close.dxf");
    let picked = close_path.to_string_lossy().to_string();
    let mut closing = CadApp::new(Session::new(), Services { pick_save: Some(Box::new(move |_| Some(picked.clone()))), ..Services::default() });
    closing.session.execute("circle", &json!({"center":[0,0], "radius":1})).unwrap();
    closing.request_close(None);
    closing.answer_close(Choice::Save);
    assert_eq!(closing.session.docs.len(), 1, "close waits for the background save");
    finish(&mut closing);
    let status = closing.run("save.status", json!({}));
    assert!(
        closing.session.docs.is_empty(),
        "drawing closes after its save succeeds: status={status:?}, dirty={:?}, log={:?}",
        closing.session.docs.first().map(|d| d.is_dirty()),
        closing.session.log
    );

    let missing = dir.join("missing").join("drawing.dxf");
    app.run("saveas", json!({"path":missing.to_string_lossy()})).unwrap();
    finish(&mut app);
    assert_eq!(app.run("save.status", json!({})).unwrap()["state"], "failed");
    assert_eq!(app.session.state().unwrap().path.as_deref(), Some(path.to_string_lossy().as_ref()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "2");
    std::fs::remove_dir_all(dir).unwrap();
}
