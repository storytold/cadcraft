//! EXTRIM (#2802): everything that crosses one cutting edge loses its part on the side picked.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

/// The handle a drawing command returns.
fn h(v: &Value) -> String {
    v["handle"].as_str().or_else(|| v["handles"][0].as_str()).unwrap().to_string()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    h(&s.execute("line", &json!({ "points": [a, b] })).unwrap())
}

fn exists(s: &Session, hex: &str) -> bool {
    s.doc().unwrap().entity(Handle::parse_hex(hex).unwrap()).is_some()
}

/// The lines of the drawing, as their end points rounded to 1e-6.
fn lines(s: &Session) -> Vec<[[i64; 2]; 2]> {
    let r = |x: f64| (x * 1e6).round() as i64;
    let mut v: Vec<[[i64; 2]; 2]> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => {
                let (a, b) = ([r(l.a.x), r(l.a.y)], [r(l.b.x), r(l.b.y)]);
                Some(if a <= b { [a, b] } else { [b, a] })
            }
            _ => None,
        })
        .collect();
    v.sort();
    v
}

/// `[[x1, y1], [x2, y2]]` in the units of [`lines`].
fn seg(a: [f64; 2], b: [f64; 2]) -> [[i64; 2]; 2] {
    let r = |x: f64| (x * 1e6).round() as i64;
    let (a, b) = ([r(a[0]), r(a[1])], [r(b[0]), r(b[1])]);
    if a <= b { [a, b] } else { [b, a] }
}

fn extrim(s: &mut Session, edge: &str, side: [f64; 2]) -> Value {
    s.execute("extrim", &json!({ "edge": edge, "side": side })).unwrap()
}

#[test]
fn trims_one_side_of_a_line() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    for x in [2.0, 5.0, 8.0] {
        line(&mut s, [x, -5.0], [x, 5.0]);
    }
    let v = extrim(&mut s, &edge, [5.0, 3.0]);
    assert_eq!(v["trimmed"], 3);
    let mut want = vec![seg([0.0, 0.0], [10.0, 0.0])];
    want.extend([2.0, 5.0, 8.0].map(|x| seg([x, -5.0], [x, 0.0])));
    want.sort();
    assert_eq!(lines(&s), want);
}

#[test]
fn trims_the_other_side_of_a_line() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [5.0, -5.0], [5.0, 5.0]);
    extrim(&mut s, &edge, [5.0, -3.0]);
    let mut want = vec![seg([0.0, 0.0], [10.0, 0.0]), seg([5.0, 0.0], [5.0, 5.0])];
    want.sort();
    assert_eq!(lines(&s), want);
}

#[test]
fn an_open_edge_trims_only_along_its_length() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    // Crosses the line the edge lies on, but past its end.
    line(&mut s, [20.0, -5.0], [20.0, 5.0]);
    let v = extrim(&mut s, &edge, [5.0, 3.0]);
    assert_eq!(v["trimmed"], 0);
    assert_eq!(lines(&s).len(), 2);
}

#[test]
fn inside_and_outside_a_circle() {
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap());
    line(&mut s, [-10.0, 0.0], [10.0, 0.0]);
    extrim(&mut s, &c, [0.0, 1.0]);
    assert_eq!(lines(&s), vec![seg([-10.0, 0.0], [-5.0, 0.0]), seg([5.0, 0.0], [10.0, 0.0])]);

    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap());
    line(&mut s, [-10.0, 0.0], [10.0, 0.0]);
    let v = extrim(&mut s, &c, [0.0, 8.0]);
    assert_eq!(v["trimmed"], 1);
    assert_eq!(lines(&s), vec![seg([-5.0, 0.0], [5.0, 0.0])]);
}

#[test]
fn inside_a_closed_polyline() {
    let mut s = Session::new();
    let r = h(&s.execute("pline", &json!({ "points": [[0, 0], [10, 0], [10, 6], [0, 6]], "closed": true })).unwrap());
    line(&mut s, [-5.0, 3.0], [15.0, 3.0]);
    line(&mut s, [5.0, -5.0], [5.0, 11.0]);
    let v = extrim(&mut s, &r, [2.0, 2.0]);
    assert_eq!(v["trimmed"], 2);
    let mut want = vec![seg([-5.0, 3.0], [0.0, 3.0]), seg([10.0, 3.0], [15.0, 3.0]), seg([5.0, -5.0], [5.0, 0.0]), seg([5.0, 6.0], [5.0, 11.0])];
    want.sort();
    assert_eq!(lines(&s), want);
}

