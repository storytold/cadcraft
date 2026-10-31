//! Press-first clicks (#1001): a click counts when the button goes down, where it went down, even
//! while the mouse is moving; holding the button after the pointer left a small tolerance makes a
//! drag. The harness runs at 60 frames a second and each queued event is one frame, so the number
//! of moves between press and release is how long the button was held.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::prompt::Input;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Press first", None);
    session.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
    session.settings.osmode = 0;
    session.settings.snapmode = false;
    session.settings.orthomode = false;
    session.execute("line", &json!({"points": [[20, 20], [60, 20]]})).unwrap();
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

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn button(h: &mut Harness<'_, CadApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

/// Press at `at`, then move `dx` points to the right over `frames` frames and let go there.
fn moving_click(h: &mut Harness<'_, CadApp>, at: Pos2, dx: f32, frames: usize) {
    h.event(Event::PointerMoved(at));
    button(h, at, true);
    for i in 1..=frames {
        h.event(Event::PointerMoved(at + vec2(dx * i as f32 / frames as f32, 0.0)));
    }
    button(h, at + vec2(dx, 0.0), false);
    h.run_steps(2);
}

/// A click that moves 15 points while the button is down, let go within two frames.
fn quick_moving_click(h: &mut Harness<'_, CadApp>, at: Vec2) {
    let pos = screen(h, at);
    moving_click(h, pos, 15.0, 2);
}

fn start(h: &mut Harness<'_, CadApp>, id: &str) {
    h.state_mut().start(id);
    h.run_steps(2);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some(id));
}

fn enter(h: &mut Harness<'_, CadApp>) {
    h.state_mut().session.input(Input::Enter).unwrap();
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
fn move_takes_the_base_and_second_point_where_the_button_went_down() {
    let mut h = harness();
    start(&mut h, "move");
    quick_moving_click(&mut h, Vec2::new(40.0, 20.0));
    enter(&mut h);
    // Base point and second point, both clicked on the move: 15 points is 1.5 drawing units here.
    quick_moving_click(&mut h, Vec2::new(30.0, 40.0));
    quick_moving_click(&mut h, Vec2::new(50.0, 40.0));
    let got = lines(&h);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].0.near(Vec2::new(40.0, 20.0), 1e-3) && got[0].1.near(Vec2::new(80.0, 20.0), 1e-3), "moved by (20, 0): {got:?}");
}

#[test]
fn an_object_clicked_on_the_move_is_picked_with_no_command_running() {
    let mut h = harness();
    quick_moving_click(&mut h, Vec2::new(40.0, 20.0));
    assert_eq!(h.state().session.selection().len(), 1);
    assert!(h.state().session.pending_window.is_none());
}

#[test]
fn an_object_clicked_on_the_move_is_picked_at_select_objects() {
    let mut h = harness();
    start(&mut h, "erase");
    quick_moving_click(&mut h, Vec2::new(40.0, 20.0));
    enter(&mut h);
    assert!(lines(&h).is_empty(), "the line was picked and erased");
}

#[test]
fn both_corners_of_a_click_click_window_count_while_the_mouse_moves() {
    let mut h = harness();
    // Lower left, then upper right: a window around the whole line.
    quick_moving_click(&mut h, Vec2::new(10.0, 10.0));
    assert!(h.state().session.pending_window.is_some(), "a quick click leaves the window open for the second corner");
    quick_moving_click(&mut h, Vec2::new(70.0, 30.0));
    assert!(h.state().session.pending_window.is_none());
    assert_eq!(h.state().session.selection().len(), 1);
}

#[test]
fn holding_the_button_while_moving_drags_a_window() {
    let mut h = harness();
    // Half a second of dragging from lower left to upper right of the line.
    let from = screen(&h, Vec2::new(10.0, 10.0));
    let to = screen(&h, Vec2::new(70.0, 30.0));
    h.event(Event::PointerMoved(from));
    button(&mut h, from, true);
    for i in 1..=30 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 30.0)));
    }
    button(&mut h, to, false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_none(), "letting go closes the dragged window");
    assert_eq!(h.state().session.selection().len(), 1);
}

#[test]
fn holding_still_then_moving_and_letting_go_quickly_is_a_click() {
    let mut h = harness();
    let at = screen(&h, Vec2::new(10.0, 10.0));
    h.event(Event::PointerMoved(at));
    button(&mut h, at, true);
    // Held in place for half a second: no drag yet.
    h.run_steps(30);
    h.event(Event::PointerMoved(at + vec2(40.0, 0.0)));
    button(&mut h, at + vec2(40.0, 0.0), false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_some(), "still waiting for the second corner");
}

#[test]
fn two_window_corners_clicked_within_one_frame_both_count() {
    let mut h = harness();
    let (a, b) = (screen(&h, Vec2::new(10.0, 10.0)), screen(&h, Vec2::new(70.0, 30.0)));
    // A slow frame: both clicks arrive before the next one is drawn.
    for pos in [a, b] {
        h.input_mut().events.push(Event::PointerMoved(pos));
        for pressed in [true, false] {
            h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
    }
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_none());
    assert_eq!(h.state().session.selection().len(), 1);
}

#[test]
fn cpolygon_points_clicked_on_the_move_all_count() {
    // #1546: a narrow crossing polygon over the line's right end (x 58.6–59.9, the line ends at
    // 60), each point clicked while moving. Taken where the buttons came up (1.5 units further
    // right) it would miss the line.
    let mut h = harness();
    start(&mut h, "erase");
    h.state_mut().session.input(Input::Keyword("CP".into())).unwrap();
    h.run_steps(2);
    for p in [Vec2::new(58.6, 15.0), Vec2::new(59.9, 15.0), Vec2::new(59.9, 25.0), Vec2::new(58.6, 25.0)] {
        quick_moving_click(&mut h, p);
    }
    enter(&mut h);
    enter(&mut h);
    assert!(lines(&h).is_empty(), "the polygon crossed the line and ERASE removed it");
}
