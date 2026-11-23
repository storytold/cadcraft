//! Tab at a point prompt steps through every object snap under the aperture (each endpoint of
//! each object nearby), not just the closest snap of each running mode, and comes back round.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::snap::mode;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

#[test]
fn tab_cycles_through_every_endpoint_under_the_aperture() {
    let mut s = Session::new();
    // Three line ends close together (within the aperture of the cursor between them).
    for (a, b) in [([0.0, 0.0], [10.0, 0.0]), ([0.06, 0.05], [0.06, 10.0]), ([0.03, -0.06], [-10.0, -10.0])] {
        s.execute("line", &json!({ "points": [a, b] })).unwrap();
    }
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 1 })).unwrap();
    s.execute("view.set", &json!({ "center": [0, 0], "height": 20 })).unwrap();
    let mut app = CadApp::new(s, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().session.start("line").unwrap();
    h.run_steps(2);
    let xf = h.state().canvas.xf.unwrap();
    let at = xf.to_screen(Vec2::new(0.03, 0.0));
    h.event(Event::PointerMoved(at));
    h.run_steps(2);
    let mut seen: Vec<Vec2> = Vec::new();
    for i in 0..4 {
        let p = h.state().canvas.snap.map(|s| s.point).unwrap();
        if i < 3 {
            assert!(!seen.iter().any(|q| q.near(p, 1e-9)), "Tab {i} repeats {p:?} ({seen:?})");
        } else {
            assert!(seen.first().is_some_and(|q| q.near(p, 1e-9)), "the fourth Tab comes back round");
        }
        seen.push(p);
        h.event(Event::Text("\t".into()));
        h.run_steps(2);
    }
    for want in [Vec2::new(0.0, 0.0), Vec2::new(0.06, 0.05), Vec2::new(0.03, -0.06)] {
        assert!(seen.iter().any(|q| q.near(want, 1e-9)), "{want:?} in {seen:?}");
    }
}

/// Three endpoints near 10,0: 10,0 (the nearest to the cursor), 10.03,0 and 9.95,0. Endpoint is
/// the only running snap, so every candidate is an endpoint.
fn three_ends() -> Harness<'static, CadApp> {
    let mut d = Drawing::new_imperial();
    for (a, b) in [((0.0, 0.0), (10.0, 0.0)), ((10.03, 0.0), (20.0, 0.0)), ((9.95, 0.0), (9.95, -5.0))] {
        let line = Line { a: Vec2::new(a.0, a.1).to3(0.0), b: Vec2::new(b.0, b.1).to3(0.0) };
        d.add(&Space::Model, Common::default(), EntityKind::Line(line)).unwrap();
    }
    let mut session = Session::empty();
    session.open_drawing(d, "Tab snap cycling", None);
    session.execute("view.set", &json!({"center":[10,0],"height":8})).unwrap();
    session.settings.osmode = mode::END;
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
    h.state_mut().start("line");
    h.run_steps(2);
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    let ap = h.state().session.settings.aperture / h.state().canvas.xf.unwrap().scale;
    assert!(ap > 0.07, "all three endpoints lie in the aperture ({ap})");
    h
}

const CURSOR: Vec2 = Vec2::new(10.01, 0.0);

fn hover(h: &mut Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    let pos = h.state().canvas.xf.unwrap().to_screen(p);
    h.event(Event::PointerMoved(pos));
    h.run_steps(3);
    pos
}

/// Tab, as the command line receives it (`cmdline::capture_tab`).
fn tab(h: &mut Harness<'_, CadApp>) {
    h.event(Event::Text("\t".into()));
    h.run_steps(3);
}

fn snapped(h: &Harness<'_, CadApp>) -> Vec2 {
    let hit = h.state().canvas.snap.expect("a snap marker");
    assert_eq!(hit.mode, mode::END);
    hit.point
}

fn assert_at(got: Vec2, x: f64) {
    assert!(got.near(Vec2::new(x, 0.0), 1e-9), "{got:?}, expected {x},0");
}

#[test]
fn moving_the_cursor_away_starts_the_cycle_over() {
    let mut h = three_ends();
    hover(&mut h, CURSOR);
    tab(&mut h);
    assert_at(snapped(&h), 10.03);
    hover(&mut h, Vec2::new(10.0, 2.0));
    assert!(h.state().canvas.snap.is_none());
    hover(&mut h, CURSOR);
    assert_at(snapped(&h), 10.0);
}

#[test]
fn a_click_places_the_candidate_tab_chose() {
    let mut h = three_ends();
    let pos = hover(&mut h, CURSOR);
    tab(&mut h);
    assert_at(snapped(&h), 10.03);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    let base = h.state().session.current_prompt().and_then(|p| p.base).expect("LINE asks for the next point");
    assert_at(base, 10.03);
}
