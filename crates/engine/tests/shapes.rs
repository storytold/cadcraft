//! SHAPES: perimeter, area, centroid, winding, self-intersection and duplicate vertices of the
//! closed shapes in a drawing (#1436).

use cadcraft_engine::Session;
use cadcraft_engine::doc::geom::{PolyVertex, Vec2};
use cadcraft_engine::doc::{Common, EntityKind, LwPolyline, Space};
use serde_json::{Value, json};

const EPS: f64 = 1e-9;

fn pline(s: &mut Session, pts: &[(f64, f64, f64)], closed: bool) {
    let vertices = pts.iter().map(|&(x, y, b)| PolyVertex::with_bulge(Vec2::new(x, y), b)).collect();
    let k = EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: 0.0, plinegen: false });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), k).unwrap();
}

fn square(s: &mut Session) {
    pline(s, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0)], true);
}

fn shapes(s: &mut Session, p: Value) -> Value {
    s.execute("shapes", &p).unwrap()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn near(v: &Value, want: f64) {
    assert!((num(v) - want).abs() < EPS, "{v} != {want}");
}

#[test]
fn square_both_windings() {
    let mut s = Session::new();
    square(&mut s);
    pline(&mut s, &[(0.0, 10.0, 0.0), (10.0, 10.0, 0.0), (10.0, 0.0, 0.0), (0.0, 0.0, 0.0)], true);
    let r = shapes(&mut s, json!({}));
    assert_eq!(r["schema"], 1);
    assert_eq!(r["count"], 2);
    for (sh, winding) in r["shapes"].as_array().unwrap().iter().zip(["ccw", "cw"]) {
        near(&sh["area"], 100.0);
        near(&sh["perimeter"], 40.0);
        near(&sh["centroid"][0], 5.0);
        near(&sh["centroid"][1], 5.0);
        assert_eq!(sh["winding"], winding);
        assert_eq!(sh["selfIntersection"], Value::Null);
        assert_eq!(sh["duplicateVertices"], json!([]));
    }
}

#[test]
fn bulged_side_is_exact() {
    let mut s = Session::new();
    // The side x = 10 bulges outward into a semicircle of radius 5 about (10, 5).
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.0, 0.0, 1.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0)], true);
    let r = shapes(&mut s, json!({}));
    let sh = &r["shapes"][0];
    let pi = std::f64::consts::PI;
    let area = 100.0 + 12.5 * pi;
    near(&sh["area"], area);
    near(&sh["perimeter"], 30.0 + 5.0 * pi);
    near(&sh["centroid"][0], (500.0 + 12.5 * pi * (10.0 + 20.0 / (3.0 * pi))) / area);
    near(&sh["centroid"][1], 5.0);
    assert_eq!(sh["winding"], "ccw");
}

#[test]
fn bow_tie_reports_its_crossing() {
    let mut s = Session::new();
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.0, 10.0, 0.0), (10.0, 0.0, 0.0), (0.0, 10.0, 0.0)], true);
    let r = shapes(&mut s, json!({}));
    let x = &r["shapes"][0]["selfIntersection"];
    near(&x[0], 5.0);
    near(&x[1], 5.0);
}

#[test]
fn repeated_vertex_is_listed() {
    let mut s = Session::new();
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0)], true);
    let r = shapes(&mut s, json!({}));
    let sh = &r["shapes"][0];
    assert_eq!(sh["duplicateVertices"], json!([{ "index": 1, "distance": 0.0 }]));
    near(&sh["area"], 100.0);
}

#[test]
fn open_polyline_with_matching_ends_is_closed() {
    let mut s = Session::new();
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0), (0.0, 0.0, 0.0)], false);
    let r = shapes(&mut s, json!({}));
    assert_eq!(r["count"], 1);
    near(&r["shapes"][0]["area"], 100.0);
    assert_eq!(r["shapes"][0]["duplicateVertices"], json!([]));
}

#[test]
fn circle_and_skipped_entities() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [3, -2], "radius": 2})).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 0.25 })).unwrap();
    pline(&mut s, &[(0.0, 0.0, 0.0), (5.0, 0.0, 0.0), (5.0, 5.0, 0.0)], false);
    let r = shapes(&mut s, json!({}));
    assert_eq!(r["count"], 1);
    let c = &r["shapes"][0];
    assert_eq!(c["type"], "Circle");
    near(&c["area"], 4.0 * std::f64::consts::PI);
    near(&c["perimeter"], 4.0 * std::f64::consts::PI);
    assert_eq!(c["centroid"], json!([3.0, -2.0]));
    assert_eq!(c["winding"], "ccw");
    let sk = r["skipped"].as_object().unwrap();
    assert_eq!(sk.len(), 3, "{sk:?}");
    assert!(sk.values().all(|v| v["count"] == 1));
    assert!(sk.values().any(|v| v["reason"] == "open"));
    assert_eq!(sk.values().filter(|v| v["reason"] == "not a closed shape").count(), 2);
}

#[test]
fn paging_and_bad_options() {
    let mut s = Session::new();
    for _ in 0..3 {
        square(&mut s);
    }
    let r = shapes(&mut s, json!({ "limit": 1 }));
    assert_eq!((r["count"].clone(), r["returned"].clone(), r["truncated"].clone()), (json!(3), json!(1), json!(true)));
    let r = shapes(&mut s, json!({ "offset": 1 }));
    assert_eq!((r["returned"].clone(), r["truncated"].clone()), (json!(2), json!(false)));
    assert!(s.execute("shapes", &json!({ "tolerance": -1 })).is_err());
    assert!(s.execute("shapes", &json!({ "limit": "x" })).is_err());
    assert!(s.execute("shapes", &json!({ "window": [1, 2] })).is_err());
}
