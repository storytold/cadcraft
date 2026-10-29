//! BATTMAN switches an attribute definition's Verify, Preset and Lock position modes and makes it
//! multiline or single-line again (gap F17); existing references follow (ATTSYNC), and what it
//! makes survives DXF and DWG.

use cadcraft_engine::doc::{Attrib, Drawing, EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

/// Block T with the definitions A, B and C, and one reference with C = "x".
fn session() -> (Session, Handle) {
    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let mut hs = vec![l["handles"][0].clone()];
    for (tag, y) in [("A", 2), ("B", 4), ("C", 6)] {
        hs.push(s.execute("attdef", &json!({ "tag": tag, "prompt": tag, "default": "d", "at": [0, y] })).unwrap()["handle"].clone());
    }
    s.execute("block", &json!({ "name": "T", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
    let ins = h(&s.execute("insert", &json!({ "name": "T", "at": [100, 0], "attribs": { "C": "x" } })).unwrap());
    (s, ins)
}

fn row(r: &Value, tag: &str) -> Value {
    r["attributes"].as_array().unwrap().iter().find(|a| a["tag"] == tag).unwrap().clone()
}

fn definition(d: &Drawing, tag: &str) -> Attrib {
    d.block("T")
        .unwrap()
        .entities
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::AttDef(a) if a.tag == tag => Some(a.clone()),
            _ => None,
        })
        .unwrap()
}

fn reference_attrib(d: &Drawing, ins: Handle, tag: &str) -> Attrib {
    let EntityKind::Insert(i) = &d.entity(ins).unwrap().kind else { panic!("not a reference") };
    i.attribs.iter().find(|a| a.tag == tag).unwrap().clone()
}

#[test]
fn modes_are_listed_and_switched() {
    let (mut s, ins) = session();
    let r = s.execute("battman", &json!({ "name": "T" })).unwrap();
    let a = row(&r, "A");
    assert_eq!(
        (a["verify"].as_bool(), a["preset"].as_bool(), a["lockPosition"].as_bool(), a["multiline"].as_bool()),
        (Some(false), Some(false), Some(false), Some(false))
    );
    let r =
        s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "a", "verify": true, "preset": true, "lockPosition": true })).unwrap();
    let a = row(&r, "A");
    assert_eq!((a["verify"].as_bool(), a["preset"].as_bool(), a["lockPosition"].as_bool()), (Some(true), Some(true), Some(true)));
    let d = s.doc().unwrap();
    assert!(definition(d, "A").modes.verify && definition(d, "A").modes.preset);
    // The reference takes Lock position only, as AutoCAD keeps it.
    let ra = reference_attrib(d, ins, "A");
    assert!(ra.modes.lock_position && !ra.modes.verify && !ra.modes.preset);
    // Off again.
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "A", "lockPosition": false, "preset": false })).unwrap();
    let ra = reference_attrib(s.doc().unwrap(), ins, "A");
    assert!(!ra.modes.lock_position);
    assert!(definition(s.doc().unwrap(), "A").modes.verify);
}

#[test]
fn multiple_lines_on_and_off() {
    let (mut s, ins) = session();
    let r =
        s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": 12, "default": "one\ntwo" })).unwrap();
    assert_eq!(row(&r, "C")["multiline"], true);
    let d = s.doc().unwrap();
    let c = definition(d, "C");
    let ml = c.multiline.as_ref().expect("a multiline definition");
    assert_eq!(ml.mtext.width, 12.0);
    assert_eq!(c.multiline_text().unwrap().contents, "one\\Ptwo");
    // The reference follows and keeps its value.
    let rc = reference_attrib(d, ins, "C");
    assert!(rc.multiline.is_some());
    assert_eq!(rc.text.value, "x");
    assert_eq!(rc.multiline_text().unwrap().contents, "x");
    // A new width; the height reaches the multiline text too.
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": 30, "height": 0.5 })).unwrap();
    let c = definition(s.doc().unwrap(), "C");
    let ml = c.multiline.as_ref().unwrap();
    assert_eq!((ml.mtext.width, ml.mtext.height, c.text.height), (30.0, 0.5, 0.5));
    // No width keeps it; an explicit 0 removes the wrapping.
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "prompt": "Room" })).unwrap();
    assert_eq!(definition(s.doc().unwrap(), "C").multiline.unwrap().mtext.width, 30.0, "omitted width kept");
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": null })).unwrap();
    assert_eq!(definition(s.doc().unwrap(), "C").multiline.unwrap().mtext.width, 30.0, "null width kept");
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": 0 })).unwrap();
    assert_eq!(definition(s.doc().unwrap(), "C").multiline.unwrap().mtext.width, 0.0, "explicit 0: no wrapping");

    // Single-line again: the lines joined by spaces, in the definition and the references.
    s.execute("attedit", &json!({ "handle": ins.hex(), "values": { "C": "first\nsecond" } })).unwrap();
    let r = s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": false })).unwrap();
    assert_eq!(row(&r, "C")["multiline"], false);
    let d = s.doc().unwrap();
    let c = definition(d, "C");
    assert!(c.multiline.is_none());
    assert_eq!(c.text.value, "one two");
    let rc = reference_attrib(d, ins, "C");
    assert!(rc.multiline.is_none());
    assert_eq!(rc.text.value, "first second");
}

