//! Variables that set what commands offer or do by default: PEDITACCEPT (PEDIT on a line or arc),
//! ARRAYTYPE (ARRAY's type), TSPACEFAC and TSPACETYPE (MTEXT's line spacing) and DELOBJ
//! (REGION keeps or replaces its objects). SETVAR checks their values; hostile values never panic.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::{Value, json};

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn setvar(s: &mut Session, name: &str, value: Value) -> bool {
    s.execute("setvar", &json!({ "name": name, "value": value })).is_ok()
}

/// Cancel the command and forget the selection it kept.
fn cancel(s: &mut Session) {
    s.cancel();
    s.set_selection(Vec::new());
}

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

const NEW: [&str; 5] = ["PEDITACCEPT", "ARRAYTYPE", "TSPACEFAC", "TSPACETYPE", "DELOBJ"];

#[test]
fn defaults_and_listing() {
    let mut s = Session::new();
    for (name, v) in [("PEDITACCEPT", json!(0)), ("ARRAYTYPE", json!(0)), ("TSPACEFAC", json!(1.0)), ("TSPACETYPE", json!(1)), ("DELOBJ", json!(3))] {
        assert_eq!(getvar(&mut s, name), v, "{name}");
    }
    let all = cadcraft_engine::sysvars::list(&s);
    for n in NEW {
        assert!(all.get(n).is_some(), "{n} listed by SETVAR ?");
    }
    s.cmdline("setvar ?").unwrap();
    let text = s.log.join("\n");
    assert!(NEW.iter().all(|n| text.contains(n)), "{text}");
}

#[test]
fn peditaccept_turns_a_line_into_a_polyline_without_asking() {
    // PEDITACCEPT 0: the question.
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    typed(&mut s, &["pedit", "5,0"]);
    assert!(s.prompt_text().contains("Do you want to turn it into one?"), "{}", s.prompt_text());
    s.cancel();
    // PEDITACCEPT 1: straight to the options, the line is a polyline already.
    let mut s = Session::new();
    s.cmdline("setvar peditaccept 1").unwrap();
    assert_eq!(getvar(&mut s, "PEDITACCEPT"), json!(1));
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    typed(&mut s, &["pedit", "5,0"]);
    assert!(s.prompt_text().contains("Enter an option"), "{}", s.prompt_text());
    assert!(matches!(s.doc().unwrap().model.last().unwrap().kind, EntityKind::LwPolyline(_)));
    typed(&mut s, &["w", "0.5", ""]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    // Selected first (noun-verb) too.
    let r = s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.set_selection(vec![h]);
    s.cmdline("pedit").unwrap();
    assert!(s.prompt_text().contains("Enter an option"), "{}", s.prompt_text());
    assert!(matches!(s.doc().unwrap().entity(h).unwrap().kind, EntityKind::LwPolyline(_)));
    s.cancel();
}

#[test]
fn arraytype_is_the_type_array_offers_and_the_last_one_chosen() {
    let mut s = Session::new();
    assert!(setvar(&mut s, "ARRAYTYPE", json!(2)));
    s.execute("circle", &json!({ "center": [10, 0], "radius": 1 })).unwrap();
    typed(&mut s, &["array", "l", ""]);
    assert!(s.prompt_text().contains("<POlar>"), "{}", s.prompt_text());
    // Enter takes it: the polar array asks for its center.
    typed(&mut s, &[""]);
    assert!(s.prompt_text().to_ascii_lowercase().contains("center"), "{}", s.prompt_text());
    cancel(&mut s);
    // Choosing a type makes it the next default.
    typed(&mut s, &["array", "l", "", "pa"]);
    assert_eq!(getvar(&mut s, "ARRAYTYPE"), json!(1));
    cancel(&mut s);
    typed(&mut s, &["array", "l", ""]);
    assert!(s.prompt_text().contains("<PAth>"), "{}", s.prompt_text());
    cancel(&mut s);
    typed(&mut s, &["array", "l", "", "r"]);
    assert_eq!(getvar(&mut s, "ARRAYTYPE"), json!(0));
    cancel(&mut s);
    // The JSON form keeps its documented default (rectangular).
    let r = s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    assert!(setvar(&mut s, "ARRAYTYPE", json!(2)));
    let out = s.execute("array", &json!({ "handles": [r["handle"]], "rows": 1, "cols": 2, "rowSpacing": 1, "colSpacing": 5 })).unwrap();
    assert!(out.get("created").is_some(), "{out}");
}

fn last_mtext(s: &Session) -> cadcraft_engine::doc::MText {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::MText(m) => m.clone(),
        k => panic!("expected multiline text, got {k:?}"),
    }
}

