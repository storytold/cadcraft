//! A polyline's vertices come back in the order its own vertex list stores them, not sorted by
//! handle (#1856). A vertex inserted into an existing polyline gets a newer, higher handle, so a
//! handle-ordered read silently reorders the outline (wrong shape, longer length).
//!
//! The DWG is written here with acadrust, giving the second 3D-polyline vertex the highest
//! handle: exactly what an editing program produces when it inserts a vertex.
#![cfg(not(target_arch = "wasm32"))]

use acadrust::entities::{EntityType, Polyline3D, Vertex3DPolyline};
use acadrust::types::{DxfVersion, Handle, Vector3};
use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec3;

/// A drawing with one 6-vertex 3D polyline whose vertex handles are not ascending: the second
/// vertex is the newest (highest) handle.
fn dwg_with_out_of_order_vertex_handles(version: DxfVersion) -> Vec<u8> {
    let mut doc = acadrust::CadDocument::with_version(version);
    let handles = [0x113C, 0x1141, 0x113D, 0x113E, 0x113F, 0x1140];
    let points = [(0.0, 0.0, 0.0), (10.0, 0.0, 1.0), (10.0, 10.0, 2.0), (0.0, 10.0, 3.0), (0.0, 20.0, 4.0), (10.0, 20.0, 5.0)];
    let mut polyline = Polyline3D::new();
    polyline.vertices = handles
        .iter()
        .zip(points)
        .map(|(handle, (x, y, z))| Vertex3DPolyline {
            handle: Handle::new(*handle),
            layer: "0".to_string(),
            position: Vector3::new(x, y, z),
            flags: 32,
        })
        .collect();
    doc.add_entity(EntityType::Polyline3D(polyline)).expect("add the polyline");
    acadrust::DwgWriter::write_to_vec(&doc).expect("write the DWG")
}

/// Reads `dwg` through `cadcraft_io::read` and returns the points of its only 3D polyline, in the
/// order the reader produced them; panics if the drawing cannot be read or holds no 3D polyline,
/// since either would mean the test is no longer exercising vertex order.
fn polyline3d_points(dwg: &[u8]) -> Vec<Vec3> {
    let drawing = cadcraft_io::read(dwg, "x.dwg").expect("read the DWG");
    let polyline = drawing.model.iter().find_map(|e| if let EntityKind::Polyline3d(p) = &e.kind { Some(p) } else { None });
    polyline.expect("one 3D polyline").points.clone()
}

/// Regression guard for #1856, run for both DWG versions the writer emits (AC1018 and AC1032): the
/// reader must keep a polyline's vertices in the order the entity's own vertex list lists them,
/// even when a mid-outline vertex carries the highest handle. Ordering by handle would move the
/// second point to the end, changing the outline's shape and length.
#[test]
fn dwg_read_keeps_polyline_vertex_order() {
    let expected = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(10.0, 0.0, 1.0),
        Vec3::new(10.0, 10.0, 2.0),
        Vec3::new(0.0, 10.0, 3.0),
        Vec3::new(0.0, 20.0, 4.0),
        Vec3::new(10.0, 20.0, 5.0),
    ];
    for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
        let dwg = dwg_with_out_of_order_vertex_handles(version);
        assert_eq!(polyline3d_points(&dwg), expected, "{version:?}");
    }
}
