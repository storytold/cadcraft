//! INSERT (typed or from a menu) opens the Block Manager, as AutoCAD's INSERT opens its Blocks
//! palette: pinned beside Properties, or as a window when the user chose that; -INSERT and
//! calls with parameters still run the command.

use cadcraft_engine::Session;
use cadcraft_ui_egui::docking::Panel;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

#[test]
fn insert_opens_the_block_manager_as_a_panel_or_a_window() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.run("insert", serde_json::Value::Null).unwrap();
    assert!(app.ui.docking.layout.contains(&Panel::Blocks), "the Blocks panel is open");
    assert!(app.session.running.is_none(), "no INSERT prompt");
    app.ui.blocks_window = true;
    app.run("ui.dialog.blocks", serde_json::Value::Null).unwrap();
    assert_eq!(app.ui.dialog.as_deref(), Some("blocks"));
}
