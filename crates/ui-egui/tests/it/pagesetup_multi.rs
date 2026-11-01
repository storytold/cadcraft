//! The Page Setup Manager with several layouts picked: choosing the value the layout shown
//! already has still applies it to the others, which is how mixed layouts are made to match.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// An app in a window, as the other integration tests build it.
fn harness(app: CadApp) -> Harness<'static, CadApp> {
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

/// Whether layout `layout` is landscape.
fn landscape(app: &CadApp, layout: &str) -> bool {
    app.session.doc().unwrap().layout(layout).unwrap().page.landscape
}

/// Choosing the value the shown layout has makes the other picked layouts match.
#[test]
fn picking_the_value_shown_makes_the_other_picked_layouts_match() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    // Layout1 is shown and landscape; Layout2 is portrait.
    app.run("pagesetup", json!({ "layout": "Layout1", "landscape": true })).unwrap();
    app.run("pagesetup", json!({ "layout": "Layout2", "landscape": false })).unwrap();
    app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
    let mut h = harness(app);
    menus::activate(h.state_mut(), "pagesetup");
    h.run_steps(5);

    // With one layout picked, choosing what it already has changes nothing.
    h.get_by_label("Landscape").click();
    h.run_steps(3);
    assert!(!landscape(h.state(), "Layout2"), "one layout picked: the other is left alone");

    // With both picked, the same click is a change for both.
    h.get_by_label("Select all").click();
    h.run_steps(3);
    h.get_by_label("Landscape").click();
    h.run_steps(3);
    assert!(landscape(h.state(), "Layout1") && landscape(h.state(), "Layout2"), "both layouts are landscape now");
}
