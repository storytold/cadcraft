//! OPTIONS checkbox and real pointer gestures share the saved PICKDRAG preference.
use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    app.session.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
    app.session.execute("view.set", &json!({"center":[5,0],"height":30})).unwrap();
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

fn button(h: &mut Harness<'_, CadApp>, p: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn drag(h: &mut Harness<'_, CadApp>, a: Pos2, b: Pos2) {
    button(h, a, true);
    for i in 1..=4 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 4.0)));
        h.run_steps(1);
    }
    button(h, b, false);
    h.run_steps(2);
}

#[test]
fn selection_tab_checkbox_updates_and_persists_the_preference() {
    let mut h = harness();
    h.state_mut().ui.dialog = Some("options:selection".into());
    h.run_steps(3);
    h.get_by_label("Select using two clicks only").click();
    h.run_steps(2);
    assert!(!h.state().session.settings.pickdrag);
    let saved = h.state().profile_json();
    let mut restored = CadApp::new(Session::new(), Services::default());
    restored.load_profile(&saved);
    assert!(!restored.session.settings.pickdrag);
    if let Some(dir) = std::env::var_os("CADCRAFT_SELECTION_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("selection-two-clicks.png")).unwrap();
    }
    h.get_by_label("Select using two clicks only").click();
    h.run_steps(2);
    assert!(h.state().session.settings.pickdrag);
}

#[test]
fn drag_is_disabled_but_two_clicks_still_complete_the_window() {
    for prompting in [false, true] {
        let mut h = harness();
        h.state_mut().run("options", json!({"PICKDRAG":0})).unwrap();
        if prompting {
            h.state_mut().session.start("select").unwrap();
        }
        h.run_steps(2);
        let xf = h.state().canvas.xf.unwrap();
        let a = xf.to_screen(Vec2::new(-2.0, -2.0));
        let b = xf.to_screen(Vec2::new(12.0, 2.0));
        drag(&mut h, a, b);
        assert!(h.state().session.selection().is_empty(), "drag must not complete selection");
        h.state_mut().session.cancel();
        h.state_mut().session.set_selection(Vec::new());
        if prompting {
            h.state_mut().session.start("select").unwrap();
        }
        h.run_steps(2);
        button(&mut h, a, true);
        button(&mut h, a, false);
        assert!(h.state().session.pending_window.is_some());
        // A held-button movement after the first click must not consume the second corner.
        drag(&mut h, a, b);
        assert!(h.state().session.pending_window.is_some());
        assert!(h.state().session.selection().is_empty());
        h.run_steps(30); // Separate deliberate clicks from egui's double-click interval.
        button(&mut h, b, true);
        button(&mut h, b, false);
        assert!(h.state().session.pending_window.is_none());
        assert_eq!(h.state().session.selection().len(), 1);
    }
}

#[test]
fn default_mode_keeps_drag_selection_available() {
    let mut h = harness();
    let xf = h.state().canvas.xf.unwrap();
    drag(&mut h, xf.to_screen(Vec2::new(-2.0, -2.0)), xf.to_screen(Vec2::new(12.0, 2.0)));
    assert!(h.state().session.pending_window.is_none());
    assert_eq!(h.state().session.selection().len(), 1);
}
