//! Ribbon linetype edits preserve drawing defaults and active commands.
use cadcraft_engine::{Session, doc::Handle};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    harness_at_width(1800.0)
}

fn harness_at_width(width: f32) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.run("ui.layout.pc", json!({})).unwrap();
    app.ui.start_tab = false;
    let mut h = Harness::builder().with_size(vec2(width, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}
fn pick(h: &mut Harness<'_, CadApp>, name: &str) {
    h.get_by_label("Ribbon Linetype").click();
    h.run_steps(4);
    h.get_by_label("Linetype Manager").parent().unwrap().get_by_role_and_label(egui::accesskit::Role::Button, name).click();
    h.run_steps(4);
}
fn line(h: &mut Harness<'_, CadApp>, y: i32) -> Handle {
    let result = h.state_mut().run("line", json!({"points":[[0,y],[100,y]]})).unwrap();
    Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap()
}

#[test]
fn collapsed_properties_preserves_selected_linetype_picker() {
    let mut h = harness_at_width(900.0);
    let selected = line(&mut h, 0);
    let sibling = line(&mut h, 40);
    h.state_mut().run("linetype", json!({"current":"Continuous"})).unwrap();
    h.state_mut().session.set_selection(vec![selected]);
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Properties").click();
    h.run_steps(4);
    pick(&mut h, "ByBlock");
    let doc = h.state().session.doc().unwrap();
    assert_eq!(doc.entity(selected).unwrap().common.linetype, "ByBlock");
    assert_eq!(doc.entity(sibling).unwrap().common.linetype, "ByLayer");
    assert_eq!(doc.header.str("CELTYPE", ""), "Continuous");
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(selected).unwrap().common.linetype, "ByLayer");
    h.state_mut().run("redo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(selected).unwrap().common.linetype, "ByBlock");
}

#[test]
fn selected_and_mixed_linetypes_preserve_creation_defaults_and_undo() {
    let mut h = harness();
    pick(&mut h, "ByBlock");
    let a = line(&mut h, 0);
    pick(&mut h, "ByLayer");
    let b = line(&mut h, 40);
    h.state_mut().run("linetype", json!({"current":"Continuous"})).unwrap();
    h.state_mut().session.set_selection(vec![a, b]);
    h.run_steps(4);
    assert_eq!(h.get_by_label("Ribbon Linetype").accesskit_node().value().unwrap_or_default(), "");
    pick(&mut h, "ByLayer");
    let doc = h.state().session.doc().unwrap();
    assert_eq!(doc.header.str("CELTYPE", ""), "Continuous");
    assert_eq!(doc.entity(a).unwrap().common.linetype, "ByLayer");
    assert_eq!(doc.entity(b).unwrap().common.linetype, "ByLayer");
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.linetype, "ByBlock");
    h.state_mut().run("redo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.linetype, "ByLayer");
}

#[test]
fn loaded_pattern_can_be_assigned_without_changing_default_or_sibling() {
    let mut h = harness();
    let selected = line(&mut h, 0);
    let sibling = line(&mut h, 40);
    h.state_mut().run("linetype", json!({"load":"DASHED"})).unwrap();
    h.state_mut().session.set_selection(vec![selected]);
    h.run_steps(4);
    pick(&mut h, "DASHED");
    assert_eq!(h.get_by_label("Ribbon Linetype").accesskit_node().value().as_deref(), Some("DASHED"));
    let doc = h.state().session.doc().unwrap();
    assert_eq!(doc.entity(selected).unwrap().common.linetype, "DASHED");
    assert_eq!(doc.entity(sibling).unwrap().common.linetype, "ByLayer");
    assert_eq!(doc.header.str("CELTYPE", ""), "ByLayer");
}

#[test]
fn locked_selection_disables_assignments_but_keeps_existing_manager_available() {
    let mut h = harness();
    let a = line(&mut h, 0);
    h.state_mut().run("layer.set", json!({"name":"0","locked":true})).unwrap();
    h.state_mut().session.set_selection(vec![a]);
    h.run_steps(4);
    h.get_by_label("Ribbon Linetype").click();
    h.run_steps(4);
    assert!(h.get_by_role_and_label(egui::accesskit::Role::Button, "ByBlock").accesskit_node().is_disabled());
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Linetype Manager").click();
    h.run_steps(4);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("linetype"));
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.linetype, "ByLayer");
}

#[test]
fn picking_during_line_does_not_supply_a_canvas_point() {
    let mut h = harness();
    h.state_mut().session.start("line").unwrap();
    h.state_mut().session.input(cadcraft_engine::Input::Point(cadcraft_engine::geom::Vec2::new(0.0, 0.0))).unwrap();
    h.run_steps(4);
    pick(&mut h, "ByBlock");
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    h.state_mut().session.input(cadcraft_engine::Input::Point(cadcraft_engine::geom::Vec2::new(100.0, 0.0))).unwrap();
    let doc = h.state().session.doc().unwrap();
    let entities = doc.space(&cadcraft_engine::doc::Space::Model).unwrap();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities.iter().next().unwrap().common.linetype, "ByBlock");
}
