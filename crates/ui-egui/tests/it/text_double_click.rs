//! Double-clicking text with no command running edits it, as in AutoCAD (#842).

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
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
fn double_click_elsewhere_starts_no_command() {
    for at in [Vec2::new(30.0, 5.0), Vec2::new(15.0, 30.0)] {
        let mut h = harness();
        double_click(&mut h, at);
        assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), None, "{at:?}");
    }
}

#[test]
fn double_click_on_a_multileader_edits_its_text() {
    let mut h = harness();
    h.state_mut().session.execute("mleader", &json!({"points": [[10, 5], [0, 0]], "text": "example text"})).unwrap();
    h.run_steps(2);
    // The multileader's text sits at the end of its landing.
    double_click(&mut h, Vec2::new(-0.45, 0.05));
    let s = &h.state().session;
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("textedit"), "the clicked multileader is already picked");
    let p = s.current_prompt().unwrap();
    assert_eq!(p.message, "Enter new text");
    assert_eq!(p.default.as_deref(), Some("example text"));
    h.state_mut().session.input(Input::Text("changed text".into())).unwrap();
    h.run_steps(2);
    let got = h
        .state()
        .session
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::MLeader(m) => m.text.as_ref().map(|t| t.contents.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(got, "changed text");
}

#[test]
fn double_click_on_a_block_multileader_starts_no_text_edit() {
    let mut h = harness();
    let s = &mut h.state_mut().session;
    let sq = s.execute("pline", &json!({"vertices": [[-1, -1], [1, -1], [1, 1], [-1, 1]], "closed": true})).unwrap();
    s.execute("block", &json!({"name": "TAG", "base": [-1, -1], "handles": [sq["handle"].as_str().or_else(|| sq["handles"][0].as_str()).unwrap()], "keep": "delete"})).unwrap();
    s.execute("mleader", &json!({"points": [[10, 5], [0, 0]], "block": "TAG"})).unwrap();
    h.run_steps(2);
    // On the leader line, halfway along.
    double_click(&mut h, Vec2::new(5.0, 2.5));
    assert_ne!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("textedit"), "a block multileader has no text");
}
