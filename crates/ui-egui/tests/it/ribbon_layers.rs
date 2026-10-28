//! Reference workflow: current/selected/mixed layer control with undo and command isolation.
use cadcraft_engine::{Input, Session, doc::Handle, geom::Vec2};
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
    app.run("layer.new", json!({"name":"CC_A","color":1})).unwrap();
    app.run("layer.new", json!({"name":"CC_B","color":3})).unwrap();
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
    h.get_by_label("Ribbon Layer").click();
    h.run_steps(4);
    h.get_by_label(name).click();
    h.run_steps(4);
}
fn line(h: &mut Harness<'_, CadApp>, y: i32) -> Handle {
    let v = h.state_mut().run("line", json!({"points":[[0,y],[100,y]]})).unwrap();
    Handle::parse_hex(v["handles"][0].as_str().unwrap()).unwrap()
}
fn current(h: &Harness<'_, CadApp>) -> String {
    h.state().session.doc().unwrap().header.str("CLAYER", "0")
}
#[test]
fn current_and_selected_layer_changes_are_distinct_and_undoable() {
    let mut h = harness();
    pick(&mut h, "CC_A");
    assert_eq!(current(&h), "CC_A");
    assert_eq!(h.get_by_label("Ribbon Layer").accesskit_node().value().as_deref(), Some("CC_A"));
    let a = line(&mut h, 0);
    pick(&mut h, "CC_B");
    let b = line(&mut h, 40);
    pick(&mut h, "0");
    h.state_mut().session.set_selection(vec![a, b]);
    h.run_steps(4);
    // Mixed layers have no selected row; choosing one edits both objects, not CLAYER.
    h.get_by_label("Ribbon Layer").click();
    h.run_steps(4);
    assert_eq!(h.get_by_label("Ribbon Layer").accesskit_node().value().unwrap_or_default(), "");
    h.get_by_label("CC_A").click();
    h.run_steps(4);
    assert_eq!(current(&h), "0");
    for handle in [a, b] {
        assert_eq!(h.state().session.doc().unwrap().entity(handle).unwrap().common.layer, "CC_A");
    }
    assert_eq!(h.state().session.selection(), vec![a, b]);
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(b).unwrap().common.layer, "CC_B");
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.layer, "CC_A");
    assert_eq!(current(&h), "0");
    h.state_mut().run("redo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().entity(b).unwrap().common.layer, "CC_A");
}
#[test]
fn layer_picker_during_line_does_not_cancel_or_supply_a_point() {
    let mut h = harness();
    h.state_mut().session.start("line").unwrap();
    h.state_mut().session.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    h.run_steps(3);
    pick(&mut h, "CC_B");
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    assert_eq!(current(&h), "CC_B");
    h.state_mut().session.input(Input::Point(Vec2::new(100.0, 0.0))).unwrap();
    let doc = h.state().session.doc().unwrap();
    let entities = doc.space(&cadcraft_engine::doc::Space::Model).unwrap();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities.iter().next().unwrap().common.layer, "CC_B");
}
#[test]
fn layer_state_buttons_preserve_current_layer_and_locked_objects_are_not_reassigned() {
    let mut h = harness();
    pick(&mut h, "CC_A");
    let a = line(&mut h, 0);
    pick(&mut h, "0");
    h.state_mut().run("layer.set", json!({"name":"CC_A","locked":true})).unwrap();
    h.state_mut().session.set_selection(vec![a]);
    h.run_steps(4);
    assert_eq!(h.get_by_label("Ribbon Layer").accesskit_node().value().as_deref(), Some("CC_A"));
    h.get_by_label("Ribbon Layer").click();
    h.run_steps(4);
    assert!(h.get_by_label("CC_B").accesskit_node().is_disabled());
    assert!(h.get_all_by_label("Freeze/Thaw").next().unwrap().accesskit_node().is_disabled());
    let row_y = h.get_all_by_label("CC_A").find(|node| node.accesskit_node().is_disabled()).unwrap().rect().center().y;
    h.get_all_by_label("Lock/Unlock").find(|node| (node.rect().center().y - row_y).abs() < 2.0).unwrap().click();
    h.run_steps(4);
    assert!(!h.state().session.doc().unwrap().layer("CC_A").unwrap().locked);
    h.get_by_label("CC_B").click();
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().entity(a).unwrap().common.layer, "CC_B");
    assert_eq!(current(&h), "0");
}
