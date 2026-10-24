//! Drafting Settings exposes POLARMODE's Shift-to-Acquire bit (#1600).

use cadcraft_engine::Session;
use cadcraft_engine::snap::tracking::polarmode;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness(flags: u32) -> Harness<'static, CadApp> {
    let mut session = Session::new();
    session.settings.polar_flags = flags;
    let mut app = CadApp::new(session, Services::default());
    app.ui.dialog = Some("dsettings".into());
    let mut h = Harness::builder().with_size(egui::vec2(800.0, 600.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    h
}

#[test]
fn shift_to_acquire_radio_controls_only_bit_eight() {
    let preserved = polarmode::RELATIVE | polarmode::TRACK_POLAR | polarmode::ADDITIONAL;
    let mut h = harness(preserved);

    h.get_by_label("Shift to Acquire").click();
    h.run_steps(2);
    assert_eq!(h.state().session.settings.polar_flags, preserved | polarmode::SHIFT_ACQUIRE);

    h.get_by_label("Automatic").click();
    h.run_steps(2);
    assert_eq!(h.state().session.settings.polar_flags, preserved);
}
