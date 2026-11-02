//! `blocks.dynamic` and `blocks.list` report a dynamic block's parameters and actions as read
//! from another program's file (kept foreign data, built by hand here).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::foreign::{Foreign, ForeignExtra, ForeignObject};
use cadcraft_engine::doc::{Handle, RawTag};
use serde_json::json;

fn obj(handle: u64, kind: &str, tags: &[(i32, &str)]) -> ForeignObject {
    ForeignObject { handle: Handle(handle), kind: kind.into(), tags: tags.iter().map(|(c, v)| RawTag { code: *c, value: (*v).into() }).collect() }
}

/// A session whose block "Door" has a visibility parameter with two states.
fn session() -> Session {
    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Door", "base": [0, 0], "handles": [l] })).unwrap();
    let line = s.doc().unwrap().block("Door").unwrap().entities.handles()[0].hex();
    let mut f = Foreign::default();
    f.records.insert(("BLOCK_RECORD".into(), "DOOR".into()), ForeignExtra { xdict: Some(Handle(0xF00)), ..Default::default() });
    f.objects.push(obj(0xF00, "DICTIONARY", &[(100, "AcDbDictionary"), (3, "ACAD_ENHANCEDBLOCK"), (360, "F01")]));
    f.objects.push(obj(0xF01, "ACAD_EVALUATION_GRAPH", &[(100, "AcDbEvalGraph"), (91, "0"), (95, "1"), (360, "F02")]));
    f.objects.push(obj(
        0xF02,
        "BLOCKVISIBILITYPARAMETER",
        &[
            (100, "AcDbEvalExpr"),
            (90, "1"),
            (100, "AcDbBlockElement"),
            (300, "Visibility State"),
            (100, "AcDbBlockParameter"),
            (280, "1"),
            (100, "AcDbBlock1PtParameter"),
            (1010, "2"),
            (1020, "3"),
            (100, "AcDbBlockVisibilityParameter"),
            (301, "Visibility1"),
            (302, ""),
            (93, "1"),
            (331, &line),
            (92, "2"),
            (303, "Open"),
            (94, "1"),
            (332, &line),
            (95, "0"),
            (303, "Closed"),
            (94, "0"),
            (95, "0"),
        ],
    ));
    let mut d = s.doc().unwrap().clone();
    d.foreign = Arc::new(f);
    s.open_drawing(d, "dynamic", None);
    s
}

#[test]
fn dynamic_block_data_is_reported() {
    let mut s = session();
    let list = s.execute("blocks.list", &json!({})).unwrap();
    let flags: Vec<_> = list["blocks"].as_array().unwrap().iter().map(|b| (b["name"].as_str().unwrap().to_string(), b["dynamic"].clone())).collect();
    assert!(flags.contains(&("Door".into(), json!(true))), "{flags:?}");

    let m = s.execute("blocks.dynamic", &json!({ "name": "DOOR" })).unwrap();
    let p = &m["parameters"][0];
    assert_eq!((p["node"].clone(), p["element"].clone(), p["showProperties"].clone()), (json!(1), json!("Visibility State"), json!(true)));
    assert_eq!(p["kind"]["type"], "visibility");
    assert_eq!(p["kind"]["label"], "Visibility1");
    assert_eq!(p["kind"]["at"], json!({ "x": 2.0, "y": 3.0, "z": 0.0 }));
    let states: Vec<_> =
        p["kind"]["states"].as_array().unwrap().iter().map(|st| (st["name"].clone(), st["visible"].as_array().unwrap().len())).collect();
    assert_eq!(states, [(json!("Open"), 1), (json!("Closed"), 0)]);
    assert!(p["kind"]["controlled"][0].is_string(), "handles are hex strings");
    assert_eq!(m["actions"], json!([]));
}

#[test]
fn blocks_without_dynamic_data_and_bad_names() {
    let mut s = session();
    let l = s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Window", "base": [0, 0], "handles": [l] })).unwrap();
    assert_eq!(s.execute("blocks.dynamic", &json!({ "name": "Window" })).unwrap(), json!(null));
    assert_eq!(s.execute("blocks.dynamic", &json!({ "name": "*Model_Space" })).unwrap(), json!(null));
    for p in [json!({}), json!({ "name": 3 }), json!({ "name": "Nope" }), json!(null)] {
        assert!(s.execute("blocks.dynamic", &p).is_err(), "{p}");
    }
}

#[test]
fn a_reference_to_a_dynamic_block_reports_its_custom_properties() {
    let mut s = session();
    // BLOCK made the line a reference to Door, which has Door's own values.
    let insert = s.doc().unwrap().model.handles()[0].hex();
    let r = s.execute("blocks.reference", &json!({ "handle": insert })).unwrap();
    assert_eq!(r["definition"], "Door");
    assert_eq!(r["values"], json!({}));
    assert_eq!(r["properties"], json!([{ "name": "Visibility1", "value": "Open", "choices": ["Open", "Closed"], "editable": true }]));

    // A plain block's reference, objects that aren't references and bad parameters.
    let l = s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].clone();
    let plain = s.execute("block", &json!({ "name": "Window", "base": [0, 0], "handles": [l.clone()] })).unwrap()["insert"].clone();
    assert_eq!(s.execute("blocks.reference", &json!({ "handle": plain })).unwrap(), json!(null));
    let l = s.execute("line", &json!({ "points": [[0, 0], [2, 2]] })).unwrap()["handles"][0].clone();
    assert!(s.execute("blocks.reference", &json!({ "handle": l })).is_err());
    for p in [json!({}), json!({ "handle": "zz" }), json!({ "handle": "FFFFFF" }), json!(null)] {
        assert!(s.execute("blocks.reference", &p).is_err(), "{p}");
    }
}

#[test]
fn a_visibility_state_is_changed_in_one_undo_step() {
    let mut s = session();
    let insert = s.doc().unwrap().model.handles()[0].hex();
    let r = s.execute("blocks.reference.set", &json!({ "handle": insert, "name": "Visibility1", "value": "Closed" })).unwrap();
    assert_eq!(r["properties"][0]["value"], "Closed");
    let block = |s: &Session| match s.doc().unwrap().model.iter().next().map(|e| e.kind.clone()) {
        Some(cadcraft_engine::doc::EntityKind::Insert(i)) => i.block,
        other => panic!("{other:?}"),
    };
    assert!(block(&s).starts_with("*U"));
    // Still a reference to Door, in its new state.
    let r = s.execute("blocks.reference", &json!({ "handle": insert })).unwrap();
    assert_eq!((r["definition"].clone(), r["properties"][0]["value"].clone()), (json!("Door"), json!("Closed")));

    s.execute("undo", &json!({})).unwrap();
    assert_eq!(block(&s), "Door");
    let r = s.execute("blocks.reference", &json!({ "handle": insert })).unwrap();
    assert_eq!(r["properties"][0]["value"], "Open");

    for p in [
        json!({ "handle": insert, "name": "Visibility1", "value": "Ajar" }),
        json!({ "handle": insert, "name": "Visibility1" }),
        json!({ "name": "Visibility1", "value": "Open" }),
        json!({ "handle": insert, "value": "Open" }),
    ] {
        assert!(s.execute("blocks.reference.set", &p).is_err(), "{p}");
    }
}
