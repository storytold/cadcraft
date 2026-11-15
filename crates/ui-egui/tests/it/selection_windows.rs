//! STRETCH and TRIM select like every other command (#1001 follow-up, #1116): a press counts at
//! once, a click-click window waits for the next press, and a drag closes the window its press
//! opened where the button comes up (PICKDRAG 2), whether the window is the session's or TRIM's
//! own crossing. The harness runs at 60 frames a second; each queued event is one frame.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::prompt::Input;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// A horizontal line from (20, 20) to (60, 20) and a vertical one at x = 40 crossing it.
fn session() -> Session {
    let mut s = Session::empty();
    s.open_drawing(Drawing::new_imperial(), "Selection windows", None);
    s.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
    s.settings.osmode = 0;
    s.settings.snapmode = false;
    s.settings.orthomode = false;
    s.execute("line", &json!({"points": [[20, 20], [60, 20]]})).unwrap();
    s.execute("line", &json!({"points": [[40, 10], [40, 30]]})).unwrap();
    s
}

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(session(), Services::default());
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

/// Press at `a`, move to `b` over `frames` frames and let go there (30 frames: half a second, a
/// drag; 2 frames: a click made on the move).
fn sweep(h: &mut Harness<'_, CadApp>, a: Vec2, b: Vec2, frames: usize) {
    let (from, to) = (screen(h, a), screen(h, b));
    h.event(Event::PointerMoved(from));
    button(h, from, true);
    for i in 1..=frames {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / frames as f32)));
    }
    button(h, to, false);
    h.run_steps(2);
}

fn start(h: &mut Harness<'_, CadApp>, id: &str) {
    h.state_mut().start(id);
    h.run_steps(2);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some(id));
}

fn input(h: &mut Harness<'_, CadApp>, i: Input) {
    h.state_mut().session.input(i).unwrap();
    h.run_steps(2);
}

fn lines(s: &Session) -> Vec<(Vec2, Vec2)> {
    let mut v: Vec<(Vec2, Vec2)> = s
        .state()
        .unwrap()
        .doc
        .space(&Space::Model)
        .unwrap()
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect();
    v.sort_by(|p, q| (p.0.x, p.0.y, p.1.x, p.1.y).partial_cmp(&(q.0.x, q.0.y, q.1.x, q.1.y)).unwrap());
    v
}

/// STRETCH the right end of the horizontal line 5 to the right after selecting with `select`.
fn stretch_with(select: impl Fn(&mut Harness<'_, CadApp>)) -> Vec<(Vec2, Vec2)> {
    let mut h = harness();
    start(&mut h, "stretch");
    select(&mut h);
    assert!(h.state().session.selection_window().is_none(), "the window closed");
    input(&mut h, Input::Enter);
    input(&mut h, Input::Point(Vec2::new(0.0, 0.0)));
    input(&mut h, Input::Point(Vec2::new(5.0, 0.0)));
    assert!(h.state().session.running.is_none(), "{}", h.state().session.prompt_text());
    lines(&h.state().session)
}

#[test]
fn a_stretch_window_closes_by_drag_or_by_two_clicks_on_the_move() {
    let expected = vec![(Vec2::new(20.0, 20.0), Vec2::new(65.0, 20.0)), (Vec2::new(40.0, 10.0), Vec2::new(40.0, 30.0))];
    // Right to left over the horizontal line's right end only: a crossing.
    let dragged = stretch_with(|h| sweep(h, Vec2::new(70.0, 25.0), Vec2::new(55.0, 15.0), 30));
    assert_eq!(dragged, expected, "a drag closes STRETCH's crossing where the button comes up");
    let clicked = stretch_with(|h| {
        sweep(h, Vec2::new(70.0, 25.0), Vec2::new(70.5, 25.0), 2);
        assert!(h.state().session.selection_window().is_some(), "a click leaves the window open");
        sweep(h, Vec2::new(55.0, 15.0), Vec2::new(57.0, 15.0), 2);
    });
    assert_eq!(clicked, expected, "the second corner is where the button went down");
}

/// TRIM's crossing on the right of the cutting line, made in a session by typed points.
fn trimmed_by_typed_crossing() -> Vec<(Vec2, Vec2)> {
    let mut s = session();
    s.viewport_px = (1600.0, 1000.0);
    s.start("trim").unwrap();
    s.input(Input::Point(Vec2::new(55.0, 25.0))).unwrap();
    s.input(Input::Point(Vec2::new(45.0, 15.0))).unwrap();
    s.input(Input::Enter).unwrap();
    lines(&s)
}

#[test]
fn trims_crossing_closes_where_a_drag_lets_go() {
    let mut h = harness();
    start(&mut h, "trim");
    sweep(&mut h, Vec2::new(55.0, 25.0), Vec2::new(45.0, 15.0), 30);
    assert!(h.state().session.selection_window().is_none(), "the drag closed TRIM's crossing");
    input(&mut h, Input::Enter);
    let got = lines(&h.state().session);
    assert_eq!(got, trimmed_by_typed_crossing());
    assert!(got.iter().all(|(a, b)| a.x <= 40.0 + 1e-9 && b.x <= 40.0 + 1e-9), "the right part is trimmed off: {got:?}");
}

#[test]
fn with_pickdrag_off_a_drag_leaves_trims_crossing_open() {
    let mut h = harness();
    h.state_mut().run("options", json!({"PICKDRAG": 0})).unwrap();
    start(&mut h, "trim");
    sweep(&mut h, Vec2::new(55.0, 25.0), Vec2::new(45.0, 15.0), 30);
    let w = h.state().session.selection_window().expect("two clicks only: the window waits");
    assert!(w.corner.dist(Vec2::new(55.0, 25.0)) < 0.5, "{w:?}");
    sweep(&mut h, Vec2::new(45.0, 15.0), Vec2::new(45.0, 15.0), 1);
    assert!(h.state().session.selection_window().is_none());
    input(&mut h, Input::Enter);
    assert_eq!(lines(&h.state().session), trimmed_by_typed_crossing());
}

#[test]
fn with_pickdrag_1_the_release_always_closes_the_window() {
    // Drag only: even a quick press and release on the move closes the window where the button
    // comes up, at the session's "Select objects" and at TRIM's crossing alike.
    let mut h = harness();
    h.state_mut().run("options", json!({"PICKDRAG": 1})).unwrap();
    start(&mut h, "stretch");
    sweep(&mut h, Vec2::new(70.0, 25.0), Vec2::new(55.0, 15.0), 2);
    assert!(h.state().session.selection_window().is_none());
    assert_eq!(h.state().session.selection().len(), 1, "the crossing took the horizontal line");
    h.state_mut().session.cancel();
    start(&mut h, "trim");
    sweep(&mut h, Vec2::new(55.0, 25.0), Vec2::new(45.0, 15.0), 2);
    assert!(h.state().session.selection_window().is_none());
    input(&mut h, Input::Enter);
    assert_eq!(lines(&h.state().session), trimmed_by_typed_crossing());
}
