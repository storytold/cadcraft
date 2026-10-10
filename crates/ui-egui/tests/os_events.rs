//! Requests from the operating system (the macOS Apple events) reach the app: open and quit.

use std::cell::RefCell;
use std::rc::Rc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_ui_egui::{CadApp, OsEvent, Services};

fn frame(app: &mut CadApp, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    out.textures_delta.clear();
}

#[test]
fn opened_documents_and_quit_reach_the_app() {
    set_io(IoHooks { read: |_, _| Ok(cadcraft_doc::Drawing::new_imperial()), write: |_, _| Err("no writer in tests".into()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-os-events-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files: Vec<String> = ["a.dxf", "b.dxf"]
        .iter()
        .map(|n| {
            let p = dir.join(n);
            std::fs::write(&p, "  0\r\nEOF\r\n").unwrap();
            p.to_string_lossy().into_owned()
        })
        .collect();
    // Issue #158: Finder sends one event for the whole selection.
    let queue = Rc::new(RefCell::new(vec![OsEvent::Open(files.clone())]));
    let q = Rc::clone(&queue);
    let services = Services { os_events: Some(Box::new(move || std::mem::take(&mut *q.borrow_mut()))), ..Default::default() };
    let mut app = CadApp::new(Session::new(), services);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let paths: Vec<Option<String>> = app.session.docs.iter().skip(1).map(|d| d.path.clone()).collect();
    assert_eq!(paths, files.iter().cloned().map(Some).collect::<Vec<_>>());
    assert_eq!(app.session.active, 2, "the last opened drawing is active");
    assert!(!app.quit_requested);
    // Dock ▸ Quit takes the Cmd+Q path, which asks about unsaved changes.
    queue.borrow_mut().push(OsEvent::Quit);
    frame(&mut app, &ctx);
    assert!(app.quit_requested);
    let _ = std::fs::remove_dir_all(&dir);
}
