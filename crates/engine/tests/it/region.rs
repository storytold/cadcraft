//! Regions (#510): REGION turns closed objects into region objects; AREA and MASSPROP measure
//! them; UNION, SUBTRACT and INTERSECT combine regions whose boundaries don't meet; they move,
//! explode and snap like other objects.

use cadcraft_doc::{EntityKind, Handle, Region, Space};
use cadcraft_engine::Session;
use cadcraft_geom::{PI, Vec2};
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn region(s: &Session, hd: Handle) -> Region {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::Region(r) => r.clone(),
        k => panic!("not a region: {k:?}"),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

/// A region from a rectangle, and one from a circle.
fn rect_region(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = h(&s.execute("rectang", &json!({"p1": a, "p2": b})).unwrap());
    s.execute("region", &json!({"handles": [r.hex()]})).unwrap();
    r
}

/// Add a circle and turn it into a region, returning the handle of the region's source circle.
fn circle_region(s: &mut Session, c: [f64; 2], r: f64) -> Handle {
    let hd = h(&s.execute("circle", &json!({"center": c, "radius": r})).unwrap());
    s.execute("region", &json!({"handles": [hd.hex()]})).unwrap();
    hd
}

/// REGION turns closed rectangles and circles into regions and refuses an open line; AREA and
/// MASSPROP then report the expected area, perimeter, centroid and moments (a rectangle's and a
/// circle's integrals are known exactly).
#[test]
fn region_from_closed_objects_measures() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 4.0]);
    let c = circle_region(&mut s, [20.0, 0.0], 2.0);
    s.execute("line", &json!({"points": [[0, 10], [5, 10]]})).unwrap();
    let line = s.doc().unwrap().model.iter().last().unwrap().handle;
    let r = s.execute("region", &json!({"handles": [line.hex()]})).unwrap();
    assert!(r["message"].as_str().unwrap().starts_with('0'), "an open line makes no region");
    assert_eq!(region(&s, a).loops.len(), 1);
    assert_eq!(s.doc().unwrap().entity(a).unwrap().kind.type_name(), "Region");

    let area = s.execute("area", &json!({"handle": a.hex()})).unwrap();
    assert!(near(area["area"].as_f64().unwrap(), 40.0) && near(area["perimeter"].as_f64().unwrap(), 28.0), "{area}");
    let area = s.execute("area", &json!({"handle": c.hex()})).unwrap();
    assert!(near(area["area"].as_f64().unwrap(), 4.0 * PI), "{area}");

    let m = s.execute("massprop", &json!({"handles": [a.hex()]})).unwrap();
    let r0 = &m["regions"][0];
    assert!(near(r0["area"].as_f64().unwrap(), 40.0));
    assert!(near(r0["centroid"][0].as_f64().unwrap(), 5.0) && near(r0["centroid"][1].as_f64().unwrap(), 2.0), "{r0}");
    // ∫y² dA over 0..10 × 0..4 = 10 · 4³/3; ∫x² dA = 4 · 10³/3; ∫xy dA = 100/2 · 16/2.
    assert!(near(r0["momentsOfInertia"][0].as_f64().unwrap(), 640.0 / 3.0), "{r0}");
    assert!(near(r0["momentsOfInertia"][1].as_f64().unwrap(), 4000.0 / 3.0), "{r0}");
    assert!(near(r0["productOfInertia"].as_f64().unwrap(), 400.0), "{r0}");
    assert!(m["message"].as_str().unwrap().contains("Centroid"));
    // The circle's moments about its centre come out as π r⁴ / 4 (principal, both).
    let m = s.execute("massprop", &json!({"handles": [c.hex()]})).unwrap();
    let pm = &m["regions"][0]["principalMoments"];
    assert!((pm[0].as_f64().unwrap() - 4.0 * PI).abs() < 3e-3 && (pm[1].as_f64().unwrap() - 4.0 * PI).abs() < 3e-3, "{pm}");
    assert!(s.execute("massprop", &json!({"handles": [line.hex()]})).is_err());
}

