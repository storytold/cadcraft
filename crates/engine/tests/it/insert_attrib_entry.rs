//! INSERT and the attribute modes (gap F17): Verify attributes are asked twice, Preset ones take
//! their default without asking, ATTREQ 0 gives every attribute its default, ATTDIA 0 (the
//! default) asks at the command line and ATTDIA 1 asks for the values all at once in the Edit
//! Attributes dialog (`Session::attribute_entry`), typed answers still working. The JSON form never asks. ATTREQ and ATTDIA are user settings, as in
//! AutoCAD (DWG keeps them in its header only before R2000).

use std::sync::Arc;

use cadcraft_engine::doc::{Attrib, AttribModes, Block, Entity, EntityKind, Handle, Text};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{AttribAsk, Input, Session};
use serde_json::json;

fn def(tag: &str, prompt: &str, default: &str, y: f64, constant: bool, modes: AttribModes) -> Attrib {
    Attrib {
        tag: tag.into(),
        text: Text {
            insert: Vec3::new(0.0, y, 0.0),
            align_pt: None,
            height: 1.0,
            value: default.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        },
        invisible: false,
        constant,
        prompt: prompt.into(),
        props: Default::default(),
        multiline: None,
        modes,
    }
}

/// Block T with the definitions A (Verify), B (Preset), C (plain) and D (constant); ATTREQ and
/// ATTDIA as given (their defaults, ATTREQ 1 and ATTDIA 0, otherwise).
fn session(attreq: Option<bool>, attdia: Option<bool>) -> Session {
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let mut b = Block::new("T");
    let none = AttribModes::default();
    for a in [
        def("A", "First", "1", 2.0, false, AttribModes { verify: true, ..none }),
        def("B", "Second", "two", 4.0, false, AttribModes { preset: true, ..none }),
        def("C", "Third", "3", 6.0, false, none),
        def("D", "", "fixed", 8.0, true, none),
    ] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::AttDef(a)));
    }
    d.blocks.insert("T".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "attribs", None);
    if let Some(v) = attreq {
        s.settings.attreq = v;
    }
    if let Some(v) = attdia {
        s.settings.attdia = v;
    }
    s
}

/// Start INSERT of T at 100,0 and answer the scale and rotation prompts.
fn insert_up_to_the_attributes(s: &mut Session) {
    s.start("insert").unwrap();
    s.cmdline("T").unwrap();
    s.input(Input::Point(Vec2::new(100.0, 0.0))).unwrap();
    s.input(Input::Enter).unwrap(); // X scale 1
    s.input(Input::Enter).unwrap(); // Y scale = X
    s.input(Input::Enter).unwrap(); // rotation 0
}

/// The attribute values of the last block reference, in order.
fn values(s: &Session) -> Vec<(String, String)> {
    let d = s.doc().unwrap();
    let Some(EntityKind::Insert(i)) = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Insert(_))).last().map(|e| e.kind.clone()) else {
        panic!("no block reference")
    };
    i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(t, v)| (t.to_string(), v.to_string())).collect()
}

#[test]
fn typed_insert_asks_verify_twice_and_skips_preset_and_constant() {
    // AutoCAD's default, ATTDIA 0, with no value in the drawing: the command line asks.
    let mut s = session(None, None);
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTDIA" })).unwrap()["value"], 0);
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTREQ" })).unwrap()["value"], 1);
    insert_up_to_the_attributes(&mut s);
    assert!(s.attribute_entry().is_none(), "ATTDIA 0: no dialog");
    assert_eq!(s.prompt_text(), "INSERT First <1>:", "A first");
    s.input(Input::Text("10".into())).unwrap();
    assert_eq!(s.prompt_text(), "INSERT Verify attribute value <10>:");
    s.input(Input::Text("11".into())).unwrap(); // the value typed at Verify replaces it
    assert_eq!(s.prompt_text(), "INSERT Third <3>:", "B is Preset: not asked");
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "11"), ("B", "two"), ("C", "3")]));

    // Enter at Verify keeps the value given; Enter at A takes its default, verified.
    insert_up_to_the_attributes(&mut s);
    s.input(Input::Enter).unwrap();
    assert_eq!(s.prompt_text(), "INSERT Verify attribute value <1>:");
    s.input(Input::Enter).unwrap();
    s.input(Input::Text("c".into())).unwrap();
    assert_eq!(values(&s), pairs(&[("A", "1"), ("B", "two"), ("C", "c")]));

    // A script answers in turn too.
    s.script("-INSERT T 0,50 1 1 0\na\na\nc\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "a"), ("B", "two"), ("C", "c")]));
}

