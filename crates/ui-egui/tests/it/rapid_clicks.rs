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

/// A harness at 60 frames a second, with two lines to select: a horizontal one through (20, 50)
/// and (40, 50), and a vertical one at x = 80.
fn select_harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Moving clicks", None);
    session.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
    session.settings.osmode = 0;
    session.execute("line", &json!({"points": [[20, 50], [40, 50]]})).unwrap();
    session.execute("line", &json!({"points": [[80, 30], [80, 70]]})).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
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

/// Press at `down`, move on (the hand is already going somewhere) and let go at `up`, `frames`
/// frames later.
fn press_moving(h: &mut Harness<'_, CadApp>, down: Vec2, up: Vec2, frames: usize) {
    let (d, u) = (screen(h, down), screen(h, up));
    button(h, d, true);
    h.run_steps(1);
    for i in 1..=frames {
        let t = i as f32 / frames as f32;
        h.event(Event::PointerMoved(d + (u - d) * t));
        h.run_steps(1);
    }
    button(h, u, false);
    h.run_steps(2);
}

fn handles(h: &Harness<'_, CadApp>) -> Vec<cadcraft_engine::doc::Handle> {
    let st = h.state().session.state().unwrap();
    st.doc.space(&Space::Model).unwrap().iter().map(|e| e.handle).collect()
}

/// #1001: in STRETCH (whose "Select objects" takes points), a click made while the mouse moves
/// picks the object where the button went down; on empty space it starts STRETCH's window.
#[test]
fn a_click_made_while_moving_selects_in_stretch() {
    let mut h = select_harness();
    let ids = handles(&h);
    h.state_mut().start("stretch");
    h.run_steps(2);
    // Down on the horizontal line, up a few units off it, a few frames later.
    press_moving(&mut h, Vec2::new(30.0, 50.0), Vec2::new(32.0, 54.0), 4);
    assert_eq!(h.state().session.selection(), vec![ids[0]], "the line under the button is picked");

    // Down on empty space while moving: the window starts where the button went down.
    press_moving(&mut h, Vec2::new(70.0, 20.0), Vec2::new(72.0, 22.0), 4);
    assert_eq!(h.state().session.current_prompt().unwrap().message, "Specify opposite corner");
    let corner = screen(&h, Vec2::new(90.0, 80.0));
    button(&mut h, corner, true);
    h.run_steps(1);
    button(&mut h, corner, false);
    h.run_steps(2);
    assert_eq!(h.state().session.selection(), vec![ids[0], ids[1]], "the window took the vertical line");
}

/// At a generic "Select objects" (MOVE), a moving click picks the object where the button went
/// down instead of closing a tiny window; a slow press-and-drag is still a window.
#[test]
fn a_click_made_while_moving_selects_in_move_and_a_slow_drag_is_a_window() {
    let mut h = select_harness();
    let ids = handles(&h);
    h.state_mut().start("move");
    h.run_steps(2);
    press_moving(&mut h, Vec2::new(30.0, 50.0), Vec2::new(32.0, 54.0), 4);
    assert_eq!(h.state().session.selection(), vec![ids[0]]);
    assert!(h.state().session.pending_window.is_none());
    // Half a second from (70, 20) to (90, 80): a window around the vertical line.
    press_moving(&mut h, Vec2::new(70.0, 20.0), Vec2::new(90.0, 80.0), 30);
    assert!(h.state().session.pending_window.is_none(), "closed where the drag ended");
    assert_eq!(h.state().session.selection(), vec![ids[0], ids[1]]);
}

/// With no command running, the same moving click picks the object.
#[test]
fn a_click_made_while_moving_picks_with_no_command() {
    let mut h = select_harness();
    let ids = handles(&h);
    press_moving(&mut h, Vec2::new(80.0, 40.0), Vec2::new(83.0, 41.0), 4);
    assert_eq!(h.state().session.selection(), vec![ids[1]]);
}

/// TRIM's "Select object to trim": a click made while moving trims the part the button went down
/// on.
#[test]
fn a_click_made_while_moving_trims_in_trim() {
    let mut h = select_harness();
    // A vertical cutting edge through the horizontal line, at x = 30.
    h.state_mut().session.execute("line", &json!({"points": [[30, 40], [30, 60]]})).unwrap();
    h.state_mut().start("trim");
    h.run_steps(2);
    press_moving(&mut h, Vec2::new(36.0, 50.0), Vec2::new(39.0, 56.0), 4);
    let got = lines(&h);
    assert!(
        got.iter().any(|(a, b)| a.near(Vec2::new(20.0, 50.0), 1e-6) && b.near(Vec2::new(30.0, 50.0), 1e-6)),
        "the right part of the horizontal line was trimmed: {got:?}"
    );
}

/// With SELECTIONCYCLING 2, a click made while moving over overlapping objects opens selection
/// cycling's list, as a still click does, instead of picking one of them.
#[test]
fn a_click_made_while_moving_over_overlapping_objects_lists_them() {
    let mut h = select_harness();
    h.state_mut().session.execute("line", &json!({"points": [[20, 50], [60, 50]]})).unwrap();
    h.state_mut().session.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": 2 })).unwrap();
    h.state_mut().session.set_selection(Vec::new());
    h.run_steps(2);
    press_moving(&mut h, Vec2::new(30.0, 50.0), Vec2::new(32.0, 54.0), 4);
    assert_eq!(h.state().canvas.cycle_rows().len(), 2, "both lines are listed");
    assert!(h.state().session.selection().is_empty(), "nothing is picked until a row is chosen");
}
