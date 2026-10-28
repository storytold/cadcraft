//! HATCH and HATCHEDIT options (#451): HPGAPTOL gap tolerance, pattern origin, separate
//! hatches, island style, linking and unlinking boundaries, recreated boundaries, -HATCHEDIT.

use cadcraft_engine::doc::{EntityKind, Handle, Hatch};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hatch(s: &Session, h: &str) -> Hatch {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn all_hatches(s: &Session) -> Vec<(String, Hatch)> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((e.handle.hex(), h.clone())) } else { None }).collect()
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

#[test]
fn gap_tolerance_closes_small_gaps() {
    let mut s = Session::new();
    for pts in [[[0.0, 0.0], [9.5, 0.0]], [[10.0, 0.0], [10.0, 10.0]], [[10.0, 10.0], [0.0, 10.0]], [[0.0, 10.0], [0.0, 0.5]]] {
        s.execute("line", &json!({ "points": pts })).unwrap();
    }
    assert!(s.execute("hatch", &json!({ "points": [[5, 5]] })).is_err(), "open without a gap tolerance");
    s.execute("setvar", &json!({ "name": "HPGAPTOL", "value": 1 })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap());
    let hh = hatch(&s, &h);
    assert_eq!(hh.loops[0].sources.len(), 4);
    // Still associative: the gaps stay within the tolerance after a move of one line.
    let right = s.doc().unwrap().model.iter().nth(1).unwrap().handle.hex();
    s.execute("move", &json!({ "handles": [right], "delta": [0.3, 0] })).unwrap();
    assert!(hatch(&s, &h).associative);
}

#[test]
fn lines_ending_on_an_ellipse_close_a_boundary() {
    // #1690: the verticals end on the true ellipse, which the boundary tracer sees as chords
    // that pass just below those ends.
    let mut s = Session::new();
    s.execute("ellipse", &json!({ "center": [0, 0], "major": [10, 0], "ratio": 0.5 })).unwrap();
    for x in [1.0f64, 2.0] {
        let y = 5.0 * (1.0 - x * x / 100.0).sqrt();
        s.execute("line", &json!({ "points": [[x, 8.0], [x, y]] })).unwrap();
    }
    let top = s.execute("line", &json!({ "points": [[0, 8], [3, 8]] })).unwrap()["handles"][0].clone();
    let h = handle(&s.execute("hatch", &json!({ "points": [[1.5, 7]] })).unwrap());
    let hh = hatch(&s, &h);
    assert_eq!(hh.loops.len(), 1);
    assert_eq!(hh.loops[0].sources.len(), 4);
    // The associative refill traces the same way: sliding the top edge keeps the hatch.
    s.execute("move", &json!({ "handles": [top], "delta": [0.5, 0] })).unwrap();
    assert!(hatch(&s, &h).associative);
}

#[test]
fn pattern_origin_options() {
    let mut s = Session::new();
    rect(&mut s, [5.0, 5.0], [15.0, 10.0]);
    let a = handle(&s.execute("hatch", &json!({ "points": [[6, 6]], "originMode": "bottomLeft" })).unwrap());
    assert_eq!(hatch(&s, &a).origin, Vec2::new(5.0, 5.0));
    let b = handle(&s.execute("hatch", &json!({ "points": [[6, 6]], "origin": [1, 2] })).unwrap());
    assert_eq!(hatch(&s, &b).origin, Vec2::new(1.0, 2.0));
    // HPORIGIN / HPORIGINMODE are the defaults.
    s.execute("setvar", &json!({ "name": "HPORIGIN", "value": [3, 4] })).unwrap();
    let c = handle(&s.execute("hatch", &json!({ "points": [[6, 6]] })).unwrap());
    assert_eq!(hatch(&s, &c).origin, Vec2::new(3.0, 4.0));
    s.execute("setvar", &json!({ "name": "HPORIGINMODE", "value": 3 })).unwrap();
    let d = handle(&s.execute("hatch", &json!({ "points": [[6, 6]] })).unwrap());
    assert_eq!(hatch(&s, &d).origin, Vec2::new(15.0, 10.0));
    s.execute("hatchedit", &json!({ "handles": [d.clone()], "originMode": "center" })).unwrap();
    assert_eq!(hatch(&s, &d).origin, Vec2::new(10.0, 7.5));
    s.execute("hatchedit", &json!({ "handles": [d.clone()], "origin": [0, 0] })).unwrap();
    assert_eq!(hatch(&s, &d).origin, Vec2::ZERO);
    assert!(s.execute("hatchedit", &json!({ "handles": [d], "originMode": "middle" })).is_err());
}

