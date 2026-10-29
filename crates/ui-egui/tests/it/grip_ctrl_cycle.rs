//! Ctrl tapped while a grip is hot steps through the grip's options, as in AutoCAD's
//! multifunctional grips (gaps U5): a polyline vertex goes Stretch, Add Vertex, Remove Vertex and
//! back. Ctrl held for another key (Ctrl+Z) is not a tap. Driven with real pointer and modifier
//! events.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> (Harness<'static, CadApp>, Handle) {
    let mut session = Session::new();
    session.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10]] })).unwrap();
    let h = session.doc().unwrap().model.iter().last().unwrap().handle;
    session.execute("view.set", &json!({ "center": [5, 5], "height": 30 })).unwrap();
    session.set_selection(vec![h]);
    let mut app = CadApp::new(session, Services::default());
    app.ui.start_tab = false;
    app.canvas.zoom_pending = false;
    let mut harness = Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    (harness, h)
}

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn click(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

/// Ctrl down and up: across two frames, or within one (a quick tap).
fn tap_ctrl(h: &mut Harness<'_, CadApp>, quick: bool) {
    h.event(Event::ModifiersChanged(Modifiers::CTRL));
    if !quick {
        h.run_steps(2);
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

/// Ctrl as egui-winit reports it on Windows and Linux: `command` is set with `ctrl`, so the
/// Ctrl shortcuts (Ctrl+Z, Ctrl+E) fire.
const CTRL: Modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };

/// Ctrl+`key`, one event per frame, with Ctrl let go before the key (`ctrl_up_first`, common in
/// a quick chord) or after it.
fn chord(h: &mut Harness<'_, CadApp>, key: egui::Key, ctrl_up_first: bool) {
    let k = |pressed, modifiers| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    let mut events = vec![Event::ModifiersChanged(CTRL), k(true, CTRL)];
    if ctrl_up_first {
        events.extend([Event::ModifiersChanged(Modifiers::NONE), k(false, Modifiers::NONE)]);
    } else {
        events.extend([k(false, CTRL), Event::ModifiersChanged(Modifiers::NONE)]);
    }
    for e in events {
        h.event(e);
        h.run_steps(2);
    }
}

fn prompt(h: &Harness<'_, CadApp>) -> String {
    h.state().session.prompt_text()
}

fn vertex_count(h: &Harness<'_, CadApp>, pl: Handle) -> usize {
    match &h.state().session.doc().unwrap().entity(pl).unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.len(),
        k => panic!("{k:?}"),
    }
}

#[test]
fn ctrl_steps_through_a_vertex_grips_options() {
    let (mut h, pl) = harness();
    let grip = screen(&h, Vec2::new(10.0, 0.0));
    click(&mut h, grip);
    assert!(prompt(&h).contains("** STRETCH **"), "{}", prompt(&h));
    tap_ctrl(&mut h, false);
    assert!(prompt(&h).contains("** ADD VERTEX **"), "{}", prompt(&h));
    tap_ctrl(&mut h, true);
    assert!(prompt(&h).contains("** REMOVE VERTEX **"), "{}", prompt(&h));
    tap_ctrl(&mut h, false);
    assert!(prompt(&h).contains("** STRETCH **"), "{}", prompt(&h));
    // Ctrl+Z is not a tap, even with Ctrl let go before Z.
    chord(&mut h, egui::Key::Z, true);
    assert!(!prompt(&h).contains("** ADD VERTEX **"), "{}", prompt(&h));
    // To Remove Vertex again; a click anywhere removes the hot vertex.
    if h.state().session.running.is_none() {
        click(&mut h, grip);
    }
    while !prompt(&h).contains("** REMOVE VERTEX **") {
        tap_ctrl(&mut h, false);
        assert!(prompt(&h).contains("**"), "the grip edit ended: {}", prompt(&h));
    }
    let away = screen(&h, Vec2::new(3.0, 7.0));
    click(&mut h, away);
    assert!(h.state().session.running.is_none(), "{}", prompt(&h));
    assert_eq!(vertex_count(&h, pl), 2);
}

#[test]
fn ctrl_without_a_hot_grip_does_nothing() {
    let (mut h, pl) = harness();
    tap_ctrl(&mut h, false);
    assert!(h.state().session.running.is_none());
    assert_eq!(vertex_count(&h, pl), 3);
}

/// A Ctrl shortcut never steps the grip, whether Ctrl comes up before or after its key: the tap
/// is read before the shortcuts consume the key press.
#[test]
fn ctrl_shortcuts_are_not_taps() {
    for (key, ctrl_up_first) in [(egui::Key::E, false), (egui::Key::E, true), (egui::Key::Num1, true), (egui::Key::Z, true)] {
        let (mut h, _) = harness();
        let grip = screen(&h, Vec2::new(10.0, 0.0));
        click(&mut h, grip);
        assert!(prompt(&h).contains("** STRETCH **"), "{}", prompt(&h));
        h.state_mut().session.log.clear();
        chord(&mut h, key, ctrl_up_first);
        let case = format!("Ctrl+{key:?}, Ctrl up first: {ctrl_up_first}");
        let log = h.state().session.log.join(" | ");
        assert!(!log.contains("** ADD VERTEX **") && !prompt(&h).contains("** ADD VERTEX **"), "{case}: {log}");
        match key {
            egui::Key::E => {
                assert!(log.contains("<Isoplane"), "{case}: the shortcut ran: {log}");
                assert!(prompt(&h).contains("** STRETCH **"), "{case}: {}", prompt(&h));
            }
            egui::Key::Z => assert!(log.contains("Nothing to undo"), "{case}: the shortcut ran: {log}"),
            _ => assert!(prompt(&h).contains("** STRETCH **"), "{case}: {}", prompt(&h)),
        }
    }
}

/// Ctrl with Shift or Alt is not a tap; two clean taps within one frame step twice.
#[test]
fn ctrl_with_another_modifier_is_not_a_tap_and_quick_taps_all_count() {
    let (mut h, _) = harness();
    let grip = screen(&h, Vec2::new(10.0, 0.0));
    click(&mut h, grip);
    let shift = Modifiers { shift: true, ..Modifiers::NONE };
    let alt = Modifiers { alt: true, ..Modifiers::NONE };
    // Ctrl down, Shift down, Ctrl up (Shift up after): not a tap.
    for e in [CTRL, CTRL | shift, shift, Modifiers::NONE] {
        h.event(Event::ModifiersChanged(e));
        h.run_steps(2);
    }
    assert!(prompt(&h).contains("** STRETCH **"), "{}", prompt(&h));
    // Shift held first, then Ctrl tapped: not a tap either.
    for e in [shift, CTRL | shift, shift, Modifiers::NONE] {
        h.event(Event::ModifiersChanged(e));
        h.run_steps(2);
    }
    assert!(prompt(&h).contains("** STRETCH **"), "{}", prompt(&h));
    // Ctrl+Alt within one frame.
    for e in [CTRL, CTRL | alt, Modifiers::NONE] {
        h.event(Event::ModifiersChanged(e));
    }
    h.run_steps(2);
    assert!(prompt(&h).contains("** STRETCH **"), "{}", prompt(&h));
    // Two clean taps in one frame: Add Vertex, then Remove Vertex.
    for e in [CTRL, Modifiers::NONE, CTRL, Modifiers::NONE] {
        h.event(Event::ModifiersChanged(e));
    }
    h.run_steps(2);
    assert!(prompt(&h).contains("** REMOVE VERTEX **"), "{}", prompt(&h));
}
