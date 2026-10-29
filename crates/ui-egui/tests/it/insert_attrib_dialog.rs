//! Edit Attributes (ATTDIA 1): a typed INSERT asks for its attribute values in one dialog. OK
//! gives them to INSERT (a Verify one is not asked again), Enter in a field is OK, Cancel inserts
//! nothing, and with ATTDIA 0 or a JSON call there is no dialog.

use cadcraft_engine::Input;
use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services, dialogs};
use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::json;

/// Block T with the definitions A ("Room", default "101") and B ("Floor", default "1"), and
/// ATTDIA 1 (the default is 0, the command line).
fn app() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("setvar", json!({ "name": "ATTDIA", "value": 1 })).unwrap();
    let a = app.run("attdef", json!({ "tag": "A", "prompt": "Room", "default": "101", "at": [0, 2] })).unwrap();
    let b = app.run("attdef", json!({ "tag": "B", "prompt": "Floor", "default": "1", "at": [0, 4] })).unwrap();
    app.run("block", json!({ "name": "T", "base": [0, 0], "handles": [a["handle"], b["handle"]], "keep": "delete" })).unwrap();
    app
}

/// INSERT of T typed, answered up to its attribute values.
fn insert(app: &mut CadApp) {
    app.start("insert");
    app.cmdline("T");
    for i in [Input::Point(Vec2::new(50.0, 0.0)), Input::Enter, Input::Enter, Input::Enter] {
        app.session.input(i).unwrap();
    }
}

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(900.0, 700.0)).build_ui_state(|ui, app: &mut CadApp| dialogs::show(app, ui.ctx()), app);
    h.run_steps(3);
    h
}

fn values(app: &CadApp) -> Option<Vec<(String, String)>> {
    let d = app.session.doc().unwrap();
    d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Insert(i) => Some(i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()),
        _ => None,
    })
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(Role::TextInput).value(shown))
}

fn replace(h: &mut Harness<'static, CadApp>, shown: &str, with: &str) {
    field(h, shown).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(h, shown).type_text(with);
    h.run_steps(2);
}

#[test]
fn ok_gives_the_values_to_insert() {
    let mut a = app();
    insert(&mut a);
    let mut h = harness(a);
    h.get_by_label("Edit Attributes");
    // The fields are named by their prompts.
    assert_eq!(h.get_by_label("Room").value().as_deref(), Some("101"));
    replace(&mut h, "1", "3");
    replace(&mut h, "101", "205");
    h.get_by_label("OK").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none(), "{}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "205".into()), ("B".into(), "3".into())]));
    assert!(h.query_by_label("Edit Attributes").is_none(), "closed");
}

#[test]
fn enter_in_a_field_is_ok_and_verify_is_not_asked_again() {
    let mut a = app();
    a.run("battman", json!({ "name": "T", "action": "edit", "tag": "A", "default": "7" })).unwrap();
    // Make A a Verify attribute in the block definition.
    let d = a.session.doc_mut().unwrap();
    let mut b = (**d.block("T").unwrap()).clone();
    for e in b.entities.iter().map(|e| e.handle).collect::<Vec<_>>() {
        b.entities.modify(e, |e| {
            if let EntityKind::AttDef(ad) = &mut e.kind
                && ad.tag == "A"
            {
                ad.modes.verify = true;
            }
        });
    }
    d.blocks.insert("T".into(), std::sync::Arc::new(b));
    insert(&mut a);
    let mut h = harness(a);
    replace(&mut h, "7", "8");
    h.key_press(Key::Enter);
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none(), "{}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "8".into()), ("B".into(), "1".into())]));
}

#[test]
fn cancel_inserts_nothing() {
    let mut a = app();
    insert(&mut a);
    let mut h = harness(a);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none());
    assert_eq!(values(app), None);
}

#[test]
fn no_dialog_with_attdia_0_or_from_json() {
    let mut a = app();
    a.run("setvar", json!({ "name": "ATTDIA", "value": 0 })).unwrap();
    insert(&mut a);
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none());
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");

    // ATTDIA's default, 0, asks at the command line too.
    let mut a = app();
    a.session.settings = Default::default();
    assert!(!a.session.settings.attdia);
    insert(&mut a);
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none());
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");

    let mut a = app();
    a.run("insert", json!({ "name": "T", "at": [0, 0], "attribs": { "A": "x" } })).unwrap();
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none());
    assert_eq!(values(h.state()), Some(vec![("A".into(), "x".into()), ("B".into(), "1".into())]));
}
