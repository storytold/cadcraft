//! #1658: `attdef` takes a justification and a text style, as `text` and `mtext` do, so a
//! title-block attribute can be built centred or right-aligned in its style without an `entmod`.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Attrib, EntityKind, HAlign, Handle, VAlign};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    v["handle"].as_str().and_then(Handle::parse_hex).unwrap()
}

fn attdef(s: &Session, h: Handle) -> Attrib {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::AttDef(a) => a.clone(),
        k => panic!("{k:?}"),
    }
}

/// Runs `attdef` with `p` and returns the definition it made.
fn add(s: &mut Session, p: Value) -> Attrib {
    let made = h(&s.execute("attdef", &p).unwrap());
    attdef(s, made)
}

/// The issue's drawing: a text style ROMANS beside the current Standard.
fn session() -> Session {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "ROMANS", "font": "romans.shx"})).unwrap();
    s.execute("style.current", &json!({"name": "Standard"})).unwrap();
    s
}

#[test]
fn justify_and_style_are_applied() {
    let mut s = session();
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10], "height": 2.5, "justify": "MC", "style": "ROMANS"}));
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(a.text.style, "ROMANS");
    assert_eq!(a.text.align_pt, Some(Vec3::new(10.0, 10.0, 0.0)), "justified text is placed by its alignment point");
    // Every code `text` takes, and the style name in any case (kept as the table spells it).
    for (j, want) in [
        ("BR", (HAlign::Right, VAlign::Bottom)),
        ("tl", (HAlign::Left, VAlign::Top)),
        ("C", (HAlign::Center, VAlign::Baseline)),
        ("M", (HAlign::Middle, VAlign::Baseline)),
    ] {
        let a = add(&mut s, json!({"tag": "T", "at": [0, 0], "justify": j, "style": "romans"}));
        assert_eq!((a.text.halign, a.text.valign), want, "{j}");
        assert_eq!(a.text.style, "ROMANS");
    }
}

#[test]
fn without_them_nothing_changes() {
    let mut s = session();
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10]}));
    assert_eq!((a.text.halign, a.text.valign, a.text.align_pt), (HAlign::Left, VAlign::Baseline, None));
    assert_eq!(a.text.style, "Standard", "the current text style");
    // With another current style, that one: not always Standard.
    s.execute("style.current", &json!({"name": "ROMANS"})).unwrap();
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10]}));
    assert_eq!(a.text.style, "ROMANS", "the current text style");
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10], "justify": "L"}));
    assert_eq!((a.text.halign, a.text.valign, a.text.align_pt), (HAlign::Left, VAlign::Baseline, None));
    assert_eq!(a.text.style, "ROMANS", "the current text style");
}

#[test]
fn a_bad_justification_or_style_is_refused() {
    let mut s = session();
    let n = s.doc().unwrap().model.len();
    assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "justify": "XX"})).is_err());
    assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "style": "NOPE"})).is_err());
    // Align and Fit need a second baseline point, which attdef has no way to take.
    for j in ["A", "ALIGN", "F", "fit"] {
        assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "justify": j})).is_err(), "{j}");
    }
    assert_eq!(s.doc().unwrap().model.len(), n, "nothing added");
}

/// The issue's use: a block built from the definition gives its attributes the same placement,
/// and a save and reopen keeps them.
#[test]
fn a_block_and_a_saved_file_keep_them() {
    let mut s = session();
    let ad =
        h(&s.execute("attdef", &json!({"tag": "TITLE", "default": "x", "at": [50, 10], "height": 2.5, "justify": "MC", "style": "ROMANS"})).unwrap());
    let l = s.execute("line", &json!({"points": [[0, 0], [100, 0]]})).unwrap()["handles"][0].as_str().and_then(Handle::parse_hex).unwrap();
    s.execute("block", &json!({"name": "TB", "base": [0, 0], "handles": [ad.hex(), l.hex()], "keep": "delete"})).unwrap();
    let ins = h(&s.execute("insert", &json!({"name": "TB", "at": [0, 0], "attribs": {"TITLE": "PLAN"}})).unwrap());
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(ins).unwrap().kind else { panic!() };
    let att = i.attribs.first().unwrap();
    assert_eq!(
        (att.text.halign, att.text.valign, att.text.style.as_str(), att.text.value.as_str()),
        (HAlign::Center, VAlign::Middle, "ROMANS", "PLAN")
    );
    let read = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let def =
        read.block("TB").unwrap().entities.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).unwrap();
    assert_eq!((def.text.halign, def.text.valign, def.text.style.as_str()), (HAlign::Center, VAlign::Middle, "ROMANS"), "reopened definition");
    let att = read.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { i.attribs.first().cloned() } else { None }).unwrap();
    assert_eq!((att.text.halign, att.text.valign, att.text.style.as_str()), (HAlign::Center, VAlign::Middle, "ROMANS"), "reopened attribute");
}
