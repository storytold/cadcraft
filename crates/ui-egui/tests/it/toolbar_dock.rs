//! The Tool Bar is a row when docked at the top and a column when docked on the left, and its
//! buttons are the user's to add, remove and reorder.

use cadcraft_engine::Session;
use cadcraft_ui_egui::toolbar::{self, Dock};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(dock: Dock) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.ui.toolbar_dock = dock;
    let mut h = Harness::builder().with_size(vec2(1280.0, 860.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

#[test]
fn docked_at_the_top_the_buttons_form_a_row() {
    let h = harness(Dock::Top);
    let preview = h.get_by_label("Plot Preview").rect();
    let zoom = h.get_by_label("Zoom Window").rect();
    assert!((preview.center().y - zoom.center().y).abs() < 1.0, "{preview:?} {zoom:?}");
    assert!(zoom.left() > preview.right());
    assert!(preview.top() < 60.0, "the row sits along the top: {preview:?}");
}

#[test]
fn docked_on_the_left_the_buttons_form_a_column() {
    let h = harness(Dock::Left);
    let preview = h.get_by_label("Plot Preview").rect();
    let zoom = h.get_by_label("Zoom Window").rect();
    assert!((preview.center().x - zoom.center().x).abs() < 1.0, "{preview:?} {zoom:?}");
    assert!(zoom.top() > preview.bottom());
    let canvas_left = h.get_by_label("Drawing1").rect().left();
    assert!(preview.right() < canvas_left, "the column sits left of the drawing tabs: {preview:?}");
    assert!(preview.left() < toolbar::THICKNESS, "the column sits along the left edge: {preview:?}");
}

#[test]
fn the_dock_command_moves_the_bar() {
    let mut h = harness(Dock::Top);
    h.state_mut().run("ui.toolbar.dock", json!({ "position": "left" })).unwrap();
    h.run_steps(5);
    assert_eq!(h.state().ui.toolbar_dock, Dock::Left);
    assert!(h.get_by_label("Plot Preview").rect().left() < toolbar::THICKNESS);
    assert!(h.state_mut().run("ui.toolbar.dock", json!({ "position": "bottom" })).is_err());
}

#[test]
fn buttons_are_added_moved_and_removed() {
    let mut h = harness(Dock::Top);
    let app = h.state_mut();
    app.run("ui.toolbar.add", json!({ "command": "circle", "index": 0 })).unwrap();
    assert_eq!(app.ui.toolbar_items.first().map(String::as_str), Some("circle"));
    assert!(app.run("ui.toolbar.add", json!({ "command": "circle" })).is_err(), "a button appears once");
    assert!(app.run("ui.toolbar.add", json!({ "command": "not-a-command" })).is_err());
    app.run("ui.toolbar.move", json!({ "from": 0, "to": 3 })).unwrap();
    assert_eq!(app.ui.toolbar_items.get(2).map(String::as_str), Some("circle"));
    app.run("ui.toolbar.remove", json!({ "command": "plot" })).unwrap();
    assert!(!app.ui.toolbar_items.iter().any(|i| i == "plot"));
    h.run_steps(5);
    assert!(h.query_by_label("Print").is_none(), "the removed button is gone");
    h.state_mut().run("ui.toolbar.reset", json!({})).unwrap();
    assert_eq!(h.state().ui.toolbar_items, toolbar::default_items());
}

#[test]
fn customize_opens_from_the_window_menu_command() {
    let mut h = harness(Dock::Top);
    h.state_mut().start("ui.dialog.toolbar");
    h.run_steps(5);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("toolbar"));
    assert!(h.query_by_label("Add Separator").is_some());
}

#[test]
fn the_layers_bar_is_a_row_at_the_top_with_the_current_layer() {
    let mut h = harness(Dock::Top);
    let combo = h.get_by_label("Current Layer").rect();
    assert!(combo.width() > 150.0, "a row shows the drop-down: {combo:?}");
    assert!(combo.top() < 120.0, "under the menu bar: {combo:?}");
    h.state_mut().run("ui.toolbar.dock", json!({ "bar": "layers", "position": "left" })).unwrap();
    h.run_steps(5);
    let combo = h.get_by_label("Current Layer").rect();
    assert!(combo.width() < 30.0, "a column shows a button: {combo:?}");
    h.state_mut().run("ui.toggle.toolbar.layers", json!({ "on": false })).unwrap();
    h.run_steps(5);
    assert!(h.query_by_label("Current Layer").is_none());
}

#[test]
fn the_tool_sets_bar_is_a_column_on_the_left_and_can_move_to_the_top() {
    let mut h = harness(Dock::Top);
    let st = toolbar::state(&h.state().ui, toolbar::Bar::ToolSets);
    assert!(st.show && st.dock == Dock::Left && st.items.first().map(String::as_str) == Some("line"));
    let more = h.get_by_label("More Tool Sets bar commands").rect();
    assert!(more.left() < 2.0 * toolbar::THICKNESS, "the column runs along the left: {more:?}");
    let app = h.state_mut();
    app.run("ui.toolbar.dock", json!({ "bar": "toolSets", "position": "top" })).unwrap();
    app.run("ui.toolbar.add", json!({ "bar": "toolSets", "command": "revcloud", "index": 0 })).unwrap();
    assert_eq!(toolbar::state(&app.ui, toolbar::Bar::ToolSets).items.first().map(String::as_str), Some("revcloud"));
    assert!(app.run("ui.toolbar.add", json!({ "bar": "nope", "command": "line" })).is_err());
    h.run_steps(5);
    assert!(h.query_by_label("More Tool Sets bar commands").is_none(), "a row at 1280 px holds every tool");
}
