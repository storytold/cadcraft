//! Object snaps find the geometry inside block references (#314).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Insert, Space};
use cadcraft_engine::geom::{Mat3, Vec2, Vec3};
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

fn handle(s: &mut Session, cmd: &str, p: serde_json::Value) -> String {
    let r = s.execute(cmd, &p).unwrap();
    r.get("handle").unwrap_or(&r["handles"][0]).as_str().unwrap().to_string()
}

/// Block "B": a line (0,0)-(10,0), a circle at (20,0) r2 and an arc at (0,10) r5 from 0° to 90°.
fn block_b(s: &mut Session) {
    let l = handle(s, "line", json!({ "points": [[0, 0], [10, 0]] }));
    let c = handle(s, "circle", json!({ "center": [20, 0], "radius": 2 }));
    let a = handle(s, "arc", json!({ "center": [0, 10], "radius": 5, "start": 0, "end": 90 }));
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [l, c, a], "keep": "delete" })).unwrap();
}

/// Adds a reference of `block`; `grid` is (columns, rows, column spacing, row spacing).
fn insert(s: &mut Session, block: &str, at: (f64, f64), scale: (f64, f64), rotation_deg: f64, grid: (u32, u32, f64, f64)) -> String {
    let kind = EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(at.0, at.1, 0.0),
        scale: Vec3::new(scale.0, scale.1, 1.0),
        rotation: rotation_deg.to_radians(),
        attribs: Vec::new(),
        cols: grid.0,
        rows: grid.1,
        col_spacing: grid.2,
        row_spacing: grid.3,
    });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), kind).unwrap().hex()
}

const ONE: (u32, u32, f64, f64) = (1, 1, 0.0, 0.0);

/// The snap of `modes` near `p` (a hair off it, inside a 0.5 aperture).
fn snap_at(s: &Session, p: Vec2, modes: u32, base: Option<Vec2>) -> Option<(Vec2, u32)> {
    snap::osnap(s.doc().unwrap(), &Space::Model, p + Vec2::new(0.05, -0.04), 0.5, modes, base, false).map(|h| (h.point, h.mode))
}

fn assert_snaps(s: &Session, expect: Vec2, modes: u32) {
    let (p, m) = snap_at(s, expect, modes, None).unwrap_or_else(|| panic!("no snap near {expect:?}"));
    assert_eq!(m, modes, "snap mode near {expect:?}");
    assert!(p.near(expect, 1e-9), "snapped to {p:?}, expected {expect:?}");
}

#[test]
fn snaps_inside_rotated_scaled_and_mirrored_references() {
    let mut s = Session::new();
    block_b(&mut s);
    insert(&mut s, "B", (100.0, 100.0), (2.0, 2.0), 30.0, ONE);
    let rot = 30f64.to_radians();
    let m = Mat3::translate(Vec2::new(100.0, 100.0)).then_before(Mat3::rotate(rot)).then_before(Mat3::scale(2.0, 2.0));
    let at = |x: f64, y: f64| m.apply(Vec2::new(x, y));
    assert_snaps(&s, at(10.0, 0.0), mode::END);
    assert_snaps(&s, at(5.0, 0.0), mode::MID);
    assert_snaps(&s, at(20.0, 0.0), mode::CEN);
    // Quadrants stay on the world axes: the top of the circle (radius 4 once placed).
    assert_snaps(&s, at(20.0, 0.0) + Vec2::new(0.0, 4.0), mode::QUA);
    // The arc's start point.
    assert_snaps(&s, at(5.0, 10.0), mode::END);
    // Perpendicular from a point outside the block onto its line.
    let (p, m2) = snap_at(&s, at(4.0, 0.0), mode::PER, Some(at(4.0, 3.0))).unwrap();
    assert!(m2 == mode::PER && p.near(at(4.0, 0.0), 1e-9), "{p:?}");
    // The insertion point still snaps.
    assert_snaps(&s, Vec2::new(100.0, 100.0), mode::INS);

    // Mirrored in X and stretched 3x along Y: the circle becomes an ellipse.
    let mut s = Session::new();
    block_b(&mut s);
    insert(&mut s, "B", (0.0, 0.0), (-1.0, 3.0), 0.0, ONE);
    assert_snaps(&s, Vec2::new(-10.0, 0.0), mode::END);
    assert_snaps(&s, Vec2::new(-20.0, 0.0), mode::CEN);
    for q in [Vec2::new(-20.0, 6.0), Vec2::new(-20.0, -6.0), Vec2::new(-18.0, 0.0), Vec2::new(-22.0, 0.0)] {
        assert_snaps(&s, q, mode::QUA);
    }
    // The arc, now elliptical, ends where its ends were placed: (5,10) -> (-5,30), (0,15) -> (0,45).
    assert_snaps(&s, Vec2::new(-5.0, 30.0), mode::END);
    assert_snaps(&s, Vec2::new(0.0, 45.0), mode::END);

    // Mirrored in Y with a uniform scale of 2 the arc stays an arc.
    let mut s = Session::new();
    block_b(&mut s);
    insert(&mut s, "B", (0.0, 0.0), (2.0, -2.0), 0.0, ONE);
    assert_snaps(&s, Vec2::new(10.0, -20.0), mode::END);
    assert_snaps(&s, Vec2::new(0.0, -30.0), mode::END);
    assert_snaps(&s, Vec2::new(0.0, -20.0), mode::CEN);
    assert_snaps(&s, Vec2::new(0.0, -20.0) + Vec2::from_angle(-45f64.to_radians()) * 10.0, mode::MID);
    assert_snaps(&s, Vec2::new(10.0, -20.0), mode::QUA);
}