#[test]
fn separate_hatches_and_island_style() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    s.execute("circle", &json!({ "center": [5, 5], "radius": 2 })).unwrap();
    rect(&mut s, [20.0, 0.0], [30.0, 10.0]);
    let r = s.execute("hatch", &json!({ "points": [[1, 1], [21, 1]], "separate": true, "style": "outer" })).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    assert!(all_hatches(&s).iter().all(|(_, h)| h.style == 1));
    s.execute("erase", &json!({ "handles": r["handles"].clone() })).unwrap();
    // One hatch for both areas, split by HATCHEDIT: the island stays with its area.
    let h = handle(&s.execute("hatch", &json!({ "points": [[1, 1], [21, 1]] })).unwrap());
    assert_eq!(hatch(&s, &h).loops.len(), 3);
    let r = s.execute("hatchedit", &json!({ "handles": [h.clone()], "separate": true, "style": "ignore" })).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    let hs = all_hatches(&s);
    assert_eq!(hs.len(), 2);
    let mut loops: Vec<usize> = hs.iter().map(|(_, h)| h.loops.len()).collect();
    loops.sort();
    assert_eq!(loops, vec![1, 2]);
    assert!(hs.iter().all(|(_, h)| h.associative && h.style == 2));
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "style": "odd" })).is_err());
    assert!(s.execute("hatch", &json!({ "points": [[21, 1]], "style": "odd" })).is_err());
    assert!(s.execute("hatch", &json!({ "points": [[21, 1]], "originMode": "middle" })).is_err());
}

#[test]
fn hatchedit_unlinks_relinks_and_recreates_boundaries() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()] })).unwrap());
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "associative": false })).unwrap();
    assert!(!hatch(&s, &h).associative);
    // Linked again to the rectangle of the same shape, it follows it.
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "associative": true })).unwrap();
    assert!(hatch(&s, &h).associative);
    s.execute("move", &json!({ "handles": [r.clone()], "delta": [10, 0] })).unwrap();
    assert_eq!(hatch(&s, &h).loops[0].vertices[0].p.x, 10.0);
    // With the boundary erased, a recreated boundary takes its place.
    s.execute("erase", &json!({ "handles": [r] })).unwrap();
    assert!(!hatch(&s, &h).associative);
    let v = s.execute("hatchedit", &json!({ "handles": [h.clone()], "associative": true })).unwrap();
    assert_eq!(v["messages"].as_array().unwrap().len(), 1, "nothing to link to");
    let v = s.execute("hatchedit", &json!({ "handles": [h.clone()], "recreate": "polyline", "associate": true })).unwrap();
    let p = v["handles"][0].as_str().unwrap().to_string();
    assert!(hatch(&s, &h).associative);
    s.execute("move", &json!({ "handles": [p], "delta": [0, 10] })).unwrap();
    assert_eq!(hatch(&s, &h).loops[0].vertices[0].p.y, 10.0);
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "recreate": 5 })).is_err());
}

#[test]
fn hatchedit_needs_a_hatch() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    assert!(s.execute("hatchedit", &json!({ "handles": [r], "scale": 2 })).is_err());
}

#[test]
fn hatchedit_adds_and_removes_boundary_loops() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let island = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [outer] })).unwrap());
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "addBoundary": [island.clone()], "addPoints": [[35, 5]] })).unwrap();
    let edited = hatch(&s, &h);
    assert_eq!(edited.loops.len(), 3);
    assert_eq!(edited.loops.iter().map(|l| l.outer).collect::<Vec<_>>(), vec![true, true, false]);
    assert!(edited.associative);
    assert_eq!(edited.loops[2].sources, vec![Handle::parse_hex(&island).unwrap()]);

    s.execute("hatchedit", &json!({ "handles": [h.clone()], "removeLoops": [2] })).unwrap();
    let edited = hatch(&s, &h);
    assert_eq!(edited.loops.len(), 2);
    assert!(edited.loops.iter().all(|l| l.outer));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(hatch(&s, &h).loops.len(), 3, "one edit is one undo step");
}

