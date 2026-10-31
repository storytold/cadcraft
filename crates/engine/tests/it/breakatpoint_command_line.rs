//! BREAKATPOINT at the command line and from a script, with AutoCAD 2024's prompts ("Select
//! object:", then "Specify break point:"); BREAK and BREAKATPOINT end the command when a circle
//! can't be broken at one point, and refuse an object on a locked layer when it is picked. Measured
//! in AutoCAD 2024 (accoreconsole) on these drawings.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

/// The model space objects in order: handle and kind.
fn objects(s: &Session) -> Vec<(Handle, EntityKind)> {
    s.doc().unwrap().model.iter().map(|e| (e.handle, e.kind.clone())).collect()
}

fn line_ends(k: &EntityKind) -> ((f64, f64), (f64, f64)) {
    let EntityKind::Line(l) = k else { panic!("not a line: {k:?}") };
    ((l.a.x, l.a.y), (l.b.x, l.b.y))
}

#[test]
fn breakatpoint_asks_for_the_object_then_the_point_and_splits_the_line_there() {
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({ "points": [[1000, 0], [1010, 0]] })).unwrap());
    s.cmdline("BREAKATPOINT").unwrap();
    assert!(s.prompt_text().ends_with("Select object:"), "{}", s.prompt_text());
    s.cmdline("1002,0").unwrap();
    assert!(s.prompt_text().ends_with("Specify break point:"), "{}", s.prompt_text());
    // Off the line: the point projects onto it.
    s.cmdline("1005,3").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let objs = objects(&s);
    assert_eq!(objs.len(), 2);
    assert_eq!(objs[0].0, l);
    assert_eq!(line_ends(&objs[0].1), ((1000.0, 0.0), (1005.0, 0.0)));
    assert_eq!(line_ends(&objs[1].1), ((1005.0, 0.0), (1010.0, 0.0)));
}

#[test]
fn breakatpoint_from_a_script_splits_an_arc_and_a_closed_polyline() {
    let mut s = Session::new();
    let a = handle(&s.execute("arc", &json!({ "center": [700, 0], "radius": 10, "start": 0, "end": 90 })).unwrap());
    let p = handle(&s.execute("pline", &json!({ "vertices": [[250, 0], [270, 0], [270, 20], [250, 20]], "closed": true })).unwrap());
    s.script("BREAKATPOINT 707.0710678,7.0710678 707.0710678,7.0710678\nBREAKATPOINT 260,0 260,0\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let objs = objects(&s);
    assert_eq!(objs.iter().map(|o| o.0).take(2).collect::<Vec<_>>(), [a, p]);
    let EntityKind::Arc(arc) = &objs[0].1 else { panic!("not an arc") };
    assert!((arc.start - 0.0).abs() < 1e-9 && (arc.end.to_degrees() - 45.0).abs() < 1e-6, "{arc:?}");
    let EntityKind::Arc(rest) = &objs[2].1 else { panic!("not an arc") };
    assert!((rest.start.to_degrees() - 45.0).abs() < 1e-6 && (rest.end.to_degrees() - 90.0).abs() < 1e-9, "{rest:?}");
    let pts = |k: &EntityKind| match k {
        EntityKind::LwPolyline(l) => (l.closed, l.vertices.iter().map(|v| (v.p.x, v.p.y)).collect::<Vec<_>>()),
        k => panic!("not a polyline: {k:?}"),
    };
    assert_eq!(pts(&objs[1].1), (false, vec![(250.0, 0.0), (260.0, 0.0)]));
    assert_eq!(pts(&objs[3].1), (false, vec![(260.0, 0.0), (270.0, 0.0), (270.0, 20.0), (250.0, 20.0), (250.0, 0.0)]));
}

#[test]
fn breakatpoint_and_break_at_one_point_of_a_circle_report_it_and_end() {
    for script in ["BREAKATPOINT 1300,10 1300,10\n", "BREAK 1300,10 1300,10\n"] {
        let mut s = Session::new();
        let c = handle(&s.execute("circle", &json!({ "center": [1300, 0], "radius": 10 })).unwrap());
        s.script(script).unwrap();
        assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
        assert!(s.log.iter().any(|l| l == "Arc cannot be full 360 degrees"), "{script}: {:?}", s.log);
        let objs = objects(&s);
        assert_eq!(objs.len(), 1);
        assert!(objs[0].0 == c && matches!(objs[0].1, EntityKind::Circle(_)), "{objs:?}");
    }
}

#[test]
fn enter_ends_breakatpoint_and_leaves_nothing_selected() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.script("BREAKATPOINT\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    // Enter at the break point after picking: the line is not left selected.
    s.script("BREAKATPOINT 5,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.selection().is_empty());
    assert_eq!(objects(&s).len(), 1);
}

#[test]
fn an_object_on_a_locked_layer_is_refused_when_picked() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("layer.set", &json!({ "name": "0", "locked": true })).unwrap();
    for cmd in ["BREAKATPOINT", "BREAK"] {
        s.cmdline(cmd).unwrap();
        s.cmdline("5,0").unwrap();
        assert!(s.prompt_text().ends_with("Select object:"), "{cmd}: {}", s.prompt_text());
        assert!(s.log.iter().any(|l| l == "The object is on a locked layer."), "{cmd}: {:?}", s.log);
        s.cmdline("").unwrap();
        assert!(s.running.is_none(), "{cmd}: {}", s.prompt_text());
    }
    assert_eq!(objects(&s).len(), 1);
}

#[test]
fn breakatpoint_from_lisp_with_a_bare_entity_name() {
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({ "points": [[1000, 0], [1010, 0]] })).unwrap());
    s.cmdline("(command \"_.BREAKATPOINT\" (entlast) '(1004.0 0.0))").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let objs = objects(&s);
    assert_eq!(objs.len(), 2);
    assert_eq!(objs[0].0, l);
    assert_eq!(line_ends(&objs[0].1), ((1000.0, 0.0), (1004.0, 0.0)));
}

#[test]
fn breakatpoint_from_lisp_with_an_entity_and_point() {
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({ "points": [[1000, 0], [1010, 0]] })).unwrap());
    s.cmdline("(command \"_.BREAKATPOINT\" (list (entlast) '(1005.0 0.0)) '(1005.0 0.0))").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let objs = objects(&s);
    assert_eq!(objs.len(), 2);
    assert_eq!(objs[0].0, l);
    assert_eq!(line_ends(&objs[0].1), ((1000.0, 0.0), (1005.0, 0.0)));
}
