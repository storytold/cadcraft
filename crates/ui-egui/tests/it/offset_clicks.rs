//! OFFSET on the canvas: the distance can be clicked, and picking the source is not moved by the
//! grid snap (#618).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec2::new(0.0, 2.0).to3(0.0), b: Vec2::new(10.0, 2.0).to3(0.0) })).unwrap();
    let mut session = Session::empty();
    session.open_drawing(d, "Offset clicks", None);
    session.execute("view.set", &json!({"center":[5,5],"height":20})).unwrap();
    session.settings.osmode = 0;
    session.settings.snapmode = true;
    session.execute("setvar", &json!({"name":"SNAPUNIT","value":[5,5]})).unwrap();
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

fn click(h: &mut Harness<'_, CadApp>, world: Vec2) {
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(world);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

// Regression test for #618: OFFSET's distance could not be clicked, and with grid snap on a click
// meant to pick the source object was moved to the nearest grid point, missing the object.
#[test]
fn offset_takes_a_clicked_distance_and_picks_its_source_under_grid_snap() {
    let mut h = harness();
    h.state_mut().start("offset");
    h.run_steps(2);
    // Two clicks, snapped to the grid points (5, 5) and (10, 5): a distance of 5.
    click(&mut h, Vec2::new(5.1, 4.9));
    assert!(h.state().session.prompt_text().contains("second point"), "{}", h.state().session.prompt_text());
    click(&mut h, Vec2::new(9.9, 5.1));
    assert_eq!(h.state().session.doc().unwrap().header.f64("OFFSETDIST", -1.0), 5.0);
    assert!(h.state().session.prompt_text().contains("Select object"), "{}", h.state().session.prompt_text());
    // The source line is at y = 2, two units from the grid point (5, 0) the click would snap to.
    click(&mut h, Vec2::new(7.0, 2.04));
    let prompt = h.state().session.prompt_text();
    assert!(prompt.contains("side to offset"), "the line was not picked: {prompt}");
    // A side click above the line (snapped to (5, 5)) makes the copy 5 units up, at y = 7.
    click(&mut h, Vec2::new(7.0, 7.0));
    let lines: Vec<(f64, f64)> = h
        .state()
        .session
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(line) => Some((line.a.y, line.b.y)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2, "OFFSET added no line: {lines:?}");
    assert!(lines.iter().any(|&(a, b)| (a - 7.0).abs() < 1e-9 && (b - 7.0).abs() < 1e-9), "no copy at y = 7: {lines:?}");
    // OFFSET asks for the next object; Enter ends it.
    assert!(h.state().session.prompt_text().contains("Select object to offset"), "{}", h.state().session.prompt_text());
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().session.prompt_text(), "Command:", "Enter ends OFFSET");
}
