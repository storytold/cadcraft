//! BREAK, BREAKATPOINT and JOIN leave objects on locked layers alone (#727).

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn h(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

/// Everything in the drawing a locked-layer edit could disturb.
fn snapshot(s: &Session) -> String {
    let d = s.doc().unwrap();
    format!("{:?}", d.model.iter().map(|e| (e.handle, e.common.layer.clone(), e.kind.clone())).collect::<Vec<_>>())
}

fn set_lock(s: &mut Session, locked: bool) {
    s.execute("layer.set", &json!({"name": "0", "locked": locked})).unwrap();
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    h(&s.execute("line", &json!({"points": [a, b]})).unwrap())
}

fn lines_ab(s: &mut Session) -> (String, String) {
    (line(s, [0.0, 0.0], [10.0, 0.0]), line(s, [10.0, 0.0], [20.0, 0.0]))
}

#[test]
fn break_and_breakatpoint_refuse_a_locked_line() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let control = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    set_lock(&mut s, true);
    let before = snapshot(&s);
    assert!(s.execute("break", &json!({"handle": a, "p1": [3, 0], "p2": [7, 0]})).is_err());
    assert!(s.execute("breakatpoint", &json!({"handle": a, "at": [5, 0]})).is_err());
    // Selection fallback reaches the same guard.
    s.execute("select", &json!({"handles": [a]})).unwrap();
    assert!(s.execute("break", &json!({"p1": [3, 0], "p2": [7, 0]})).is_err());
    assert!(s.execute("breakatpoint", &json!({"at": [5, 0]})).is_err());
    assert_eq!(snapshot(&s), before);
    // Unlocked, the same edits work, and undo and redo them.
    set_lock(&mut s, false);
    let unlocked = snapshot(&s);
    let r = s.execute("break", &json!({"handle": a, "p1": [3, 0], "p2": [7, 0]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    assert!(s.doc().unwrap().entity(cadcraft_engine::doc::Handle::parse_hex(&a).unwrap()).is_none());
    assert!(s.doc().unwrap().entity(cadcraft_engine::doc::Handle::parse_hex(&control).unwrap()).is_some());
    let broken = snapshot(&s);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(snapshot(&s), unlocked);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(snapshot(&s), broken);
}

#[test]
fn break_refuses_a_locked_arc_and_circle() {
    let mut s = Session::new();
    let arc = h(&s.execute("arc", &json!({"center": [0, 0], "radius": 5, "start": 0, "end": 90})).unwrap());
    let circle = h(&s.execute("circle", &json!({"center": [20, 0], "radius": 3})).unwrap());
    set_lock(&mut s, true);
    let before = snapshot(&s);
    assert!(s.execute("break", &json!({"handle": arc, "p1": [5, 0], "p2": [0, 5]})).is_err());
    assert!(s.execute("break", &json!({"handle": circle, "p1": [23, 0], "p2": [20, 3]})).is_err());
    assert_eq!(snapshot(&s), before);
}

#[test]
fn join_refuses_when_any_line_is_locked() {
    let mut s = Session::new();
    let (a, b) = lines_ab(&mut s);
    set_lock(&mut s, true);
    let before = snapshot(&s);
    assert!(s.execute("join", &json!({"handles": [a, b]})).is_err());
    s.execute("select", &json!({"handles": [a, b]})).unwrap();
    assert!(s.execute("join", &json!({})).is_err());
    assert_eq!(snapshot(&s), before);
    set_lock(&mut s, false);
    let r = s.execute("join", &json!({"handles": [a, b]})).unwrap();
    assert!(r["handle"].is_string());
    assert_eq!(s.doc().unwrap().model.len(), 1);
}

#[test]
fn join_keeps_a_locked_original_in_a_mixed_layer_pair() {
    for locked_first in [true, false] {
        let mut s = Session::new();
        s.execute("layer.new", &json!({"name": "L"})).unwrap();
        // The first line goes on the soon-locked layer 0, the second on "L" (or the reverse).
        let on_l = |s: &mut Session, a: [f64; 2], b: [f64; 2]| {
            s.execute("layer.current", &json!({"name": "L"})).unwrap();
            let r = line(s, a, b);
            s.execute("layer.current", &json!({"name": "0"})).unwrap();
            r
        };
        let (a, b) = if locked_first {
            (line(&mut s, [0.0, 0.0], [10.0, 0.0]), on_l(&mut s, [10.0, 0.0], [20.0, 0.0]))
        } else {
            (on_l(&mut s, [0.0, 0.0], [10.0, 0.0]), line(&mut s, [10.0, 0.0], [20.0, 0.0]))
        };
        let locked = if locked_first { &a } else { &b };
        set_lock(&mut s, true);
        let before = snapshot(&s);
        assert!(s.execute("join", &json!({"handles": [a, b]})).is_err());
        assert_eq!(snapshot(&s), before, "locked_first = {locked_first}");
        assert!(s.doc().unwrap().entity(cadcraft_engine::doc::Handle::parse_hex(locked).unwrap()).is_some());
    }
}

#[test]
fn interactive_break_and_join_refuse_locked_lines() {
    let mut s = Session::new();
    let (a, b) = lines_ab(&mut s);
    set_lock(&mut s, true);
    let before = snapshot(&s);
    // BREAK: pick the line, then the second break point.
    s.start("break").unwrap();
    s.input(Input::Point(Vec2::new(3.0, 0.0))).unwrap();
    let _ = s.input(Input::Point(Vec2::new(7.0, 0.0)));
    assert!(s.log.last().is_some_and(|m| m.contains("locked layer")), "{:?}", s.log.last());
    assert_eq!(snapshot(&s), before);
    // JOIN on a preselected pair reports the refusal on the command line.
    s.cancel();
    s.execute("select", &json!({"handles": [a, b]})).unwrap();
    s.start("join").unwrap();
    assert!(s.log.last().is_some_and(|m| m.contains("locked layer")), "{:?}", s.log.last());
    assert_eq!(snapshot(&s), before);
    // Unlocked, the interactive commands still edit.
    set_lock(&mut s, false);
    s.start("break").unwrap();
    s.input(Input::Point(Vec2::new(3.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(7.0, 0.0))).unwrap();
    assert_ne!(snapshot(&s), before);
}