#[test]
fn inside_an_ellipse() {
    let mut s = Session::new();
    let e = h(&s.execute("ellipse", &json!({ "center": [0, 0], "major": [10, 0], "ratio": 0.5 })).unwrap());
    line(&mut s, [-20.0, 0.0], [20.0, 0.0]);
    extrim(&mut s, &e, [0.0, 1.0]);
    // Cut where TRIM cuts at an ellipse: on its flattened outline, within 1e-4 of the axis.
    let [[a, b], [c, d]] = lines(&s)[..] else { panic!("{:?}", lines(&s)) };
    assert_eq!((a, d), ([-20_000_000, 0], [20_000_000, 0]), "the outer parts stay");
    assert!((b[0] + 10_000_000).abs() <= 2_000 && (c[0] - 10_000_000).abs() <= 2_000, "{b:?} {c:?}");
}

#[test]
fn along_a_spline() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({ "control": [[0, 0], [3, 3], [6, -3], [9, 0]] })).unwrap());
    // A vertical line through the spline's middle, where it crosses y = 0.
    line(&mut s, [4.5, -5.0], [4.5, 5.0]);
    let v = extrim(&mut s, &sp, [4.5, 4.0]);
    assert_eq!(v["trimmed"], 1);
    let [[a, b]] = lines(&s)[..] else { panic!("{:?}", lines(&s)) };
    assert_eq!(a, [4_500_000, -5_000_000], "the lower part stays");
    assert!(b[1].abs() < 10, "cut at the spline: {b:?}");
}

#[test]
fn an_object_crossing_twice_loses_the_stretch_between() {
    let mut s = Session::new();
    let edge = line(&mut s, [-10.0, 0.0], [10.0, 0.0]);
    let u = h(&s.execute("pline", &json!({ "points": [[0, -2], [0, 2], [4, 2], [4, -2]] })).unwrap());
    let v = extrim(&mut s, &edge, [2.0, 1.0]);
    assert_eq!(v["trimmed"], 1);
    assert!(!exists(&s, &u));
    let pieces = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::LwPolyline(_))).count();
    assert_eq!(pieces, 2, "the two legs below the edge stay");
}

#[test]
fn leaves_the_edge_and_objects_that_do_not_cross_it() {
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap());
    let inside = line(&mut s, [-1.0, 0.0], [1.0, 0.0]);
    let v = extrim(&mut s, &c, [0.0, 2.0]);
    assert_eq!(v["trimmed"], 0);
    assert!(exists(&s, &c));
    assert!(exists(&s, &inside), "an object wholly on the side but not crossing the edge stays");
}

#[test]
fn skips_locked_layers_blocks_and_hatches() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("layer.new", &json!({ "name": "L1" })).unwrap();
    let locked = line(&mut s, [2.0, -5.0], [2.0, 5.0]);
    s.execute("chprop", &json!({ "handles": [locked.clone()], "layer": "L1" })).unwrap();
    s.execute("layer.set", &json!({ "name": "L1", "locked": true })).unwrap();
    let b = line(&mut s, [0.0, 0.0], [0.0, 10.0]);
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [b] })).unwrap();
    s.execute("insert", &json!({ "name": "B", "at": [5, -5] })).unwrap();
    let r = h(&s.execute("rectang", &json!({ "p1": [7, -2], "p2": [9, 2] })).unwrap());
    s.execute("hatch", &json!({ "handles": [r] })).unwrap();
    let v = extrim(&mut s, &edge, [5.0, 3.0]);
    assert_eq!(v["skipped"]["locked"], 1);
    assert!(v["skipped"]["other"].as_u64().unwrap() >= 2, "{v}");
    assert!(exists(&s, &locked), "the locked line is not trimmed");
    assert!(v["message"].as_str().unwrap().contains("locked"));
}

#[test]
fn one_undo_restores_everything() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    for x in [2.0, 5.0, 8.0] {
        line(&mut s, [x, -5.0], [x, 5.0]);
    }
    let before = lines(&s);
    extrim(&mut s, &edge, [5.0, 3.0]);
    assert_ne!(lines(&s), before);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(lines(&s), before);
}

#[test]
fn never_leaves_a_piece_of_no_length() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    // Starts a hair below the edge: trimming above would leave a sliver.
    line(&mut s, [5.0, -1e-8], [5.0, 5.0]);
    extrim(&mut s, &edge, [5.0, 3.0]);
    let d = s.doc().unwrap();
    for e in d.model.iter() {
        if let EntityKind::Line(l) = &e.kind {
            assert!(l.a.xy().dist(l.b.xy()) > 1e-6, "sliver left: {l:?}");
        }
    }
}

