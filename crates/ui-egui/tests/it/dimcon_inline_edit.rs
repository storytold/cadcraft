//! Double-clicking a dynamic dimensional constraint on the canvas edits its expression inline:
//! clicks on the label stay off the canvas (no selection window, no text edit behind it), slight
//! pointer movement during the double-click still opens the box, the existing expression starts
//! selected so typing replaces it, and Enter or focus loss commits while Escape cancels.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Dimcon inline edit", None);
    session.settings.osmode = 0;
    session.execute("line", &json!({ "points": [[0, 0], [20, 0]] })).unwrap();
    let handle = session.doc().unwrap().model.iter().next().unwrap().handle.hex();
    session.execute("dchorizontal", &json!({ "h1": handle, "value": 20.0, "name": "d1" })).unwrap();
    session.execute("view.set", &json!({ "center": [10, 5], "height": 40 })).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    // 60 frames a second: the default step (0.25 s) spreads two clicks past the double-click delay.
    let mut h = Harness::builder().with_size(vec2(1280.0, 760.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// Screen centre of `d1`'s dynamic dimension label (26 pt above the horizontal line's midpoint).
fn pill_pos(h: &Harness<'_, CadApp>) -> Pos2 {
    let xf = h.state().canvas.xf.unwrap();
    let d = h.state().session.doc().unwrap();
    let (a, b) = d
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .unwrap();
    let s1 = xf.to_screen(a);
    let s2 = xf.to_screen(b);
    pos2((s1.x + s2.x) * 0.5, s1.y.min(s2.y) - 26.0)
}

fn click_at(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn double_click_at(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    click_at(h, pos);
    click_at(h, pos);
    h.run_steps(2);
}

fn d1_expr(h: &Harness<'_, CadApp>) -> String {
    let d = h.state().session.doc().unwrap();
    d.constraints.iter().find(|c| c.name == "d1").unwrap().expr.clone()
}

fn line_length(h: &Harness<'_, CadApp>) -> f64 {
    let d = h.state().session.doc().unwrap();
    d.model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Line(l) => Some(l.a.xy().dist(l.b.xy())),
            _ => None,
        })
        .unwrap()
}

#[test]
fn double_click_replaces_expression_without_opening_selection_window() {
    let mut h = harness();
    let pos = pill_pos(&h);

    // Hover and single-click on the label must not open a canvas selection window.
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    click_at(&mut h, pos);
    h.run_steps(2);
    assert!(h.state().session.pending_window.is_none(), "clicking the label opened a selection window");

    // Double-click opens the inline editor with the existing expression selected, so typing
    // replaces it instead of appending to `20`.
    double_click_at(&mut h, pos);
    assert!(h.state().session.pending_window.is_none());
    assert!(h.get_by_role_and_label(Role::TextInput, "d1 =").is_focused(), "the inline expression editor has keyboard focus");
    h.event(Event::Text("30".into()));
    h.run_steps(2);
    assert!(h.state().cmd.buffer.is_empty(), "keys went to the command line instead of the inline box");
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "30");
    assert!((line_length(&h) - 30.0).abs() < 1e-6);

    // Wait past egui's double-click window so the next gesture is a fresh double-click, then
    // re-open and verify the text box starts with the updated expression selected cleanly.
    h.run_steps(25);
    let pos2 = pill_pos(&h);
    double_click_at(&mut h, pos2);
    h.event(Event::Text("25".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "25");
    assert!((line_length(&h) - 25.0).abs() < 1e-6);
}

#[test]
fn double_click_with_slight_pointer_drift_opens_editor() {
    let mut h = harness();
    let pos = pill_pos(&h);
    let drifted = pos + vec2(2.0, 1.5);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);

    // First click drifts 2.5 pt across multiple pointer moves while held (typical trackpad/mouse
    // jitter); second click lands at the drifted position.
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        h.event(Event::PointerMoved(pos + (drifted - pos) * t));
    }
    h.event(Event::PointerButton { pos: drifted, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    click_at(&mut h, drifted);
    h.run_steps(2);

    assert!(h.state().session.pending_window.is_none());
    h.event(Event::Text("35".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "35");
    assert!((line_length(&h) - 35.0).abs() < 1e-6);
}

#[test]
fn double_click_on_pill_over_text_edits_constraint_not_text() {
    let mut h = harness();
    let pos = pill_pos(&h);
    let world = h.state().canvas.xf.unwrap().to_world(pos);
    h.state_mut().run("text", json!({ "at": [world.x - 4.0, world.y - 1.2], "text": "LONG TEXT LABEL", "height": 2.5 })).unwrap();
    h.run_steps(3);

    double_click_at(&mut h, pos);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), None, "TEXTEDIT stole the double-click");
    h.event(Event::Text("28".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "28");
}

#[test]
fn clicking_outside_commits_changed_expression_and_escape_cancels() {
    let mut h = harness();
    let pos = pill_pos(&h);

    double_click_at(&mut h, pos);
    h.event(Event::Text("42".into()));
    h.run_steps(2);
    // Click empty canvas away from the popup to finish editing without Enter (matches Parameters Manager).
    let outside = h.state().canvas.xf.unwrap().to_screen(Vec2::new(10.0, -10.0));
    click_at(&mut h, outside);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "42");

    // Dismiss any selection window opened by the outside click, then re-open and cancel with Escape.
    h.key_press(Key::Escape);
    h.run_steps(2);
    let pos2 = pill_pos(&h);
    double_click_at(&mut h, pos2);
    h.event(Event::Text("99".into()));
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "42");
}

