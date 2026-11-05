//! Format > Dimension Style... (DIMSTYLE, D) opens the Dimension Style Manager: a list of the
//! drawing's dimension styles with New, Rename, Delete and Set Current beside four tabs of the
//! selected style's variables. Edits run `dimstyle`, so the dimensions of that style change.

use cadcraft_doc::{Dimension, EntityKind};
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(1000.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

/// A drawing with one linear dimension in the Standard style, and the dialog opened from the menu.
fn opened() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    menus::activate(&mut app, "dimstyle");
    assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"));
    harness(app)
}

fn the_dimension(app: &CadApp) -> Dimension {
    let d = app.session.doc().unwrap();
    d.model.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.clone()) } else { None }).unwrap()
}

/// Type `value` into the text field labelled `label` and press Enter.
fn enter(h: &mut Harness<'static, CadApp>, label: &str, value: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, label).type_text(value);
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn menus_and_typed_names_open_it_but_json_calls_do_not() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for cmd in ["dimstyle", "dimstyle.dimension"] {
        app.ui.dialog = None;
        menus::activate(&mut app, cmd);
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{cmd}");
    }
    for typed in ["DIMSTYLE", "D", "ddim", "dst"] {
        app.ui.dialog = None;
        assert!(app.cmdline(typed).is_none(), "bare {typed} is not an error");
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{typed}");
    }
    app.ui.dialog = None;
    app.run("dimstyle", json!({ "name": "Scripted", "current": false, "DIMTXT": 0.25 })).unwrap();
    app.cmdline(r#"dimstyle {"name": "Typed", "current": false}"#);
    assert!(app.ui.dialog.is_none());
    let d = app.session.doc().unwrap();
    assert!(d.dim_style("Scripted").is_some_and(|s| s.text_height == 0.25));
    assert!(d.dim_style("Typed").is_some());
}

#[test]
fn every_tab_renders_with_named_fields() {
    let mut h = opened();
    h.get_by_label("Current dimension style: Standard");
    for (tab, field) in [
        ("Lines & Arrows", "Offset from origin"),
        ("Text", "Text height"),
        ("Primary Units", "Measurement scale factor"),
        ("Tolerances", "Upper value"),
    ] {
        h.get_by_label(tab).click();
        h.run_steps(3);
        h.get_by_role_and_label(Role::TextInput, field);
        let unnamed: Vec<_> = h
            .query_all_by_role(Role::TextInput)
            .filter(|n| n.accesskit_node().label().as_deref().is_none_or(str::is_empty) && n.accesskit_node().labelled_by().next().is_none())
            .map(|n| n.accesskit_node().value().unwrap_or_default().to_string())
            .collect();
        assert!(unnamed.is_empty(), "{tab}: unnamed text inputs {unnamed:?}");
        assert_eq!(h.state().ui.dialog.as_deref(), Some("dimstyle"), "{tab}: the dialog stays open");
    }
}

#[test]
fn an_edit_changes_the_style_and_its_existing_dimension() {
    let mut h = opened();
    let before = cadcraft_render::dimension_in(h.state().session.doc().unwrap(), &the_dimension(h.state()));
    h.get_by_label("Primary Units").click();
    h.run_steps(3);
    enter(&mut h, "Measurement scale factor", "2");
    let app = h.state();
    let d = app.session.doc().unwrap();
    assert_eq!(d.dim_style("Standard").map(|s| s.linear_factor), Some(2.0), "the edit reached the style");
    let after = cadcraft_render::dimension_in(d, &the_dimension(app));
    assert_ne!(before.value, after.value, "the existing dimension shows the new measurement");
    assert!(after.value.starts_with("20"), "{}", after.value);

    h.get_by_label("Text").click();
    h.run_steps(3);
    enter(&mut h, "Text height", "0.5");
    assert_eq!(h.state().session.doc().unwrap().dim_style("Standard").map(|s| s.text_height), Some(0.5));
    // The edit is one undoable step.
    h.state_mut().run("u", json!({})).unwrap();
    assert_ne!(h.state().session.doc().unwrap().dim_style("Standard").map(|s| s.text_height), Some(0.5));
}

#[test]
fn deviation_can_be_picked_with_equal_values() {
    let mut h = opened();
    h.get_by_label("Tolerances").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "Method").click();
    h.run_steps(3);
    h.get_by_label("Deviation").click();
    h.run_steps(4);
    let st = h.state().session.doc().unwrap().dim_style("Standard").cloned().unwrap();
    assert!(st.tolerance && !st.limits && st.tol_plus == st.tol_minus, "tolerances on, both values still equal");
    let shown = h.get_by_role_and_label(Role::ComboBox, "Method").accesskit_node().value().unwrap_or_default().to_string();
    assert_eq!(shown, "Deviation", "the pick stays although the values are equal");
    h.get_by_role_and_label(Role::TextInput, "Lower value");
    enter(&mut h, "Upper value", "0.1");
    let st = h.state().session.doc().unwrap().dim_style("Standard").cloned().unwrap();
    assert_eq!((st.tol_plus, st.tol_minus), (0.1, 0.0), "Upper value leaves the lower one");
}

