//! Double-clicking text with no command running edits it, as in AutoCAD (#842).

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, ImeEvent, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("text", json!({ "at": [0, 0], "text": "HELLO", "height": 2.5 })).unwrap();
    app.run("mtext", json!({ "at": [0, 20], "text": "first\\Psecond", "height": 2.5, "width": 20 })).unwrap();
    app.run("line", json!({ "points": [[30, 0], [30, 20]] })).unwrap();
    app.session.execute("view.set", &json!({ "center": [15, 10], "height": 40 })).unwrap();
    app.canvas.zoom_pending = false;
    // 60 frames a second: the default step (0.25 s) spreads two clicks past the double-click delay.
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

fn double_click(h: &mut Harness<'_, CadApp>, at: Vec2) {
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(at);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

fn text_at(h: &Harness<'_, CadApp>, i: usize) -> String {
    let d = h.state().session.doc().unwrap();
    let e = d.model.iter().nth(i).unwrap();
    match &e.kind {
        EntityKind::Text(t) => t.value.clone(),
        EntityKind::MText(t) => t.contents.clone(),
        k => panic!("not text: {}", k.type_name()),
    }
}

#[test]
fn double_click_on_text_starts_textedit_for_that_object() {
    for (i, at, old, new) in [(0, Vec2::new(4.0, 1.2), "HELLO", "HELLO WORLD"), (1, Vec2::new(3.0, 18.8), "first\nsecond", "edited")] {
        let mut h = harness();
        double_click(&mut h, at);
        let s = &h.state().session;
        assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("textedit"), "object {i}");
        let p = s.current_prompt().unwrap();
        assert_eq!(p.message, "Enter new text", "object {i}: the clicked text is already picked");
        assert_eq!(p.default.as_deref(), Some(old), "object {i}");
        h.state_mut().session.input(Input::Text(new.into())).unwrap();
        h.run_steps(2);
        assert_eq!(text_at(&h, i), new);
        // TEXTEDIT goes on asking for objects (its Multiple mode), as AutoCAD's does.
        assert_eq!(h.state().session.current_prompt().unwrap().message, "Select an annotation object");
    }
}

#[test]
fn double_clicked_text_takes_ime_composition_and_commit() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(4.0, 1.2));
    assert_eq!(h.state().session.current_prompt().unwrap().message, "Enter new text");
    // Composition is shown, not saved; the commit is inserted exactly once (#1761).
    h.event(Event::Ime(ImeEvent::Preedit { text: "zhong".into(), active_range_chars: None }));
    h.run_steps(1);
    assert_eq!(h.state().cmd.preedit, "zhong");
    assert!(h.state().cmd.buffer.is_empty());
    h.event(Event::Ime(ImeEvent::Commit("中文".into())));
    h.run_steps(1);
    assert!(h.state().cmd.preedit.is_empty());
    assert_eq!(h.state().cmd.buffer, "中文");
    h.key_press(Key::Enter);
    h.run_steps(1);
    assert_eq!(text_at(&h, 0), "中文");
}

#[test]
fn double_click_elsewhere_starts_no_command() {
    for at in [Vec2::new(30.0, 5.0), Vec2::new(15.0, 30.0)] {
        let mut h = harness();
        double_click(&mut h, at);
        assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), None, "{at:?}");
    }
}
