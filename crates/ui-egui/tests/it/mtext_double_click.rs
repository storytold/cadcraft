//! Double-clicking an MTEXT opens the in-place text editor on it with its contents (TEXTEDIT), as
//! in AutoCAD; Ctrl+Enter keeps what was typed.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> (Harness<'static, CadApp>, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("mtext", json!({ "at": [0, 5], "text": "hello", "height": 1 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let mut harness = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness.state_mut().session.execute("view.set", &json!({ "center": [2, 4], "height": 20 })).unwrap();
    harness.state_mut().canvas.zoom_pending = false;
    harness.run_steps(3);
    (harness, h)
}

fn contents(h: &Harness<'_, CadApp>, m: Handle) -> String {
    match &h.state().session.doc().unwrap().entity(m).unwrap().kind {
        EntityKind::MText(x) => x.contents.clone(),
        _ => panic!("not an mtext"),
    }
}

#[test]
fn double_clicking_mtext_opens_the_editor() {
    let (mut h, m) = harness();
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(Vec2::new(0.5, 4.5));
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(3);
    let s = &h.state().session;
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("textedit"));
    assert_eq!(s.editing_mtext().map(|t| t.contents), Some("hello".to_string()), "the editor has the text");
    h.event(Event::Text("!".into()));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    assert_eq!(contents(&h, m), "hello!");
    assert!(h.state().session.editing_mtext().is_none(), "the editor closed");
}
