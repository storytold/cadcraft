//! OVERKILL (#439): duplicates within a tolerance, ignored properties, overlapping and end-to-end
//! collinear lines, arcs on one circle, polyline segments, associativity, typed options.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    s.execute("line", &json!({ "points": [a, b] })).unwrap();
    s.doc().unwrap().model.last().unwrap().handle
}

fn arc(s: &mut Session, start: f64, end: f64) -> Handle {
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": start, "end": end })).unwrap();
    s.doc().unwrap().model.last().unwrap().handle
}

fn lines(s: &Session) -> Vec<[f64; 4]> {
    let mut v: Vec<[f64; 4]> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => {
                let (a, b) = if (l.a.x, l.a.y) <= (l.b.x, l.b.y) { (l.a, l.b) } else { (l.b, l.a) };
                Some([a.x, a.y, b.x, b.y].map(|x| (x * 1e6).round() / 1e6))
            }
            _ => None,
        })
        .collect();
    v.sort_by(|x, y| x.partial_cmp(y).unwrap());
    v
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

fn ok(s: &mut Session, p: Value) -> Value {
    s.execute("overkill", &p).unwrap()
}

#[test]
fn duplicates_tolerance_and_ignored_properties() {
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("circle", &json!({ "center": [50, 50], "radius": 3 })).unwrap();
    s.execute("circle", &json!({ "center": [50.001, 50], "radius": 3 })).unwrap();
    let red = {
        s.execute("circle", &json!({ "center": [80, 80], "radius": 3 })).unwrap();
        s.execute("circle", &json!({ "center": [80, 80], "radius": 3 })).unwrap();
        s.doc().unwrap().model.last().unwrap().handle
    };
    s.execute("properties.set", &json!({ "handles": [red.hex()], "color": "red" })).unwrap();
    let r = ok(&mut s, json!({}));
    assert_eq!((r["duplicates"].as_u64(), r["overlapping"].as_u64()), (Some(1), Some(0)), "{r}");
    assert_eq!(count(&s), 5);
    // A coarser tolerance catches the shifted circle; ignoring colour the red one.
    let r = ok(&mut s, json!({ "tolerance": 0.01 }));
    assert_eq!(r["deleted"], 1);
    let r = ok(&mut s, json!({ "ignore": ["color"] }));
    assert_eq!(r["deleted"], 1);
    assert_eq!(count(&s), 3);
    assert!(r["message"].as_str().unwrap().contains("duplicate(s) deleted"));
}

#[test]
fn collinear_lines_overlap_and_end_to_end() {
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [15.0, 0.0], [5.0, 0.0]); // partial overlap, reversed
    line(&mut s, [2.0, 0.0], [3.0, 0.0]); // inside
    line(&mut s, [15.0, 0.0], [20.0, 0.0]); // end to end
    line(&mut s, [0.0, 0.5], [10.0, 0.5]); // parallel
    line(&mut s, [0.0, 0.0], [0.0, 8.0]); // another direction
    let undo = s.state().unwrap().undo.len();
    // Partial overlap off: only the contained line goes.
    let r = ok(&mut s, json!({ "partialOverlap": false }));
    assert_eq!(r["overlapping"], 1);
    assert_eq!(count(&s), 5);
    let r = ok(&mut s, json!({}));
    assert_eq!(r["overlapping"], 1);
    assert_eq!(lines(&s), [[0.0, 0.0, 0.0, 8.0], [0.0, 0.0, 15.0, 0.0], [0.0, 0.5, 10.0, 0.5], [15.0, 0.0, 20.0, 0.0]]);
    let r = ok(&mut s, json!({ "endToEnd": true }));
    assert_eq!(r["overlapping"], 1);
    assert_eq!(lines(&s), [[0.0, 0.0, 0.0, 8.0], [0.0, 0.0, 20.0, 0.0], [0.0, 0.5, 10.0, 0.5]]);
    assert_eq!(s.state().unwrap().undo.len(), undo + 3, "one undo step each");
    // Diagonal lines far from the origin.
    let mut s = Session::new();
    line(&mut s, [1e5, 1e5], [1e5 + 30.0, 1e5 + 40.0]);
    line(&mut s, [1e5 + 15.0, 1e5 + 20.0], [1e5 + 60.0, 1e5 + 80.0]);
    let r = ok(&mut s, json!({}));
    assert_eq!(r["overlapping"], 1);
    assert_eq!(lines(&s), [[1e5, 1e5, 1e5 + 60.0, 1e5 + 80.0]]);
}

