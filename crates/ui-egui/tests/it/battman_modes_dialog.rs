//! Block Attribute Manager: the Verify, Preset, Lock position and Multiple lines check boxes switch
//! the selected definition's modes through the `battman` command, and its reference follows.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::accesskit::{Role, Toggled};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

/// Block T with the definition ROOM, one reference, and BATTMAN opened from its menu item.
fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    let a = app.run("attdef", json!({ "tag": "ROOM", "prompt": "Room", "default": "101", "at": [0, 2] })).unwrap();
    app.run("block", json!({ "name": "T", "base": [0, 0], "handles": [a["handle"]], "keep": "delete" })).unwrap();
    app.run("insert", json!({ "name": "T", "at": [10, 0], "attribs": { "ROOM": "7" } })).unwrap();
    menus::activate(&mut app, "battman");
    assert_eq!(app.ui.dialog.as_deref(), Some("battman"));
    let mut h = Harness::builder().with_size(vec2(900.0, 800.0)).build_ui_state(|ui, app: &mut CadApp| dialogs::show(app, ui.ctx()), app);
    h.run_steps(3);
    h
}

/// Block T with the definitions ROOM and AREA, and BATTMAN opened from its menu item.
fn two_rows() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    let a = app.run("attdef", json!({ "tag": "ROOM", "prompt": "Room", "default": "101", "at": [0, 2] })).unwrap();
    let b = app.run("attdef", json!({ "tag": "AREA", "prompt": "Area", "default": "12", "at": [0, 0] })).unwrap();
    app.run("block", json!({ "name": "T", "base": [0, 0], "handles": [a["handle"], b["handle"]], "keep": "delete" })).unwrap();
    menus::activate(&mut app, "battman");
    let mut h = Harness::builder().with_size(vec2(900.0, 800.0)).build_ui_state(|ui, app: &mut CadApp| dialogs::show(app, ui.ctx()), app);
    h.run_steps(3);
    h
}

fn listed(app: &mut CadApp) -> serde_json::Value {
    app.run("battman", json!({ "name": "T" })).unwrap()["attributes"][0].clone()
}

/// Click the field labelled `label` and type `text` at the end of its value, without leaving it.
fn type_at_end(h: &mut Harness<'static, CadApp>, role: Role, label: &str, text: &str) {
    h.get_by_role_and_label(role, label).click();
    h.run_steps(2);
    h.key_press(egui::Key::End);
    h.run_steps(1);
    h.get_by_role_and_label(role, label).type_text(text);
    h.run_steps(2);
}

fn checked(h: &Harness<'static, CadApp>, label: &str) -> bool {
    h.get_by_role_and_label(Role::CheckBox, label).accesskit_node().toggled() == Some(Toggled::True)
}

#[test]
fn check_boxes_switch_the_modes_and_multiple_lines() {
    let mut h = harness();
    for label in ["Verify", "Preset", "Lock position", "Multiple lines"] {
        assert!(!checked(&h, label), "{label} starts off");
        h.get_by_role_and_label(Role::CheckBox, label).click();
        h.run_steps(3);
        assert!(checked(&h, label), "{label} on");
    }
    let row = listed(h.state_mut());
    assert_eq!(
        (row["verify"].as_bool(), row["preset"].as_bool(), row["lockPosition"].as_bool(), row["multiline"].as_bool()),
        (Some(true), Some(true), Some(true), Some(true))
    );
    // "Apply changes to existing references" is on: the reference is multiline and locked now.
    let app = h.state();
    let d = app.session.doc().unwrap();
    let EntityKind::Insert(i) = &d.model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().kind else { panic!() };
    assert!(i.attribs[0].multiline.is_some() && i.attribs[0].modes.lock_position);
    assert_eq!(i.attribs[0].text.value, "7");

    // Off again, as one undo step each.
    h.get_by_role_and_label(Role::CheckBox, "Multiple lines").click();
    h.run_steps(3);
    assert!(!checked(&h, "Multiple lines"));
    assert_eq!(listed(h.state_mut())["multiline"], false);
    h.state_mut().run("u", json!({})).unwrap();
    assert_eq!(listed(h.state_mut())["multiline"], true, "undo brings Multiple lines back");
}

