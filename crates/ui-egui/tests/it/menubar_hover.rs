//! The in-window menu bar (#1567): once a menu is open, pointing at another title switches to that
//! menu. With no menu open, pointing at a title opens nothing.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// An item only the View menu shows.
const IN_VIEW: &str = "Regen All";
/// An item only the Edit menu shows.
const IN_EDIT: &str = "Paste to Original Coordinates";
/// An item only View ▸ Zoom shows.
const IN_ZOOM: &str = "Extents";

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.in_window_menu = true;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

fn shown(harness: &Harness<'_, CadApp>, label: &str) -> bool {
    harness.query_by_label(label).is_some()
}

fn hover_title(harness: &mut Harness<'_, CadApp>, name: &str) {
    harness.get_by_role_and_label(Role::Button, name).hover();
    harness.run_steps(3);
}

fn click_title(harness: &mut Harness<'_, CadApp>, name: &str) {
    harness.get_by_role_and_label(Role::Button, name).click();
    harness.run_steps(3);
}

#[test]
fn pointing_at_another_title_switches_the_open_menu() {
    let mut harness = harness();
    assert!(!shown(&harness, IN_VIEW) && !shown(&harness, IN_EDIT), "no menu is open at start");

    click_title(&mut harness, "View");
    assert!(shown(&harness, IN_VIEW), "a click opens View");
    assert!(!shown(&harness, IN_EDIT));

    hover_title(&mut harness, "Edit");
    assert!(shown(&harness, IN_EDIT), "pointing at Edit opens Edit without a click");
    assert!(!shown(&harness, IN_VIEW), "and closes View");

    hover_title(&mut harness, "View");
    assert!(shown(&harness, IN_VIEW), "pointing back at View opens it again");
    assert!(!shown(&harness, IN_EDIT));
}

#[test]
fn pointing_at_a_title_with_no_menu_open_opens_nothing() {
    let mut harness = harness();
    hover_title(&mut harness, "View");
    hover_title(&mut harness, "Edit");
    assert!(!shown(&harness, IN_VIEW) && !shown(&harness, IN_EDIT));
}

#[test]
fn escape_closes_the_open_menu_and_pointing_then_opens_nothing() {
    let mut harness = harness();
    click_title(&mut harness, "View");
    assert!(shown(&harness, IN_VIEW));

    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!shown(&harness, IN_VIEW), "Esc closes the menu");

    hover_title(&mut harness, "Edit");
    assert!(!shown(&harness, IN_EDIT), "once closed, pointing does not open a menu");
}

#[test]
fn clicking_the_open_title_closes_it() {
    let mut harness = harness();
    click_title(&mut harness, "View");
    assert!(shown(&harness, IN_VIEW));

    click_title(&mut harness, "View");
    assert!(!shown(&harness, IN_VIEW), "a second click on View closes it");

    hover_title(&mut harness, "Edit");
    assert!(!shown(&harness, IN_EDIT));
}

#[test]
fn submenus_open_and_close_with_their_menu() {
    let mut harness = harness();
    click_title(&mut harness, "View");
    // egui 0.37 names the submenu button "Zoom"; the arrow is painted, not part of the label.
    harness.get_by_role_and_label(Role::Button, "Zoom").hover();
    harness.run_steps(3);
    assert!(shown(&harness, IN_ZOOM), "pointing at Zoom opens its submenu");

    hover_title(&mut harness, "Edit");
    assert!(shown(&harness, IN_EDIT), "pointing at Edit switches menus with a submenu open");
    assert!(!shown(&harness, IN_ZOOM) && !shown(&harness, IN_VIEW), "View and its submenu close");
}
