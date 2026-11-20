//! While Batch Plot is open the drawing takes no keys: Ctrl+A picks the window's sheets, not the
//! drawing's objects, and Delete removes the picked sheets, not the drawing's selection.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

#[test]
fn batch_plot_keeps_the_keys_from_the_drawing() {
    let mut h = harness();
    h.state_mut().run("circle", json!({"center": [0, 0], "radius": 1})).unwrap();
    let circle = h.state().session.doc().unwrap().model.iter().next().map(|e| e.handle).unwrap();
    h.state_mut().session.set_selection(Vec::new());
    h.state_mut().ui.dialog = Some("publish".into());
    h.run_steps(4);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(3);
    assert!(h.state().session.selection().is_empty(), "Ctrl+A did not select the drawing's objects");
    h.state_mut().session.set_selection(vec![circle]);
    h.run_steps(2);
    h.key_press(Key::Delete);
    h.run_steps(3);
    assert!(h.state().session.doc().unwrap().entity(circle).is_some(), "Delete did not erase the selected circle");
    assert_eq!(h.state().ui.dialog.as_deref(), Some("publish"));
}
