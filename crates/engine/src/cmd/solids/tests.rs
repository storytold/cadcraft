use cadcraft_doc::{Circle, EntityKind, Face3d, Handle, LwPolyline};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde_json::{Value, json};

use crate::{Input, Session, Step};

fn faces(s: &Session) -> Vec<Face3d> {
    let d = s.doc().unwrap();
    d.model.iter().filter_map(|e| if let EntityKind::Face3d(f) = &e.kind { Some(f.clone()) } else { None }).collect()
}

fn handles(r: &Value) -> Vec<Handle> {
    r["handles"].as_array().unwrap().iter().map(|h| Handle::parse_hex(h.as_str().unwrap()).unwrap()).collect()
}

/// The solid is closed (every edge in two triangles, opposite ways) and faces outward (volume > 0).
fn assert_closed_outward(s: &Session, r: &Value) -> f64 {
    let m = super::mesh_of(s.doc().unwrap(), &handles(r)).unwrap();
    let mut edges = std::collections::HashMap::new();
    for t in &m.triangles {
        for k in 0..3 {
            *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    for (&(a, b), &n) in &edges {
        assert_eq!(n, 1, "directed edge {a}->{b} used {n} times");
        assert_eq!(edges.get(&(b, a)), Some(&1), "edge {a}->{b} has no opposite");
    }
    let vol: f64 = m.triangles.iter().map(|t| m.vertices[t[0]].dot(m.vertices[t[1]].cross(m.vertices[t[2]])) / 6.0).sum();
    assert!(vol > 0.0, "volume {vol} (inward facing?)");
    vol
}

fn z_range(s: &Session) -> (f64, f64) {
    let f = faces(s);
    let zs = f.iter().flat_map(|f| f.corners.iter().map(|c| c.z));
    (zs.clone().fold(f64::MAX, f64::min), zs.fold(f64::MIN, f64::max))
}

#[test]
fn every_primitive_is_a_closed_outward_solid() {
    let cases = [
        ("box", json!({"p1": [0, 0, 0], "p2": [4, 3, 2]})),
        ("box", json!({"center": [0, 0, 0], "length": 2, "width": 2, "height": 2})),
        ("box", json!({"corner": [0, 0], "side": 3})),
        ("cylinder", json!({"center": [0, 0], "radius": 2, "height": 5})),
        ("cylinder", json!({"center": [0, 0, 4], "diameter": 4, "height": -5, "segments": 12})),
        ("cone", json!({"center": [0, 0], "radius": 2, "height": 5})),
        ("cone", json!({"center": [0, 0], "radius": 2, "height": -5})),
        ("sphere", json!({"center": [1, 1, 1], "radius": 3})),
        ("torus", json!({"center": [0, 0], "radius": 5, "tubeRadius": 1})),
        ("pyramid", json!({"center": [0, 0], "side": 4, "height": 3})),
        ("pyramid", json!({"center": [0, 0], "radius": 4, "sides": 6, "height": -3})),
        ("extrude", json!({"profile": [[0, 0], [4, 0], [4, 3], [0, 3]], "height": 2})),
        ("extrude", json!({"profile": [[0, 0], [0, 3], [4, 3], [4, 0]], "height": -2})),
        ("revolve", json!({"profile": [[1, 0], [3, 0], [3, 2], [1, 2]]})),
        ("revolve", json!({"profile": [[0, 0], [3, 0], [0, 2]], "segments": 16})),
        ("sweep", json!({"profile": [[-1, -1], [1, -1], [1, 1], [-1, 1]], "path": [[0, 0, 0], [0, 0, 5], [4, 0, 8]]})),
        ("loft", json!({"sections": [[[0, 0, 0], [4, 0, 0], [4, 4, 0], [0, 4, 0]], [[1, 1, 3], [3, 1, 3], [3, 3, 3], [1, 3, 3]]]})),
    ];
    for (id, p) in cases {
        let mut s = Session::new();
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let n = r["faces"].as_u64().unwrap() as usize;
        assert!(n >= 4, "{id}: {n} faces");
        assert_eq!(faces(&s).len(), n);
        assert_eq!(handles(&r).len(), n);
        let (lo, hi) = z_range(&s);
        assert!(hi - lo > 0.5, "{id}: not 3D ({lo}..{hi})");
        assert_closed_outward(&s, &r);
        assert_eq!(s.doc().unwrap().groups.len(), 1, "{id}");
    }
}

#[test]
fn box_dimensions_and_negative_height() {
    let mut s = Session::new();
    s.execute("box", &json!({"p1": [1, 2, 3], "p2": [5, 6], "height": -2})).unwrap();
    let f = faces(&s);
    let (mut lo, mut hi) = (Vec3::new(1e9, 1e9, 1e9), Vec3::new(-1e9, -1e9, -1e9));
    for c in f.iter().flat_map(|f| f.corners) {
        lo = Vec3::new(lo.x.min(c.x), lo.y.min(c.y), lo.z.min(c.z));
        hi = Vec3::new(hi.x.max(c.x), hi.y.max(c.y), hi.z.max(c.z));
    }
    assert_eq!((lo, hi), (Vec3::new(1.0, 2.0, 1.0), Vec3::new(5.0, 6.0, 3.0)));
    // Coplanar neighbours hide their shared diagonal: a box shows 12 edges, not 18.
    assert!(f.iter().any(|f| f.hidden_edges != 0));
}

#[test]
fn undo_redo_is_one_step_and_group_is_the_solid() {
    let mut s = Session::new();
    let r = s.execute("cylinder", &json!({"center": [0, 0], "radius": 2, "height": 5})).unwrap();
    let n = faces(&s).len();
    assert_eq!(n, r["faces"].as_u64().unwrap() as usize);
    let d = s.doc().unwrap();
    assert_eq!(d.groups.len(), 1);
    let g = &d.groups[0];
    assert!(g.name.starts_with("*A") && g.selectable && g.description == super::SOLID_GROUP);
    assert_eq!(g.members.len(), n);
    // Picking one face selects the whole solid.
    let one = handles(&r)[0];
    assert_eq!(crate::cmd::groups::expand(&s, vec![one]).len(), n);
    assert_eq!(super::solid_groups(s.doc().unwrap(), &[one]).first().map(|(_, m)| m.len()), Some(n));

    s.undo().unwrap();
    assert!(faces(&s).is_empty());
    assert!(s.doc().unwrap().groups.is_empty());
    s.redo().unwrap();
    assert_eq!(faces(&s).len(), n);
    assert_eq!(s.doc().unwrap().groups.len(), 1);
}

#[test]
fn moving_the_selected_solid_moves_every_face() {
    let mut s = Session::new();
    let r = s.execute("box", &json!({"p1": [0, 0, 0], "p2": [2, 2, 2]})).unwrap();
    let sel = crate::cmd::groups::expand(&s, vec![handles(&r)[0]]);
    s.set_selection(sel.clone());
    s.execute("move", &json!({"handles": sel.iter().map(|h| h.hex()).collect::<Vec<_>>(), "delta": [10, 0]})).unwrap();
    let xs: Vec<f64> = faces(&s).iter().flat_map(|f| f.corners.map(|c| c.x)).collect();
    assert!(xs.iter().all(|x| (10.0..=12.0).contains(x)), "{xs:?}");
}

#[test]
fn new_faces_take_the_current_layer() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "SOLIDS", "current": true})).unwrap();
    s.execute("box", &json!({"p1": [0, 0, 0], "p2": [1, 1, 1]})).unwrap();
    assert!(s.doc().unwrap().model.iter().all(|e| e.common.layer == "SOLIDS"));
}

