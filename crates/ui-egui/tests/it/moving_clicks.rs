//! A click made while the mouse is moving still selects (#1546): egui drops a click whose button
//! comes up away from where it went down, and the pick or the window's first corner never
//! happened. As with placing points (#815), the click counts — landing where the button came up.

use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    app.session.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
    app.session.execute("view.set", &json!({"center":[5,0],"height":30})).unwrap();
    app.session.settings.osmode = 0;
    app.session.settings.snapmode = false;
    app.session.settings.orthomode = false;
    // Two-click selection: a moving click must never be a press-and-drag window.
    app.session.execute("options", &json!({"PICKDRAG":0})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn button(h: &mut Harness<'_, CadApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

#[test]
fn a_click_made_while_the_mouse_moves_still_picks() {
    let mut h = harness();
    // The press starts well above the line; the hand is on its way down when the button comes up,
    // landing inside the pick box around the line.
    let (down, up) = (screen(&h, Vec2::new(5.0, 1.0)), screen(&h, Vec2::new(5.0, 0.1)));
    button(&mut h, down, true);
    button(&mut h, up, false);
    h.run_steps(2);
    assert_eq!(h.state().session.selection().len(), 1);
}

#[test]
fn a_click_made_while_the_mouse_moves_still_opens_a_selection_window() {
    let mut h = harness();
    // The first corner lands while the hand is moving, to the upper right of the line.
    let (down, up) = (screen(&h, Vec2::new(12.0, 3.0)), screen(&h, Vec2::new(12.6, 2.4)));
    button(&mut h, down, true);
    button(&mut h, up, false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_some(), "the moving click must open the window");
    // The second corner, deliberately: lower and to the left, so the window crosses the line.
    h.run_steps(30); // Separate the deliberate click from egui's double-click interval.
    let b = screen(&h, Vec2::new(-2.0, -3.0));
    button(&mut h, b, true);
    button(&mut h, b, false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_none());
    assert_eq!(h.state().session.selection().len(), 1, "the crossing window must take the line");
}

#[test]
fn a_click_made_while_the_mouse_moves_still_picks_at_a_select_prompt() {
    let mut h = harness();
    h.state_mut().session.start("select").unwrap();
    h.run_steps(2);
    let (down, up) = (screen(&h, Vec2::new(5.0, 1.0)), screen(&h, Vec2::new(5.0, 0.1)));
    button(&mut h, down, true);
    button(&mut h, up, false);
    h.run_steps(2);
    assert_eq!(h.state().session.selection().len(), 1);
}

#[test]
fn a_moving_release_never_consumes_a_pending_corner() {
    let mut h = harness();
    let a = screen(&h, Vec2::new(5.0, -5.0));
    button(&mut h, a, true);
    button(&mut h, a, false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_some(), "the corner click must open the window");
    // A held-button movement after the corner is not the second click, however short: the window
    // stays open for the deliberate click that follows.
    button(&mut h, a + vec2(0.0, 20.0), true);
    button(&mut h, a + vec2(12.0, 20.0), false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_some(), "the movement must not consume the corner");
    assert!(h.state().session.selection().is_empty());
    h.run_steps(30);
    let b = screen(&h, Vec2::new(-2.0, 2.0));
    button(&mut h, b, true);
    button(&mut h, b, false);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_none());
    assert_eq!(h.state().session.selection().len(), 1, "the deliberate click completes the window");
}
