//! Drafting Settings exposes POLARMODE bit 8, Shift to Acquire (#1600).

use cadcraft_engine::Session;
use cadcraft_engine::snap::tracking::polarmode;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// Builds a headless harness showing Drafting Settings with POLARMODE preset to `flags`.
///
/// The dialog is opened through `CadApp::ui.dialog` and the frame is stepped a few times so
/// widget ids and labels are registered before the test queries them by text.
fn harness(flags: u32) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("setvar", json!({"name": "POLARMODE", "value": flags})).expect("set POLARMODE");
    app.ui.dialog = Some("dsettings".into());
    let mut h = Harness::builder().with_size(egui::vec2(800.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    h
}

/// Guards that the "Shift to Acquire" radio button toggles POLARMODE bit 8 alone (#1600).
///
/// Regression: the two new controls must not reset the bits already set (relative, track
/// polar, additional angles), and the change must reach the POLARMODE sysvar; selecting
/// "Automatic" must clear bit 8 and leave the remaining bits untouched.
#[test]
fn shift_to_acquire_sets_and_clears_only_bit_8() {
    // Bits 1 (relative), 2 (track polar) and 4 (additional angles) are on: they must survive.
    let mut h = harness(polarmode::RELATIVE | polarmode::TRACK_POLAR | polarmode::ADDITIONAL);
    let others = polarmode::RELATIVE | polarmode::TRACK_POLAR | polarmode::ADDITIONAL;
    assert_eq!(h.state().session.settings.polar_flags, others);

    h.get_by_label("Shift to Acquire").click();
    h.run_steps(2);
    assert_eq!(h.state().session.settings.polar_flags, others | polarmode::SHIFT_ACQUIRE);
    assert_eq!(h.state_mut().run("getvar", json!({"name": "POLARMODE"})).expect("getvar")["value"], json!(15));

    h.get_by_label("Automatic").click();
    h.run_steps(2);
    assert_eq!(h.state().session.settings.polar_flags, others);
    assert_eq!(h.state_mut().run("getvar", json!({"name": "POLARMODE"})).expect("getvar")["value"], json!(7));
}
