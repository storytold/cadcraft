//! A failed autosave reaches the user: a notice and one command-line message, gone once a write
//! works. SAVEFILEPATH is process-wide, so everything runs in one test.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};

fn wait_for(app: &mut CadApp, ctx: &egui::Context, done: impl Fn(&CadApp) -> bool) {
    for _ in 0..500 {
        app.autosave_tick(ctx);
        if done(app) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The text the notice paints (the second frame: a new window sizes itself in the first).
fn painted_text(app: &mut CadApp, ctx: &egui::Context) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(t) => text.push_str(t.galley.text()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let mut text = String::new();
    for _ in 0..2 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| cadcraft_ui_egui::autosave::failure_notice(app, ui.ctx()));
        out.textures_delta.clear(); // no renderer here
        text.clear();
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    }
    text
}

#[test]
fn a_failed_autosave_is_shown_until_a_write_works() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let root = std::env::temp_dir().join(format!("cadcraft-autosave-failure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // The folder can't be created while a file holds its name.
    let dir = root.join("out");
    std::fs::write(&dir, b"not a folder").unwrap();
    cadcraft_engine::autosave::set_savefilepath(&dir.to_string_lossy());

    let mut app = CadApp::new(Session::new(), Services::default());
    app.enable_autosave(&dir);
    let ctx = egui::Context::default();
    app.autosave_tick(&ctx);
    assert!(app.autosave_notice.is_none(), "nothing failed yet");

    app.run("circle", serde_json::json!({"center": [0, 0], "radius": 1})).unwrap();
    app.gpu_faults.error("forces an autosave now".into());
    app.gpu_fault_tick(&ctx);
    wait_for(&mut app, &ctx, |a| a.autosave_notice.is_some());
    let n = app.autosave_notice.clone().expect("the failure reaches the notice");
    assert!(!n.dismissed && n.message.contains("out"), "{n:?}");
    let said = |app: &CadApp| app.session.log.iter().filter(|l| l.starts_with("Autosave failed:")).count();
    assert_eq!(said(&app), 1);
    assert!(painted_text(&mut app, &ctx).contains("could not write a recovery copy"));

    // Dismissed, it stays away through retries and isn't repeated at the command line.
    app.autosave_notice.as_mut().unwrap().dismissed = true;
    app.autosave_tick(&ctx);
    assert!(app.autosave_notice.as_ref().is_some_and(|n| n.dismissed));
    assert!(painted_text(&mut app, &ctx).is_empty());
    assert_eq!(said(&app), 1);

    // The folder comes back: the next write clears it.
    std::fs::remove_file(&dir).unwrap();
    app.run("circle", serde_json::json!({"center": [5, 0], "radius": 1})).unwrap();
    app.gpu_faults.error("forces an autosave now".into());
    app.gpu_fault_tick(&ctx);
    wait_for(&mut app, &ctx, |a| a.autosave_notice.is_none());
    assert!(app.autosave_notice.is_none(), "a write that worked ends the notice");

    app.shutdown_autosave();
    let _ = std::fs::remove_dir_all(&root);
}
