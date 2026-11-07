//! Drafting Settings stays on a short screen and scrolls to its last options, and shows them all
//! without scrolling when the screen has room (#2339).

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// The smallest window the app allows (`with_min_inner_size` in `apps/cadcraft`).
const SHORT: egui::Vec2 = egui::vec2(900.0, 560.0);
/// Room for the whole dialog.
const TALL: egui::Vec2 = egui::vec2(900.0, 1000.0);

fn harness(screen: egui::Vec2) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.dialog = Some("dsettings".into());
    let mut h = Harness::builder().with_size(screen).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    h
}

fn window(h: &Harness<'_, CadApp>) -> egui::Rect {
    h.get_by_role_and_label(Role::Window, "Drafting Settings").rect()
}

/// Ortho (F8), the last option.
fn ortho(h: &Harness<'_, CadApp>) -> egui::Rect {
    h.get_by_label("Ortho (F8)").rect()
}

#[test]
fn drafting_settings_scrolls_on_a_short_screen() {
    let mut h = harness(SHORT);
    let w = window(&h);
    assert!(w.bottom() <= SHORT.y, "the dialog runs off the bottom of the screen: {w:?}");
    h.get_by_label("Ortho (F8)").scroll_to_me();
    h.run_steps(3);
    let (w, o) = (window(&h), ortho(&h));
    assert!(w.contains_rect(o), "Ortho (F8) doesn't scroll into the dialog: {o:?} outside {w:?}");
}

/// The dialog shows every option, ending just below the last one.
fn assert_fits_contents(h: &Harness<'_, CadApp>, when: &str) {
    let (w, o) = (window(h), ortho(h));
    assert!(w.contains_rect(o), "{when}: the dialog scrolls although the screen has room: {o:?} outside {w:?}");
    assert!(w.bottom() - o.bottom() < 40.0, "{when}: the dialog is taller than its contents: {o:?} at the end of {w:?}");
}

#[test]
fn drafting_settings_shows_every_option_when_the_screen_has_room() {
    let mut h = harness(TALL);
    assert_fits_contents(&h, "opened on a tall screen");
    // Shrinking the screen and growing it again brings the full height back.
    h.set_size(SHORT);
    h.run_steps(5);
    assert!(window(&h).bottom() <= SHORT.y, "the dialog runs off the bottom of the shrunk screen: {:?}", window(&h));
    h.set_size(TALL);
    h.run_steps(5);
    assert_fits_contents(&h, "after the screen grows again");
}