#[test]
fn pending_inline_edit_survives_window_blur() {
    let mut h = harness();
    let pos = pill_pos(&h);
    double_click_at(&mut h, pos);
    h.event(Event::Text("32".into()));
    h.run_steps(2);
    h.event(Event::WindowFocused(false));
    h.run_steps(2);
    h.event(Event::WindowFocused(true));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "32");
}

#[test]
fn clicking_pill_during_command_passes_point_to_drawing() {
    let mut h = harness();
    let pos = pill_pos(&h);
    h.state_mut().start("line");
    h.run_steps(2);
    assert_eq!(h.state().session.current_prompt().unwrap().message, "Specify first point");

    click_at(&mut h, pos);
    h.run_steps(2);
    assert_eq!(h.state().session.current_prompt().unwrap().message, "Specify next point");
}

#[test]
fn clicking_pill_during_lisp_prompt_passes_point_to_drawing() {
    let mut h = harness();
    let pos = pill_pos(&h);
    h.state_mut().session.cmdline("(setq p (getpoint \"\nPick: \"))").unwrap();
    h.run_steps(2);
    assert!(h.state().session.prompting());

    click_at(&mut h, pos);
    h.run_steps(2);
    assert!(!h.state().session.lisp_running(), "the click on the label answered the LISP prompt");
    assert!(h.state().session.lisp_var("P").as_point().is_some(), "the LISP prompt received a point");
}

#[test]
fn switching_drawing_tabs_drops_pending_edit_without_writing_to_other_drawing() {
    let mut session = Session::empty();
    let add_d1 = |session: &mut Session, title: &str, length: f64| {
        session.open_drawing(Drawing::new_imperial(), title, None);
        session.settings.osmode = 0;
        session.execute("line", &json!({ "points": [[0, 0], [length, 0]] })).unwrap();
        let handle = session.doc().unwrap().model.iter().next().unwrap().handle.hex();
        session.execute("dchorizontal", &json!({ "h1": handle, "value": length, "name": "d1" })).unwrap();
        session.execute("view.set", &json!({ "center": [length / 2.0, 5.0], "height": 40 })).unwrap();
    };
    add_d1(&mut session, "First drawing", 20.0);
    let first = session.active;
    add_d1(&mut session, "Second drawing", 10.0);
    let second = session.active;
    session.execute("document.switch", &json!({ "index": first })).unwrap();

    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1280.0, 760.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);

    let pos = pill_pos(&h);
    double_click_at(&mut h, pos);
    h.event(Event::Text("99".into()));
    h.run_steps(2);

    // Click the other drawing's tab: switching drawings before the canvas draws must drop the
    // pending inline edit instead of committing `99` to the other drawing's `d1`.
    h.get_by_label("Second drawing").click();
    h.run_steps(3);

    assert_eq!(h.state().session.active, second);
    assert!(h.query_by_role_and_label(Role::TextInput, "d1 =").is_none(), "inline editor closed on tab switch");
    assert_eq!(d1_expr(&h), "10", "the second drawing's d1 was not overwritten");
    assert!((line_length(&h) - 10.0).abs() < 1e-6);

    h.get_by_label("First drawing").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active, first);
    assert_eq!(d1_expr(&h), "20", "the first drawing's pending edit was dropped");
}

#[test]
fn enter_to_commit_does_not_repeat_last_command() {
    let mut h = harness();
    let pos = pill_pos(&h);

    // Edit once, then Undo via `u` (sets `last_command = Some("u")`, matching Cmd+Z).
    double_click_at(&mut h, pos);
    h.event(Event::Text("30".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "30");

    h.state_mut().start("u");
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "20");
    assert_eq!(h.state().session.last_command.as_deref(), Some("u"));

    // Committing with Enter must not leak into `cmdline::keyboard` in the same frame and repeat Undo.
    h.run_steps(25);
    let pos2 = pill_pos(&h);
    double_click_at(&mut h, pos2);
    h.event(Event::Text("35".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "35", "Enter must not repeat Undo and revert the edit");
    assert!((line_length(&h) - 35.0).abs() < 1e-6);

    // Same when `last_command` is an interactive drawing command (LINE): Enter must not start it.
    h.state_mut().start("line");
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().session.last_command.as_deref(), Some("line"));

    h.run_steps(25);
    let pos3 = pill_pos(&h);
    double_click_at(&mut h, pos3);
    h.event(Event::Text("40".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(d1_expr(&h), "40");
    assert!(h.state().session.running.is_none(), "Enter must not repeat LINE");
}
