//! SHAPES: perimeter, area, centroid, winding, self-intersection and duplicate vertices of the
//! closed shapes in a drawing (#1436).

use cadcraft_engine::Session;
use cadcraft_engine::doc::geom::{PolyVertex, Vec2, Vec3};
use cadcraft_engine::doc::{Common, EntityKind, Face3d, LwPolyline, Space};
use serde_json::{Value, json};

const EPS: f64 = 1e-9;

/// Add a polyline through `(x, y, bulge)` vertices to model space.
fn pline(s: &mut Session, pts: &[(f64, f64, f64)], closed: bool) {
    let vertices = pts.iter().map(|&(x, y, b)| PolyVertex::with_bulge(Vec2::new(x, y), b)).collect();
    let k = EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: 0.0, plinegen: false });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), k).unwrap();
}

/// Add a unit 3D face at height `z`; a face is not a closed 2D shape, so SHAPES must skip it.
fn face(s: &mut Session, z: f64) {
    let corners = [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]].map(|c| Vec3::new(c[0], c[1], c[2]));
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Face3d(Face3d { corners, hidden_edges: 0 })).unwrap();
}

/// The last model-space entity, made invisible so it lies outside the drawing extents.
fn hide_last(s: &mut Session) {
    let h = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.visible = false).unwrap();
}

/// Add a closed 10 × 10 square polyline at the origin.
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

/// Two coincident consecutive vertices are reported with their index and distance, while the
/// shape is still measured (the duplicate does not change its area).
#[test]
fn repeated_vertex_is_listed() {
    let mut s = Session::new();
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0)], true);
    let r = shapes(&mut s, json!({}));
    let sh = &r["shapes"][0];
    assert_eq!(sh["duplicateVertices"], json!([{ "index": 1, "distance": 0.0 }]));
    near(&sh["area"], 100.0);
}

/// An open polyline whose last vertex repeats its first is treated as closed: one shape, its area
/// measured, and the coincident closing vertex not reported as a duplicate.
#[test]
fn open_polyline_with_matching_ends_is_closed() {
    let mut s = Session::new();
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0), (0.0, 10.0, 0.0), (0.0, 0.0, 0.0)], false);
    let r = shapes(&mut s, json!({}));
    assert_eq!(r["count"], 1);
    near(&r["shapes"][0]["area"], 100.0);
    assert_eq!(r["shapes"][0]["duplicateVertices"], json!([]));
}

/// A circle is measured exactly, and objects that are not closed shapes (a line, a text, an open
/// polyline) are counted under `skipped` with their reasons instead of polluting the result.
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

/// Paging reports only the requested window of shapes, and a malformed option is an error rather
/// than silently falling back to a default.
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

/// #1440: an agent must be told what an analysis covered: found, analyzed and skipped, the caps
/// it hit, and values that cannot be right. The existing keys stay as they were.
#[test]
fn self_check_counts_found_analyzed_and_skipped() {
    let mut s = Session::new();
    square(&mut s);
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("text", &json!({"at": [0, 0], "text": "NOTE", "height": 0.25})).unwrap();
    face(&mut s, 0.0);
    let r = shapes(&mut s, json!({}));
    // Add-only: the old keys and their meanings are unchanged.
    assert_eq!((r["schema"].clone(), r["count"].clone(), r["returned"].clone()), (json!(1), json!(1), json!(1)));
    for key in ["offset", "truncated", "shapes", "skipped"] {
        assert!(!r[key].is_null(), "{key}: {r}");
    }
    let sc = &r["selfCheck"];
    assert_eq!(sc["schema"], 1);
    assert_eq!(sc["found"], 4);
    assert_eq!(sc["analyzed"], 1);
    assert_eq!(sc["skipped"]["3D Face"], json!({ "count": 1, "reason": "not a closed shape" }));
    assert_eq!(sc["skipped"]["Line"], json!({ "count": 1, "reason": "not a closed shape" }));
    assert_eq!(sc["skipped"]["Text"], json!({ "count": 1, "reason": "not a closed shape" }));
    assert_eq!(sc["skipped"], r["skipped"], "the self-check repeats the skip map it is built from");
    assert_eq!(sc["caps"], json!({}));
    assert_eq!(sc["implausible"], json!([]));
    let analyzed = sc["analyzed"].as_u64().unwrap();
    let skipped: u64 = sc["skipped"].as_object().unwrap().values().map(|v| v["count"].as_u64().unwrap()).sum();
    assert_eq!(sc["found"].as_u64().unwrap(), analyzed + skipped);
}

