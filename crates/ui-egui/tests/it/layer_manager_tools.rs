//! Layer Properties Manager: a layer state can be deleted from the list without restoring it
//! first, and Delete Empty Layers keeps the object selection.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A line on layer "A", the Layer Properties Manager open (with the app's fonts: the table draws ▲).
fn manager() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("layer.new", json!({ "name": "A", "current": true })).unwrap();
    app.run("line", json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    app.ui.dialog = Some("layers".into());
    // The manager's state list and transparency spinners have no accessible names yet; this
    // test is about what the buttons do, not about naming them.
    let mut h = Harness::builder()
        .with_size(vec2(1400.0, 800.0))
        .with_accessibility_check(false)
        .with_context_setup(|ctx| theme::install_fonts(ctx, "en"))
        .build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(&ui.ctx().clone());
                dialogs::show(app, ui.ctx());
            },
            app,
        );
    h.run_steps(3);
    h
}

fn layer_a_on(app: &CadApp) -> bool {
    app.session.doc().unwrap().layer("A").unwrap().on
}

#[test]
fn a_state_is_deleted_from_the_list_without_being_restored() {
    let mut h = manager();
    let app = h.state_mut();
    app.run("layerstate.save", json!({ "name": "S" })).unwrap();
    let line = app.session.doc().unwrap().model.handles()[0];
    app.run("layoff", json!({ "handles": [line.hex()] })).unwrap();
    assert!(!layer_a_on(app));
    h.run_steps(3);
    h.get_all_by_role(Role::ComboBox).find(|n| n.value().as_deref() == Some("Unsaved Layer State")).expect("the layer state list").click();
    h.run_steps(3);
    h.get_by_label("Delete Layer State").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert!(app.session.doc().unwrap().layer_states.is_empty(), "the state is deleted");
    assert!(!layer_a_on(app), "deleting the state does not restore it: A stays off");
}

#[test]
fn delete_empty_layers_keeps_the_selection() {
    let mut h = manager();
    let app = h.state_mut();
    app.run("layer.new", json!({ "name": "Empty" })).unwrap();
    let line = app.session.doc().unwrap().model.handles()[0];
    app.session.set_selection(vec![line]);
    h.run_steps(3);
    h.get_by_label("Delete Empty Layers").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert!(app.session.doc().unwrap().layer("Empty").is_none(), "the empty layer is purged");
    assert_eq!(app.session.selection(), vec![line], "the selected line stays selected");
}