#[test]
fn snaps_inside_nested_and_arrayed_references_but_not_hidden_objects() {
    let mut s = Session::new();
    block_b(&mut s);
    // Block "N" holds a reference of B at (0,100); N is placed at (1000,0) as 3 columns 50 apart.
    let inner = insert(&mut s, "B", (0.0, 100.0), (1.0, 1.0), 0.0, ONE);
    s.execute("block", &json!({ "name": "N", "base": [0, 0], "handles": [inner], "keep": "delete" })).unwrap();
    insert(&mut s, "N", (1000.0, 0.0), (1.0, 1.0), 0.0, (3, 1, 50.0, 0.0));
    assert_snaps(&s, Vec2::new(1010.0, 100.0), mode::END);
    assert_snaps(&s, Vec2::new(1120.0, 100.0), mode::CEN);
    // A loose line crossing the nested line of the second column.
    s.execute("line", &json!({ "points": [[1055, 90], [1055, 110]] })).unwrap();
    assert_snaps(&s, Vec2::new(1055.0, 100.0), mode::INT);

    // A reference rotated 30° inside one mirrored and stretched 3x in Y: the arc becomes an
    // elliptical arc whose axes don't follow the stretch; its ends still snap where they are.
    let inner = insert(&mut s, "B", (0.0, 0.0), (1.0, 1.0), 30.0, ONE);
    s.execute("block", &json!({ "name": "S", "base": [0, 0], "handles": [inner], "keep": "delete" })).unwrap();
    insert(&mut s, "S", (0.0, 500.0), (1.0, -3.0), 0.0, ONE);
    let m = Mat3::translate(Vec2::new(0.0, 500.0)).then_before(Mat3::scale(1.0, -3.0)).then_before(Mat3::rotate(30f64.to_radians()));
    assert_snaps(&s, m.apply(Vec2::new(5.0, 10.0)), mode::END);
    assert_snaps(&s, m.apply(Vec2::new(0.0, 15.0)), mode::END);

    // Objects on a layer that is off don't snap.
    let mut s = Session::new();
    let line = EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) });
    let common = Common { layer: "Hidden".into(), ..Common::default() };
    let l = s.doc_mut().unwrap().add(&Space::Model, common, line).unwrap().hex();
    s.execute("block", &json!({ "name": "H", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
    insert(&mut s, "H", (0.0, 0.0), (1.0, 1.0), 0.0, ONE);
    assert_snaps(&s, Vec2::new(10.0, 0.0), mode::END);
    s.doc_mut().unwrap().layer_mut("Hidden").unwrap().on = false;
    assert_eq!(snap_at(&s, Vec2::new(10.0, 0.0), mode::END, None), None);
}