#[test]
fn attreq_0_gives_every_attribute_its_default() {
    let mut s = session(Some(false), None);
    insert_up_to_the_attributes(&mut s);
    assert!(s.running.is_none(), "no attribute prompt: {}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "1"), ("B", "two"), ("C", "3")]));
    // SETVAR changes it.
    s.execute("setvar", &json!({ "name": "ATTREQ", "value": 1 })).unwrap();
    s.execute("setvar", &json!({ "name": "ATTDIA", "value": 0 })).unwrap();
    insert_up_to_the_attributes(&mut s);
    assert_eq!(s.prompt_text(), "INSERT First <1>:");
}

#[test]
fn attdia_1_asks_for_every_value_at_once_and_typed_answers_still_work() {
    let mut s = session(None, None);
    s.execute("setvar", &json!({ "name": "ATTDIA", "value": 1 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTDIA" })).unwrap()["value"], 1);
    s.start("insert").unwrap();
    s.cmdline("T").unwrap();
    assert!(s.attribute_entry().is_none(), "not before the attribute prompts");
    s.input(Input::Point(Vec2::new(100.0, 0.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    let e = s.attribute_entry().expect("the dialog's rows");
    assert_eq!(e.block, "T");
    let ask =
        |tag: &str, prompt: &str, default: &str, verify: bool| AttribAsk { tag: tag.into(), prompt: prompt.into(), default: default.into(), verify };
    assert_eq!(e.rows, vec![ask("A", "First", "1", true), ask("C", "Third", "3", false)]);
    // The dialog's OK: the answers typed in turn, Verify ones twice.
    let answers = e.answers(&["x".into(), String::new()]);
    assert_eq!(answers, ["x", "x", ""]);
    for a in answers {
        s.input(Input::Text(a)).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "x"), ("B", "two"), ("C", "")]));

    // Typed instead: the dialog goes away after the first answer.
    insert_up_to_the_attributes(&mut s);
    assert!(s.attribute_entry().is_some());
    s.input(Input::Text("typed".into())).unwrap();
    assert!(s.attribute_entry().is_none());
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(values(&s), pairs(&[("A", "typed"), ("B", "two"), ("C", "3")]));

    // Cancel from the dialog inserts nothing.
    let before = s.doc().unwrap().model.len();
    insert_up_to_the_attributes(&mut s);
    s.cancel();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.len(), before);
}

#[test]
fn json_insert_never_asks_whatever_attreq_and_attdia_say() {
    for (req, dia) in [(false, false), (true, true)] {
        let mut s = session(Some(req), Some(dia));
        let r = s.execute("insert", &json!({ "name": "T", "at": [0, 0], "attribs": { "a": "given", "B": "preset given" } })).unwrap();
        assert!(r["handle"].is_string());
        assert!(s.running.is_none() && s.attribute_entry().is_none());
        assert_eq!(values(&s), pairs(&[("A", "given"), ("B", "preset given"), ("C", "3")]));
    }
}

#[test]
fn hostile_values_never_panic() {
    let mut s = session(None, Some(true));
    insert_up_to_the_attributes(&mut s);
    let e = s.attribute_entry().expect("asks");
    // Too few answers take the defaults, too many are ignored.
    assert_eq!(e.answers(&[]), ["1", "1", "3"]);
    assert_eq!(e.answers(&["p".into(), "q".into(), "r".into()]), ["p", "p", "q"]);
    s.cancel();
    for bad in [json!(2), json!(-1), json!("x"), json!(null), json!(1.5)] {
        assert!(s.execute("setvar", &json!({ "name": "ATTDIA", "value": bad })).is_err(), "{bad}");
        assert!(s.execute("setvar", &json!({ "name": "ATTREQ", "value": bad })).is_err(), "{bad}");
    }
    assert!(s.settings.attdia && s.settings.attreq, "a refused value changes nothing");
    // An INSERT of a block with only Preset and constant attributes asks nothing.
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let mut b = Block::new("P");
    let h: Handle = d.new_handle();
    b.entities.push(Entity::new(h, EntityKind::AttDef(def("P", "p", "v", 0.0, false, AttribModes { preset: true, ..AttribModes::default() }))));
    d.blocks.insert("P".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "preset", None);
    s.script("-INSERT P 0,0 1 1 0\n").unwrap();
    assert!(s.running.is_none() && s.attribute_entry().is_none());
    assert_eq!(values(&s), pairs(&[("P", "v")]));
}

#[test]
fn two_definitions_with_one_tag_are_both_asked_once() {
    // INSERT counted its answers by tag: the second X was asked for ever.
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let mut b = Block::new("DUP");
    for (prompt, y) in [("first", 0.0), ("second", 2.0)] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::AttDef(def("X", prompt, "d", y, false, AttribModes::default()))));
    }
    d.blocks.insert("DUP".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "dup", None);
    s.settings.attdia = false;
    s.script("-INSERT DUP 0,0 1 1 0\na\nb\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s).len(), 2);
}
