//! Regions (#510): REGION turns closed objects into region objects; AREA and MASSPROP measure
//! them; UNION, SUBTRACT and INTERSECT combine regions with line and arc boundaries; they move,
//! explode and snap like other objects.

use cadcraft_doc::{EntityKind, Handle, Region, Space};
use cadcraft_engine::Session;
use cadcraft_geom::{PI, Vec2};
use serde_json::{Value, json};

/// Parse the handle returned by a command in the test drawing.
fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

/// Read a region and fail the test if the command replaced it with another object type.
fn region(s: &Session, hd: Handle) -> Region {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::Region(r) => r.clone(),
        k => panic!("not a region: {k:?}"),
    }
}

/// Compare measured areas with a relative tolerance and a small absolute floor.
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
}

/// Typed SUBTRACT uses separate source and removal selections to cut an interior hole.
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

/// Region boundaries remain usable by transforms, grips, snaps, selection and EXPLODE.
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

/// Malformed command parameters and extreme imported coordinates never escape as panics.
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

/// F21: crossing regions give the known union, difference and intersection, and the whole
/// command is one undo step. The first input keeps its handle and properties.
#[test]
fn overlapping_regions_boolean_areas_and_undo() {
    for (command, expected) in [("union", 175.0), ("subtract", 75.0), ("intersect", 25.0)] {
        let mut s = Session::new();
        let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
        let b = rect_region(&mut s, [5.0, 5.0], [15.0, 15.0]);
        let before_a = s.doc().unwrap().entity(a).unwrap().clone();
        let before_b = s.doc().unwrap().entity(b).unwrap().clone();
        let params = if command == "subtract" { json!({"from": [a.hex()], "remove": [b.hex()]}) } else { json!({"handles": [a.hex(), b.hex()]}) };
        let result = s.execute(command, &params).unwrap();
        assert_eq!(h(&result), a);
        assert!(near(region(&s, a).area(), expected), "{command}");
        assert_eq!(s.doc().unwrap().entity(a).unwrap().common, before_a.common);
        assert!(s.doc().unwrap().entity(b).is_none());
        s.execute("u", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().entity(a).unwrap(), &before_a);
        assert_eq!(s.doc().unwrap().entity(b).unwrap(), &before_b);
        s.execute("redo", &json!({})).unwrap();
        assert!(near(region(&s, a).area(), expected));
    }
}

/// A clipped circular region keeps its analytic area and curved boundary through save and EXPLODE.
#[test]
fn overlapping_region_arcs_survive_explode_and_dxf() {
    let mut s = Session::new();
    let a = circle_region(&mut s, [0.0, 0.0], 5.0);
    let b = rect_region(&mut s, [3.0, -10.0], [10.0, 10.0]);
    s.execute("intersect", &json!({"handles": [a.hex(), b.hex()]})).unwrap();
    let expected = 25.0 * 0.6_f64.acos() - 12.0;
    let r = region(&s, a);
    assert!(near(r.area(), expected));
    assert!(r.loops.iter().flat_map(|l| &l.vertices).any(|v| v.bulge != 0.0));
    let bytes = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(bytes.as_bytes()).unwrap();
    let EntityKind::Region(saved) = &back.entity(a).unwrap().kind else { panic!("saved region missing") };
    assert!(near(saved.area(), expected));
    s.execute("explode", &json!({"handles": [a.hex()]})).unwrap();
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::Arc(_))));
}

/// Subtracting a crossing strip opens a hole and leaves two correctly classified outer pieces.
#[test]
fn overlapping_subtract_splits_a_region_with_a_hole() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let hole = rect_region(&mut s, [2.0, 2.0], [8.0, 8.0]);
    s.execute("subtract", &json!({"from": [a.hex()], "remove": [hole.hex()]})).unwrap();
    let strip = rect_region(&mut s, [4.0, -1.0], [6.0, 11.0]);
    s.execute("subtract", &json!({"from": [a.hex()], "remove": [strip.hex()]})).unwrap();
    let r = region(&s, a);
    assert!(near(r.area(), 56.0));
    assert_eq!(r.loops.len(), 2);
    assert!(r.loops.iter().all(|l| l.outer));
    assert!(!r.contains(Vec2::new(5.0, 1.0)));
    assert!(r.contains(Vec2::new(1.0, 5.0)) && r.contains(Vec2::new(9.0, 5.0)));
}

