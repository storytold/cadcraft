use cadcraft_engine::Session;
use cadcraft_ui_egui::{
    CadApp, Services,
    docking::Panel,
    host_panel::{self, Event, Kind},
    menus,
};
use egui::{Event as Input, Key, Modifiers, accesskit::Role, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::{Value, json};

fn message() -> Value {
    json!({"type":"cadcraft:panel", "panel": {
        "title":"Host table", "notice":"Host notice", "sections":[{
            "id":"s", "title":"Section", "filter":true, "empty":"No rows",
            "columns":[{"id":"name","label":"Name"},{"id":"value","label":"Value"}],
            "actions":[{"id":"add","label":"Add row"},{"id":"disabled","label":"Disabled action","disabled":true}],
            "rows":[
                {"id":"edit","group":"Group", "cells":{"name":{"kind":"text","text":"Width"},"value":{"kind":"edit","text":"old","label":"Edit value"}}},
                {"id":"toggle","group":"Group", "cells":{"name":{"kind":"text","text":"Flag"},"value":{"kind":"toggle","on":true,"label":"Toggle row"}}},
                {"id":"button","cells":{"name":{"kind":"text","text":"Remove"},"value":{"kind":"button","text":"Drop","label":"Drop row"}}},
                {"id":"disabled","cells":{"value":{"kind":"button","text":"Disabled","label":"Disabled cell","disabled":true,"error":"Host error"}}}
            ]
        }]
    }})
}

fn model() -> host_panel::Model {
    host_panel::parse(&message()).unwrap().unwrap()
}

#[test]
fn host_panel_parse_valid_null_unknown_kind_and_unknown_fields() {
    assert_eq!(model().sections[0].rows.len(), 4);
    assert_eq!(host_panel::parse(&json!({"type":"cadcraft:panel","panel":null})).unwrap(), None);
    let mut value = message();
    value["panel"]["future"] = json!({"id":"x".repeat(1000)});
    value["panel"]["sections"][0]["rows"][0]["cells"]["value"]["kind"] = json!("future-kind");
    let model = host_panel::parse(&value).unwrap().unwrap();
    assert_eq!(model.sections[0].rows[0].cells["value"].kind, Kind::Text("old".into()));
}

#[test]
fn host_panel_parse_takes_a_section_without_empty_or_filter() {
    let value = json!({"type":"cadcraft:panel","panel":{"title":"Host table","sections":[{
        "id":"types","title":"Types","actions":[{"id":"add","label":"Add","disabled":true}],
        "columns":[{"id":"name","label":"Name"},{"id":"delete","label":""}],
        "rows":[{"id":"0","cells":{"name":{"kind":"edit","text":"Type 2"},"delete":{"kind":"button","text":"Delete","label":"Delete Type 2"}}}]
    }]}});
    let model = host_panel::parse(&value).unwrap().unwrap();
    assert_eq!(model.sections[0].empty, "");
    assert!(!model.sections[0].filter);
}

#[test]
fn host_panel_parse_refuses_each_collection_bound() {
    let valid = message();
    for (field, count) in [("columns", 65), ("rows", 2001)] {
        let mut bad = valid.clone();
        let item = bad["panel"]["sections"][0][field][0].clone();
        bad["panel"]["sections"][0][field] = Value::Array(vec![item; count]);
        assert!(host_panel::parse(&bad).is_err(), "{field}");
    }
    let mut bad = valid.clone();
    bad["panel"]["sections"] = Value::Array(vec![valid["panel"]["sections"][0].clone(); 9]);
    assert!(host_panel::parse(&bad).is_err());
    let mut bad = valid;
    bad["panel"]["sections"][0]["rows"][0]["cells"] = Value::Object((0..129).map(|n| (n.to_string(), json!({"kind":"text","text":"x"}))).collect());
    assert!(host_panel::parse(&bad).is_err());
}

#[test]
fn host_panel_parse_refuses_strings_and_ids_over_their_limits() {
    for pointer in [
        "/panel/title",
        "/panel/notice",
        "/panel/sections/0/title",
        "/panel/sections/0/empty",
        "/panel/sections/0/columns/0/label",
        "/panel/sections/0/actions/0/label",
        "/panel/sections/0/rows/0/group",
        "/panel/sections/0/rows/0/cells/value/text",
        "/panel/sections/0/rows/0/cells/value/label",
        "/panel/sections/0/rows/3/cells/value/error",
    ] {
        let mut bad = message();
        *bad.pointer_mut(pointer).unwrap() = json!("界".repeat(513));
        assert!(host_panel::parse(&bad).is_err(), "{pointer}");
    }
    for pointer in ["/panel/sections/0/id", "/panel/sections/0/columns/0/id", "/panel/sections/0/rows/0/id", "/panel/sections/0/actions/0/id"] {
        let mut bad = message();
        *bad.pointer_mut(pointer).unwrap() = json!("x".repeat(65));
        assert!(host_panel::parse(&bad).is_err(), "{pointer}");
    }
    let mut bad = message();
    bad["panel"]["sections"][0]["rows"][0]["cells"].as_object_mut().unwrap().insert("x".repeat(65), json!({"kind":"text","text":""}));
    assert!(host_panel::parse(&bad).is_err());
    let mut valid = message();
    valid["panel"]["title"] = json!("界".repeat(512));
    assert!(host_panel::parse(&valid).is_ok(), "Unicode characters, not UTF-8 bytes");
}

#[test]
fn host_panel_parse_missing_fields_are_refused() {
    for pointer in [
        "",
        "/panel",
        "/panel/sections/0",
        "/panel/sections/0/columns/0",
        "/panel/sections/0/rows/0",
        "/panel/sections/0/rows/0/cells/value",
        "/panel/sections/0/rows/1/cells/value",
        "/panel/sections/0/rows/2/cells/value",
        "/panel/sections/0/actions/0",
    ] {
        let valid = message();
        let keys: Vec<_> = valid.pointer(pointer).unwrap().as_object().unwrap().keys().cloned().collect();
        for key in keys {
            if matches!(key.as_str(), "notice" | "group" | "filter" | "empty" | "actions" | "label")
                && !pointer.ends_with("columns/0")
                && !pointer.ends_with("actions/0")
                && !pointer.contains("rows/1/cells")
                && !pointer.contains("rows/2/cells")
            {
                continue;
            }
            let mut bad = valid.clone();
            bad.pointer_mut(pointer).unwrap().as_object_mut().unwrap().remove(&key);
            assert!(host_panel::parse(&bad).is_err(), "{pointer}/{key}");
        }
    }
}

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.host_panel.enabled = true;
    app.receive_host_panel(Some(model()));
    let mut h = Harness::builder().with_size(vec2(1440.0, 1100.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn events(h: &mut Harness<'_, CadApp>) -> Vec<Value> {
    std::mem::take(&mut h.state_mut().host_panel.events).iter().map(|event| serde_json::to_value(event).unwrap()).collect()
}

fn type_value(h: &mut Harness<'_, CadApp>, value: &str) {
    h.get_by_role_and_label(Role::TextInput, "Edit value").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.event(Input::Text(value.into()));
    h.run_steps(2);
}

#[test]
fn host_panel_docked_right_clicks_emit_only_events_and_null_removes() {
    let mut h = harness();
    let location = h.state().ui.docking.layout.location(&Panel::Host).unwrap();
    assert_eq!(serde_json::to_value(location.group).unwrap()["column"]["side"], json!("End"));
    let before = h.state().host_panel.model.clone();
    for (label, expected) in [
        ("Toggle row", json!({"type":"cadcraft:panel-edit","section":"s","row":"toggle","column":"value","value":false})),
        ("Drop row", json!({"type":"cadcraft:panel-edit","section":"s","row":"button","column":"value","value":null})),
        ("Add row", json!({"type":"cadcraft:panel-action","section":"s","action":"add"})),
    ] {
        h.get_by_label(label).click();
        h.run_steps(3);
        assert_eq!(events(&mut h), vec![expected]);
        assert_eq!(h.state().host_panel.model, before);
    }
    assert!(h.query_by_label("Host error").is_some());
    if let Ok(path) = std::env::var("CRAFT_HOST_PANEL_SNAPSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    h.state_mut().receive_host_panel(None);
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Host));
    assert!(!h.state().ui.docking.hidden.iter().any(|(panel, _)| *panel == Panel::Host));
}

#[test]
fn host_panel_edit_enter_escape_focus_leave_and_parent_update() {
    let mut h = harness();
    type_value(&mut h, "typed");
    let mut update = message();
    update["panel"]["sections"][0]["rows"][0]["cells"]["value"]["text"] = json!("parent update");
    h.state_mut().receive_host_panel(host_panel::parse(&update).unwrap());
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(events(&mut h), vec![json!({"type":"cadcraft:panel-edit","section":"s","row":"edit","column":"value","value":"typed"})]);
    type_value(&mut h, "abandoned");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(events(&mut h).is_empty());
    type_value(&mut h, "blurred");
    h.get_by_role_and_label(Role::TextInput, "Search").click();
    h.run_steps(3);
    assert_eq!(events(&mut h), vec![json!({"type":"cadcraft:panel-edit","section":"s","row":"edit","column":"value","value":"blurred"})]);
    assert!(h.state().cmd.buffer.is_empty());
}

#[test]
fn host_panel_user_close_reopen_and_nonhost_layout_guard() {
    let mut h = harness();
    h.state_mut().run("ui.dock", json!({"operation":"close","panel":"host"})).unwrap();
    h.state_mut().receive_host_panel(Some(model()));
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Host));
    h.state_mut().receive_host_panel(None);
    h.state_mut().receive_host_panel(Some(model()));
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Host), "null does not undo user-close");
    assert!(menus::tree(h.state()).iter().any(
        |(name, entries)| name == "Window" && entries.iter().any(|entry| matches!(entry,menus::Entry::Item { id, .. } if id == "ui.panel.host"))
    ));
    h.state_mut().run("ui.panel.host", json!({})).unwrap();
    assert!(h.state().ui.docking.layout.contains(&Panel::Host));
    h.state_mut().run("ui.dock", json!({"operation":"close","panel":"host"})).unwrap();
    h.state_mut().host_panel_open();
    h.state_mut().receive_host_panel(Some(model()));
    assert!(h.state().ui.docking.layout.contains(&Panel::Host));
    h.state_mut().host_panel.enabled = false;
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Host));
    assert!(h.state_mut().run("ui.dock", json!({"operation":"open","panel":"host"})).is_err());
    assert!(
        !menus::tree(h.state())
            .iter()
            .any(|(_, entries)| entries.iter().any(|entry| matches!(entry,menus::Entry::Item { id, .. } if id == "ui.panel.host")))
    );
}

#[test]
fn host_panel_event_json_is_the_v1_contract() {
    let event = Event::Action { section: "s".into(), action: "a".into() };
    assert_eq!(serde_json::to_value(event).unwrap(), json!({"type":"cadcraft:panel-action","section":"s","action":"a"}));
}

#[test]
fn host_panel_disabled_controls_filter_empty_and_no_drawing() {
    let mut h = harness();
    for label in ["Disabled cell", "Disabled action"] {
        let pos = h.get_by_label(label).rect().center();
        h.event(Input::PointerMoved(pos));
        for pressed in [true, false] {
            h.event(Input::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    assert!(events(&mut h).is_empty());
    h.get_by_role_and_label(Role::TextInput, "Search").click();
    h.event(Input::Text("Width".into()));
    h.run_steps(3);
    assert!(h.query_by_label("Edit value").is_some());
    assert!(h.query_by_label("Toggle row").is_none());

    let mut empty = model();
    empty.sections[0].rows.clear();
    h.state_mut().receive_host_panel(Some(empty));
    h.state_mut().session = Session::empty();
    h.run_steps(3);
    assert!(h.query_by_label("No rows").is_some());
    assert!(h.state().ui.docking.layout.contains(&Panel::Host));
}