/// #1440: a cap that stopped the analysis is reported with its value — the page `limit` when it
/// truncates, and `crossingSegments` when a shape has more edges than the self-intersection check
/// covers (which then reports no self-intersection) — while an unreached cap stays out of the
/// report.
#[test]
fn self_check_reports_the_caps_it_hits() {
    let mut s = Session::new();
    for _ in 0..3 {
        square(&mut s);
    }
    let r = shapes(&mut s, json!({ "limit": 1 }));
    assert_eq!(r["truncated"], true);
    assert_eq!(r["selfCheck"]["caps"], json!({ "limit": 1 }), "the page cap and its value: {r}");
    assert_eq!((r["selfCheck"]["found"].clone(), r["selfCheck"]["analyzed"].clone()), (json!(3), json!(3)));

    let r = shapes(&mut s, json!({ "limit": 10 }));
    assert_eq!(r["selfCheck"]["caps"], json!({}), "a cap that was not reached is not reported");

    // A closed polyline with more edges than the self-intersection cap can check.
    let zig: Vec<(f64, f64, f64)> = (0..=4_000).map(|i| (f64::from(i), 0.0, 0.0)).collect();
    pline(&mut s, &zig, true);
    let r = shapes(&mut s, json!({ "limit": 10 }));
    assert_eq!(r["selfCheck"]["caps"], json!({ "crossingSegments": 4_000 }), "{r}");
    assert_eq!(r["shapes"].as_array().unwrap().last().unwrap()["selfIntersection"], Value::Null);
}

/// #1440: geometry that cannot be right is flagged with the bound it broke — a perimeter past 10×
/// the shape's bounding-box diagonal, and an area larger than the drawing extents (invisible
/// objects lie outside the extents, which is how the test makes an out-of-extents shape).
#[test]
fn self_check_flags_implausible_geometry() {
    let mut s = Session::new();
    square(&mut s); // visible extents: 10 x 10, area 100
    // A sawtooth in the same 10 x 10 box: perimeter about 410, past 10 x the 14.14 diagonal.
    let teeth: Vec<(f64, f64, f64)> = (0..=40).map(|i| (f64::from(i) * 0.25, if i % 2 == 0 { 0.0 } else { 10.0 }, 0.0)).collect();
    pline(&mut s, &teeth, true);
    // Invisible (so outside the drawing extents): 100 x 100, area 10,000.
    pline(&mut s, &[(0.0, 0.0, 0.0), (100.0, 0.0, 0.0), (100.0, 100.0, 0.0), (0.0, 100.0, 0.0)], true);
    hide_last(&mut s);
    let r = shapes(&mut s, json!({}));
    let flags = r["selfCheck"]["implausible"].as_array().unwrap();
    let perimeter = flags.iter().find(|f| f["field"] == "perimeter").unwrap_or_else(|| panic!("no perimeter flag: {r}"));
    assert!(perimeter["reason"].as_str().unwrap().contains("10"), "{perimeter}");
    assert_eq!(perimeter["type"], "Polyline");
    let area = flags.iter().find(|f| f["field"] == "area").unwrap_or_else(|| panic!("no area flag: {r}"));
    assert!(area["reason"].as_str().unwrap().contains("drawing extents"), "{area}");
    assert!(area["value"].as_f64().unwrap() > area["bound"].as_f64().unwrap());
}

/// #1440 review: a border rectangle that fills the drawing extents has an area equal to them in
/// exact arithmetic, so it is not implausible; a shape clearly larger than the extents still is.
#[test]
fn self_check_does_not_flag_an_extents_filling_rectangle() {
    let mut s = Session::new();
    square(&mut s); // the extents are its own 10 x 10 box, area 100.
    let r = shapes(&mut s, json!({}));
    assert_eq!(r["selfCheck"]["implausible"], json!([]), "an extents-filling rectangle is not a bug: {r}");

    // The same rectangle a millionth larger, invisible so it lies outside the extents: clearly
    // past the tolerance, so it is still flagged.
    pline(&mut s, &[(0.0, 0.0, 0.0), (10.000001, 0.0, 0.0), (10.000001, 10.000001, 0.0), (0.0, 10.000001, 0.0)], true);
    hide_last(&mut s);
    let r = shapes(&mut s, json!({}));
    let flags = r["selfCheck"]["implausible"].as_array().unwrap();
    let area = flags.iter().find(|f| f["field"] == "area").unwrap_or_else(|| panic!("no area flag: {r}"));
    assert!(area["value"].as_f64().unwrap() > area["bound"].as_f64().unwrap() * (1.0 + 1e-9), "{area}");
}

/// #1440 review: with no visible extents (the only shape is invisible) there is no drawing bound
/// to compare an area against, so it is not flagged as larger than the extents.
#[test]
fn self_check_has_no_area_bound_without_visible_extents() {
    let mut s = Session::new();
    square(&mut s);
    hide_last(&mut s);
    assert!(s.doc().unwrap().extents(&Space::Model).is_empty());
    let r = shapes(&mut s, json!({}));
    assert_eq!(r["count"], 1);
    assert_eq!(r["selfCheck"]["implausible"], json!([]), "no extents means no area bound: {r}");
}