#[test]
fn extrude_existing_profiles_by_handle() {
    let mut s = Session::new();
    let poly = s
        .add_entity(EntityKind::LwPolyline(LwPolyline {
            vertices: [(0.0, 0.0), (6.0, 0.0), (6.0, 4.0), (0.0, 4.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect(),
            closed: true,
            const_width: 0.0,
            elevation: 1.0,
            plinegen: false,
        }))
        .unwrap();
    let circ = s.add_entity(EntityKind::Circle(Circle { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0 })).unwrap();
    let r = s.execute("extrude", &json!({"handles": [poly.hex(), circ.hex()], "height": 3})).unwrap();
    assert_eq!(r["solids"], 2);
    assert_eq!(s.doc().unwrap().groups.len(), 2);
    let (lo, hi) = z_range(&s);
    assert_eq!((lo, hi), (0.0, 4.0), "the polyline sits at elevation 1, the circle at 0");
    // The sources stay unless asked.
    assert!(s.doc().unwrap().entity(poly).is_some());
    s.execute("extrude", &json!({"handles": [poly.hex()], "height": 1, "deleteSource": true})).unwrap();
    assert!(s.doc().unwrap().entity(poly).is_none());
    // The selection is used when no handles are given; non-profiles in it are skipped.
    s.set_selection(vec![circ]);
    let r = s.execute("extrude", &json!({"height": 1})).unwrap();
    assert_eq!(r["solids"], 1);
}

#[test]
fn extrude_a_polyline_with_a_bulge() {
    let mut s = Session::new();
    let vs = vec![PolyVertex::with_bulge(Vec2::new(0.0, 0.0), 1.0), PolyVertex::new(Vec2::new(4.0, 0.0))];
    let h =
        s.add_entity(EntityKind::LwPolyline(LwPolyline { vertices: vs, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false })).unwrap();
    let r = s.execute("extrude", &json!({"handle": h.hex(), "height": 2})).unwrap();
    assert!(assert_closed_outward(&s, &r) > 0.0);
}

#[test]
fn revolve_a_drawn_profile_about_its_axis() {
    let mut s = Session::new();
    let vs = [(5.0, 0.0), (8.0, 0.0), (8.0, 10.0), (5.0, 10.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    let h =
        s.add_entity(EntityKind::LwPolyline(LwPolyline { vertices: vs, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false })).unwrap();
    // Axis at x = 5 (the profile's inner edge): a solid cylinder of radius 3.
    let r = s.execute("revolve", &json!({"handles": [h.hex()], "axisX": 5, "segments": 24, "center": [100, 0, 0]})).unwrap();
    assert_closed_outward(&s, &r);
    let xs: Vec<f64> = faces(&s).iter().flat_map(|f| f.corners.map(|c| c.x)).collect();
    assert!(xs.iter().all(|x| (96.9..=103.1).contains(x)));
}

#[test]
fn revolve_supports_partial_turns_and_drawn_arbitrary_axes() {
    let p = json!({"profile": [[1, 0], [2, 0], [2, 3], [1, 3]], "segments": 12, "startAngle": 30, "angle": 90});
    let mut raw = Session::new();
    let r = raw.execute("revolve", &p).unwrap();
    assert_closed_outward(&raw, &r);
    assert_eq!(r["faces"].as_u64().unwrap(), 2 * 12 * 4 + 2 * 2);

    let mut drawn = Session::new();
    let h = drawn
        .add_entity(EntityKind::LwPolyline(LwPolyline {
            vertices: [(0.0, 1.0), (4.0, 1.0), (4.0, 2.0), (0.0, 2.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect(),
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }))
        .unwrap();
    let r = drawn.execute("revolve", &json!({"handles": [h.hex()], "axis": [[0, 0, 0], [4, 0, 0]], "segments": 12, "angle": 90})).unwrap();
    assert_closed_outward(&drawn, &r);
    assert!(drawn.undo().is_ok());
    assert!(faces(&drawn).is_empty());
    assert!(drawn.redo().is_ok());
    assert_closed_outward(&drawn, &r);
}

#[test]
fn sweep_options_and_tilted_mismatched_loft_sections_are_integrated() {
    let mut s = Session::new();
    let sweep = s
        .execute(
            "sweep",
            &json!({
                "profile": [[-1, -1], [1, -1], [1, 1], [-1, 1]],
                "path": [[0, 0, 0], [0, 0, 2], [0, 0, 6]],
                "twist": 180,
                "scaleStart": 1,
                "scaleEnd": 2
            }),
        )
        .unwrap();
    assert_closed_outward(&s, &sweep);
    let m = super::mesh_of(s.doc().unwrap(), &handles(&sweep)).unwrap();
    assert_eq!(m.vertices.len(), 12);
    assert_ne!(m.vertices[0], m.vertices[8]);

    let loft = s
        .execute(
            "loft",
            &json!({"sections": [
                [[0, 0, 0], [2, 0, 0], [2, 2, 0], [0, 2, 0]],
                [[0, 0, 3], [3, 0, 3], [1.5, 2, 4]]
            ]}),
        )
        .unwrap();
    assert_closed_outward(&s, &loft);
    let m = super::mesh_of(s.doc().unwrap(), &handles(&loft)).unwrap();
    assert_eq!(m.vertices.len(), 8);
}

#[test]
fn profile_commands_persist_checked_exact_bodies() {
    let mut s = Session::new();
    let commands = [
        ("extrude", json!({"profile": [[0, 0], [2, 0], [2, 2], [0, 2]], "height": 3})),
        ("revolve", json!({"profile": [[1, 0], [2, 0], [2, 2], [1, 2]], "angle": 180})),
        ("sweep", json!({"profile": [[-1, -1], [1, -1], [1, 1], [-1, 1]], "path": [[0, 0, 0], [0, 0, 2]], "twist": 45})),
        ("loft", json!({"sections": [[[0, 0, 0], [2, 0, 0], [2, 2, 0], [0, 2, 0]], [[0, 0, 3], [2, 0, 3], [1, 2, 3]]]})),
    ];
    for (command, params) in commands {
        let result = s.execute(command, &params).unwrap();
        assert_eq!(result["representation"], "analyticBrep", "{command}");
        let body = cadcraft_doc::brep::read(s.doc().unwrap(), &handles(&result)).unwrap().unwrap();
        body.validate().unwrap();
    }

    let circle = s.add_entity(EntityKind::Circle(Circle { center: Vec3::new(10.0, 20.0, 0.0), radius: 2.0 })).unwrap();
    let result = s.execute("extrude", &json!({"handles": [circle.hex()], "height": -4, "segments": 12})).unwrap();
    let body = cadcraft_doc::brep::read(s.doc().unwrap(), &handles(&result)).unwrap().unwrap();
    assert!(
        body.geometry
            .boundaries
            .iter()
            .flat_map(|shell| &shell.edges)
            .any(|edge| matches!(edge.curve, cadcraft_geom::brep::kernel::Curve::NurbsCurve(_)))
    );
    let exact_geometry = serde_json::to_value(&body.geometry).unwrap();
    let detailed = s.execute("extrude", &json!({"handles": [circle.hex()], "height": -4, "segments": 64})).unwrap();
    let detailed_body = cadcraft_doc::brep::read(s.doc().unwrap(), &handles(&detailed)).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&detailed_body.geometry).unwrap(),
        exact_geometry,
        "exact circle extrusion must not depend on proxy tessellation"
    );
    assert!(detailed["faces"].as_u64().unwrap() > result["faces"].as_u64().unwrap(), "display tessellation still follows `segments`");
}

#[test]
fn profile_option_errors_are_atomic() {
    let mut s = Session::new();
    let h = s
        .add_entity(EntityKind::LwPolyline(LwPolyline {
            vertices: [(0.0, 1.0), (4.0, 1.0), (4.0, 2.0), (0.0, 2.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect(),
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }))
        .unwrap();
    let before = s.doc().unwrap().clone();
    for params in [
        json!({"handles": [h.hex()], "axis": [[0, 0, 0], [0, 0, 1]], "angle": 90}),
        json!({"handles": [h.hex()], "axis": [[0, 0, 0], [0, 0, 0]]}),
        json!({"profile": [[1, 0], [2, 0], [2, 1]], "angle": 0}),
        json!({"profile": [[1, 0], [2, 0], [2, 1]], "startAngle": "NaN"}),
    ] {
        assert!(s.execute("revolve", &params).is_err(), "{params}");
        assert_eq!(s.doc().unwrap(), &before, "failed revolve changed the drawing");
    }
    for params in [
        json!({"profile": [[0, 0], [1, 0], [1, 1]], "path": [[0, 0, 0], [0, 0, 2]], "scaleEnd": 0}),
        json!({"profile": [[0, 0], [1, 0], [1, 1]], "path": [[0, 0, 0], [0, 0, 2]], "twist": 1e9}),
    ] {
        assert!(s.execute("sweep", &params).is_err(), "{params}");
        assert_eq!(s.doc().unwrap(), &before, "failed sweep changed the drawing");
    }
}

#[test]
fn sweep_along_a_drawn_path_and_loft_from_profiles() {
    let mut s = Session::new();
    let sq = |x: f64, y: f64, w: f64, z: f64| {
        EntityKind::LwPolyline(LwPolyline {
            vertices: [(x, y), (x + w, y), (x + w, y + w), (x, y + w)].iter().map(|&(a, b)| PolyVertex::new(Vec2::new(a, b))).collect(),
            closed: true,
            const_width: 0.0,
            elevation: z,
            plinegen: false,
        })
    };
    let prof = s.add_entity(sq(50.0, 50.0, 2.0, 0.0)).unwrap();
    let path = s.add_entity(EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(0.0, 0.0, 10.0) })).unwrap();
    let r = s.execute("sweep", &json!({"handles": [prof.hex()], "pathHandle": path.hex()})).unwrap();
    assert_closed_outward(&s, &r);
    let xs: Vec<f64> = handles(&r)
        .iter()
        .filter_map(|h| s.doc().unwrap().entity(*h).cloned())
        .filter_map(|e| if let EntityKind::Face3d(f) = &e.kind { Some(f.corners[0].x) } else { None })
        .collect();
    assert!(xs.iter().all(|x| x.abs() <= 1.0 + 1e-9), "the profile is centred on the path");

    let a = s.add_entity(sq(0.0, 0.0, 4.0, 0.0)).unwrap();
    let b = s.add_entity(sq(1.0, 1.0, 2.0, 5.0)).unwrap();
    let r = s.execute("loft", &json!({"handles": [b.hex(), a.hex()]})).unwrap();
    assert_closed_outward(&s, &r);
    // Same elevation: stacked by spacing instead.
    let c = s.add_entity(sq(0.0, 0.0, 2.0, 0.0)).unwrap();
    assert!(s.execute("loft", &json!({"handles": [a.hex(), c.hex()]})).is_err());
    let r = s.execute("loft", &json!({"handles": [a.hex(), c.hex()], "spacing": 3})).unwrap();
    assert_closed_outward(&s, &r);
}

#[test]
fn hostile_params_are_errors_and_leave_the_drawing_alone() {
    let mut s = Session::new();
    let nan = Value::Null;
    let bad = [
        ("box", json!({"p1": [0, 0, 0], "p2": [0, 0, 0]})),
        ("box", json!({"p1": [0, 0, 0], "p2": [1, 1, 0]})),
        ("box", json!({"p1": [0, 0, 0], "p2": [1e300, 1, 1]})),
        ("box", json!({"p1": "x", "p2": [1, 1, 1]})),
        ("box", json!({})),
        ("cylinder", json!({"radius": -1, "height": 1})),
        ("cylinder", json!({"radius": 1, "height": 0})),
        ("cylinder", json!({"radius": 1, "height": nan})),
        ("cylinder", json!({"radius": 1, "height": 1, "segments": 1_000_000})),
        ("cylinder", json!({"radius": 1, "height": 1, "segments": -4})),
        ("cylinder", json!({"radius": 1e300, "height": 1})),
        ("cone", json!({"radius": "NaN", "height": 1})),
        ("sphere", json!({"radius": 5, "segments": 512, "rings": 256})),
        ("sphere", json!({"radius": 0})),
        ("torus", json!({"radius": 1, "tubeRadius": 2})),
        ("pyramid", json!({"radius": 1, "height": 1, "sides": 2})),
        ("pyramid", json!({"side": -1, "height": 1})),
        ("extrude", json!({"profile": [], "height": 1})),
        ("extrude", json!({"profile": [[0, 0], [1, 1]], "height": 1})),
        ("extrude", json!({"profile": [[0, 0], [4, 4], [4, 0], [0, 4]], "height": 1})),
        ("extrude", json!({"profile": [[0, 0], [1, 0], [1, 1]], "height": 0})),
        ("extrude", json!({"profile": [[0, 0], [1, 0], [1, 1]], "height": 1e300})),
        ("extrude", json!({"profile": [[0, 0], [1, 0], ["a", 1]], "height": 1})),
        ("extrude", json!({"handles": ["zzzz"], "height": 1})),
        ("extrude", json!({"handles": ["FFFFFF"], "height": 1})),
        ("extrude", json!({"height": 1})),
        ("revolve", json!({"profile": [[-1, 0], [1, 0], [1, 1]]})),
        ("revolve", json!({"profile": [[1, 0], [2, 0], [2, 1]], "segments": 2})),
        ("sweep", json!({"profile": [[0, 0], [1, 0], [1, 1]]})),
        ("sweep", json!({"profile": [[0, 0], [1, 0], [1, 1]], "path": [[0, 0, 0]]})),
        ("sweep", json!({"profile": [[0, 0], [1, 0], [1, 1]], "path": [[0, 0, 0], [0, 0, 0]]})),
        ("sweep", json!({"profile": [[0, 0], [1, 0], [1, 1]], "pathHandle": "nope"})),
        ("loft", json!({"sections": []})),
        ("loft", json!({"sections": [[[0, 0, 0], [1, 0, 0], [1, 1, 0]], [[0, 0, 1], [1, 0, 1]]]})),
        ("loft", json!({"sections": [[[0, 0, 0], [1, 0, 0], [1, 1, 0]], [[0, 0, 0], [1, 0, 0], [1, 1, 0]]]})),
    ];
    for (id, p) in bad {
        assert!(s.execute(id, &p).is_err(), "{id} {p} should fail");
    }
    assert!(faces(&s).is_empty());
    assert!(s.doc().unwrap().groups.is_empty());
    // Absurdly many sections or points are refused before allocating.
    let many: Vec<Value> = (0..5000).map(|i| json!([i, 0])).collect();
    assert!(s.execute("extrude", &json!({"profile": many, "height": 1})).is_err());
    let secs: Vec<Value> = (0..300).map(|i| json!([[0, 0, i], [1, 0, i], [1, 1, i]])).collect();
    assert!(s.execute("loft", &json!({"sections": secs})).is_err());
}

#[test]
fn the_face_cap_is_enforced() {
    let mut s = Session::new();
    // 512 x 256 would be 262k triangles: refused by the geometry limit or the command cap.
    assert!(s.execute("torus", &json!({"radius": 5, "tubeRadius": 1, "segments": 512, "tubeSegments": 512})).is_err());
    assert!(faces(&s).is_empty());
}

#[test]
fn dxf_round_trip_keeps_xyz_and_the_group() {
    let mut s = Session::new();
    s.execute("cone", &json!({"center": [3, 4, 5], "radius": 2, "height": 6, "segments": 16})).unwrap();
    let before = faces(&s);
    let bytes = cadcraft_io::write(s.doc().unwrap(), "x.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "x.dxf").unwrap();
    let after: Vec<Face3d> = back.model.iter().filter_map(|e| if let EntityKind::Face3d(f) = &e.kind { Some(f.clone()) } else { None }).collect();
    assert_eq!(after.len(), before.len());
    let zmax = after.iter().flat_map(|f| f.corners.map(|c| c.z)).fold(f64::MIN, f64::max);
    assert!((zmax - 11.0).abs() < 1e-9);
    for (a, b) in before.iter().zip(&after) {
        for (p, q) in a.corners.iter().zip(&b.corners) {
            assert!((*p - *q).len() < 1e-9);
        }
    }
    assert_eq!(back.groups.len(), 1);
    assert_eq!(back.groups[0].members.len(), before.len());
    assert!(back.groups[0].selectable);
    // The reopened solid still welds into the same closed mesh.
    let hs: Vec<Handle> = back.groups[0].members.clone();
    let m = super::mesh_of(&back, &hs).unwrap();
    assert_eq!(m.triangles.len(), before.len());
}

#[test]
fn mesh_of_welds_splits_quads_and_skips_junk() {
    let mut s = Session::new();
    let v = |x, y, z| Vec3::new(x, y, z);
    let quad = EntityKind::Face3d(Face3d { corners: [v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), v(0.0, 1.0, 0.0)], hidden_edges: 0 });
    let tri = EntityKind::Face3d(Face3d { corners: [v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(1.0, 1.0, 0.0), v(1.0, 1.0, 0.0)], hidden_edges: 0 });
    let degenerate =
        EntityKind::Face3d(Face3d { corners: [v(5.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(6.0, 0.0, 0.0), v(6.0, 0.0, 0.0)], hidden_edges: 0 });
    let hs: Vec<Handle> = [quad, tri, degenerate, EntityKind::Point(cadcraft_doc::Point { p: v(0.0, 0.0, 0.0), angle: 0.0 })]
        .into_iter()
        .map(|k| s.add_entity(k).unwrap())
        .collect();
    let m = super::mesh_of(s.doc().unwrap(), &hs).unwrap();
    assert_eq!((m.triangles.len(), m.vertices.len()), (3, 5 + 2));
    assert!(super::mesh_of(s.doc().unwrap(), &hs[2..]).is_err());
}

// ---------------- the prompt sequences ----------------

fn drive(s: &mut Session, id: &str, inputs: Vec<Input>) {
    s.set_selection(Vec::new());
    let spec = crate::find_command(id).unwrap();
    let mut m = (spec.interactive.unwrap())(s).unwrap();
    if m.begin(s).unwrap() == Step::Done {
        return;
    }
    let mut last = Step::Continue;
    for i in inputs {
        last = m.input(s, i).unwrap();
    }
    assert_eq!(last, Step::Done, "{id} did not finish");
}

#[test]
fn interactive_primitives_make_solids() {
    let p = |x: f64, y: f64| Input::Point(Vec2::new(x, y));
    let t = |s: &str| Input::Text(s.into());
    for (id, inputs) in [
        ("box", vec![p(0.0, 0.0), p(4.0, 3.0), t("2")]),
        ("box", vec![p(0.0, 0.0), p(4.0, 3.0), p(0.0, 5.0)]),
        ("cylinder", vec![p(0.0, 0.0), t("2"), t("5")]),
        ("cone", vec![p(0.0, 0.0), p(3.0, 0.0), t("-4")]),
        ("sphere", vec![p(1.0, 1.0), t("3")]),
        ("torus", vec![p(0.0, 0.0), t("5"), t("1")]),
        ("pyramid", vec![p(0.0, 0.0), t("3"), t("4")]),
    ] {
        let mut s = Session::new();
        drive(&mut s, id, inputs);
        assert!(!faces(&s).is_empty(), "{id}");
        assert_eq!(s.doc().unwrap().groups.len(), 1, "{id}");
    }
    // A bad answer is refused and asked again.
    let mut s = Session::new();
    let spec = crate::find_command("sphere").unwrap();
    let mut m = (spec.interactive.unwrap())(&s).unwrap();
    m.input(&mut s, p(0.0, 0.0)).unwrap();
    assert!(m.input(&mut s, t("-2")).is_err());
    assert!(m.input(&mut s, t("abc")).is_err());
    assert_eq!(m.input(&mut s, t("2")).unwrap(), Step::Done);
}

#[test]
fn interactive_pyramid_sides_keyword() {
    let mut s = Session::new();
    let spec = crate::find_command("pyramid").unwrap();
    let mut m = (spec.interactive.unwrap())(&s).unwrap();
    m.input(&mut s, Input::Keyword("Sides".into())).unwrap();
    m.input(&mut s, Input::Text("6".into())).unwrap();
    m.input(&mut s, Input::Point(Vec2::ZERO)).unwrap();
    m.input(&mut s, Input::Text("2".into())).unwrap();
    assert_eq!(m.input(&mut s, Input::Text("3".into())).unwrap(), Step::Done);
    assert_eq!(faces(&s).len(), 6 + 6);
}

#[test]
fn interactive_extrude_revolve_sweep_loft_use_picked_objects() {
    let mut s = Session::new();
    let circ = s.add_entity(EntityKind::Circle(Circle { center: Vec3::new(10.0, 0.0, 0.0), radius: 2.0 })).unwrap();
    let line = s.add_entity(EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 9.0) })).unwrap();
    let circ2 = s.add_entity(EntityKind::Circle(Circle { center: Vec3::new(10.0, 0.0, 0.0), radius: 1.0 })).unwrap();
    drive(&mut s, "extrude", vec![Input::Pick(vec![circ]), Input::Enter, Input::Text("4".into())]);
    drive(&mut s, "revolve", vec![Input::Pick(vec![circ]), Input::Enter, Input::Point(Vec2::new(5.0, 0.0))]);
    drive(&mut s, "sweep", vec![Input::Pick(vec![circ]), Input::Enter, Input::Pick(vec![line])]);
    drive(&mut s, "loft", vec![Input::Pick(vec![circ, circ2]), Input::Enter, Input::Text("3".into())]);
    assert_eq!(s.doc().unwrap().groups.len(), 4);
    for group in &s.doc().unwrap().groups {
        cadcraft_doc::brep::read(s.doc().unwrap(), &group.members).unwrap().unwrap().validate().unwrap();
    }
    let revolved = cadcraft_doc::brep::read(s.doc().unwrap(), &s.doc().unwrap().groups[1].members).unwrap().unwrap();
    assert!(
        revolved
            .geometry
            .boundaries
            .iter()
            .flat_map(|shell| &shell.faces)
            .any(|face| matches!(face.surface, cadcraft_geom::brep::kernel::Surface::RevolutedCurve(_))),
        "circle REVOLVE should retain its exact revolved surface"
    );

    // A pickfirst selection skips the object prompt.
    s.set_selection(vec![circ]);
    let spec = crate::find_command("extrude").unwrap();
    let mut m = (spec.interactive.unwrap())(&s).unwrap();
    assert_eq!(m.begin(&mut s).unwrap(), Step::Continue);
    assert!(m.prompt(&s).message.contains("height"));
    s.set_selection(Vec::new());
    // Esc cancels without a trace.
    let n = s.doc().unwrap().model.iter().count();
    let spec = crate::find_command("extrude").unwrap();
    let mut m = (spec.interactive.unwrap())(&s).unwrap();
    assert_eq!(m.input(&mut s, Input::Cancel).unwrap(), Step::Cancel);
    assert_eq!(s.doc().unwrap().model.iter().count(), n);
}

#[test]
fn palette_commands_are_registered_with_prompts() {
    for id in ["box", "cylinder", "cone", "sphere", "torus", "pyramid", "extrude", "revolve", "sweep", "loft"] {
        let c = crate::find_command(id).unwrap_or_else(|| panic!("{id} missing"));
        assert!(c.interactive.is_some() && !c.params.is_empty() && c.undoable, "{id}");
    }
}

// ---------------- booleans ----------------

fn volume_of(s: &Session, hs: &[Handle]) -> f64 {
    let m = super::mesh_of(s.doc().unwrap(), hs).unwrap();
    m.triangles.iter().map(|t| m.vertices[t[0]].dot(m.vertices[t[1]].cross(m.vertices[t[2]])) / 6.0).sum()
}

fn bbox_of(s: &Session, hs: &[Handle]) -> (Vec3, Vec3) {
    let m = super::mesh_of(s.doc().unwrap(), hs).unwrap();
    let f = |g: fn(f64, f64) -> f64, init: f64| {
        m.vertices.iter().fold(Vec3::new(init, init, init), |a, v| Vec3::new(g(a.x, v.x), g(a.y, v.y), g(a.z, v.z)))
    };
    (f(f64::min, 1e18), f(f64::max, -1e18))
}

fn hex(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

fn two_boxes(s: &mut Session) -> (Vec<Handle>, Vec<Handle>) {
    let a = handles(&s.execute("box", &json!({"p1": [0, 0, 0], "p2": [4, 4, 4]})).unwrap());
    let b = handles(&s.execute("box", &json!({"p1": [2, 2, 2], "p2": [6, 6, 6]})).unwrap());
    (a, b)
}

fn assert_closed(s: &Session, hs: &[Handle]) {
    let m = super::mesh_of(s.doc().unwrap(), hs).unwrap();
    let mut edges = std::collections::HashMap::new();
    for t in &m.triangles {
        for k in 0..3 {
            *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    for (&(a, b), &n) in &edges {
        assert_eq!(n, 1);
        assert_eq!(edges.get(&(b, a)), Some(&1));
    }
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).len() < 1e-6
}

#[test]
fn union_of_two_boxes() {
    let mut s = Session::new();
    let (a, b) = two_boxes(&mut s);
    let all: Vec<_> = a.iter().chain(&b).copied().collect();
    let r = s.execute("union", &json!({"handles": hex(&[a[0], b[0]])})).unwrap();
    let out = handles(&r);
    assert_closed(&s, &out);
    assert!((volume_of(&s, &out) - 120.0).abs() < 1e-6);
    let (lo, hi) = bbox_of(&s, &out);
    assert!(near(lo, Vec3::ZERO) && near(hi, Vec3::new(6.0, 6.0, 6.0)));
    // The sources and their groups are gone; one group holds the result.
    assert!(all.iter().all(|h| s.doc().unwrap().entity(*h).is_none()));
    assert_eq!(s.doc().unwrap().groups.len(), 1);
    assert_eq!(faces(&s).len(), out.len());
    // One undo step brings both originals back.
    s.undo().unwrap();
    assert_eq!(faces(&s).len(), all.len());
    assert_eq!(s.doc().unwrap().groups.len(), 2);
    assert!(all.iter().all(|h| s.doc().unwrap().entity(*h).is_some()));
}

#[test]
fn subtract_and_intersect_two_boxes() {
    let mut s = Session::new();
    let (a, b) = two_boxes(&mut s);
    let r = s.execute("subtract", &json!({"from": hex(&a[..1]), "remove": hex(&b[..1])})).unwrap();
    let out = handles(&r);
    assert_closed(&s, &out);
    assert!((volume_of(&s, &out) - 56.0).abs() < 1e-6);
    let (lo, hi) = bbox_of(&s, &out);
    assert!(near(lo, Vec3::ZERO) && near(hi, Vec3::new(4.0, 4.0, 4.0)));
    assert_eq!(s.doc().unwrap().groups.len(), 1);
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().groups.len(), 2);
    assert_eq!(faces(&s).len(), a.len() + b.len());

    let r = s.execute("intersect", &json!({"handles": hex(&[a[0], b[0]])})).unwrap();
    let out = handles(&r);
    assert_closed(&s, &out);
    assert!((volume_of(&s, &out) - 8.0).abs() < 1e-6);
    let (lo, hi) = bbox_of(&s, &out);
    assert!(near(lo, Vec3::new(2.0, 2.0, 2.0)) && near(hi, Vec3::new(4.0, 4.0, 4.0)));
    s.undo().unwrap();
    assert_eq!(faces(&s).len(), a.len() + b.len());
}

#[test]
fn boolean_of_a_box_and_a_cylinder() {
    let mut s = Session::new();
    let a = handles(&s.execute("box", &json!({"p1": [-2, -2, 0], "p2": [2, 2, 2]})).unwrap());
    let b = handles(&s.execute("cylinder", &json!({"center": [0, 0, -1], "radius": 1, "height": 4, "segments": 24})).unwrap());
    let r = s.execute("subtract", &json!({"from": hex(&a[..1]), "remove": hex(&b[..1])})).unwrap();
    let out = handles(&r);
    assert_closed(&s, &out);
    let cyl = 0.5 * 24.0 * (TAU_F / 24.0).sin() * 2.0;
    assert!((volume_of(&s, &out) - (32.0 - cyl)).abs() < 1e-6, "{}", volume_of(&s, &out));
}

const TAU_F: f64 = std::f64::consts::TAU;

#[test]
fn boolean_errors_leave_the_drawing_alone() {
    let mut s = Session::new();
    let (a, _b) = two_boxes(&mut s);
    let far = handles(&s.execute("box", &json!({"p1": [20, 20, 20], "p2": [22, 22, 22]})).unwrap());
    let line = s.add_entity(EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })).unwrap();
    let before = s.doc().unwrap().clone();
    let same = |s: &Session| s.doc().unwrap().model.iter().count() == before.model.iter().count() && s.doc().unwrap().groups == before.groups;
    // Disjoint solids have no common volume.
    let e = s.execute("intersect", &json!({"handles": hex(&[a[0], far[0]])})).unwrap_err().to_string();
    assert!(e.contains("do not overlap"), "{e}");
    assert!(same(&s));
    // One solid, a mix of solid and line, a solid subtracted from itself, bad handles.
    for (id, p) in [
        ("union", json!({"handles": hex(&a[..1])})),
        ("union", json!({"handles": [a[0].hex(), line.hex()]})),
        ("intersect", json!({"handles": [a[0].hex(), line.hex()]})),
        ("subtract", json!({"from": hex(&a[..1]), "remove": hex(&a[1..2])})),
        ("subtract", json!({"from": hex(&a[..1]), "remove": [line.hex()]})),
        ("subtract", json!({"from": hex(&a[..1])})),
        ("union", json!({"handles": ["zzzz", "FFFFFF"]})),
        ("union", json!({"handles": [a[0].hex(), "FFFFFF"]})),
        ("union", json!({"handles": [1.5, null]})),
    ] {
        assert!(s.execute(id, &p).is_err(), "{id} {p}");
        assert!(same(&s), "{id} {p} changed the drawing");
    }
}

#[test]
fn region_booleans_are_unchanged() {
    // Without 3D solids the 2D path still applies (and still reports its own errors).
    let mut s = Session::new();
    assert!(s.execute("union", &json!({"handles": []})).is_err());
}

// ---------------- exchange with other formats ----------------

fn tri_stats(d: &cadcraft_doc::Drawing) -> (usize, Vec3, Vec3) {
    let mut n = 0;
    let (mut lo, mut hi) = (Vec3::new(1e18, 1e18, 1e18), Vec3::new(-1e18, -1e18, -1e18));
    for e in d.model.iter() {
        if let EntityKind::Face3d(f) = &e.kind {
            n += 1;
            for c in &f.corners {
                lo = Vec3::new(lo.x.min(c.x), lo.y.min(c.y), lo.z.min(c.z));
                hi = Vec3::new(hi.x.max(c.x), hi.y.max(c.y), hi.z.max(c.z));
            }
        }
    }
    (n, lo, hi)
}

#[test]
fn generated_solids_survive_obj_stl_and_dxf() {
    let mut s = Session::new();
    s.execute("box", &json!({"p1": [0, 0, 0], "p2": [4, 3, 2]})).unwrap();
    s.execute("cylinder", &json!({"center": [10, 5, 1], "radius": 2, "height": 6, "segments": 20})).unwrap();
    let (n, lo, hi) = tri_stats(s.doc().unwrap());
    assert_eq!(n, 12 + (20 * 2 + 2 * 18));
    for name in ["x.obj", "x.stl", "x.dxf"] {
        let bytes = cadcraft_io::write(s.doc().unwrap(), name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let back = cadcraft_io::read(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (m, l2, h2) = tri_stats(&back);
        assert_eq!(m, n, "{name}: triangle count");
        let tol = if name == "x.stl" { 1e-4 } else { 1e-9 };
        assert!((l2 - lo).len() < tol && (h2 - hi).len() < tol, "{name}: bbox {l2:?} {h2:?} vs {lo:?} {hi:?}");
    }
}

// ---------------- typed prompt flows ----------------

fn line(s: &mut Session, text: &str) {
    s.cmdline(text).unwrap_or_else(|e| panic!("`{text}`: {e}"));
}

#[test]
fn typed_primitive_flows() {
    for (cmd, lines) in [
        ("box", vec!["0,0", "4,3", "2"]),
        ("cylinder", vec!["0,0", "2", "5"]),
        ("cone", vec!["0,0", "2", "5"]),
        ("sphere", vec!["0,0", "2"]),
        ("torus", vec!["0,0", "5", "1"]),
        ("pyramid", vec!["0,0", "3", "4"]),
    ] {
        let mut s = Session::new();
        s.start(cmd).unwrap();
        assert!(s.running.is_some(), "{cmd}");
        for l in &lines {
            line(&mut s, l);
        }
        assert!(s.running.is_none(), "{cmd} still running: {}", s.prompt_text());
        assert!(!faces(&s).is_empty(), "{cmd}");
        assert_eq!(s.doc().unwrap().groups.len(), 1);
        s.undo().unwrap();
        assert!(faces(&s).is_empty(), "{cmd}: one undo step");
    }
}

#[test]
fn typed_profile_flows() {
    let mut s = Session::new();
    let sq = |s: &mut Session, x: f64, w: f64| {
        s.add_entity(EntityKind::LwPolyline(LwPolyline {
            vertices: [(x, 0.0), (x + w, 0.0), (x + w, w), (x, w)].iter().map(|&(a, b)| PolyVertex::new(Vec2::new(a, b))).collect(),
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }))
        .unwrap()
    };
    let p1 = sq(&mut s, 5.0, 2.0);
    let p2 = sq(&mut s, 5.0, 1.0);
    let path = s.add_entity(EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 9.0) })).unwrap();

    s.set_selection(vec![p1]);
    s.start("extrude").unwrap();
    assert!(s.prompt_text().to_lowercase().contains("height"));
    line(&mut s, "4");
    assert!(s.running.is_none());

    s.set_selection(vec![p1]);
    s.start("revolve").unwrap();
    line(&mut s, "0,0");
    assert!(s.running.is_none());

    s.set_selection(vec![p1]);
    s.start("sweep").unwrap();
    s.input(Input::Pick(vec![path])).unwrap();
    assert!(s.running.is_none());

    s.set_selection(vec![p1, p2]);
    s.start("loft").unwrap();
    line(&mut s, "3");
    assert!(s.running.is_none());

    assert_eq!(s.doc().unwrap().groups.len(), 4);
    // Without a pre-selection the object prompt comes first, and Enter on nothing cancels.
    s.set_selection(Vec::new());
    s.start("extrude").unwrap();
    assert!(s.prompt_text().contains("Select"));
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().groups.len(), 4);
}

