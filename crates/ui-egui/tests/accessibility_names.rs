//! Semantic accessibility: controls expose an accessible name, not only a placeholder, and the
//! AccessKit root carries the language the interface is shown in.
//!
//! `hint_text` becomes an AccessKit placeholder, so a screen reader sees an unnamed text input
//! unless the widget is associated with a visible label (`labelled_by`) or given a name.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, i18n, menus};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

/// A 1400×900 kittest harness whose frame runs `logic` and then `ui` on the supplied app.
fn harness(app: CadApp) -> Harness<'static, CadApp> {
    Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

/// Every editable text input must advertise a non-empty accessible name: its own label or the
/// visible label it is associated with. A placeholder alone is not a name.
fn unnamed_text_inputs(h: &Harness<'_, CadApp>) -> Vec<String> {
    let text_inputs = h.query_all_by_role(Role::TextInput).chain(h.query_all_by_role(Role::MultilineTextInput));
    text_inputs
        .filter(|node| {
            let node = node.accesskit_node();
            node.label().as_deref().is_none_or(str::is_empty) && node.labelled_by().next().is_none()
        })
        .map(|node| {
            let value = node.accesskit_node().value().unwrap_or_default().to_string();
            let placeholder = node.accesskit_node().placeholder().unwrap_or_default().to_string();
            format!("value={value:?} placeholder={placeholder:?}")
        })
        .collect()
}

/// The Options ▸ Files autosave path field is labelled by its visible row label, and no text input in the dialog is unnamed.
#[test]
fn options_autosave_path_has_a_semantic_label() {
    let mut app = CadApp::new(Session::new(), Services::default());
    menus::activate(&mut app, "options");
    // The path field lives on Open and Save, not on the default Display tab.
    app.ui.dialog = Some("options:files".into());
    let h = harness(app);
    let name = "Automatic save file location";
    let field = h.get_by_role_and_label(Role::TextInput, name);
    let labelled_by: Vec<_> = field.accesskit_node().labelled_by().map(|node| node.id()).collect();
    assert_eq!(labelled_by.len(), 1, "the field is labelled by the visible row label");
    let label = h.get_all_by_role(Role::Label).find(|node| node.accesskit_node().id() == labelled_by[0]).expect("the referenced label node");
    assert_eq!(label.accesskit_node().value().as_deref(), Some(name));
    let unnamed = unnamed_text_inputs(&h);
    assert!(unnamed.is_empty(), "unnamed text inputs in OPTIONS: {unnamed:?}");
}

/// Every interactive control on every Options tab is named by the visible label beside it. The
/// harness's own strict accessibility check already runs while it settles; this test makes the
/// tab-by-tab coverage explicit and reports the offending tab in its own assertion.
#[test]
fn every_options_tab_names_its_controls() {
    for tab in ["display", "files", "drafting", "selection", "preferences", "profiles"] {
        let mut app = CadApp::new(Session::new(), Services::default());
        menus::activate(&mut app, "options");
        app.ui.dialog = Some(format!("options:{tab}"));
        let h = harness(app);
        let unnamed = h.unnamed_widgets();
        assert!(unnamed.is_empty(), "unnamed controls on Options {tab}: {unnamed:?}");
    }
}

/// The Parameters Manager's expression fields are the concrete unnamed fields the review found;
/// each one is named by the parameter's name label in the same row.
#[test]
fn parameters_manager_fields_have_a_semantic_label() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("parameters", json!({ "name": "user1", "expr": "4" })).unwrap();
    app.run("parameters", json!({ "name": "user2", "expr": "user1*2" })).unwrap();
    app.ui.dialog = Some("parameters".into());
    let h = harness(app);
    let unnamed = unnamed_text_inputs(&h);
    assert!(unnamed.is_empty(), "unnamed text inputs in the Parameters Manager: {unnamed:?}");
    let labels: Vec<String> = h.query_all_by_role(Role::TextInput).filter_map(|node| node.accesskit_node().label()).collect();
    assert!(labels.iter().any(|label| label == "user1"), "the first expression field is named by its parameter: {labels:?}");
    assert!(labels.iter().any(|label| label == "user2"), "the second expression field is named by its parameter: {labels:?}");
}

/// The AccessKit root names the language the interface is actually shown in, so a screen reader
/// switches voice instead of reading a translated interface with the OS default.
#[test]
fn accesskit_root_carries_the_interface_language() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.interface_language = i18n::Preference::German;
    let h = harness(app);
    assert_eq!(h.state().language(), "de");
    assert_eq!(h.root().accesskit_node().data().language(), Some("de"), "the root node carries the shown language");
}
