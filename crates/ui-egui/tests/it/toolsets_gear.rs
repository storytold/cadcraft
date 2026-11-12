//! The gear on a Tool Sets category header opens that category's options menu and no longer
//! collapses the category it sits on.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// A tool name that appears exactly once in the Drafting palette, so the accessibility tree can
/// be queried for it without running into a toolbar button of the same name.
const HIDDEN: &str = "Revision Cloud";

/// The same tool as a command id, which is what the UI state stores.
const HIDDEN_CMD: &str = "revcloud";

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

#[test]
fn the_gear_opens_the_options_and_the_header_keeps_collapsing() {
    let mut harness = harness();
    let header = harness.get_by_label("Draw");
    let gear = harness.get_by_label("Draw options");
    assert!(gear.rect().width() < header.rect().width() / 2.0, "the gear is a target of its own, not part of the header row's");

    harness.get_by_label("Draw").click();
    harness.run_steps(5);
    assert!(harness.state().ui.collapsed_groups.iter().any(|g| g == "Draw"), "clicking the header collapses the category");
    harness.get_by_label("Draw").click();
    harness.run_steps(5);
    assert!(harness.state().ui.collapsed_groups.iter().all(|g| g != "Draw"), "clicking it again expands the category");

    harness.get_by_label("Draw options").click();
    harness.run_steps(5);
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::CheckBox, HIDDEN).is_some(), "the gear opens the category's tool list");
    assert!(harness.state().ui.collapsed_groups.is_empty(), "the gear itself must not collapse the category");
}

#[test]
fn the_options_menu_hides_a_tool_and_remembers_the_choice() {
    let mut harness = harness();
    assert!(harness.query_by_label(HIDDEN).is_some(), "the palette starts with every tool of the category");

    harness.get_by_label("Draw options").click();
    harness.run_steps(5);
    harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, HIDDEN).click();
    harness.run_steps(5);

    assert_eq!(harness.state().ui.hidden_tools, vec!["revcloud".to_string()], "the choice is kept in the UI state");
}

#[test]
fn a_hidden_tool_leaves_the_palette_and_show_all_puts_it_back() {
    let mut harness = harness();
    harness.state_mut().ui.hidden_tools.push(HIDDEN_CMD.to_string());
    harness.run_steps(5);
    assert!(harness.query_by_label(HIDDEN).is_none(), "a hidden tool leaves the palette");
    assert!(harness.query_by_label("Construction Line").is_some(), "the rest of the category stays");

    harness.get_by_label("Draw options").click();
    harness.run_steps(5);
    harness.get_by_label("Show All").click();
    harness.run_steps(5);

    assert!(harness.state().ui.hidden_tools.is_empty(), "Show all empties the list");
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Button, HIDDEN).is_some(), "the tool is back in the palette");
}
