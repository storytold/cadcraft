//! A layout tab's right-click menu has Move or Copy...: its dialog lists the layouts to put the
//! layout before (or the end), with Create a Copy; OK reorders the tabs through `layout.move`
//! (gaps U7). Driven with real pointer events on the status bar's tabs.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

fn tabs(app: &CadApp) -> Vec<String> {
    let mut ls: Vec<_> = app.session.doc().unwrap().layouts.iter().map(|l| (l.tab_order, l.name.clone())).collect();
    ls.sort();
    ls.into_iter().map(|l| l.1).collect()
}

fn harness(render: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.start_tab = false;
    app.run("layout.new", json!({ "name": "Sheet" })).unwrap();
    assert_eq!(tabs(&app), ["Layout1", "Layout2", "Sheet"]);
    let builder = Harness::builder().with_size(vec2(1200.0, 760.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// Right-click the tab of `layout` and choose Move or Copy....
fn open_dialog(h: &mut Harness<'static, CadApp>, layout: &str) {
    h.get_by_role_and_label(Role::Tab, layout).click_secondary();
    h.run_steps(3);
    h.get_by_label("Move or Copy...").click();
    h.run_steps(4);
    assert_eq!(h.state().ui.dialog.as_deref(), Some(format!("layoutmove:{layout}").as_str()));
    h.get_by_label("Before layout:");
}

/// The dialog's row for `label` (the status bar has a tab of the same name).
fn row(h: &Harness<'static, CadApp>, label: &str) -> egui::Pos2 {
    let rows: Vec<_> = h.query_all_by_label(label).filter(|n| n.accesskit_node().role() != Role::Tab).collect();
    assert_eq!(rows.len(), 1, "one {label} row in the list");
    rows[0].rect().center()
}

fn click_at(h: &mut Harness<'static, CadApp>, pos: egui::Pos2) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

#[test]
fn move_a_layout_before_another() {
    let mut h = harness(false);
    open_dialog(&mut h, "Sheet");
    let at = row(&h, "Layout1");
    click_at(&mut h, at);
    h.get_by_label("OK").click();
    h.run_steps(4);
    let app = h.state();
    assert_eq!(app.ui.dialog, None, "OK closes the dialog");
    assert_eq!(tabs(app), ["Sheet", "Layout1", "Layout2"]);
    // The status bar shows them in the new order.
    let x = |h: &Harness<'static, CadApp>, n: &str| h.get_by_role_and_label(Role::Tab, n).rect().left();
    h.run_steps(2);
    assert!(x(&h, "Sheet") < x(&h, "Layout1") && x(&h, "Layout1") < x(&h, "Layout2"));
    // One undo puts it back.
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(tabs(h.state()), ["Layout1", "Layout2", "Sheet"]);
}

#[test]
fn copy_to_the_end_and_cancel() {
    let directory = std::env::var_os("CADCRAFT_LAYOUT_SHOTS").map(std::path::PathBuf::from);
    let mut h = harness(directory.is_some());
    open_dialog(&mut h, "Layout1");
    let end = row(&h, "(move to end)");
    click_at(&mut h, end);
    h.get_by_label("Create a copy").click();
    h.run_steps(3);
    if let Some(directory) = &directory {
        std::fs::create_dir_all(directory).unwrap();
        h.render().unwrap().save(directory.join("cadcraft-layout-move-copy.png")).unwrap();
    }
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert_eq!(tabs(h.state()), ["Layout1", "Layout2", "Sheet", "Layout1 (2)"]);

    // Cancel changes nothing, and the next opening starts afresh (no copy ticked).
    open_dialog(&mut h, "Layout2");
    let first = row(&h, "Layout1");
    click_at(&mut h, first);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog, None);
    assert_eq!(tabs(h.state()), ["Layout1", "Layout2", "Sheet", "Layout1 (2)"]);
    open_dialog(&mut h, "Layout2");
    assert_eq!(h.get_by_label("Create a copy").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
}

#[test]
fn the_model_tab_has_no_move_or_copy() {
    let mut h = harness(false);
    h.get_by_role_and_label(Role::Tab, "Model").click_secondary();
    h.run_steps(3);
    h.get_by_label("New Layout");
    assert!(h.query_by_label("Move or Copy...").is_none());
}
