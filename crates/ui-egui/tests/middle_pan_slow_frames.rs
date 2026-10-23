//! Middle-button pan on slow frames (#1131): a heavy drawing makes frames slow, so a whole drag
//! (and even its press and release) can arrive in one frame. The pan follows the events, not just
//! where the pointer is when the frame ends.

use cadcraft_engine::{Session, View};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("circle", json!({"center":[100,70],"radius":26})).unwrap();
    app.run("view.set", json!({"center":[100,70],"height":220})).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn middle(at: egui::Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Middle, pressed, modifiers: Modifiers::NONE }
}

fn view(h: &Harness<'_, CadApp>) -> View {
    h.state().session.state().unwrap().view()
}

/// The drawing moved left by `dx` screen points (the pointer moved right by `dx`).
fn panned_by(h: &Harness<'_, CadApp>, before: View, after: View, dx: f64) -> bool {
    let scale = f64::from(h.state().canvas.rect.unwrap().height()) / before.height;
    (before.center.x - after.center.x - dx / scale).abs() < 1e-6 && (before.center.y - after.center.y).abs() < 1e-9
}

#[test]
fn a_middle_drag_that_ends_within_one_slow_frame_still_pans() {
    let mut h = harness();
    let c = h.state().canvas.rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(c));
    h.run_steps(1);
    let before = view(&h);
    h.input_mut().events.push(middle(c, true));
    h.run_steps(1);
    for s in 1..=5 {
        h.input_mut().events.push(Event::PointerMoved(c + vec2(20.0 * s as f32, 0.0)));
    }
    h.input_mut().events.push(middle(c + vec2(100.0, 0.0), false));
    h.run_steps(1);
    let after = view(&h);
    assert!(panned_by(&h, before, after, 100.0), "{before:?} -> {after:?}");
}

#[test]
fn a_whole_middle_drag_within_one_slow_frame_still_pans() {
    let mut h = harness();
    let c = h.state().canvas.rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(c));
    h.run_steps(1);
    let before = view(&h);
    h.input_mut().events.push(middle(c, true));
    for s in 1..=4 {
        h.input_mut().events.push(Event::PointerMoved(c + vec2(15.0 * s as f32, 0.0)));
    }
    h.input_mut().events.push(middle(c + vec2(60.0, 0.0), false));
    h.run_steps(1);
    let after = view(&h);
    assert!(panned_by(&h, before, after, 60.0), "{before:?} -> {after:?}");
    // The drag is over: moving the pointer afterwards leaves the view alone.
    h.input_mut().events.push(Event::PointerMoved(c + vec2(200.0, 40.0)));
    h.run_steps(1);
    assert_eq!(view(&h), after);
}
