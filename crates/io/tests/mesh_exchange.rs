use cadcraft_doc::{Common, Drawing, EntityKind, Face3d, Space};
use cadcraft_geom::{Vec3, brep::Body};

fn faces(d: &Drawing) -> Vec<[Vec3; 4]> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Face3d(f) => Some(f.corners),
            _ => None,
        })
        .collect()
}
fn cube() -> Drawing {
    let mut d = Drawing::default();
    let v = [
        Vec3::new(0., 0., 1.),
        Vec3::new(2., 0., 1.),
        Vec3::new(2., 3., 1.),
        Vec3::new(0., 3., 1.),
        Vec3::new(0., 0., 4.),
        Vec3::new(2., 0., 4.),
        Vec3::new(2., 3., 4.),
        Vec3::new(0., 3., 4.),
    ];
    for [a, b, c] in
        [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]]
    {
        d.add(&Space::Model, Common::default(), EntityKind::Face3d(Face3d { corners: [v[a], v[b], v[c], v[c]], hidden_edges: 0 })).unwrap();
    }
    d
}
#[test]
fn cube_roundtrip_preserves_xyz_and_winding() {
    let d = cube();
    for ext in ["obj", "stl", "dxf"] {
        let name = format!("cube.{ext}");
        let data = cadcraft_io::write(&d, &name).unwrap();
        let reread = cadcraft_io::read(&data, &name).unwrap();
        assert_eq!(faces(&d), faces(&reread), "{ext}");
    }
}

#[test]
fn dxf_roundtrip_keeps_exact_solid_payload_on_grouped_display_faces() {
    let body = Body::cylinder(Vec3::ZERO, 2.0, 4.0).unwrap();
    let mesh = body.tessellate(0.1).unwrap();
    let mut source = Drawing::new_metric();
    let (_, members) = cadcraft_doc::brep::insert(&mut source, &body, &mesh, &Common::default()).unwrap();
    let data = cadcraft_io::write(&source, "cylinder.dxf").unwrap();
    let restored = cadcraft_io::read(&data, "cylinder.dxf").unwrap();
    let group = restored.groups.iter().find(|g| g.description == "3D solid").expect("solid group survives DXF");
    let exact = cadcraft_doc::brep::read(&restored, &group.members).unwrap().expect("exact payload survives DXF");
    assert_eq!(exact.geometry.boundaries[0].edges.len(), body.geometry.boundaries[0].edges.len());
    assert_eq!(group.members.len(), members.len());
}
#[test]
fn obj_groups_and_negative_indices() {
    let d = cadcraft_io::read(b"g roof\nv 0 0 2\nv 1 0 2\nv 1 1 3\nf -3 -2 -1\n", "x.obj").unwrap();
    assert_eq!(d.model.iter().next().unwrap().common.layer, "roof");
    assert_eq!(faces(&d)[0][2], Vec3::new(1., 1., 3.));
}
#[test]
fn reject_incomplete_and_bad_meshes() {
    for (data, name) in [
        (b"v 0 0 1\nf 1 2 3\n".as_slice(), "x.obj"),
        (b"solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\n".as_slice(), "x.stl"),
        (b"v NaN 0 0\n".as_slice(), "x.obj"),
    ] {
        assert!(cadcraft_io::read(data, name).is_err());
    }
}
#[test]
fn binary_stl_preserves_vertices() {
    let mut data = vec![0u8; 80];
    data.extend_from_slice(&1u32.to_le_bytes());
    for value in [0f32, 0., 1., 0., 0., 2., 1., 0., 2., 0., 1., 3.] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&0u16.to_le_bytes());
    let d = cadcraft_io::read(&data, "x.stl").unwrap();
    assert_eq!(faces(&d)[0][2], Vec3::new(0., 1., 3.));
    data.pop();
    assert!(cadcraft_io::read(&data, "x.stl").is_err());
}

#[test]
fn rejects_concave_and_large_obj_polygons() {
    let vertices = "v 0 0 1\nv 2 0 1\nv 0.5 0.5 1\nv 0 2 1\nv -1 1 1\n";
    for face in ["f 1 2 3 4\n", "f 1 2 3 4 5\n"] {
        assert!(cadcraft_io::read(format!("{vertices}{face}").as_bytes(), "concave.obj").is_err());
    }
}

#[test]
fn rejects_hostile_values_and_incomplete_references() {
    for obj in [
        "v 0 0 1\nv 1 0 1\nv 0 1 1\nf -9223372036854775808 2 3\n",
        "v 0 0 1\nv 1 0 1\nv 0 1 1\nf 1/ 2 3\n",
        "v 0 0 1\nv 1 0 1\nv 0 1 1\nf 1/1 2 3\n",
        "vt 1 2 3 4\nv 0 0 1\nv 1 0 1\nv 0 1 1\nf 1 2 3\n",
    ] {
        assert!(cadcraft_io::read(obj.as_bytes(), "x.obj").is_err());
    }
    assert!(cadcraft_io::read(&vec![0; 64 * 1024 * 1024 + 1], "x.stl").is_err());
}

#[test]
fn exports_reject_nonfinite_and_non_mesh_entities() {
    let mut d = Drawing::default();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Face3d(Face3d {
            corners: [Vec3::new(f64::NAN, 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.), Vec3::new(0., 1., 0.)],
            hidden_edges: 0,
        }),
    )
    .unwrap();
    for name in ["x.obj", "x.stl"] {
        assert!(cadcraft_io::write(&d, name).is_err());
    }
    let mut d = cube();
    d.add(&Space::Model, Common::default(), EntityKind::Point(cadcraft_doc::Point { p: Vec3::ZERO, angle: 0. })).unwrap();
    for name in ["x.obj", "x.stl"] {
        assert!(cadcraft_io::write(&d, name).is_err());
    }
}

#[test]
fn stl_export_has_independently_verified_winding_and_normals() {
    let bytes = cadcraft_io::write(&cube(), "cube.stl").unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let mut facets = 0;
    let mut normal = [0.0; 3];
    let mut vertices = Vec::new();
    for line in text.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if words.first() == Some(&"facet") {
            normal = [words[2].parse::<f64>().unwrap(), words[3].parse().unwrap(), words[4].parse().unwrap()];
            vertices.clear();
        } else if words.first() == Some(&"vertex") {
            vertices.push([words[1].parse::<f64>().unwrap(), words[2].parse().unwrap(), words[3].parse().unwrap()]);
        } else if words.first() == Some(&"endfacet") {
            assert_eq!(vertices.len(), 3);
            let u = std::array::from_fn::<_, 3, _>(|i| vertices[1][i] - vertices[0][i]);
            let v = std::array::from_fn::<_, 3, _>(|i| vertices[2][i] - vertices[0][i]);
            let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            assert!(cross.iter().zip(normal).map(|(a, b)| a * b).sum::<f64>() > 0.0);
            facets += 1;
        }
    }
    assert_eq!(facets, 12);
}