/// Typed UNION accepts two crossing regions and completes with the known union area.
#[test]
fn overlapping_union_on_the_command_line() {
    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    rect_region(&mut s, [5.0, 5.0], [15.0, 15.0]);
    s.set_selection(Vec::new());
    for text in ["union", "0,5", "15,10", ""] {
        s.cmdline(text).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert!(near(region(&s, a).area(), 175.0));
}

/// Failure on a later pair must preserve all input regions and the preceding undo step.
#[test]
fn a_failed_later_region_keeps_all_inputs_and_undo() {
    use cadcraft_geom::PolyVertex;

    let mut s = Session::new();
    let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let b = rect_region(&mut s, [5.0, 5.0], [15.0, 15.0]);
    // A malformed boundary in an imported drawing: the combined input exceeds the cap.
    let bad = Region::from_loop(vec![PolyVertex::new(Vec2::ZERO); cadcraft_geom::area_boolean::MAX_EDGES + 1]);
    let c = s.doc_mut().unwrap().add(&Space::Model, Default::default(), EntityKind::Region(bad)).unwrap();
    let before_a = region(&s, a);
    let before_b = region(&s, b);
    let error = s.execute("union", &json!({"handles": [a.hex(), b.hex(), c.hex()]})).unwrap_err().to_string();
    assert!(error.contains("too complex"), "{error}");
    assert_eq!(region(&s, a), before_a);
    assert_eq!(region(&s, b), before_b);
    assert!(s.doc().unwrap().entity(c).is_some());
    // A rejected self-subtraction also leaves everything in place.
    assert!(s.execute("subtract", &json!({"from": [a.hex()], "remove": [a.hex()]})).is_err());
    s.execute("u", &json!({})).unwrap();
    assert!(matches!(&s.doc().unwrap().entity(b).unwrap().kind, EntityKind::LwPolyline(_)), "undo still undoes the preceding REGION");
}

/// An unresolved near-straight arc must never turn overlapping regions into an empty result:
/// all boolean commands fail atomically and keep the selection, undo and pending redo.
#[test]
fn unresolved_arc_booleans_keep_the_drawing_and_history() {
    use cadcraft_geom::PolyVertex;

    for command in ["intersect", "union", "subtract"] {
        let mut s = Session::new();
        let a = Region::from_loop(vec![
            PolyVertex::with_bulge(Vec2::new(0.0, 0.5), 1e-6),
            PolyVertex::new(Vec2::new(1.0, 0.5)),
            PolyVertex::new(Vec2::new(1.0, 1.0)),
            PolyVertex::new(Vec2::new(0.0, 1.0)),
        ]);
        let a = s.doc_mut().unwrap().add(&Space::Model, Default::default(), EntityKind::Region(a)).unwrap();
        let b = rect_region(&mut s, [0.01, 0.0], [0.01 + 0.1, 0.5 - 1.5e-7]);
        s.execute("move", &json!({"handles": [b.hex()], "delta": [0, 1]})).unwrap();
        s.undo().unwrap();
        s.set_selection(vec![a, b]);
        let before = s.state().unwrap().clone();
        let params = if command == "subtract" { json!({"from": [a.hex()], "remove": [b.hex()]}) } else { json!({"handles": [a.hex(), b.hex()]}) };
        let result = s.execute(command, &params);
        assert!(result.is_err(), "{command} must reject unresolved geometry: {result:?}");
        assert!(result.unwrap_err().to_string().contains("too close to combine reliably"));
        let after = s.state().unwrap();
        assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
        assert_eq!(after.selection, before.selection);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.undo.len(), before.undo.len());
        assert_eq!(after.redo.len(), before.redo.len());
        assert!(s.redo().unwrap().is_some());
        assert!(region(&s, b).bounds().min.near(Vec2::new(0.01, 1.0), 1e-9));
    }
}

/// Build an imported polygon contour without involving spline tessellation in the oracle.
fn polygon_region(s: &mut Session, x: f64, n: usize) -> Handle {
    let vertices = (0..n).map(|i| cadcraft_geom::PolyVertex::new(Vec2::polar(Vec2::new(x, 0.0), 5.0, 2.0 * PI * i as f64 / n as f64))).collect();
    s.doc_mut().unwrap().add(&Space::Model, Default::default(), EntityKind::Region(Region::from_loop(vertices))).unwrap()
}

/// Check one imported-contour command against the analytic polygon or clipped half-area.
fn check_large_region_boolean(n: usize, crossed: bool, command: &str) {
    let polygon_area = n as f64 * 12.5 * (2.0 * PI / n as f64).sin();
    let mut s = Session::new();
    let a = polygon_region(&mut s, 0.0, n);
    let b = if crossed { rect_region(&mut s, [0.0, -10.0], [10.0, 10.0]) } else { polygon_region(&mut s, 20.0, n) };
    let overlap = if crossed { polygon_area * 0.5 } else { 0.0 };
    let other_area = if crossed { 200.0 } else { polygon_area };
    let want = match command {
        "union" => polygon_area + other_area - overlap,
        "subtract" => polygon_area - overlap,
        _ => overlap,
    };
    let params = if command == "subtract" { json!({"from": [a.hex()], "remove": [b.hex()]}) } else { json!({"handles": [a.hex(), b.hex()]}) };
    let result = s.execute(command, &params).unwrap();
    if want == 0.0 {
        assert!(result["handle"].is_null());
    } else {
        assert!(near(region(&s, a).area(), want), "{n}, {crossed}, {command}");
    }
    assert!(s.doc().unwrap().entity(b).is_none());
}

/// Large imported contours retain their analytic areas, both disjoint and crossed by a square.
#[test]
fn large_region_booleans_keep_the_expected_areas() {
    for n in [1000, 2000] {
        for crossed in [false, true] {
            for command in ["union", "subtract", "intersect"] {
                check_large_region_boolean(n, crossed, command);
            }
        }
    }
}

/// A shallow imported bulge does not invalidate booleans whose crossings are elsewhere.
#[test]
fn a_distant_shallow_arc_does_not_fail_region_commands() {
    for bulge in [1e-6, 1e-8, 1e-10] {
        for crossed in [false, true] {
            let mut s = Session::new();
            let a = rect_region(&mut s, [0.0, 0.0], [10.0, 10.0]);
            s.doc_mut()
                .unwrap()
                .modify_entity(a, |e| {
                    if let EntityKind::Region(r) = &mut e.kind {
                        r.loops[0].vertices[0].bulge = bulge;
                    }
                })
                .unwrap();
            let b = if crossed { rect_region(&mut s, [5.0, 5.0], [15.0, 15.0]) } else { rect_region(&mut s, [20.0, 0.0], [30.0, 10.0]) };
            s.execute("union", &json!({"handles": [a.hex(), b.hex()]})).unwrap();
            let expected = if crossed { 175.0 } else { 200.0 } + 100.0 * bulge / 3.0;
            assert!(near(region(&s, a).area(), expected));
            assert!(s.doc().unwrap().entity(b).is_none());
        }
    }
}
