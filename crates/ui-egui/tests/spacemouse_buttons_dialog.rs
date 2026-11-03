//! The Buttons section of the 3D Mouse dialog: a row per button (the two defaults plus any the
//! device has reported), a command picker with None, per-row and whole-table resets. The edits go
//! through `ui.spacemouse`, so they are validated like the control method's.

use cadcraft_engine::Session;
use cadcraft_ui_egui::spacemouse::bindings::default_buttons;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("ui.spacemouse", Value::Null).expect("opens the dialog");
    let mut h = Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn shows_the_two_default_buttons_and_any_the_device_reported() {
    let mut h = harness();
    h.get_by_label("Button 1");
    h.get_by_label("Button 2");
    assert!(h.query_by_label("Button 3").is_none());
    h.state_mut().spacemouse.highest_button = 15;
    h.state_mut().spacemouse.held = 1 << 14;
    h.run_steps(3);
    h.get_by_label("Button 15");
    // A table with a high bound button shows the rows up to it, and a hostile 32 does not panic.
    h.state_mut().spacemouse.highest_button = 32;
    h.state_mut().spacemouse.held = u32::MAX;
    h.run_steps(3);
    h.get_by_label("Button 2");
    h.get_by_label("Calibrate");
}

#[test]
fn reset_all_restores_the_defaults() {
    let mut h = harness();
    assert!(h.get_by_label("Reset all bindings").accesskit_node().is_disabled(), "nothing to reset at the defaults");
    h.state_mut().run("ui.spacemouse", json!({"settings": {"buttons": {"4": {"command": "undo"}}}})).expect("rebind");
    h.run_steps(3);
    h.get_by_label("Button 4");
    h.get_by_label("Reset all bindings").click();
    h.run_steps(3);
    assert_eq!(h.state().spacemouse.settings.buttons, default_buttons());
    assert!(h.state().prefs_json().find("\"spacemouse\"").is_none(), "defaults are not saved");
}

#[test]
fn picking_none_unbinds_a_button_and_reset_brings_it_back() {
    let mut h = harness();
    h.get_all_by_role(egui::accesskit::Role::ComboBox).next().expect("the first row's picker").click();
    h.run_steps(3);
    h.get_by_label("None").click();
    h.run_steps(3);
    assert!(!h.state().spacemouse.settings.buttons.contains_key(&1));
    assert!(h.state().spacemouse.settings.buttons.contains_key(&2), "the other button is untouched");
    // The row's Reset is the first enabled one: button 1 differs from its default now.
    h.get_all_by_label("Reset").find(|b| !b.accesskit_node().is_disabled()).expect("an enabled reset").click();
    h.run_steps(3);
    assert_eq!(h.state().spacemouse.settings.buttons, default_buttons());
}
