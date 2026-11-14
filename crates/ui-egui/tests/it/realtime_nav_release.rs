//! A drag that pans in realtime places no point when it is let go, even after Enter ended the
//! transparent PAN mid-drag and handed the prompt back to the drawing command; a pan drag that
//! Escape stopped leaves the next drag free to place a point.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Realtime release", None);
    session.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
    session.settings.osmode = 0;
    session.settings.snapmode = false;
    session.settings.orthomode = false;
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn button(h: &mut Harness<'_, CadApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn lines(h: &Harness<'_, CadApp>) -> usize {
    let st = h.state().session.state().unwrap();
    st.doc.space(&Space::Model).unwrap().iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count()
}

#[test]
fn enter_mid_drag_ends_pan_and_the_release_places_no_point() {
    let mut h = harness();
    h.state_mut().start("line");
    h.run_steps(2);
    let first = screen(&h, Vec2::new(10.0, 10.0));
    button(&mut h, first, true);
    button(&mut h, first, false);
    h.run_steps(2);
    h.state_mut().session.cmdline("'PAN").unwrap();
    h.run_steps(1);
    assert!(h.state().session.realtime_pan_active());
    let (a, b) = (screen(&h, Vec2::new(40.0, 40.0)), screen(&h, Vec2::new(60.0, 30.0)));
    button(&mut h, a, true);
    for i in 1..=4 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 4.0)));
        h.run_steps(1);
    }
    h.state_mut().session.cmdline("").unwrap();
    h.run_steps(1);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    button(&mut h, b, false);
    h.run_steps(2);
    assert_eq!(lines(&h), 0, "the release of the pan drag placed a point");
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    // A click after it still places the next point.
    button(&mut h, b, true);
    button(&mut h, b, false);
    h.run_steps(2);
    assert_eq!(lines(&h), 1);
}

#[test]
fn a_pan_drag_stopped_by_escape_leaves_the_next_drag_placing_a_point() {
    let mut h = harness();
    h.state_mut().session.cmdline("PAN").unwrap();
    h.run_steps(1);
    assert!(h.state().session.realtime_pan_active());
    let (a, b) = (screen(&h, Vec2::new(40.0, 40.0)), screen(&h, Vec2::new(60.0, 30.0)));
    button(&mut h, a, true);
    for i in 1..=4 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 4.0)));
        h.run_steps(1);
    }
    for pressed in [true, false] {
        h.event(Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    button(&mut h, b, false);
    h.run_steps(2);
    assert!(!h.state().session.realtime_pan_active());
    h.state_mut().start("line");
    h.run_steps(2);
    // Screen points near the middle of the canvas, wherever the pan left the drawing.
    let first = Pos2::new(700.0, 450.0);
    button(&mut h, first, true);
    button(&mut h, first, false);
    h.run_steps(2);
    // The second point is a press, a drag and a release.
    let (c, d) = (Pos2::new(760.0, 420.0), Pos2::new(820.0, 400.0));
    button(&mut h, c, true);
    for i in 1..=4 {
        h.event(Event::PointerMoved(c + (d - c) * (i as f32 / 4.0)));
        h.run_steps(1);
    }
    button(&mut h, d, false);
    h.run_steps(2);
    assert_eq!(lines(&h), 1, "the drag after an Escape-stopped pan drag placed no point");
}
