//! A pattern hatch's centre grip is multifunctional (gaps U5): right-clicked it offers Stretch,
//! Origin Point, Hatch Angle and Hatch Scale; an option makes the grip hot and asks for its value.
//! Driven with real pointer events.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(pattern: &str, render: bool) -> (Harness<'static, CadApp>, Handle) {
    let mut session = Session::new();
    session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let r = session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": pattern })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    session.execute("view.set", &json!({ "center": [5, 5], "height": 20 })).unwrap();
    session.set_selection(vec![h]);
    let mut app = CadApp::new(session, Services::default());
    app.ui.start_tab = false;
    app.canvas.zoom_pending = false;
    let builder = Harness::builder().with_size(vec2(1200.0, 800.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut harness = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    (harness, h)
}

fn right_click(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

fn scale(h: &Harness<'_, CadApp>, hatch: Handle) -> f64 {
    match &h.state().session.doc().unwrap().entity(hatch).unwrap().kind {
        EntityKind::Hatch(x) => x.scale,
        k => panic!("{k:?}"),
    }
}

#[test]
fn hatch_scale_from_the_grip_menu() {
    let shots = std::env::var_os("CADCRAFT_HATCH_GRIP_SHOTS").map(std::path::PathBuf::from);
    let (mut h, hatch) = harness("ANSI31", shots.is_some());
    let grip = h.state().canvas.xf.unwrap().to_screen(Vec2::new(5.0, 5.0));
    right_click(&mut h, grip);
    for label in ["Origin Point", "Hatch Angle"] {
        h.get_by_label(label);
    }
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).unwrap();
        h.render().unwrap().save(dir.join("cadcraft-hatch-grip-menu.png")).unwrap();
    }
    h.get_by_label("Hatch Scale").click();
    h.run_steps(3);
    let prompt = h.state().session.prompt_text();
    assert!(prompt.contains("** HATCH SCALE ** Specify hatch scale"), "{prompt}");
    h.state_mut().cmdline("2");
    h.run_steps(2);
    assert!(h.state().session.running.is_none());
    assert!((scale(&h, hatch) - 2.0).abs() < 1e-12);
}

#[test]
fn a_solid_fill_grip_has_no_menu() {
    let (mut h, _) = harness("SOLID", false);
    let grip = h.state().canvas.xf.unwrap().to_screen(Vec2::new(5.0, 5.0));
    right_click(&mut h, grip);
    assert!(h.query_by_label("Hatch Scale").is_none());
    assert!(h.query_by_label("Origin Point").is_none());
}
