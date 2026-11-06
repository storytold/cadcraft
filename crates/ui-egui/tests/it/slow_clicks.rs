//! A click on an object selects it, however long the frames around it take (#2813).
//!
//! egui calls a press that lasts longer than `max_click_duration` (0.8 s) a drag, and it times the
//! press by frames. On a large drawing that draws slowly, an ordinary click can come down in one
//! frame and up in the next, more than 0.8 s later. The canvas took that "drag" for a press-and-drag
//! selection window that never left the click, a window of no size, and nothing was selected.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, Handle, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// Seconds each harness step advances egui's clock: two steps between press and release make a
/// press longer than egui's 0.8 s click limit.
const STEP: f32 = 0.5;

/// A drawing with one line from (10, 10) to (30, 10), no object snaps, nothing selected.
fn harness() -> (Harness<'static, CadApp>, Handle) {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Slow clicks", None);
    session.execute("view.set", &json!({"center": [50, 50], "height": 100})).unwrap();
    session.settings.osmode = 0;
    session.execute("line", &json!({"points": [[10, 10], [30, 10]]})).unwrap();
    let line = session.doc().unwrap().space(&Space::Model).unwrap().iter().next().unwrap().handle;
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(STEP).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, line)
}

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn button(h: &mut Harness<'_, CadApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

/// Press at `down`, let `frames` slow frames pass, release at `up`.
fn slow_click(h: &mut Harness<'_, CadApp>, down: Pos2, up: Pos2, frames: usize) {
    button(h, down, true);
    h.run_steps(frames);
    button(h, up, false);
    h.run_steps(2);
}

#[test]
fn a_click_spanning_slow_frames_selects_the_object() {
    let (mut h, line) = harness();
    let at = screen(&h, Vec2::new(20.0, 10.0));
    slow_click(&mut h, at, at, 2);
    assert_eq!(h.state().session.selection(), vec![line]);
    assert!(h.state().session.pending_window.is_none(), "no window is left open");
}

/// The hand drifts a pixel or two while the button is down: still a click.
#[test]
fn a_slow_click_that_drifts_within_the_click_distance_selects_the_object() {
    let (mut h, line) = harness();
    let at = screen(&h, Vec2::new(20.0, 10.0));
    slow_click(&mut h, at, at + vec2(2.0, 1.0), 2);
    assert_eq!(h.state().session.selection(), vec![line]);
}

/// In empty space a slow click does what a quick one does there: it starts a click-click window.
#[test]
fn a_slow_click_in_empty_space_starts_a_window_as_a_click_does() {
    let (mut h, _) = harness();
    let at = screen(&h, Vec2::new(20.0, 40.0));
    slow_click(&mut h, at, at, 2);
    assert!(h.state().session.selection().is_empty());
    let corner = h.state().session.pending_window.map(|w| w.corner);
    assert!(corner.is_some_and(|c| c.near(Vec2::new(20.0, 40.0), 0.5)), "{corner:?}");
}

/// A real press-and-drag still opens and closes a selection window, slow or not.
#[test]
fn a_press_and_drag_still_selects_by_window() {
    let (mut h, line) = harness();
    let (from, to) = (screen(&h, Vec2::new(5.0, 15.0)), screen(&h, Vec2::new(35.0, 5.0)));
    button(&mut h, from, true);
    h.run_steps(1);
    h.event(Event::PointerMoved(from + (to - from) / 2.0));
    h.run_steps(2);
    button(&mut h, to, false);
    h.run_steps(2);
    assert_eq!(h.state().session.selection(), vec![line]);
    assert!(h.state().session.pending_window.is_none());
}

/// While a command asks for objects, a slow click on one picks it.
#[test]
fn a_slow_click_picks_at_select_objects() {
    let (mut h, line) = harness();
    h.state_mut().start("erase");
    h.run_steps(2);
    let at = screen(&h, Vec2::new(20.0, 10.0));
    slow_click(&mut h, at, at, 2);
    h.state_mut().session.input(cadcraft_engine::Input::Enter).unwrap();
    h.run_steps(2);
    let d = h.state().session.doc().unwrap();
    assert!(d.entity(line).is_none(), "the line was picked and erased");
}
