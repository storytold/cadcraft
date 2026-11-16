//! STYLESMANAGER opens the Plot Style Manager as a dockable palette: it floats wide enough for
//! the table's columns the first time, reopens where it was closed, and docks like the palettes.

use cadcraft_engine::Session;
use cadcraft_ui_egui::docking::Panel;
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

fn app() -> CadApp {
    CadApp::new(Session::default(), Services::default())
}

fn run(h: &mut Harness<'_, CadApp>, id: &str, params: Value) -> Value {
    h.state_mut().run(id, params).unwrap()
}

#[test]
fn stylesmanager_opens_one_floating_palette_with_the_grid_tab() {
    let mut app = app();
    app.run("stylesmanager", Value::Null).unwrap();
    assert!(app.ui.dialog.is_none(), "the palette replaces the former dialog");
    assert!(app.ui.docking.layout.contains(&Panel::PlotStyles));
    let rect = app.ui.docking.layout.floating.iter().find(|group| group.contains(&Panel::PlotStyles)).expect("it floats the first time").rect;
    assert!(rect[2] >= 600.0, "wide enough for the table's columns: {rect:?}");
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_accessibility_check(false).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    assert_eq!(h.state().ui.docking.layout.floating.len(), 1, "one window, no second copy");
    let tab = h.get_by_role_and_label(egui::accesskit::Role::Tab, "Plot Style Manager");
    tab.click();
    h.run_steps(2);
    assert!(h.state().ui.docking.layout.contains(&Panel::PlotStyles));
    // The grid draws in the floating window.
    assert!(h.query_all_by_label("Plot style table").count() >= 1);
}

#[test]
fn the_palette_reopens_where_it_was_closed_and_docks_beside_properties() {
    let mut app = app();
    app.run("stylesmanager", Value::Null).unwrap();
    let rect = [160.0, 100.0, 760.0, 420.0];
    app.run("ui.dock", json!({"operation":"float", "panel":"plotStyles", "rect":rect})).unwrap();
    app.run("ui.dock", json!({"operation":"close", "panel":"plotStyles"})).unwrap();
    assert!(!app.ui.docking.layout.contains(&Panel::PlotStyles));
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_accessibility_check(false).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    run(&mut h, "stylesmanager", Value::Null);
    let reopened = h.state().ui.docking.layout.floating.iter().find(|group| group.contains(&Panel::PlotStyles)).expect("it reopens floating").rect;
    assert_eq!(reopened, rect);
    run(&mut h, "ui.dock", json!({"operation":"move", "panel":"plotStyles", "target":"properties", "zone":"center"}));
    let layout = &h.state().ui.docking.layout;
    assert!(layout.floating.is_empty(), "docking takes it out of the floating window");
    assert_eq!(layout.location(&Panel::PlotStyles).unwrap().anchor, Some(Panel::Properties));
    h.run_steps(4);
    assert!(h.get_by_role_and_label(egui::accesskit::Role::Tab, "Plot Style Manager").rect().width() > 0.0);
}

#[test]
fn the_file_menu_entry_opens_the_palette() {
    let mut app = app();
    // The File ▸ Plot Styles... item runs the engine's STYLESMANAGER command.
    menus::activate(&mut app, "stylesmanager");
    assert!(app.ui.docking.layout.contains(&Panel::PlotStyles));
}
