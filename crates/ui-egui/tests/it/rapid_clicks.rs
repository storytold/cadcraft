//! Fast drafting: every click on the canvas places a point, however fast it comes (#815).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Rapid clicks", None);
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
}

fn start_line_at(h: &mut Harness<'_, CadApp>, first: Vec2) {
    h.state_mut().start("line");
    h.run_steps(2);
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    let pos = screen(h, first);
    button(h, pos, true);
    h.run_steps(1);
    button(h, pos, false);
    h.run_steps(2);
}

fn lines(h: &Harness<'_, CadApp>) -> Vec<(Vec2, Vec2)> {
    let st = h.state().session.state().unwrap();
    st.doc
        .space(&Space::Model)
        .unwrap()
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect()
}

#[test]
fn two_clicks_within_one_frame_place_two_points() {
    let mut h = harness();
    start_line_at(&mut h, Vec2::new(10.0, 10.0));
    let (a, b) = (screen(&h, Vec2::new(40.0, 10.0)), screen(&h, Vec2::new(40.0, 40.0)));
    // A slow frame: both clicks arrive before the next one is drawn, and eframe hands them over
    // together. (`Harness::event` would run a frame per event, so they go into the input directly.)
    for pos in [a, b] {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::PointerMoved(pos));
            h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
    }
    h.run_steps(2);
    let got = lines(&h);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].1.near(Vec2::new(40.0, 10.0), 1e-4), "{got:?}");
    assert!(got[1].1.near(Vec2::new(40.0, 40.0), 1e-4), "{got:?}");
}

#[test]
fn a_click_made_while_the_mouse_moves_still_places_the_point() {
    let mut h = harness();
    start_line_at(&mut h, Vec2::new(10.0, 10.0));
    let down = screen(&h, Vec2::new(40.0, 10.0));
    button(&mut h, down, true);
    h.run_steps(1);
    // The hand is already on its way to the next point when the button comes up.
    let up = down + vec2(12.0, 0.0);
    button(&mut h, up, false);
    h.run_steps(2);
    let got = lines(&h);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].1.near(h.state().canvas.xf.unwrap().to_world(up), 1e-4), "{got:?}");
}
