//! The remaining object snaps (#519, gaps.md U8): Insertion on attribute definitions and
//! multileaders, Geometric Center on closed splines, and no Midpoint on closed curves.

use cadcraft_doc::{EntityKind, Space};
use cadcraft_engine::Session;
use cadcraft_engine::snap::{self, mode};
use cadcraft_geom::Vec2;
use serde_json::json;

/// The snap of `modes` near `p` (a hair off it, inside a 0.1 aperture).
fn snap_at(s: &Session, p: Vec2, modes: u32) -> Option<(Vec2, u32)> {
    snap::osnap(s.doc().unwrap(), &Space::Model, p + Vec2::new(0.02, -0.01), 0.1, modes, None, false).map(|h| (h.point, h.mode))
}

fn near(a: Option<(Vec2, u32)>, p: Vec2, m: u32) -> bool {
    a.is_some_and(|(q, k)| k == m && q.dist(p) < 1e-6)
}

#[test]
fn insertion_snaps_to_attribute_definitions() {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "PART", "default": "A-1", "at": [3, 4]})).unwrap();
    assert!(near(snap_at(&s, Vec2::new(3.0, 4.0), mode::INS), Vec2::new(3.0, 4.0), mode::INS));
}

#[test]
fn insertion_snaps_to_a_multileader_text() {
    let mut s = Session::new();
    s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "text": "NOTE"})).unwrap();
    let at = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::MLeader(m) => m.text.as_ref().map(|t| t.insert.xy()),
            _ => None,
        })
        .expect("a multileader with text");
    assert!(near(snap_at(&s, at, mode::INS), at, mode::INS));
}

#[test]
fn geometric_center_of_a_closed_spline() {
    let mut s = Session::new();
    // A closed fit spline symmetric about (5,5): its centroid is the centre.
    s.execute("spline", &json!({"fit": [[0, 5], [5, 0], [10, 5], [5, 10]], "closed": true})).unwrap();
    let c = Vec2::new(5.0, 5.0);
    let hit = snap::osnap(s.doc().unwrap(), &Space::Model, c + Vec2::new(0.05, 0.0), 0.5, mode::GCEN, None, false).expect("a geometric center");
    assert_eq!(hit.mode, mode::GCEN);
    assert!(hit.point.dist(c) < 1e-3, "{:?}", hit.point);
    // An open spline has none.
    let mut s = Session::new();
    s.execute("spline", &json!({"fit": [[0, 5], [5, 0], [10, 5], [5, 10]]})).unwrap();
    assert!(snap::osnap(s.doc().unwrap(), &Space::Model, c, 0.5, mode::GCEN, None, false).is_none());
}

#[test]
fn closed_curves_have_no_midpoint() {
    // As in AutoCAD: Midpoint works on arcs and elliptical arcs, not on a full circle or ellipse.
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    s.cmdline("ellipse 20,0 30,0 3").unwrap();
    for p in [Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(20.0, 3.0)] {
        assert_eq!(snap_at(&s, p, mode::MID), None, "{p:?}");
    }
}
