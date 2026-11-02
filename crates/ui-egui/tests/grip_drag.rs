//! Grips move by a click, then a click where they go, or by a drag: press on the grip, move,
//! let go (AutoCAD lets grips be dragged as well).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// A selected polyline (0,0)–(20,0)–(20,10), seen 30 units high.
fn harness() -> (Harness<'static, CadApp>, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("pline", json!({"vertices": [[0, 0], [20, 0], [20, 10]]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let mut hh = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    hh.run_steps(3);
    hh.state_mut().session.execute("view.set", &json!({ "center": [10, 5], "height": 30 })).unwrap();
    hh.state_mut().canvas.zoom_pending = false;
    hh.state_mut().session.set_selection(vec![h]);
    hh.run_steps(3);
    (hh, h)
}

fn second_vertex(hh: &Harness<'_, CadApp>, h: Handle) -> (f64, f64) {
    match &hh.state().session.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.get(1).map(|v| (v.p.x, v.p.y)).unwrap(),
        _ => panic!("not a polyline"),
    }
}

fn screen(hh: &Harness<'_, CadApp>, x: f64, y: f64) -> Pos2 {
    hh.state().canvas.xf.unwrap().to_screen(Vec2::new(x, y))
}

fn button(hh: &mut Harness<'_, CadApp>, pos: Pos2, pressed: bool) {
    hh.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    hh.run_steps(1);
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.2 && (a.1 - b.1).abs() < 0.2
}

#[test]
fn a_grip_moves_by_click_and_click() {
    let (mut hh, h) = harness();
    let g = screen(&hh, 20.0, 0.0);
    hh.event(Event::PointerMoved(g));
    hh.run_steps(2);
    button(&mut hh, g, true);
    button(&mut hh, g, false);
    let to = screen(&hh, 25.0, 3.0);
    hh.event(Event::PointerMoved(to));
    hh.run_steps(2);
    button(&mut hh, to, true);
    button(&mut hh, to, false);
    hh.run_steps(2);
    assert!(near(second_vertex(&hh, h), (25.0, 3.0)), "{:?}", second_vertex(&hh, h));
}

#[test]
fn a_grip_moves_by_dragging_it() {
    let (mut hh, h) = harness();
    let g = screen(&hh, 20.0, 0.0);
    hh.event(Event::PointerMoved(g));
    hh.run_steps(2);
    button(&mut hh, g, true);
    let to = screen(&hh, 25.0, 3.0);
    for k in 1..=10 {
        hh.event(Event::PointerMoved(g + (to - g) * (k as f32 / 10.0)));
        hh.run_steps(1);
    }
    button(&mut hh, to, false);
    hh.run_steps(2);
    assert!(near(second_vertex(&hh, h), (25.0, 3.0)), "{:?}", second_vertex(&hh, h));
    assert!(hh.state().session.running.is_none(), "the grip edit ends with the release");
    // One undo step puts it back.
    hh.state_mut().session.execute("u", &json!({})).unwrap();
    assert!(near(second_vertex(&hh, h), (20.0, 0.0)));
}

#[test]
fn escape_cancels_a_dragged_grip() {
    let (mut hh, h) = harness();
    let g = screen(&hh, 20.0, 0.0);
    hh.event(Event::PointerMoved(g));
    hh.run_steps(2);
    button(&mut hh, g, true);
    let to = screen(&hh, 25.0, 3.0);
    for k in 1..=10 {
        hh.event(Event::PointerMoved(g + (to - g) * (k as f32 / 10.0)));
        hh.run_steps(1);
    }
    hh.event(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    hh.run_steps(1);
    button(&mut hh, to, false);
    hh.run_steps(2);
    assert!(near(second_vertex(&hh, h), (20.0, 0.0)), "{:?}", second_vertex(&hh, h));
}

#[test]
fn a_click_that_moves_a_pixel_is_not_a_drag() {
    let (mut hh, h) = harness();
    let g = screen(&hh, 20.0, 0.0);
    hh.event(Event::PointerMoved(g));
    hh.run_steps(2);
    button(&mut hh, g, true);
    hh.event(Event::PointerMoved(g + vec2(1.0, 0.0)));
    hh.run_steps(1);
    button(&mut hh, g + vec2(1.0, 0.0), false);
    hh.run_steps(2);
    // The press-release was a click: the grip edit is still waiting for its point.
    assert!(hh.state().session.running.is_some(), "a click starts the grip edit and waits");
    assert!(near(second_vertex(&hh, h), (20.0, 0.0)));
}
