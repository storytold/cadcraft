//! MLEADER block content with attributes (gap F10): the block's attribute definitions become the
//! content's attributes, placed like them, with values from `attributes` (JSON) or asked at the
//! command line after the leader, as INSERT asks them. They are drawn and saved.

use cadcraft_engine::Input;
use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, MLeader};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn last_mleader(s: &Session) -> MLeader {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::MLeader(m) => m.clone(),
        other => panic!("not a multileader: {other:?}"),
    }
}

/// A session with block "BUBBLE": a circle with a NUM attribute (default "1") and an ITEM
/// attribute (default "A"), each with a prompt.
fn with_bubble() -> Session {
    let mut s = Session::new();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap()["handle"].clone();
    let num = s.execute("attdef", &json!({"tag": "NUM", "prompt": "Number", "default": "1", "at": [-0.2, -0.1], "height": 0.25})).unwrap()["handle"]
        .clone();
    let item =
        s.execute("attdef", &json!({"tag": "ITEM", "prompt": "Item letter", "default": "A", "at": [-0.2, -0.6], "height": 0.2})).unwrap()["handle"]
            .clone();
    s.execute("block", &json!({"name": "BUBBLE", "base": [0, 0], "handles": [c, num, item], "keep": "delete"})).unwrap();
    s
}

fn values(m: &MLeader) -> Vec<(String, String)> {
    m.block.as_ref().map(|b| b.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()).unwrap_or_default()
}

#[test]
fn json_block_content_gets_its_attributes() {
    let mut s = with_bubble();
    s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "BUBBLE", "blockAttach": "insertion", "attributes": {"num": "42"}})).unwrap();
    let m = last_mleader(&s);
    // NUM from `attributes` (any case), ITEM its default.
    assert_eq!(values(&m), vec![("NUM".into(), "42".into()), ("ITEM".into(), "A".into())]);
    // Placed like the definition under the block reference.
    let b = m.block.as_ref().unwrap();
    let num = &b.attribs[0].text;
    assert!((num.insert.xy() - (b.insert.xy() + Vec2::new(-0.2, -0.1))).len() < 1e-9, "{:?} {:?}", num.insert, b.insert);
    // Drawn: the attribute text adds strokes to the multileader.
    let with = cadcraft_engine::render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &Default::default()).prims.len();
    s.execute("mleader", &json!({"points": [[0, 20], [10, 25]], "block": "BUBBLE", "attributes": {"NUM": "", "ITEM": ""}})).unwrap();
    let both = cadcraft_engine::render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &Default::default()).prims.len();
    assert!(both > with, "the second multileader draws");
    assert!(with > 3, "{with}");
    // `attributes` must be an object.
    assert!(s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "BUBBLE", "attributes": ["x"]})).is_err());
}

#[test]
fn mleader_prompts_ask_the_attribute_values() {
    let mut s = with_bubble();
    s.start("mleader").unwrap();
    // Options > Content type > Block > BUBBLE > Insertion point, then leave the Options.
    s.input(Input::Keyword("Options".into())).unwrap();
    s.input(Input::Keyword("Content type".into())).unwrap();
    s.input(Input::Keyword("Block".into())).unwrap();
    s.input(Input::Text("bubble".into())).unwrap();
    s.input(Input::Keyword("Insertion point".into())).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(10.0, 5.0))).unwrap();
    assert!(s.prompt_text().contains("Number"), "{}", s.prompt_text());
    s.input(Input::Text("7".into())).unwrap();
    assert!(s.prompt_text().contains("Item letter"), "{}", s.prompt_text());
    // Enter keeps the default.
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(values(&last_mleader(&s)), vec![("NUM".into(), "7".into()), ("ITEM".into(), "A".into())]);
}

#[test]
fn block_content_without_attributes_asks_nothing() {
    let mut s = Session::new();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "DOT", "base": [0, 0], "handles": [c], "keep": "delete"})).unwrap();
    s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "DOT"})).unwrap();
    assert!(last_mleader(&s).block.as_ref().is_some_and(|b| b.attribs.is_empty()));
}

#[test]
fn attributes_survive_dxf_and_dwg() {
    let mut s = with_bubble();
    s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "BUBBLE", "attributes": {"NUM": "12", "ITEM": "Z"}})).unwrap();
    let m = last_mleader(&s);
    let back = cadcraft_io::read_dxf(cadcraft_io::write(s.doc().unwrap(), "x.dxf").unwrap().as_slice()).unwrap();
    let got: Vec<&MLeader> = back.model.iter().filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m) } else { None }).collect();
    assert_eq!(got.len(), 1);
    assert_eq!(values(got[0]), values(&m));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(s.doc().unwrap(), "x.dwg").unwrap();
        let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
        let got: Vec<&MLeader> = back.model.iter().filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m) } else { None }).collect();
        assert_eq!(got.len(), 1);
        assert_eq!(values(got[0]), values(&m));
    }
}

#[test]
fn hostile_attribute_params_never_panic() {
    let mut s = with_bubble();
    for p in [
        json!({"points": [[0, 0], [10, 5]], "block": "BUBBLE", "attributes": {"NUM": null, "ITEM": {"x": 1}}}),
        json!({"points": [[0, 0], [10, 5]], "block": "BUBBLE", "attributes": {"NUM": 1e308, "": "x", "ITEM": true}}),
        json!({"points": [[0, 0], [10, 5]], "block": "BUBBLE", "attributes": "NUM=1"}),
    ] {
        let _ = s.execute("mleader", &p);
    }
    let _ = cadcraft_engine::render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &Default::default());
}