#[test]
fn mesh_booleans_refuse_locked_group_members_atomically() {
    for command in ["union", "subtract", "intersect"] {
        for locked_operand in [0, 1] {
            let mut s = Session::new();
            let (a, b) = two_boxes(&mut s);
            s.execute("layer.new", &json!({"name": "Protected"})).unwrap();
            // Protect an unpicked face: checking only the supplied handles is insufficient.
            let protected = if locked_operand == 0 { a[1] } else { b[1] };
            s.doc_mut().unwrap().modify_entity(protected, |e| e.common.layer = "Protected".into()).unwrap();
            s.execute("layer.set", &json!({"name": "Protected", "locked": true})).unwrap();
            let params = if command == "subtract" {
                json!({"from": [a[0].hex()], "remove": [b[0].hex()]})
            } else {
                json!({"handles": [a[0].hex(), b[0].hex()]})
            };
            let before = s.state().unwrap().doc.clone();
            let error = s.execute(command, &params).unwrap_err().to_string();
            assert!(error.contains("locked"), "{command}: {error}");
            assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc), "{command} changed the document");
            s.execute("layer.set", &json!({"name": "Protected", "locked": false})).unwrap();
            assert!(s.execute(command, &params).is_ok(), "{command} should work after unlocking");
        }
    }
}

