//! A hatch boundary stored in a DWG as an elliptic edge keeps its sweep: a circle (0 to 2π) stays a
//! circle, so the hatch leaves the hole open. The DWG decoder writes the edge's angles in radians
//! and DXF reads degrees, so the circle used to come out as an arc of 6.28 degrees.

// The DWG bridge is unavailable on wasm and FreeBSD ARM64 (#2526); DXF still works.
#![cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]

use acadrust::entities::{BoundaryEdge, BoundaryPath, BoundaryPathFlags, EllipticArcEdge, EntityType, Hatch, LineEdge};
use acadrust::types::Vector2;
use cadcraft_doc::EntityKind;

fn line(a: (f64, f64), b: (f64, f64)) -> BoundaryEdge {
    BoundaryEdge::Line(LineEdge { start: Vector2::new(a.0, a.1), end: Vector2::new(b.0, b.1) })
}

/// A DWG with a pattern hatch over a 100 x 100 square with a round hole of radius 10 stored as an
/// elliptic edge from `start` to `end` radians.
fn dwg_with_hole(start: f64, end: f64) -> Vec<u8> {
    let mut hatch = Hatch::new();
    hatch.is_solid = false;
    let mut outer = BoundaryPath::new();
    outer.flags = BoundaryPathFlags::EXTERNAL;
    outer.edges =
        vec![line((0.0, 0.0), (100.0, 0.0)), line((100.0, 0.0), (100.0, 100.0)), line((100.0, 100.0), (0.0, 100.0)), line((0.0, 100.0), (0.0, 0.0))];
    let mut hole = BoundaryPath::new();
    hole.flags = BoundaryPathFlags::DEFAULT;
    hole.edges = vec![BoundaryEdge::EllipticArc(EllipticArcEdge {
        center: Vector2::new(50.0, 50.0),
        major_axis_endpoint: Vector2::new(-10.0, 0.0),
        minor_axis_ratio: 1.0,
        start_angle: start,
        end_angle: end,
        counter_clockwise: true,
    })];
    hatch.paths = vec![outer, hole];
    let mut doc = acadrust::CadDocument::new();
    doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
    acadrust::DwgWriter::write_to_vec(&doc).unwrap()
}

/// The x range of the hatch's hole, from the bulge polyline the drawing keeps for it.
fn hole_extent(dwg: &[u8]) -> (f64, f64) {
    cadcraft_io::dwg_worker::allow_in_process();
    let d = cadcraft_io::read(dwg, "hole.dwg").unwrap();
    let EntityKind::Hatch(h) = &d.model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).expect("the hatch").kind else { panic!("hatch") };
    let hole = h.loops.iter().find(|l| !l.outer).expect("the hole's loop");
    let pl = cadcraft_geom::Polyline { vertices: hole.vertices.clone(), closed: true };
    let mut pts = Vec::new();
    for s in pl.segments() {
        s.tessellate(0.01, &mut pts);
    }
    let (min, max) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    (min, max)
}

#[test]
fn a_circle_stored_as_an_elliptic_edge_stays_a_circle() {
    let (min, max) = hole_extent(&dwg_with_hole(0.0, std::f64::consts::TAU));
    // The hole spans its whole diameter: x from 40 to 60. Read as 6.28 degrees it spanned about 0.07.
    assert!(max - min > 19.9 && max - min < 20.1, "the hole spans {} in x", max - min);
}

#[test]
fn a_quarter_arc_stored_as_an_elliptic_edge_keeps_its_quarter() {
    let (min, max) = hole_extent(&dwg_with_hole(0.0, std::f64::consts::FRAC_PI_2));
    // From the point at 0 degrees (60, 50) to the one at 90 degrees (50, 60), closed by a chord: x spans 10.
    assert!(max - min > 9.9 && max - min < 10.5, "the arc spans {} in x", max - min);
}