#[test]
fn hatchedit_added_object_boundary_stays_associative() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let island = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [outer] })).unwrap());
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "addBoundary": [island.clone()] })).unwrap();
    let edited = hatch(&s, &h);
    assert!(edited.associative, "{edited:?}");
    assert_eq!(edited.loops.iter().map(|l| l.outer).collect::<Vec<_>>(), vec![true, false]);
    assert_eq!(edited.loops[1].sources, vec![Handle::parse_hex(&island).unwrap()]);
    s.execute("move", &json!({ "handles": [island], "delta": [1, 0] })).unwrap();
    assert_eq!(hatch(&s, &h).loops[1].vertices[0].p.x, 6.0);
}

#[test]
fn hatchedit_refuses_invalid_boundary_changes_without_editing() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [outer] })).unwrap());
    let before = hatch(&s, &h);
    for change in [
        json!({ "removeLoops": [0] }),
        json!({ "removeLoops": [9] }),
        json!({ "addBoundary": ["BAD-HANDLE"] }),
        json!({ "addPoints": [[f64::NAN, 1.0]] }),
    ] {
        let mut p = change.as_object().unwrap().clone();
        p.insert("handles".into(), json!([h.clone()]));
        p.insert("scale".into(), json!(2.0));
        assert!(s.execute("hatchedit", &Value::Object(p)).is_err());
        assert_eq!(hatch(&s, &h), before);
    }
}

#[test]
fn hatch_origin_and_separate_keywords_at_the_command_line() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    rect(&mut s, [20.0, 0.0], [30.0, 10.0]);
    s.script("-HATCH\nE\nO\nB\n1,1\n21,1\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let hs = all_hatches(&s);
    assert_eq!(hs.len(), 2, "separate hatches");
    let mut origins: Vec<f64> = hs.iter().map(|(_, h)| h.origin.x).collect();
    origins.sort_by(f64::total_cmp);
    assert_eq!(origins, vec![0.0, 20.0], "each from its own bottom-left corner");
}

#[test]
fn dash_hatchedit_options_at_the_command_line() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[1, 1]] })).unwrap());
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.script("-HATCHEDIT\nP\nANSI32\n2\n45\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let hh = hatch(&s, &h);
    assert_eq!((hh.pattern.as_str(), hh.scale), ("ANSI32", 2.0));
    assert!((hh.angle.to_degrees() - 45.0).abs() < 1e-9);
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.script("-HATCHEDIT\nD\n").unwrap();
    assert!(!hatch(&s, &h).associative);
    s.set_selection(Vec::new());
    s.script("-HATCHEDIT\nW\n-1,-1\n11,11\nAS\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(hatch(&s, &h).associative, "{:?}", s.log);
    assert_eq!(hatch(&s, &h).loops[0].sources, vec![Handle::parse_hex(&r).unwrap()]);
}

#[test]
fn dash_hatchedit_adds_and_removes_boundaries() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let island = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [outer] })).unwrap());
    let selected = Handle::parse_hex(&h).unwrap();
    s.set_selection(vec![selected]);
    s.start("hatchedit").unwrap();
    s.input(Input::Keyword("add Object".into())).unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&island).unwrap()])).unwrap();
    assert_eq!(hatch(&s, &h).loops.len(), 2);

    s.set_selection(vec![selected]);
    s.start("hatchedit").unwrap();
    s.input(Input::Keyword("remove Boundary".into())).unwrap();
    s.input(Input::Text("2".into())).unwrap();
    assert_eq!(hatch(&s, &h).loops.len(), 1);
}

#[test]
fn hatchedit_refuses_unknown_patterns_and_region_boundaries() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    let before = hatch(&s, &h).pattern;
    assert!(s.execute("hatchedit", &json!({ "handles": [h.clone()], "pattern": "NO-SUCH-PATTERN" })).is_err());
    assert_eq!(hatch(&s, &h).pattern, before);
    // Its own pattern, SOLID and library patterns are fine.
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "pattern": before.to_lowercase() })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "pattern": "solid" })).unwrap();
    assert!(hatch(&s, &h).solid);
    // Region boundaries need REGION objects; the hatch is left as it was.
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "recreate": "region" })).is_err());
}

#[test]
fn multiline_wall_hatch_keeps_the_room_as_an_island() {
    // A closed multiline wall: picking inside the wall hatches the band between its outer and
    // inner lines; the room the inner lines enclose is an island, not filled.
    let mut s = Session::new();
    s.execute("mline", &json!({ "points": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true, "scale": 1, "justification": "zero" })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[0, 5]], "pattern": "SOLID" })).unwrap());
    assert_eq!(hatch(&s, &h).loops.len(), 2, "outer face + the room as an island");
}