#[test]
fn new_copies_the_selected_style() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("dimstyle", json!({ "name": "Mech", "DIMTXT": 0.3, "current": false })).unwrap();
    menus::activate(&mut app, "dimstyle");
    let mut h = harness(app);
    h.get_by_label("Mech").click();
    h.run_steps(3);
    h.get_by_label("Properties of Mech");
    enter(&mut h, "New style name", "Mech2");
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.dim_style("Mech2").map(|s| s.text_height), Some(0.3), "a copy of Mech, not of the current Standard");
    assert_eq!(d.header.str("DIMSTYLE", ""), "Standard");
    h.state_mut().run("u", json!({})).unwrap();
    assert!(h.state().session.doc().unwrap().dim_style("Mech2").is_none(), "one undo step");
}

#[test]
fn a_style_in_use_cannot_be_deleted_and_a_refused_value_says_why() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("dimstyle", json!({ "name": "Used" })).unwrap();
    app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    app.run("dimstyle", json!({ "name": "Spare", "current": false })).unwrap();
    app.run("dimstyle.current", json!({ "name": "Standard" })).unwrap();
    menus::activate(&mut app, "dimstyle");
    let mut h = harness(app);
    for (style, enabled) in [("Used", false), ("Spare", true)] {
        h.get_by_label(style).click();
        h.run_steps(3);
        assert_eq!(!h.get_by_label("Delete").accesskit_node().is_disabled(), enabled, "{style}");
    }
    h.get_by_label("Used").click();
    h.run_steps(3);
    enter(&mut h, "Arrow size", "-1");
    h.get_by_label("Arrow size must be 0 or more");
    assert!(h.state().session.doc().unwrap().dim_style("Used").is_some_and(|s| s.arrow_size >= 0.0));
    // An accepted edit clears the message.
    enter(&mut h, "Arrow size", "0.2");
    assert!(h.query_by_label("Arrow size must be 0 or more").is_none());
}

#[test]
fn new_rename_set_current_and_delete() {
    let mut h = opened();
    enter(&mut h, "New style name", "Mech");
    assert!(h.state().session.doc().unwrap().dim_style("Mech").is_some(), "New makes the style");
    h.get_by_label("Properties of Mech");
    enter(&mut h, "Rename to", "Mechanical");
    let d = h.state().session.doc().unwrap();
    assert!(d.dim_style("Mech").is_none() && d.dim_style("Mechanical").is_some(), "Rename renames it");
    h.get_by_label("Set Current").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().header.str("DIMSTYLE", ""), "Mechanical");
    h.get_by_label("Current dimension style: Mechanical");
    // The current style can't be deleted; select Standard, make it current, then delete Mechanical.
    h.get_by_label("Standard").click();
    h.run_steps(2);
    h.get_by_label("Set Current").click();
    h.run_steps(3);
    h.get_by_label("Mechanical").click();
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert!(h.state().session.doc().unwrap().dim_style("Mechanical").is_none(), "Delete deletes it");
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dimstyle"), "the dialog stays open");
}
