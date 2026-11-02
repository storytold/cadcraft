//! LOOPS: implied closed loops from loose LINE and ARC chains (#1437).

use cadcraft_engine::Session;
use cadcraft_engine::doc::geom::{PolyVertex, Vec2, Vec3};
use cadcraft_engine::doc::{Arc, Common, EntityKind, Line, LwPolyline, Space};
use serde_json::{Value, json};

fn handle(v: &Value) -> String {
    v["handle"].as_str().or_else(|| v["handles"][0].as_str()).expect("a handle").to_string()
}

fn line(s: &mut Session, a: (f64, f64), b: (f64, f64)) -> String {
    handle(&s.execute("line", &json!({ "points": [[a.0, a.1], [b.0, b.1]] })).unwrap())
}

fn arc(s: &mut Session, center: (f64, f64), radius: f64, start: f64, end: f64) -> String {
    handle(&s.execute("arc", &json!({ "center": [center.0, center.1], "radius": radius, "start": start, "end": end })).unwrap())
}

/// Add an entity straight to the document, for shapes the commands refuse to draw.
fn add(s: &mut Session, kind: EntityKind) -> String {
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), kind).unwrap().hex()
}

fn rectangle(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> Vec<String> {
    vec![line(s, (x, y), (x + w, y)), line(s, (x + w, y), (x + w, y + h)), line(s, (x + w, y + h), (x, y + h)), line(s, (x, y + h), (x, y))]
}

fn loops(s: &mut Session, p: Value) -> Value {
    s.execute("loops", &p).unwrap()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn near(v: &Value, want: f64) {
    assert!((num(v) - want).abs() < 1e-9, "{v} != {want}");
}

#[test]
fn four_lines_make_a_closed_loop() {
    let mut s = Session::new();
    let hs = rectangle(&mut s, 0.0, 0.0, 10.0, 10.0);
    let before = s.doc().unwrap().clone();
    let revision = s.state().unwrap().revision;
    let r = loops(&mut s, json!({}));
    assert_eq!(r["schema"], 1);
    assert_eq!(r["capped"], false);
    assert_eq!((r["count"].clone(), r["returned"].clone(), r["offset"].clone()), (json!(1), json!(1), json!(0)));
    assert_eq!(r["truncated"], false);
    assert!(num(&r["tolerance"]) > 0.0 && num(&r["tolerance"]).is_finite(), "{r}");
    assert!(r["pairTests"].as_u64().unwrap() >= 28, "8 endpoints: {r}");
    let loop_ = &r["loops"][0];
    assert_eq!(loop_["members"], json!(hs));
    assert_eq!(loop_["winding"], "ccw");
    near(&loop_["area"], 100.0);
    near(&loop_["perimeter"], 40.0);
    assert_eq!(r["openChains"]["count"], 0, "{r}");
    assert_eq!(r["junctions"]["count"], 0, "{r}");
    // Read-only: no entity, no undo step, no revision.
    assert_eq!(s.state().unwrap().revision, revision);
    assert_eq!(*s.doc().unwrap(), before);
}

#[test]
fn loop_facts_match_shapes() {
    let mut s = Session::new();
    rectangle(&mut s, 0.0, 0.0, 10.0, 10.0);
    let vertices: Vec<PolyVertex> =
        [(5.0, 0.0), (15.0, 0.0), (15.0, 10.0), (5.0, 10.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    s.doc_mut()
        .unwrap()
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::LwPolyline(LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }),
        )
        .unwrap();
    let shapes = s.execute("shapes", &json!({})).unwrap();
    let loops = loops(&mut s, json!({}));
    let (a, b) = (&loops["loops"][0], &shapes["shapes"][0]);
    for k in ["perimeter", "area", "winding"] {
        assert_eq!(a[k], b[k], "{k}: {a} vs {b}");
    }
}

#[test]
fn a_corner_off_by_a_gap_is_an_open_chain() {
    let mut s = Session::new();
    let hs = vec![
        line(&mut s, (0.0, 0.0), (10.0, 0.0)),
        line(&mut s, (10.0, 0.0), (10.0, 10.0)),
        line(&mut s, (10.0, 10.0), (0.0, 10.0)),
        line(&mut s, (0.0, 10.0), (0.0, 0.5)),
    ];
    let r = loops(&mut s, json!({}));
    assert_eq!(r["count"], 0, "{r}");
    assert_eq!(r["openChains"]["count"], 1, "{r}");
    let c = &r["openChains"]["chains"][0];
    assert_eq!(c["members"], json!(hs));
    near(&c["gap"], 0.5);
    near(&c["at"][0], 0.0);
    near(&c["at"][1], 0.25);
    assert_eq!(c["ends"], json!([[0.0, 0.0], [0.0, 0.5]]));
    near(&c["length"], 39.5);
    // A tolerance that spans the gap closes the loop; one that does not leaves it open.
    let r = loops(&mut s, json!({ "tolerance": 0.4 }));
    assert_eq!((r["count"].clone(), r["openChains"]["count"].clone()), (json!(0), json!(1)), "{r}");
    let r = loops(&mut s, json!({ "tolerance": 0.6 }));
    assert_eq!((r["count"].clone(), r["openChains"]["count"].clone()), (json!(1), json!(0)), "{r}");
    near(&r["loops"][0]["area"], 100.0);
    near(&r["loops"][0]["perimeter"], 40.0);
}

#[test]
fn a_slot_of_lines_and_arcs() {
    let mut s = Session::new();
    let hs = vec![
        line(&mut s, (0.0, -1.0), (10.0, -1.0)),
        arc(&mut s, (10.0, 0.0), 1.0, -90.0, 90.0),
        line(&mut s, (10.0, 1.0), (0.0, 1.0)),
        arc(&mut s, (0.0, 0.0), 1.0, 90.0, 270.0),
    ];
    let r = loops(&mut s, json!({}));
    assert_eq!(r["count"], 1, "{r}");
    let loop_ = &r["loops"][0];
    assert_eq!(loop_["members"], json!(hs));
    assert_eq!(loop_["winding"], "ccw");
    let pi = std::f64::consts::PI;
    near(&loop_["area"], 20.0 + pi);
    near(&loop_["perimeter"], 20.0 + 2.0 * pi);
    assert_eq!(r["openChains"]["count"], 0, "{r}");
}

#[test]
fn clockwise_chain_reports_cw() {
    let mut s = Session::new();
    // The same rectangle drawn the other way round.
    let hs = vec![
        line(&mut s, (0.0, 0.0), (0.0, 10.0)),
        line(&mut s, (0.0, 10.0), (10.0, 10.0)),
        line(&mut s, (10.0, 10.0), (10.0, 0.0)),
        line(&mut s, (10.0, 0.0), (0.0, 0.0)),
    ];
    let r = loops(&mut s, json!({}));
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(r["loops"][0]["members"], json!(hs));
    assert_eq!(r["loops"][0]["winding"], "cw");
    near(&r["loops"][0]["area"], 100.0);
}

#[test]
fn a_t_junction_is_reported_not_resolved() {
    let mut s = Session::new();
    let hs = [line(&mut s, (0.0, 0.0), (10.0, 0.0)), line(&mut s, (0.0, 0.0), (0.0, 10.0)), line(&mut s, (0.0, 0.0), (-10.0, 0.0))];
    // A far-away rectangle still closes: the junction does not poison the whole report.
    let rect = rectangle(&mut s, 100.0, 0.0, 10.0, 10.0);
    let r = loops(&mut s, json!({}));
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(r["loops"][0]["members"], json!(rect));
    assert_eq!(r["junctions"]["count"], 1, "{r}");
    let j = &r["junctions"]["junctions"][0];
    assert_eq!(j["ends"], 3, "{r}");
    near(&j["at"][0], 0.0);
    near(&j["at"][1], 0.0);
    let mut members: Vec<&str> = j["members"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    members.sort_unstable();
    let mut want: Vec<&str> = hs.iter().map(String::as_str).collect();
    want.sort_unstable();
    assert_eq!(members, want);
    // Nothing around the junction is resolved into a chain either.
    let open_members: Vec<&Value> = r["openChains"]["chains"].as_array().unwrap().iter().flat_map(|c| c["members"].as_array().unwrap()).collect();
    assert!(open_members.iter().all(|m| !hs.iter().any(|h| h == m.as_str().unwrap())), "{r}");
}

#[test]
fn paging_caps_loops_open_chains_and_junctions() {
    let mut s = Session::new();
    for i in 0..3 {
        rectangle(&mut s, i as f64 * 100.0, 0.0, 10.0, 10.0);
    }
    let r = loops(&mut s, json!({ "limit": 1 }));
    assert_eq!((r["count"].clone(), r["returned"].clone(), r["truncated"].clone()), (json!(3), json!(1), json!(true)), "{r}");
    let r = loops(&mut s, json!({ "limit": 1, "offset": 1 }));
    assert_eq!((r["returned"].clone(), r["offset"].clone(), r["truncated"].clone()), (json!(1), json!(1), json!(true)), "{r}");
    let r = loops(&mut s, json!({ "offset": 2 }));
    assert_eq!((r["returned"].clone(), r["truncated"].clone()), (json!(1), json!(false)), "{r}");

    let mut s = Session::new();
    for i in 0..3 {
        line(&mut s, (i as f64 * 100.0, 0.0), (i as f64 * 100.0 + 1.0, 0.0));
    }
    let r = loops(&mut s, json!({ "limit": 2 }));
    assert_eq!(
        (r["openChains"]["count"].clone(), r["openChains"]["chains"].as_array().map(Vec::len).unwrap(), r["openChains"]["truncated"].clone()),
        (json!(3), 2, json!(true)),
        "{r}"
    );
}

#[test]
fn the_pair_cap_is_reported() {
    // The default cap with more endpoints than it can compare.
    let mut s = Session::new();
    for i in 0..800 {
        let x = i as f64 * 10.0;
        add(&mut s, EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 1.0, 0.0, 0.0) }));
    }
    let r = loops(&mut s, json!({}));
    assert_eq!(r["capped"], true, "{r}");
    assert_eq!(r["count"], 0, "{r}");
    assert_eq!(r["pairTests"], 0, "{r}");
    assert_eq!(r["pairLimit"].as_u64().unwrap(), 1_000_000, "{r}");
    assert!(r["pairTestsNeeded"].as_u64().unwrap() > 1_000_000, "{r}");

    // An explicit, smaller cap with a rectangle's 28 pairs.
    let mut s = Session::new();
    rectangle(&mut s, 0.0, 0.0, 10.0, 10.0);
    let r = loops(&mut s, json!({ "maxPairs": 10 }));
    assert_eq!(r["capped"], true, "{r}");
    assert_eq!((r["pairTests"].clone(), r["pairLimit"].clone(), r["pairTestsNeeded"].clone()), (json!(0), json!(10), json!(28)), "{r}");
    assert_eq!(r["loops"].as_array().map(Vec::len), Some(0), "{r}");
}

