//! Ribbon color edits distinguish selected objects from creation defaults.
use cadcraft_engine::{Session, doc::Handle};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.run("ui.layout.pc", json!({})).unwrap();
    app.ui.start_tab = false;
    let mut h = Harness::builder().with_size(vec2(1800.0, 1000.0)).build_ui_state(
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
    h.get_by_label("Ribbon Color").click();
    h.run_steps(4);
    h.get_by_label("ByBlock").parent().unwrap().get_by_role_and_label(egui::accesskit::Role::Button, name).click();
    h.run_steps(4);
}
fn line(h: &mut Harness<'_, CadApp>, y: i32) -> Handle {
    let result = h.state_mut().run("line", json!({"points":[[0,y],[100,y]]})).unwrap();
    Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap()
}
#[test]
fn selected_and_mixed_colors_change_without_changing_creation_defaults() {
    let mut h = harness();
    pick(&mut h, "ByBlock");
    assert_eq!(h.state().session.doc().unwrap().header.i64("CECOLOR", -1), 0);
    let a = line(&mut h, 0);
    pick(&mut h, "ByLayer");
    let b = line(&mut h, 40);
    h.state_mut().run("color", json!({"color":"blue"})).unwrap();
    h.state_mut().session.set_selection(vec![a, b]);
    h.run_steps(4);
    assert_eq!(h.get_by_label("Ribbon Color").accesskit_node().value().unwrap_or_default(), "");
    pick(&mut h, "ByLayer");
    assert_eq!(h.get_by_label("Ribbon Color").accesskit_node().value().as_deref(), Some("ByLayer"));
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.header.i64("CECOLOR", -1), 5);
    assert_eq!(d.entity(a).unwrap().common.color, cadcraft_color::Color::ByLayer);
    assert_eq!(d.entity(b).unwrap().common.color, cadcraft_color::Color::ByLayer);
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.color, cadcraft_color::Color::ByBlock);
    h.state_mut().run("redo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.color, cadcraft_color::Color::ByLayer);
}
#[test]
fn locked_selection_cannot_apply_color() {
    let mut h = harness();
    let a = line(&mut h, 0);
    h.state_mut().run("layer.set", json!({"name":"0","locked":true})).unwrap();
    h.state_mut().session.set_selection(vec![a]);
    h.run_steps(4);
    h.get_by_label("Ribbon Color").click();
    h.run_steps(4);
    assert!(h.get_by_label("ByBlock").accesskit_node().is_disabled());
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.color, cadcraft_color::Color::ByLayer);
}

#[test]
fn picking_a_color_preserves_an_active_line_and_supplies_no_canvas_point() {
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
    assert_eq!(entities.iter().next().unwrap().common.color, cadcraft_color::Color::ByBlock);
}

#[test]
fn true_color_palette_edits_selected_rgb_without_changing_creation_default() {
    let mut h = harness();
    let selected = line(&mut h, 0);
    h.state_mut().run("properties.set", json!({"handles":[selected.hex()],"color":"12,34,56"})).unwrap();
    h.state_mut().run("color", json!({"color":"blue"})).unwrap();
    h.state_mut().session.set_selection(vec![selected]);
    h.run_steps(4);
    h.get_by_label("Ribbon Color").click();
    h.run_steps(4);
    // Change the stored red component through the actual palette control before applying.
    let (target_node, target_tree) = h
        .get_by_label("Apply true color")
        .parent()
        .unwrap()
        .get_all_by_role(egui::accesskit::Role::SpinButton)
        .find(|node| node.accesskit_node().numeric_value() == Some(12.0))
        .unwrap()
        .accesskit_node()
        .locate();
    h.event(egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::SetValue,
        target_node,
        target_tree,
        data: Some(egui::accesskit::ActionData::NumericValue(98.0)),
    }));
    h.run_steps(4);
    h.get_by_label("Apply true color").click();
    h.run_steps(4);
    let doc = h.state().session.doc().unwrap();
    assert_eq!(doc.entity(selected).unwrap().common.color, cadcraft_color::Color::True(cadcraft_color::Rgb(98, 34, 56)));
    assert_eq!(doc.header.i64("CECOLOR", -1), 5);
}
