//! EXPLODE turns a polyface mesh into its 3D faces, as AutoCAD does; hidden edges stay hidden.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, PolyfaceMesh, Space};
use cadcraft_geom::Vec3;
use serde_json::json;

#[test]
fn exploding_a_polyface_mesh_gives_its_3d_faces() {
    let mut s = Session::new();
    let mesh = PolyfaceMesh {
        vertices: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 2.0), Vec3::new(0.0, 10.0, 2.0)],
        faces: vec![[1, 2, -3, 0], [3, 4, -1, 0]],
    };
    let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::PolyfaceMesh(mesh)).unwrap();
    s.execute("explode", &json!({"handles": [h.hex()]})).unwrap();
    let faces: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Face3d(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(faces.len(), 2);
    assert!(s.doc().unwrap().model.iter().all(|e| !matches!(e.kind, EntityKind::PolyfaceMesh(_))));
    // The diagonal starts at the third corner of the first triangle: edge 3 (and the closing 4th) hidden.
    assert_eq!(faces[0].corners[2], Vec3::new(10.0, 10.0, 2.0));
    assert_eq!(faces[0].hidden_edges, 4 | 8);
}