#[test]
fn degenerate_entities_and_bad_options_never_panic() {
    let mut s = Session::new();
    add(&mut s, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::ZERO }));
    add(&mut s, EntityKind::Line(Line { a: Vec3::new(f64::NAN, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) }));
    add(&mut s, EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 0.0, start: 0.0, end: 1.0 }));
    add(&mut s, EntityKind::Arc(Arc { center: Vec3::new(0.0, f64::NAN, 0.0), radius: 1.0, start: 0.0, end: 1.0 }));
    let r = loops(&mut s, json!({}));
    assert_eq!(r["count"], 0, "{r}");
    assert_eq!(r["skipped"]["Line"]["count"], 2, "{r}");
    assert_eq!(r["skipped"]["Arc"]["count"], 2, "{r}");
    // A full-circle ARC closes on its own.
    let h = add(&mut s, EntityKind::Arc(Arc { center: Vec3::new(50.0, 0.0, 0.0), radius: 2.0, start: 0.0, end: 0.0 }));
    let r = loops(&mut s, json!({}));
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(r["loops"][0]["members"], json!([h]));
    near(&r["loops"][0]["area"], 4.0 * std::f64::consts::PI);
    near(&r["loops"][0]["perimeter"], 4.0 * std::f64::consts::PI);
}

