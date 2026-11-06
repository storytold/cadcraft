//! The Ribbon is opt-in: nothing shows until `showRibbon` is set; then the Home tab and its
//! panels' buttons start the same commands as the Tool Sets (#306, #1492).

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

fn harness(ribbon: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.in_window_menu = false;
    app.ui.show_toolsets = false;
    app.ui.show_palettes = false;
    app.ui.show_ribbon = ribbon;
    app.integrated_titlebar = true;
    let mut harness = Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

#[test]
fn the_ribbon_shows_only_when_asked() {
    let off = harness(false);
    assert!(off.query_by_role_and_label(Role::Tab, "Home").is_none(), "the ribbon is off by default");
    let on = harness(true);
    on.get_by_role_and_label(Role::Tab, "Home");
    for label in ["Line", "Move", "Linear", "Layer Properties", "Insert Block", "Match Properties"] {
        let button = on.get_by_role_and_label(Role::Button, label);
        assert!(!button.accesskit_node().is_disabled(), "{label} is a known command, so its ribbon button is enabled");
    }
}

#[test]
fn clicking_a_ribbon_button_starts_its_command() {
    let mut harness = harness(true);
    assert!(harness.state().session.running.is_none());
    harness.get_by_role_and_label(Role::Button, "Polyline").click();
    harness.run_steps(5);
    assert_eq!(harness.state().session.running.as_ref().map(|r| r.id.as_str()), Some("pline"));
}

#[test]
fn the_options_checkbox_turns_the_ribbon_on_and_it_survives_a_restart() {
    let mut harness = harness(false);
    harness.state_mut().ui.dialog = Some("options".into());
    harness.run_steps(5);
    harness.get_by_role_and_label(Role::CheckBox, "Ribbon").click();
    harness.run_steps(5);
    assert!(harness.state().ui.show_ribbon, "the dialog's change reaches the interface state");
    harness.get_by_role_and_label(Role::Tab, "Home");

    let mut restarted = CadApp::new(Session::default(), Services::default());
    restarted.load_profile(&harness.state().profile_json());
    assert!(restarted.ui.show_ribbon);
}

#[test]
fn the_options_json_form_validates_reports_and_resets_the_ribbon() {
    let mut app = CadApp::new(Session::default(), Services::default());
    assert!(app.run("options", json!({"showRibbon": "yes"})).is_err());
    assert!(!app.ui.show_ribbon);
    assert_eq!(app.run("options", json!({"showRibbon": true})).unwrap()["showRibbon"], json!(true));
    assert!(app.ui.show_ribbon);
    assert_eq!(app.run("options", json!({"reset": true})).unwrap()["showRibbon"], json!(false));
}