#[test]
fn what_battman_makes_survives_dxf_and_dwg() {
    let (mut s, ins) = session();
    s.execute(
        "battman",
        &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": 12, "default": "one\ntwo", "verify": true, "lockPosition": true }),
    )
    .unwrap();
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "B", "preset": true })).unwrap();
    let d = s.doc().unwrap();
    // As the app saves them (AutoCAD 2018 here): multiline attributes need 2007 or later.
    let dxf = String::from_utf8(cadcraft_io::write(d, "x.dxf").unwrap()).unwrap();
    let dwg = cadcraft_io::write(d, "x.dwg").unwrap();
    for (name, r) in [("DXF", cadcraft_io::read_dxf(dxf.as_bytes()).unwrap()), ("DWG", cadcraft_io::read(&dwg, "x.dwg").unwrap())] {
        let c = definition(&r, "C");
        assert!(c.modes.verify && c.modes.lock_position, "{name}: {:?}", c.modes);
        let ml = c.multiline.as_ref().unwrap_or_else(|| panic!("{name}: C multiline"));
        assert!((ml.mtext.width - 12.0).abs() < 1e-9, "{name}: width {}", ml.mtext.width);
        assert_eq!(c.text.value.replace("\r\n", "\n"), "one\ntwo", "{name}");
        assert!(definition(&r, "B").modes.preset, "{name}: B preset");
        let EntityKind::Insert(i) = &r.model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().kind else { panic!() };
        let rc = i.attribs.iter().find(|a| a.tag == "C").unwrap();
        assert!(rc.multiline.is_some() && rc.modes.lock_position, "{name}: reference C");
        assert_eq!(rc.text.value, "x", "{name}");
    }
    let _ = ins;
}

#[test]
fn typed_battman_asks_for_the_new_modes() {
    let (mut s, _) = session();
    s.start("battman").unwrap();
    s.cmdline("T").unwrap();
    s.cmdline("e").unwrap();
    s.cmdline("A").unwrap();
    for (kw, answer) in [("Verify", "y"), ("pReset", "y"), ("locK position", "y"), ("Multiple lines", "y")] {
        s.input(Input::Keyword(kw.into())).unwrap();
        s.cmdline(answer).unwrap();
    }
    s.input(Input::Keyword("eXit".into())).unwrap();
    s.input(Input::Keyword("eXit".into())).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let a = row(&s.execute("battman", &json!({ "name": "T" })).unwrap(), "A");
    assert_eq!(
        (a["verify"].as_bool(), a["preset"].as_bool(), a["lockPosition"].as_bool(), a["multiline"].as_bool()),
        (Some(true), Some(true), Some(true), Some(true))
    );
}

#[test]
fn hostile_params_never_panic() {
    let (mut s, _) = session();
    let before = definition(s.doc().unwrap(), "C");
    // A negative width, or one that is not a number, is refused (it used to become 0).
    for bad in [json!(-3), json!(-1e308), json!("12"), json!("wide"), json!(true), json!([12])] {
        assert!(s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": bad })).is_err(), "{bad}");
    }
    assert_eq!(definition(s.doc().unwrap(), "C"), before, "a refused edit changes nothing");
    // Values of the wrong type are ignored, as for the other modes.
    for odd in [json!("yes"), json!(1), json!(null), json!([true])] {
        s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": odd, "verify": odd, "width": "wide" })).unwrap();
    }
    assert_eq!(definition(s.doc().unwrap(), "C"), before);
    // A huge width is kept as given; the drawing still saves.
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "C", "multiline": true, "width": 1e300 })).unwrap();
    let _ = cadcraft_io::write_dxf(s.doc().unwrap());
    // Switching a definition that is already single-line off again changes nothing.
    s.execute("battman", &json!({ "name": "T", "action": "edit", "tag": "A", "multiline": false })).unwrap();
    assert!(definition(s.doc().unwrap(), "A").multiline.is_none());
}
