//! Ctrl+C in one open drawing, Ctrl+V in another: the objects are pasted there.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

#[test]
fn copy_in_one_drawing_and_paste_in_another() {
    // A DXF writer for the clipboard text (the app installs the real one).
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |_, _| Ok(cadcraft_engine::doc::Drawing::new_metric()),
        write: |_, _| Ok(b"0\nSECTION\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: None,
    });
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    app.run("circle", json!({ "center": [5, 5], "radius": 2 })).unwrap();
    app.run("block", json!({ "name": "DOT", "base": [5, 5], "handles": [app.session.doc().unwrap().model.iter().nth(1).unwrap().handle.hex()], "keep": "convert" }))
        .unwrap();
    let first = app.session.active;
    let all: Vec<_> = app.session.doc().unwrap().model.iter().map(|e| e.handle).collect();
    app.session.set_selection(all);
    let mut h = harness(app);
    h.event(Event::Copy);
    h.run_steps(2);
    // The copy is mirrored to the system clipboard as text (what another drawing or another
    // CADCraft window pastes): checked on a frame of its own, where egui's output is kept.
    let text = {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| cadcraft_ui_egui::clipboard::mirror(h.state(), ui.ctx()));
        out.textures_delta.clear();
        out.platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
    };
    let text = text.expect("the copy is on the system clipboard as text");
    assert_eq!(
        h.state().session.clipboard.len(),
        2,
        "{:?}",
        h.state().session.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>()
    );
    // Another drawing, made active by clicking its tab (which must not keep the keyboard).
    h.state_mut().run("new", json!({})).unwrap();
    h.run_steps(3);
    let second = h.state().session.active;
    let title = h.state().session.docs[second].title.clone();
    h.state_mut().run("document.switch", json!({ "index": first })).unwrap();
    h.run_steps(3);
    h.get_by_label(&title).click();
    h.run_steps(3);
    assert_eq!(h.state().session.active, second);
    assert_ne!(second, first);
    assert!(h.state().session.doc().unwrap().model.is_empty());
    assert!(!h.ctx.egui_wants_keyboard_input(), "the clicked tab doesn't hold the keyboard");
    // Ctrl+V: egui reports it as a Paste event with the system clipboard's text.
    h.event(Event::Paste(text));
    h.run_steps(3);
    assert_eq!(
        h.state().session.running.as_ref().map(|r| r.id.as_str()),
        Some("pasteclip"),
        "{:?}",
        h.state().session.log.iter().rev().take(5).collect::<Vec<_>>()
    );
    h.state_mut().session.input(Input::Point(Vec2::new(100.0, 100.0))).unwrap();
    h.run_steps(2);
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.model.len(), 2);
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "DOT")));
    assert!(d.block("DOT").is_some());
}
