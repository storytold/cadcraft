//! The Options button: a gear in the top right corner that no interface toggle hides, so the
//! preferences are always one click away.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// The app with one drawing in a window `width` points wide, a few frames run.
fn harness(width: f32) -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_metric(), "Options button", None);
    let app = CadApp::new(session, Services::default());
    let mut h = Harness::builder().with_size(vec2(width, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// Click the button named Options, by its accessibility label, and check the dialog opened.
fn click_options(h: &mut Harness<'static, CadApp>) {
    h.state_mut().ui.dialog = None;
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Options").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options"));
}

/// With the default interface, the gear opens Options.
#[test]
fn the_options_button_opens_the_options_dialog() {
    let mut h = harness(1600.0);
    click_options(&mut h);
}

/// With every bar, palette and panel hidden, and on the start page, the gear is still there.
#[test]
fn no_interface_toggle_hides_the_options_button() {
    let mut h = harness(1600.0);
    {
        let ui = &mut h.state_mut().ui;
        ui.show_toolbar = false;
        ui.show_status_bar = false;
        ui.in_window_menu = false;
        ui.show_file_tabs = false;
        ui.show_toolsets = false;
        ui.show_palettes = false;
        ui.show_command_line = false;
        ui.show_viewcube = false;
    }
    h.run_steps(3);
    click_options(&mut h);
    // The start page too.
    h.state_mut().ui.start_tab = true;
    h.run_steps(3);
    click_options(&mut h);
}

/// A window too narrow for the tool bar still shows the gear.
#[test]
fn a_narrow_window_still_shows_it() {
    let mut h = harness(420.0);
    click_options(&mut h);
}
