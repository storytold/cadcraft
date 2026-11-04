//! A polyface mesh stays one object through DXF and DWG, instead of becoming loose 3D faces;
//! its hidden edges stay hidden.

use cadcraft_doc::{Drawing, EntityKind, PolyfaceMesh, Prim};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

/// A 10 × 10 square as two triangles; the diagonal (from vertex 3) is hidden in both.
const POLYFACE_DXF: &str = "0\nSECTION\n2\nENTITIES\n\
0\nPOLYLINE\n5\n30\n100\nAcDbEntity\n8\n0\n100\nAcDbPolyFaceMesh\n66\n1\n10\n0\n20\n0\n30\n0\n70\n64\n71\n4\n72\n2\n\
0\nVERTEX\n5\n31\n8\n0\n10\n0\n20\n0\n30\n0\n70\n192\n\
0\nVERTEX\n5\n32\n8\n0\n10\n10\n20\n0\n30\n0\n70\n192\n\
0\nVERTEX\n5\n33\n8\n0\n10\n10\n20\n10\n30\n2\n70\n192\n\
0\nVERTEX\n5\n34\n8\n0\n10\n0\n20\n10\n30\n2\n70\n192\n\
0\nVERTEX\n5\n35\n8\n0\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n73\n-3\n\
0\nVERTEX\n5\n36\n8\n0\n10\n0\n20\n0\n30\n0\n70\n128\n71\n3\n72\n4\n73\n-1\n\
0\nSEQEND\n5\n37\n8\n0\n0\nENDSEC\n0\nEOF\n";

fn mesh(d: &Drawing) -> PolyfaceMesh {
    let meshes: Vec<&PolyfaceMesh> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::PolyfaceMesh(m) => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(meshes.len(), 1, "one polyface mesh");
    assert!(d.model.iter().all(|e| !matches!(e.kind, EntityKind::Face3d(_))), "no loose 3D faces");
    meshes[0].clone()
}

fn expected() -> PolyfaceMesh {
    PolyfaceMesh {
        vertices: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 2.0), Vec3::new(0.0, 10.0, 2.0)],
        faces: vec![[1, 2, -3, 0], [3, 4, -1, 0]],
    }
}

#[test]
fn polyface_mesh_is_read_as_one_object_with_its_hidden_edges() {
    let m = mesh(&read_dxf(POLYFACE_DXF.as_bytes()).unwrap());
    assert_eq!(m, expected());
    assert_eq!(m.faces3d().len(), 2);
    // The square's four sides; the hidden diagonal isn't drawn.
    let edges = EntityKind::PolyfaceMesh(m).prims().into_iter().filter(|p| matches!(p, Prim::Seg(_))).count();
    assert_eq!(edges, 4);
}

#[test]
fn polyface_mesh_round_trips_through_dxf_and_dwg() {
    let d = read_dxf(POLYFACE_DXF.as_bytes()).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("AcDbPolyFaceMesh") && text.contains("AcDbFaceRecord"));
    assert_eq!(mesh(&read_dxf(text.as_bytes()).unwrap()), expected());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
        assert_eq!(mesh(&cadcraft_io::read(&dwg, "x.dwg").unwrap()), expected());
    }
}

#[test]
fn a_polyface_mesh_too_large_for_its_record_is_saved_as_3d_faces() {
    use cadcraft_doc::{Common, Space};
    use cadcraft_io::{SaveOptions, read, write_with};
    // 40,000 vertices: counts and indices past 32,767 don't fit the 16-bit groups 71–74.
    let vertices: Vec<Vec3> = (0..40_000).map(|i| Vec3::new(f64::from(i), 0.0, f64::from(i % 7))).collect();
    let mesh = PolyfaceMesh { vertices, faces: vec![[1, 2, 39_999, 0], [39_998, -39_999, 40_000, 3]] };
    let mut d = Drawing::new_metric();
    let common = Common { layer: "0".into(), ..Common::default() };
    d.add(&Space::Model, common, EntityKind::PolyfaceMesh(mesh.clone())).unwrap();
    let want = mesh.faces3d();
    for binary in [false, true] {
        let bytes = write_with(&d, "big.dxf", &SaveOptions { version: None, binary }).unwrap();
        let back = read(&bytes, "big.dxf").unwrap();
        let faces: Vec<_> = back
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Face3d(f) => Some(f.clone()),
                _ => None,
            })
            .collect();
        // Every face keeps its corners and hidden edges: no index was cut to 32,767.
        assert_eq!(faces, want, "binary: {binary}");
        assert!(back.model.iter().all(|e| !matches!(e.kind, EntityKind::PolyfaceMesh(_))), "binary: {binary}");
        let mut handles: Vec<_> = back.model.iter().map(|e| e.handle).collect();
        handles.sort();
        handles.dedup();
        assert_eq!(handles.len(), back.model.iter().count(), "each face has its own handle");
    }
}

#[test]
fn faces_naming_missing_vertices_are_dropped_on_read() {
    // Added faces: one names vertex 9 of a 4-vertex mesh, one has only two corners, one has two
    // before its first 0 (which ends the face).
    let text = POLYFACE_DXF.replace(
        "0\nSEQEND\n",
        "0\nVERTEX\n5\n38\n8\n0\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n73\n9\n\
0\nVERTEX\n5\n39\n8\n0\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n\
0\nVERTEX\n5\n3A\n8\n0\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n74\n3\n0\nSEQEND\n",
    );
    let d = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(mesh(&d), expected(), "only the faces that name its vertices");
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(mesh(&back), expected());
}

#[test]
fn faces_naming_missing_vertices_are_not_written() {
    use cadcraft_doc::{Common, Space};
    let mut m = expected();
    m.faces.push([1, 2, 9, 0]);
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common { layer: "0".into(), ..Common::default() }, EntityKind::PolyfaceMesh(m)).unwrap();
    let text = write_dxf(&d);
    assert_eq!(text.matches("AcDbFaceRecord").count(), 2, "only the faces that name its vertices");
    assert_eq!(mesh(&read_dxf(text.as_bytes()).unwrap()), expected());
}
