//! View > Viewports > Named Viewports... opens a dialog that lists the saved viewport
//! configurations: Restore shows one in the Model tab or starts placing it in a layout, and Delete
//! deletes it.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A drawing with "Two" (two tiles side by side) saved, showing one view, and the dialog open.
fn dialog() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    app.run("-vports", json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    app.run("-vports", json!({ "option": "save", "name": "Two" })).unwrap();
    app.run("-vports", json!({ "option": "single" })).unwrap();
    menus::activate(&mut app, "vports.named");
    assert_eq!(app.ui.dialog.as_deref(), Some("namedvports"));
    let mut h = Harness::builder().with_size(vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn restore_and_delete_in_the_model_tab() {
    let mut h = dialog();
    h.get_by_label("Current name: *Active");
    h.get_by_label("Two").click();
    h.run_steps(2);
    h.get_by_label("Restore").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert_eq!(app.session.model_tiles().len(), 2);
    assert_eq!(app.ui.dialog, None, "Restore closes the dialog");

    menus::activate(app, "vports.named");
    h.run_steps(3);
    h.get_by_label("Current name: Two");
    h.get_by_label("Delete").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert!(app.session.doc().unwrap().vports.iter().all(|c| c.name != "Two"));
    assert_eq!(app.ui.dialog.as_deref(), Some("namedvports"), "Delete keeps the dialog open");
    h.get_by_label("This drawing has no named viewport configurations.");
}

#[test]
fn restore_in_a_layout_asks_where_the_viewports_go() {
    let mut h = dialog();
    let app = h.state_mut();
    app.ui.dialog = None;
    app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
    menus::activate(app, "vports.named");
    h.run_steps(3);
    h.get_by_label("Two").click();
    h.run_steps(2);
    h.get_by_label("Restore").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert!(app.session.prompt_text().contains("Specify first corner"), "{}", app.session.prompt_text());
    let before = app.session.doc().unwrap().layout("Layout1").unwrap().entities.iter().count();
    app.cmdline("");
    assert!(app.session.running.is_none());
    let after = app.session.doc().unwrap().layout("Layout1").unwrap().entities.iter().count();
    assert_eq!(after, before + 2, "Enter fits the two viewports to the printable area");
}