#[test]
fn tspacefac_and_tspacetype_are_mtexts_line_spacing() {
    let mut s = Session::new();
    assert!(setvar(&mut s, "TSPACEFAC", json!(1.5)));
    assert!(setvar(&mut s, "TSPACETYPE", json!(2)));
    s.script("MTEXT 0,10 20,0\nHello\n\n").unwrap();
    let m = last_mtext(&s);
    assert_eq!(m.line_spacing, 1.5);
    assert!(m.line_spacing_exact);
    // MTEXT's Line spacing answers set them for the next text.
    s.script("MTEXT 0,30 l a 2x 20,20\nWorld\n\n").unwrap();
    assert_eq!(last_mtext(&s).line_spacing, 2.0);
    assert_eq!(getvar(&mut s, "TSPACEFAC"), json!(2.0));
    assert_eq!(getvar(&mut s, "TSPACETYPE"), json!(1));
    s.script("MTEXT 0,50 20,40\nAgain\n\n").unwrap();
    let m = last_mtext(&s);
    assert_eq!(m.line_spacing, 2.0);
    assert!(!m.line_spacing_exact);
    // The JSON form keeps its documented default.
    s.execute("mtext", &json!({ "at": [0, 60], "text": "json" })).unwrap();
    assert_eq!(last_mtext(&s).line_spacing, 1.0);
}

#[test]
fn delobj_0_keeps_the_objects_a_region_is_made_from() {
    let count = |s: &Session| s.doc().unwrap().model.iter().count();
    let mut s = Session::new();
    let r = s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    s.execute("region", &json!({ "handles": [r["handle"]] })).unwrap();
    assert_eq!(count(&s), 1, "DELOBJ 3: the region replaces the circle");
    assert!(matches!(s.doc().unwrap().model.last().unwrap().kind, EntityKind::Region(_)));
    s.cmdline("setvar delobj 0").unwrap();
    let r = s.execute("circle", &json!({ "center": [20, 0], "radius": 5 })).unwrap();
    s.execute("region", &json!({ "handles": [r["handle"]] })).unwrap();
    assert_eq!(count(&s), 3, "DELOBJ 0: the circle stays");
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    assert!(matches!(s.doc().unwrap().entity(h).unwrap().kind, EntityKind::Circle(_)));
    assert!(matches!(s.doc().unwrap().model.last().unwrap().kind, EntityKind::Region(_)));
}

#[test]
fn values_are_checked() {
    let mut s = Session::new();
    for (name, v) in [
        ("PEDITACCEPT", json!(2)),
        ("ARRAYTYPE", json!(3)),
        ("ARRAYTYPE", json!(-1)),
        ("TSPACEFAC", json!(0.2)),
        ("TSPACEFAC", json!(4.5)),
        ("TSPACEFAC", json!("wide")),
        ("TSPACETYPE", json!(0)),
        ("TSPACETYPE", json!(3)),
        ("DELOBJ", json!(4)),
        ("DELOBJ", json!(-1)),
    ] {
        assert!(!setvar(&mut s, name, v.clone()), "{name} = {v} refused");
    }
    for (name, v, back) in [
        ("PEDITACCEPT", json!(true), json!(1)),
        ("ARRAYTYPE", json!("1"), json!(1)),
        ("TSPACEFAC", json!(0.25), json!(0.25)),
        ("TSPACEFAC", json!("4"), json!(4.0)),
        ("TSPACETYPE", json!(2), json!(2)),
        ("DELOBJ", json!(0), json!(0)),
    ] {
        assert!(setvar(&mut s, name, v.clone()), "{name} = {v} accepted");
        assert_eq!(getvar(&mut s, name), back, "{name}");
    }
    s.cmdline("tspacefac 2").unwrap();
    assert_eq!(getvar(&mut s, "TSPACEFAC"), json!(2.0));
}

#[test]
fn hostile_values_never_panic() {
    let mut s = Session::new();
    let hostile = [
        json!(f64::MAX),
        json!(f64::MIN),
        json!(i64::MAX),
        json!(i64::MIN),
        json!(u64::MAX),
        json!("NaN"),
        json!("inf"),
        json!("1e400"),
        json!(""),
        json!(null),
        json!([1, 2]),
        json!({ "x": 1 }),
        json!("9".repeat(400)),
    ];
    for name in NEW {
        for v in &hostile {
            let _ = setvar(&mut s, name, v.clone());
            let g = getvar(&mut s, name);
            if let Some(f) = g.as_f64() {
                assert!(f.is_finite(), "{name} = {g} after {v}");
            }
        }
    }
    // A settings file holding nonsense is read as the defaults.
    let st: cadcraft_engine::Settings = serde_json::from_value(json!({ "arraytype": 200, "tspacefac": 1e300 })).unwrap();
    assert_eq!(st.array_type(), 2);
    assert_eq!(st.line_spacing(), 1.0);
    s.settings = st;
    assert_eq!(getvar(&mut s, "TSPACEFAC"), json!(1.0));
    s.script("MTEXT 0,10 20,0\nHello\n\n").unwrap();
    assert_eq!(last_mtext(&s).line_spacing, 1.0);
}
