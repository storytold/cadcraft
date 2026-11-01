//! Drafting Settings shows the polar increment angle as picked, not its radians round trip (#1237).

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

fn shown_increment(degrees: f64) -> (String, f64, f64) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("setvar", json!({"name": "POLARANG", "value": degrees})).unwrap();
    let stored = app.session.settings.polarang;
    app.ui.dialog = Some("dsettings".into());
    let mut h = Harness::builder().with_size(egui::vec2(800.0, 600.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    let combo = h.get_by_role(Role::ComboBox).accesskit_node().value().unwrap_or_default().to_string();
    (combo, stored, h.state().session.settings.polarang)
}

#[test]
fn polar_increment_angles_show_as_picked() {
    for (degrees, text) in [(15.0, "15"), (30.0, "30"), (22.5, "22.5"), (45.0, "45")] {
        let (shown, stored, after) = shown_increment(degrees);
        assert_eq!(shown, text, "POLARANG {degrees}");
        assert_eq!(after, stored, "opening the dialog must not change POLARANG {degrees}");
    }
}

#[test]
fn a_finer_polar_increment_is_shown_rounded_and_kept() {
    let (shown, stored, after) = shown_increment(12.3456789);
    assert_eq!(shown, "12.345679");
    assert_eq!(after, stored, "opening the dialog must not round POLARANG");
}
