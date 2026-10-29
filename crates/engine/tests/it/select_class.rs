//! The CLass keyword at "Select objects": every object of a class (object type), by its DXF
//! name, its Properties name or its ObjectARX-style name, with wildcards; `?` lists the classes in
//! the drawing. Hidden objects are skipped and hostile class names select nothing.

use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::select::{classes_in, select_class};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hx(v: &Value) -> Handle {
    Handle::parse_hex(v.as_str().unwrap()).unwrap()
}

/// Two lines and a circle.
fn three(s: &mut Session) -> (Handle, Handle, Handle) {
    let a = hx(&s.execute("line", &json!({ "points": [[1, 1], [2, 1]] })).unwrap()["handles"][0]);
    let b = hx(&s.execute("line", &json!({ "points": [[1, 3], [20, 3]] })).unwrap()["handles"][0]);
    let c = hx(&s.execute("circle", &json!({ "center": [30, 30], "radius": 1 })).unwrap()["handle"]);
    (a, b, c)
}

#[test]
fn class_keyword_selects_by_object_type() {
    let mut s = Session::new();
    let (a, b, c) = three(&mut s);
    s.start("erase").unwrap();
    s.input(Input::Text("CL".into())).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("class name"), "{:?}", s.current_prompt());
    // `?` lists the classes in the drawing and asks again.
    s.input(Input::Text("?".into())).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("class name"));
    s.input(Input::Text("line".into())).unwrap();
    s.input(Input::Enter).unwrap();
    let d = s.doc().unwrap();
    assert!(d.entity(a).is_none() && d.entity(b).is_none());
    assert!(d.entity(c).is_some());
    // The ObjectARX-style name and wildcards work too, and `_CLASS` as scripts write it.
    s.start("select").unwrap();
    s.input(Input::Text("_class".into())).unwrap();
    s.input(Input::Text("AcDbCirc*".into())).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.selection(), vec![c]);
    // An unknown class selects nothing and keeps the command going.
    s.start("select").unwrap();
    s.input(Input::Text("CLass".into())).unwrap();
    s.input(Input::Text("NOSUCHCLASS".into())).unwrap();
    assert!(s.running.is_some());
    s.cancel();
}

#[test]
fn class_names_and_the_question_mark_list() {
    let mut s = Session::new();
    let (a, b, c) = three(&mut s);
    let d = s.doc().unwrap();
    let mut lines = select_class(d, &Space::Model, "LINE");
    lines.sort();
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(lines, want);
    // Properties name, any case.
    assert_eq!(select_class(d, &Space::Model, "circle"), vec![c]);
    assert_eq!(select_class(d, &Space::Model, " AcDbLine "), lines);
    assert_eq!(select_class(d, &Space::Model, "*").len(), 3);
    assert_eq!(classes_in(d, &Space::Model), vec![("CIRCLE".to_string(), 1), ("LINE".to_string(), 2)]);
    // Paper space has none of them.
    let paper = s.doc().unwrap().layouts.first().map(|l| Space::Paper(l.name.clone()));
    if let Some(p) = paper {
        assert!(select_class(d, &p, "*").is_empty());
        assert!(classes_in(d, &p).is_empty());
    }
}

#[test]
fn hidden_objects_escape_class_and_hostile_class_names_select_nothing() {
    let mut s = Session::new();
    let (a, b, _) = three(&mut s);
    s.execute("hideobjects", &json!({ "handles": [a.hex()] })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(select_class(d, &Space::Model, "LINE"), vec![b]);
    let classes = classes_in(d, &Space::Model);
    assert!(classes.contains(&("LINE".to_string(), 1)), "{classes:?}");
    // Hostile names: empty, huge, wildcard storms, control characters.
    for pat in [String::new(), "*".repeat(100_000), "*?".repeat(5_000) + "x", "\u{0}\u{7}\n".into(), "#".repeat(9)] {
        let hs = select_class(d, &Space::Model, &pat);
        assert!(!hs.contains(&a), "{} chars", pat.len());
    }
    // Typed at the prompt too: never a panic, the command keeps asking.
    s.start("select").unwrap();
    for t in ["CL", "\u{0}", "CL", &"*?".repeat(20_000), "CL", ""] {
        s.input(Input::Text(t.to_string())).unwrap();
    }
    s.cancel();
}

#[test]
fn stretch_does_not_take_class() {
    let mut s = Session::new();
    three(&mut s);
    s.start("stretch").unwrap();
    s.input(Input::Text("CL".into())).unwrap();
    // STRETCH selects by crossing window or polygon: CL is not one of its modes.
    assert!(!s.current_prompt().unwrap().message.contains("class name"), "{:?}", s.current_prompt());
    s.cancel();
}