/// #1440: MASSPROP says what it measured and what it skipped, and carries a self-check.
#[test]
fn massprop_self_check_reports_skipped_objects() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 4.0]);
    s.execute("line", &json!({"points": [[0, 10], [5, 10]]})).unwrap();
    let line = s.doc().unwrap().model.iter().last().unwrap().handle;
    let m = s.execute("massprop", &json!({"handles": [a.hex(), line.hex()]})).unwrap();
    let sc = &m["selfCheck"];
    assert_eq!(sc["schema"], 1);
    assert_eq!((sc["found"].clone(), sc["analyzed"].clone()), (json!(2), json!(1)), "{m}");
    assert_eq!(sc["skipped"]["Line"], json!({ "count": 1, "reason": "not a region" }));
    assert_eq!(sc["caps"], json!({}));
    assert_eq!(sc["implausible"], json!([]));
}

/// Make `hd` invisible: it no longer counts towards the drawing extents.
fn hide(s: &mut Session, hd: Handle) {
    s.doc_mut().unwrap().modify_entity(hd, |e| e.common.visible = false).unwrap();
}

/// #1440 review: with no visible extents (the only region is invisible) there is no drawing bound
/// to compare an area against, so it is not flagged as larger than the extents.
#[test]
fn massprop_does_not_flag_area_without_visible_extents() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 4.0]);
    hide(&mut s, a);
    assert!(s.doc().unwrap().extents(&Space::Model).is_empty());
    let m = s.execute("massprop", &json!({"handles": [a.hex()]})).unwrap();
    assert_eq!(m["regions"][0]["area"], json!(40.0));
    assert_eq!(m["selfCheck"]["implausible"], json!([]), "no extents means no area bound: {m}");
}

/// SUBTRACT cuts a hole where a region lies inside another, and UNION adds an island when a region
/// sits inside that hole; the loops, area and containment follow, and the removed region is gone.
#[test]
fn booleans_on_separate_and_nested_regions() {
    // Subtract a circle inside a rectangle: a hole.
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let c = circle_region(&mut s, [5.0, 5.0], 2.0);
    s.execute("subtract", &json!({"from": [a.hex()], "remove": [c.hex()]})).unwrap();
    let r = region(&s, a);
    assert_eq!(r.loops.iter().map(|l| l.outer).collect::<Vec<_>>(), vec![true, false]);
    assert!(near(r.area(), 100.0 - 4.0 * PI));
    assert!(s.doc().unwrap().entity(c).is_none());
    assert!(!r.contains(Vec2::new(5.0, 5.0)) && r.contains(Vec2::new(1.0, 1.0)));

    // Union with a region inside the hole: an island in the hole.
    let d = circle_region(&mut s, [5.0, 5.0], 1.0);
    s.execute("union", &json!({"handles": [a.hex(), d.hex()]})).unwrap();
    let r = region(&s, a);
    assert_eq!(r.loops.len(), 3);
    assert!(near(r.area(), 100.0 - 4.0 * PI + PI));
    // Union with a separate region: one region of two pieces.
    let e = rect_region(&mut s, [20.0, 0.0], [22.0, 2.0]);
    s.execute("union", &json!({"handles": [a.hex(), e.hex()]})).unwrap();
    assert!(near(region(&s, a).area(), 100.0 - 3.0 * PI + 4.0));
    // Union with a region inside the filled part: absorbed.
    let f = rect_region(&mut s, [0.5, 0.5], [1.5, 1.5]);
    s.execute("union", &json!({"handles": [a.hex(), f.hex()]})).unwrap();
    assert_eq!(region(&s, a).loops.len(), 4);

    // Intersect: the inner one; separate regions leave nothing.
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let b = rect_region(&mut s, [2.0, 2.0], [3.0, 3.0]);
    s.execute("intersect", &json!({"handles": [a.hex(), b.hex()]})).unwrap();
    assert!(near(region(&s, a).area(), 1.0));
    let c = rect_region(&mut s, [20.0, 0.0], [21.0, 1.0]);
    let r = s.execute("intersect", &json!({"handles": [a.hex(), c.hex()]})).unwrap();
    assert!(r["handle"].is_null());
    assert_eq!(s.doc().unwrap().model.len(), 0);

    // Overlapping boundaries: not available yet, nothing changes.
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let b = rect_region(&mut s, [5.0, 5.0], [15.0, 15.0]);
    let err = s.execute("union", &json!({"handles": [a.hex(), b.hex()]})).unwrap_err().to_string();
    assert!(err.contains("not available yet"), "{err}");
    assert_eq!(s.doc().unwrap().model.len(), 2);
    assert!(s.execute("subtract", &json!({"from": [a.hex()], "remove": [a.hex()]})).is_err());
}

