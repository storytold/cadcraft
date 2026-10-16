//! Tab at a point prompt cycles through every object snap under the cursor, as in AutoCAD: all
//! the endpoints in the aperture, not only the closest snap of each mode (gaps.md U4).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::mode;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// Three endpoints near 10,0: 10,0 (the nearest to the cursor), 10.03,0 and 9.95,0. Endpoint is
/// the only running snap, so every candidate is an endpoint.
fn harness() -> Harness<'static, CadApp> {
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
fn tab_reaches_both_close_endpoints() {
    let mut h = harness();
    hover(&mut h, CURSOR);
    assert_at(snapped(&h), 10.0);
    tab(&mut h);
    assert_at(snapped(&h), 10.03);
}

#[test]
fn tab_goes_from_the_nearest_out_and_back_to_the_first() {
    let mut h = harness();
    hover(&mut h, CURSOR);
    let mut seen = vec![snapped(&h)];
    for _ in 0..3 {
        tab(&mut h);
        seen.push(snapped(&h));
    }
    for (got, x) in seen.into_iter().zip([10.0, 10.03, 9.95, 10.0]) {
        assert_at(got, x);
    }
}

#[test]
fn moving_the_cursor_away_starts_the_cycle_over() {
    let mut h = harness();
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
    let mut h = harness();
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
