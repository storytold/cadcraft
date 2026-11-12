//! JOIN with a single arc or elliptical arc offers cLose, which makes it the full circle or
//! ellipse (#1175).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn h(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn kind(s: &Session, h: &str) -> EntityKind {
    s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind.clone()
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

#[test]
fn join_close_turns_a_single_arc_into_its_circle() {
    let mut s = Session::new();
    s.script("ARC C 50,50 80,50 50,80\nJOIN 80,50\n\n").unwrap();
    let p = s.current_prompt().expect("JOIN asks what to do with the single arc");
    assert!(p.keywords.iter().any(|k| k == "cLose"), "{p:?}");
    s.cmdline("L").unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(count(&s), 1);
    match &s.doc().unwrap().model.iter().next().unwrap().kind {
        EntityKind::Circle(c) => {
            assert!(c.center.xy().near(cadcraft_engine::geom::Vec2::new(50.0, 50.0), 1e-9), "{c:?}");
            assert!((c.radius - 30.0).abs() < 1e-9, "{c:?}");
        }
        k => panic!("not a circle: {k:?}"),
    }
    // Undo brings the arc back.
    s.execute("undo", &json!({})).unwrap();
    assert!(matches!(s.doc().unwrap().model.iter().next().unwrap().kind, EntityKind::Arc(_)));
}

#[test]
fn join_close_turns_a_single_elliptical_arc_into_its_ellipse() {
    let mut s = Session::new();
    let a = h(&s.execute("ellipse", &json!({ "center": [0, 0], "major": [4, 0], "ratio": 0.5, "start": 0, "end": 90 })).unwrap());
    s.script("JOIN 4,0\n\n").unwrap();
    let p = s.current_prompt().expect("JOIN asks what to do with the single elliptical arc");
    assert_eq!(p.display(), "Select elliptical arcs to join to source or [cLose]:");
    s.cmdline("cLose").unwrap();
    assert_eq!(count(&s), 1);
    match kind(&s, &a) {
        EntityKind::Ellipse(e) => {
            assert!(e.start.abs() < 1e-9 && (e.end - std::f64::consts::TAU).abs() < 1e-9, "{e:?}");
            assert!((e.ratio - 0.5).abs() < 1e-12 && (e.major.x - 4.0).abs() < 1e-12, "{e:?}");
        }
        k => panic!("not an ellipse: {k:?}"),
    }
}

#[test]
fn join_close_by_json_and_its_refusals() {
    let mut s = Session::new();
    let arc = h(&s.execute("arc", &json!({ "center": [50, 50], "radius": 30, "start": 0, "end": 90 })).unwrap());
    let r = s.execute("join", &json!({ "handles": [arc], "close": true })).unwrap();
    assert_eq!(h(&r), arc, "the arc keeps its handle");
    match kind(&s, &arc) {
        EntityKind::Circle(c) => assert!((c.radius - 30.0).abs() < 1e-9 && c.center.xy().near(cadcraft_engine::geom::Vec2::new(50.0, 50.0), 1e-9)),
        k => panic!("not a circle: {k:?}"),
    }
    // Close is for one arc or elliptical arc: a line, a circle or two objects are refused,
    // the drawing unchanged.
    let line = h(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let other = h(&s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap());
    let before = format!("{:?}", s.doc().unwrap().model.iter().map(|e| e.kind.clone()).collect::<Vec<_>>());
    for hs in [json!([line]), json!([arc]), json!([other, line])] {
        assert!(s.execute("join", &json!({ "handles": hs, "close": true })).is_err(), "{hs}");
    }
    assert_eq!(format!("{:?}", s.doc().unwrap().model.iter().map(|e| e.kind.clone()).collect::<Vec<_>>()), before);
}

#[test]
fn join_close_prompt_takes_more_arcs_and_is_only_for_arcs() {
    // Arcs picked at the prompt join to the source as they would from the selection.
    let mut s = Session::new();
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 90, "end": 180 })).unwrap();
    s.script("JOIN 5,0\n\n").unwrap();
    s.cmdline("-5,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(count(&s), 1);
    // A single line has nothing to close: the command ends with the usual refusal.
    let mut s = Session::new();
    s.script("LINE 0,0 10,0\n\nJOIN 5,0\n\n").unwrap();
    assert!(s.current_prompt().is_none());
    assert!(s.log.iter().any(|l| l == "Select at least two objects to join."), "{:?}", s.log);
}