#[test]
fn subtract_on_the_command_line() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let _c = circle_region(&mut s, [5.0, 5.0], 2.0);
    s.set_selection(Vec::new());
    for t in ["subtract", "0,5", "", "3,5", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(region(&s, a).loops.len(), 2);
}

#[test]
fn regions_move_grip_explode_and_snap() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 4.0]);
    let c = circle_region(&mut s, [5.0, 2.0], 1.0);
    s.execute("subtract", &json!({"from": [a.hex()], "remove": [c.hex()]})).unwrap();
    s.execute("move", &json!({"handles": [a.hex()], "delta": [1, 1]})).unwrap();
    s.execute("rotate", &json!({"handles": [a.hex()], "base": [1, 1], "angle": 90})).unwrap();
    s.execute("mirror", &json!({"handles": [a.hex()], "p1": [1, 0], "p2": [1, 1], "erase": true})).unwrap();
    let a = s.doc().unwrap().model.iter().last().unwrap().handle;
    let r = region(&s, a);
    assert!(near(r.area(), 40.0 - PI));
    let b = r.bounds();
    assert!(b.min.near(Vec2::new(1.0, 1.0), 1e-9) && b.max.near(Vec2::new(5.0, 11.0), 1e-9), "{b:?}");
    // One grip in the middle; dragging it moves the region.
    let k = s.doc().unwrap().entity(a).unwrap().kind.clone();
    assert_eq!(k.grips(), vec![Vec2::new(3.0, 6.0)]);
    let EntityKind::Region(m) = cadcraft_engine::grips::stretch_grip(&k, 0, Vec2::new(4.0, 6.0)).unwrap() else { panic!() };
    assert!(m.bounds().min.near(Vec2::new(2.0, 1.0), 1e-9));
    // A corner snaps; a pick on the edge picks it.
    let d = s.doc().unwrap();
    let hit = cadcraft_engine::snap::osnap(d, &Space::Model, Vec2::new(5.05, 11.05), 0.3, cadcraft_engine::snap::mode::END, None, false).unwrap();
    assert!(hit.point.near(Vec2::new(5.0, 11.0), 1e-9));
    assert_eq!(cadcraft_engine::select::pick(d, &Space::Model, Vec2::new(3.0, 1.0), 0.1), Some(a));
    // EXPLODE: the boundary's lines and arcs.
    s.execute("explode", &json!({"handles": [a.hex()]})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 4);
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Arc(_))).count(), 2);
}

#[test]
fn hostile_region_params_never_panic() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [1e300, 1e300]);
    let b = circle_region(&mut s, [1.0, 1.0], 1e-9);
    for (id, p) in [
        ("union", json!({"handles": [a.hex(), b.hex(), "zz"]})),
        ("subtract", json!({"from": [a.hex()], "remove": "x"})),
        ("subtract", json!({"from": [], "remove": [b.hex()]})),
        ("intersect", json!({"handles": []})),
        ("massprop", json!({"handles": [a.hex(), b.hex()]})),
        ("region", json!({"handles": ["ffffff"]})),
    ] {
        let _ = s.execute(id, &p);
    }
    for t in ["union", "", "subtract", "", "massprop", ""] {
        let _ = s.cmdline(t);
    }
}