/// Inputs within the command limits but past the exact builders' (more than 256 profile points,
/// more than 64 loft sections) still make the solid, mesh-only, and say why (#2544 review).
#[test]
fn profiles_past_the_exact_limits_fall_back_to_mesh_only_solids() {
    let mut s = Session::new();
    let ring: Vec<Value> = (0..300)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 300.0;
            json!([10.0 * a.cos(), 10.0 * a.sin()])
        })
        .collect();
    let r = s.execute("extrude", &json!({"profile": ring, "height": 3})).unwrap();
    assert_eq!(r["representation"], "mesh", "{r}");
    assert!(r["notes"][0].as_str().unwrap().contains("mesh-only"), "{r}");
    assert_closed_outward(&s, &r);
    let info = s.execute("solidinfo", &json!({"handles": [r["handles"][0]]}));
    assert!(info.is_err(), "a mesh-only solid has no exact record");

    let sections: Vec<Value> = (0..100).map(|i| json!([[0, 0, i], [2, 0, i], [2, 2, i], [0, 2, i]])).collect();
    let r = s.execute("loft", &json!({"sections": sections})).unwrap();
    assert_eq!(r["representation"], "mesh", "{r}");
    assert_closed_outward(&s, &r);

    // Within the exact limits nothing changes.
    let r = s.execute("extrude", &json!({"profile": [[0, 0], [1, 0], [1, 1]], "height": 1})).unwrap();
    assert_eq!(r["representation"], "analyticBrep");
    assert!(r.get("notes").is_none());
}