#[test]
fn a_multiline_default_is_edited_on_several_lines() {
    let mut h = harness();
    // Single-line: a one-line field.
    h.get_by_role_and_label(Role::TextInput, "Default");
    assert!(h.query_by_role_and_label(Role::MultilineTextInput, "Default").is_none());
    h.get_by_role_and_label(Role::CheckBox, "Multiple lines").click();
    h.run_steps(3);
    // Multiple lines on: Enter in the Default field is a line break, kept when the field loses focus.
    h.get_by_role_and_label(Role::MultilineTextInput, "Default").focus();
    h.run_steps(2);
    h.key_press(egui::Key::End);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(1);
    h.get_by_role_and_label(Role::MultilineTextInput, "Default").type_text("second");
    h.run_steps(2);
    assert_eq!(listed(h.state_mut())["default"], "101", "not committed while typing");
    h.get_by_role_and_label(Role::TextInput, "Height").focus();
    h.run_steps(3);
    assert_eq!(listed(h.state_mut())["default"], "101\nsecond");
}

#[test]
fn a_multiline_default_is_committed_when_another_row_is_selected() {
    let mut h = two_rows();
    h.get_by_role_and_label(Role::CheckBox, "Multiple lines").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::MultilineTextInput, "Default").click();
    h.run_steps(2);
    h.key_press(egui::Key::End);
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(1);
    h.get_by_role_and_label(Role::MultilineTextInput, "Default").type_text("second");
    h.run_steps(2);
    // Enter was a line break: nothing is committed until the field goes. Selecting AREA does that.
    assert_eq!(listed(h.state_mut())["default"], "101");
    h.get_by_label("AREA").click();
    h.run_steps(3);
    h.get_by_label("Edit attribute AREA");
    assert_eq!(listed(h.state_mut())["default"], "101\nsecond");
    // AREA's field shows AREA's own value, and ROOM's shows the committed one when selected again.
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Default").accesskit_node().value().as_deref(), Some("12"));
    h.get_by_label("ROOM").click();
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(Role::MultilineTextInput, "Default").accesskit_node().value().as_deref(), Some("101\nsecond"));
}

#[test]
fn one_line_fields_are_committed_when_another_row_is_selected() {
    let mut h = two_rows();
    for (label, key, typed, want) in [("Prompt", "prompt", "s", "Rooms"), ("Default", "default", "9", "1019"), ("Tag", "tag", "S", "ROOMS")] {
        let tag = listed(h.state_mut())["tag"].as_str().unwrap().to_string();
        h.get_by_label(&tag).click();
        h.run_steps(3);
        type_at_end(&mut h, Role::TextInput, label, typed);
        h.get_by_label("AREA").click();
        h.run_steps(3);
        assert_eq!(listed(h.state_mut())[key], want, "{label} committed when AREA is selected");
    }
    assert_eq!(listed(h.state_mut())["prompt"], "Rooms");
}

#[test]
fn an_edit_is_committed_when_the_dialog_closes_or_a_button_is_clicked() {
    let mut h = two_rows();
    // Move Down while the prompt is being typed: the prompt is kept and ROOM moves down.
    type_at_end(&mut h, Role::TextInput, "Prompt", "s");
    h.get_by_role_and_label(Role::Button, "Move Down").click();
    h.run_steps(3);
    let rows = h.state_mut().run("battman", json!({ "name": "T" })).unwrap()["attributes"].clone();
    assert_eq!((rows[0]["tag"].as_str(), rows[1]["tag"].as_str(), rows[1]["prompt"].as_str()), (Some("AREA"), Some("ROOM"), Some("Rooms")));
    // Closing the dialog while the default is being typed keeps it.
    type_at_end(&mut h, Role::TextInput, "Default", "9");
    h.get_by_role_and_label(Role::Button, "Close window").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog, None);
    let rows = h.state_mut().run("battman", json!({ "name": "T" })).unwrap()["attributes"].clone();
    assert_eq!(rows[1]["default"], "1019");
}