#[test]
fn coincident_endpoints_never_panic() {
    let mut s = Session::new();
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    line(&mut s, (0.0, 0.0), (10.0, 0.0));
    let r = loops(&mut s, json!({}));
    // Two coincident lines join into a zero-area loop; what matters is that it is reported, not
    // that it panics or modifies the drawing.
    assert_eq!(r["junctions"]["count"], 0, "{r}");
    assert_eq!(r["count"], 1, "{r}");
    near(&r["loops"][0]["area"], 0.0);
}

#[test]
fn a_line_whose_own_ends_meet_within_a_large_tolerance_is_still_open() {
    let mut s = Session::new();
    let h = line(&mut s, (0.0, 0.0), (0.1, 0.0));
    let r = loops(&mut s, json!({ "tolerance": 1.0 }));
    assert_eq!(r["count"], 0, "{r}");
    assert_eq!(r["openChains"]["count"], 1, "{r}");
    assert_eq!(r["openChains"]["chains"][0]["members"], json!([h]));
    near(&r["openChains"]["chains"][0]["gap"], 0.1);
}

#[test]
fn bad_options_are_errors() {
    let mut s = Session::new();
    for p in [
        json!({ "tolerance": -1 }),
        json!({ "tolerance": 0 }),
        json!({ "tolerance": "x" }),
        json!({ "maxPairs": "x" }),
        json!({ "maxPairs": -1 }),
        json!({ "limit": "x" }),
        json!({ "offset": "x" }),
    ] {
        assert!(s.execute("loops", &p).is_err(), "{p}");
    }
    // Empty drawing: a well-formed empty report.
    let r = loops(&mut s, json!({}));
    assert_eq!((r["count"].clone(), r["returned"].clone(), r["capped"].clone()), (json!(0), json!(0), json!(false)), "{r}");
}