#[test]
fn arcs_on_one_circle() {
    let mut s = Session::new();
    arc(&mut s, 0.0, 90.0);
    arc(&mut s, 45.0, 180.0);
    arc(&mut s, 100.0, 120.0);
    let r = ok(&mut s, json!({}));
    assert_eq!(r["overlapping"], 2);
    let arcs: Vec<(f64, f64)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Arc(a) = &e.kind { Some((a.start.to_degrees().round(), a.end.to_degrees().round())) } else { None })
        .collect();
    assert_eq!(arcs, [(0.0, 180.0)]);
    // End to end across 0°, then a full circle.
    arc(&mut s, 270.0, 360.0);
    let r = ok(&mut s, json!({ "endToEnd": true }));
    assert_eq!(r["overlapping"], 1);
    arc(&mut s, 180.0, 270.0);
    ok(&mut s, json!({ "endToEnd": true }));
    let kinds: Vec<&str> = s.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect();
    assert_eq!(kinds, ["Circle"]);
    // A circle covers the arcs on it.
    arc(&mut s, 10.0, 20.0);
    let r = ok(&mut s, json!({}));
    assert_eq!(r["overlapping"], 1);
    assert_eq!(count(&s), 1);
}

#[test]
fn polyline_segments() {
    let mut s = Session::new();
    let pts = json!([[0, 0], [5, 0], [5, 0], [10, 0], [10, 10]]);
    s.execute("pline", &json!({ "vertices": pts })).unwrap();
    let r = ok(&mut s, json!({ "plines": false }));
    assert_eq!(r["deleted"], 0);
    let r = ok(&mut s, json!({}));
    assert_eq!(r["segments"], 2);
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => {
            assert_eq!(p.vertices.iter().map(|v| [v.p.x, v.p.y]).collect::<Vec<_>>(), [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]])
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn associated_objects_stay() {
    let mut s = Session::new();
    let followed = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("dimlinear", &json!({ "object": followed.hex(), "at": [5, 5] })).unwrap();
    let dup = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    // The duplicate comes first in draw order, so it would normally stay.
    s.execute("draworder.back", &json!({ "handles": [dup.hex()] })).unwrap();
    let r = ok(&mut s, json!({}));
    assert_eq!(r["duplicates"], 1);
    assert!(s.doc().unwrap().entity(followed).is_some(), "the line the dimension follows stays");
    assert!(s.doc().unwrap().entity(dup).is_none());
    let dup = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("draworder.back", &json!({ "handles": [dup.hex()] })).unwrap();
    ok(&mut s, json!({ "associativity": false }));
    assert!(s.doc().unwrap().entity(dup).is_some(), "the first in draw order stays");
    assert_eq!(lines(&s).len(), 1);
}

#[test]
fn hostile_params_are_refused() {
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    for p in [
        json!({ "tolerance": -1 }),
        json!({ "tolerance": 1e300 }),
        json!({ "tolerance": "big" }),
        json!({ "ignore": ["colour"] }),
        json!({ "ignore": "color" }),
        json!({ "plines": 1 }),
        json!({ "endToEnd": "yes" }),
        json!({ "handles": "1A" }),
    ] {
        assert!(s.execute("overkill", &p).is_err(), "{p}");
    }
    assert_eq!(count(&s), 2);
    // Zero and the largest tolerance are fine.
    ok(&mut s, json!({ "tolerance": 0 }));
    ok(&mut s, json!({ "tolerance": 1e6 }));
    assert_eq!(count(&s), 1);
    assert!(s.running.is_none(), "JSON calls never prompt");
}

#[test]
fn typed_options() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.2, 0.0], [20.0, 0.0]);
    let c = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.cmdline("overkill").unwrap();
    s.input(Input::Pick(vec![a, b, c])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("[Done/Ignore/tOlerance/Plines/parTial overlap/Endtoend/Associativity] <Done>"), "{}", s.prompt_text());
    s.cmdline("o").unwrap();
    s.cmdline("-2").unwrap();
    assert!(s.log.iter().any(|l| l.contains("tolerance must be")), "{:?}", s.log);
    s.cmdline("0.5").unwrap();
    s.cmdline("e").unwrap();
    s.cmdline("y").unwrap();
    s.cmdline("i").unwrap();
    s.cmdline("c").unwrap();
    assert!(s.prompt_text().contains("ignored: Color"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l == "1 duplicate(s) deleted"), "{:?}", s.log);
    assert!(s.log.iter().any(|l| l.starts_with("1 overlapping object(s)")), "{:?}", s.log);
    assert_eq!(lines(&s), [[0.0, 0.0, 20.0, 0.0]]);
}

#[test]
fn many_objects_stay_fast() {
    let mut s = Session::new();
    // 4000 touching collinear pieces plus 4000 distinct short lines.
    for i in 0..4000 {
        let x = f64::from(i);
        line(&mut s, [x, 0.0], [x + 1.0, 0.0]);
        line(&mut s, [x, 10.0 + x * 0.001], [x + 0.5, 12.0]);
    }
    let t = std::time::Instant::now();
    let r = ok(&mut s, json!({ "endToEnd": true }));
    assert_eq!(r["overlapping"], 3999);
    assert_eq!(count(&s), 4001);
    assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
}