#[test]
fn refuses_bad_input_and_changes_nothing() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [5.0, -5.0], [5.0, 5.0]);
    let t = h(&s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 0.25 })).unwrap());
    let before = lines(&s);
    for p in [
        json!({ "side": [5, 3] }),
        json!({ "edge": "zz", "side": [5, 3] }),
        json!({ "edge": 5, "side": [5, 3] }),
        json!({ "edge": "FFFFFF", "side": [5, 3] }),
        json!({ "edge": t, "side": [5, 3] }),
        json!({ "edge": edge, "side": [5, 0] }),
        json!({ "edge": edge }),
        json!({ "edge": edge, "side": ["a", 3] }),
    ] {
        assert!(s.execute("extrim", &p).is_err(), "{p}");
    }
    assert_eq!(lines(&s), before);
}

#[test]
fn refuses_an_edge_in_another_space() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    assert!(s.execute("extrim", &json!({ "edge": edge, "side": [5, 3] })).is_err());
}

#[test]
fn prompts_for_the_edge_then_the_side() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [5.0, -5.0], [5.0, 5.0]);
    s.set_selection(Vec::new());
    s.start("extrim").unwrap();
    assert!(s.prompt_text().contains("cutting edge"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![Handle::parse_hex(&edge).unwrap()])).unwrap();
    assert!(s.prompt_text().contains("side"), "{}", s.prompt_text());
    // The preview shows what goes: the upper half of the vertical line.
    let shown = s.preview(Vec2::new(5.0, 3.0));
    assert_eq!(shown.len(), 1);
    s.input(Input::Point(Vec2::new(5.0, 3.0))).unwrap();
    assert!(s.current_prompt().is_none());
    let mut want = vec![seg([0.0, 0.0], [10.0, 0.0]), seg([5.0, -5.0], [5.0, 0.0])];
    want.sort();
    assert_eq!(lines(&s), want);
}

#[test]
fn uses_a_preselected_edge() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [5.0, -5.0], [5.0, 5.0]);
    s.set_selection(vec![Handle::parse_hex(&edge).unwrap()]);
    s.start("extrim").unwrap();
    assert!(s.prompt_text().contains("side"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(5.0, -3.0))).unwrap();
    let mut want = vec![seg([0.0, 0.0], [10.0, 0.0]), seg([5.0, 0.0], [5.0, 5.0])];
    want.sort();
    assert_eq!(lines(&s), want);
}

#[test]
fn many_objects_in_one_call() {
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({ "center": [0, 0], "radius": 50 })).unwrap());
    for i in 0..500 {
        let y = -45.0 + 90.0 * f64::from(i) / 500.0;
        line(&mut s, [-100.0, y], [100.0, y]);
    }
    let v = extrim(&mut s, &c, [0.0, 0.5]);
    assert_eq!(v["trimmed"], 500);
    assert_eq!(lines(&s).len(), 1000, "two outer pieces of each line");
}

#[test]
fn refuses_more_objects_than_the_limit_and_changes_nothing() {
    let mut s = Session::new();
    let edge = line(&mut s, [0.0, 0.0], [10_002.0, 0.0]);
    // One more crossing object than one call trims.
    for i in 0..10_001 {
        let x = 0.5 + f64::from(i);
        line(&mut s, [x, -1.0], [x, 1.0]);
    }
    let before = lines(&s);
    let e = s.execute("extrim", &json!({ "edge": edge, "side": [5, 0.5] })).unwrap_err();
    assert!(e.to_string().contains("objects cross the cutting edge"), "{e}");
    assert_eq!(lines(&s), before);
}

#[test]
fn refuses_an_edge_with_too_many_points_and_changes_nothing() {
    let mut s = Session::new();
    // A zigzag polyline whose corners alone are more points than the trace may have.
    let pts: Vec<[f64; 2]> = (0..10_002).map(|i| [f64::from(i), if i % 2 == 0 { 0.0 } else { 1.0 }]).collect();
    let edge = h(&s.execute("pline", &json!({ "points": pts })).unwrap());
    line(&mut s, [5.5, -5.0], [5.5, 5.0]);
    let before = lines(&s);
    let e = s.execute("extrim", &json!({ "edge": edge, "side": [5.5, 3.0] })).unwrap_err();
    assert!(e.to_string().contains("too complex"), "{e}");
    assert_eq!(lines(&s), before);
}
