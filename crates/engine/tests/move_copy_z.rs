//! #1407: MOVE/COPY carry a Z delta; planar ROTATE/SCALE/MIRROR preserve Z.
use cadcraft_doc::{EntityKind, Handle};
use cadcraft_engine::Session;
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn poly_z(s: &Session, hd: Handle) -> Vec<Vec3> {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::Polyline3d(p) => p.points.clone(),
        k => panic!("not a 3d polyline: {k:?}"),
    }
}

fn helix_center(s: &Session, hd: Handle) -> Vec3 {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::Helix(x) => x.center,
        k => panic!("not a helix: {k:?}"),
    }
}

fn mk_poly(s: &mut Session) -> Handle {
    h(&s.execute("3dpoly", &json!({"points": [[0, 0, 0], [10, 0, 5], [10, 10, 10]]})).unwrap())
}

fn fresh_z(s: &mut Session) -> Vec<f64> {
    let a = mk_poly(s);
    poly_z(s, a).into_iter().map(|p| p.z).collect()
}

fn mk_helix(s: &mut Session) -> Handle {
    h(&s.execute("helix", &json!({"center": [0, 0], "baseRadius": 2, "height": 5, "turns": 2})).unwrap())
}

#[test]
fn move_carries_z_delta() {
    let mut s = Session::new();
    let a = mk_poly(&mut s);
    s.execute("move", &json!({"handles": [a.hex()], "delta": [1, 2, 3]})).unwrap();
    assert_eq!(poly_z(&s, a), vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(11.0, 2.0, 8.0), Vec3::new(11.0, 12.0, 13.0)]);

    // from/to with Z: the delta is to[2] - from[2].
    let b = mk_poly(&mut s);
    s.execute("move", &json!({"handles": [b.hex()], "from": [0, 0, 1], "to": [5, 5, 4]})).unwrap();
    assert_eq!(poly_z(&s, b)[0], Vec3::new(5.0, 5.0, 3.0));

    // A helix moves by its base centre.
    let c = mk_helix(&mut s);
    s.execute("move", &json!({"handles": [c.hex()], "delta": [1, 2, 3]})).unwrap();
    assert_eq!(helix_center(&s, c), Vec3::new(1.0, 2.0, 3.0));

    // A non-numeric Z is an error, never a silent drop.
    assert!(s.execute("move", &json!({"handles": [a.hex()], "delta": [0, 0, "x"]})).is_err());
}

#[test]
fn copy_carries_z_delta() {
    let mut s = Session::new();
    let a = mk_poly(&mut s);
    let out = s.execute("copy", &json!({"handles": [a.hex()], "delta": [1, 2, 3], "count": 2})).unwrap();
    let hs: Vec<Handle> = out["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    assert_eq!(hs.len(), 2);
    assert_eq!(poly_z(&s, hs[0])[2], Vec3::new(11.0, 12.0, 13.0));
    assert_eq!(poly_z(&s, hs[1])[2], Vec3::new(12.0, 14.0, 16.0));
    // The source is untouched.
    assert_eq!(poly_z(&s, a)[2], Vec3::new(10.0, 10.0, 10.0));

    let c = mk_helix(&mut s);
    let out = s.execute("copy", &json!({"handles": [c.hex()], "delta": [0, 0, 4]})).unwrap();
    let d = Handle::parse_hex(out["handles"][0].as_str().unwrap()).unwrap();
    assert_eq!(helix_center(&s, d), Vec3::new(0.0, 0.0, 4.0));
    assert_eq!(helix_center(&s, c), Vec3::ZERO);
}

#[test]
fn planar_ops_preserve_z() {
    let mut s = Session::new();
    assert_eq!(fresh_z(&mut s), vec![0.0, 5.0, 10.0]);

    let a = mk_poly(&mut s);
    s.execute("rotate", &json!({"handles": [a.hex()], "base": [0, 0], "angle": 90})).unwrap();
    assert_eq!(poly_z(&s, a).into_iter().map(|p| p.z).collect::<Vec<_>>(), vec![0.0, 5.0, 10.0]);

    let b = mk_poly(&mut s);
    s.execute("scale", &json!({"handles": [b.hex()], "base": [0, 0], "factor": 2})).unwrap();
    // SCALE scales the heights too, as AutoCAD 2024 does: Z is measured about the base point's Z.
    assert_eq!(poly_z(&s, b).into_iter().map(|p| p.z).collect::<Vec<_>>(), vec![0.0, 10.0, 20.0]);

    let c = mk_poly(&mut s);
    s.execute("mirror", &json!({"handles": [c.hex()], "p1": [0, 0], "p2": [0, 1], "erase": true})).unwrap();
    let d = s.doc().unwrap().model.iter().last().unwrap().handle;
    assert_eq!(poly_z(&s, d).into_iter().map(|p| p.z).collect::<Vec<_>>(), vec![0.0, 5.0, 10.0]);

    // A helix keeps its base height under planar edits.
    let e = mk_helix(&mut s);
    s.execute("rotate", &json!({"handles": [e.hex()], "base": [0, 0], "angle": 90})).unwrap();
    s.execute("scale", &json!({"handles": [e.hex()], "base": [0, 0], "factor": 2})).unwrap();
    assert_eq!(helix_center(&s, e).z, 0.0);
}
