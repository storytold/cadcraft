//! #1407: MOVE/COPY carry a Z delta; planar ROTATE/SCALE/MIRROR preserve Z.
use cadcraft_doc::{EntityKind, HAlign, Handle, RayLine, Text, VAlign};
use cadcraft_engine::Session;
use cadcraft_geom::{FitOptions, Spline, Vec2, Vec3};
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

fn mk_text(s: &mut Session) -> Handle {
    s.add_entity(EntityKind::Text(Text {
        insert: Vec3::new(1.0, 2.0, 4.0),
        align_pt: Some(Vec3::new(3.0, 5.0, 6.0)),
        height: 1.0,
        value: "text".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Center,
        valign: VAlign::Baseline,
    }))
    .unwrap()
}

fn mk_insert_with_attribute(s: &mut Session) -> Handle {
    let def = h(&s.execute("attdef", &json!({"tag": "TAG", "at": [1, 2], "height": 1, "default": "D"})).unwrap());
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": [def.hex()], "keep": "delete"})).unwrap();
    let ins = h(&s.execute("insert", &json!({"name": "B", "at": [10, 20], "attribs": {"TAG": "value"}})).unwrap());
    s.doc_mut()
        .unwrap()
        .modify_entity(ins, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.insert.z = 4.0;
                i.attribs[0].text.insert.z = 4.0;
                i.attribs[0].text.align_pt = Some(Vec3::new(11.0, 22.0, 6.0));
            }
        })
        .unwrap();
    ins
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
    assert_eq!(poly_z(&s, b).into_iter().map(|p| p.z).collect::<Vec<_>>(), vec![0.0, 5.0, 10.0]);

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

#[test]
fn move_carries_z_to_text_and_insert_attributes() {
    let mut s = Session::new();
    let text = mk_text(&mut s);
    let insert = mk_insert_with_attribute(&mut s);

    s.execute("move", &json!({"handles": [text.hex(), insert.hex()], "delta": [1, 2, 3]})).unwrap();

    let EntityKind::Text(t) = &s.doc().unwrap().entity(text).unwrap().kind else { panic!("not text") };
    assert_eq!(t.insert, Vec3::new(2.0, 4.0, 7.0));
    assert_eq!(t.align_pt, Some(Vec3::new(4.0, 7.0, 9.0)));

    let EntityKind::Insert(i) = &s.doc().unwrap().entity(insert).unwrap().kind else { panic!("not insert") };
    assert_eq!(i.insert, Vec3::new(11.0, 22.0, 7.0));
    assert_eq!(i.attribs[0].text.insert, Vec3::new(12.0, 24.0, 7.0));
    assert_eq!(i.attribs[0].text.align_pt, Some(Vec3::new(12.0, 24.0, 9.0)));
}

#[test]
fn spline_is_planar_and_tangents_are_directions() {
    let mut s = Session::new();
    let spline = Spline::fit_with(
        &[Vec2::new(0.0, 0.0), Vec2::new(2.0, 1.0), Vec2::new(4.0, 0.0)],
        false,
        FitOptions { start_tangent: Some(Vec2::X), end_tangent: Some(-Vec2::X), ..FitOptions::default() },
    );
    let original_control = spline.control.clone();
    let original_fit = spline.fit.clone();
    let original_start = spline.fit_opts.start_tangent;
    let original_end = spline.fit_opts.end_tangent;
    let spline_h = s.add_entity(EntityKind::Spline(spline)).unwrap();
    let ray_h = s.add_entity(EntityKind::Ray(RayLine { base: Vec3::new(0.0, 0.0, 4.0), dir: Vec3::new(1.0, 0.0, 9.0) })).unwrap();

    s.execute("move", &json!({"handles": [spline_h.hex(), ray_h.hex()], "delta": [1, 2, 3]})).unwrap();

    let EntityKind::Spline(moved) = &s.doc().unwrap().entity(spline_h).unwrap().kind else { panic!("not spline") };
    assert_eq!(moved.control, original_control.iter().map(|p| *p + Vec2::new(1.0, 2.0)).collect::<Vec<_>>());
    assert_eq!(moved.fit, original_fit.iter().map(|p| *p + Vec2::new(1.0, 2.0)).collect::<Vec<_>>());
    assert_eq!(moved.fit_opts.start_tangent, original_start);
    assert_eq!(moved.fit_opts.end_tangent, original_end);

    let EntityKind::Ray(moved) = &s.doc().unwrap().entity(ray_h).unwrap().kind else { panic!("not ray") };
    assert_eq!(moved.base, Vec3::new(1.0, 2.0, 7.0));
    assert_eq!(moved.dir.z, 0.0);
}

#[test]
fn move_and_copy_translate_planar_entity_elevations() {
    let mut s = Session::new();
    let lw = s.add_entity(EntityKind::LwPolyline(serde_json::from_value(json!({"vertices": [], "closed": false, "elevation": 4})).unwrap())).unwrap();
    let hatch = s
        .add_entity(EntityKind::Hatch(serde_json::from_value(json!({"pattern": "SOLID", "solid": true, "loops": [], "elevation": 6})).unwrap()))
        .unwrap();
    s.execute("move", &json!({"handles": [lw.hex(), hatch.hex()], "delta": [0, 0, 3]})).unwrap();
    let elevation = |s: &Session, h| match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::LwPolyline(p) => p.elevation,
        EntityKind::Hatch(p) => p.elevation,
        _ => panic!("not an elevated entity"),
    };
    assert_eq!(elevation(&s, lw), 7.0);
    assert_eq!(elevation(&s, hatch), 9.0);
    let copied = s.execute("copy", &json!({"handles": [lw.hex(), hatch.hex()], "delta": [0, 0, 2]})).unwrap();
    assert_eq!(elevation(&s, lw), 7.0);
    assert_eq!(elevation(&s, hatch), 9.0);
    let elevations: Vec<_> =
        copied["handles"].as_array().unwrap().iter().map(|h| elevation(&s, Handle::parse_hex(h.as_str().unwrap()).unwrap())).collect();
    assert!(elevations.contains(&9.0) && elevations.contains(&11.0), "{elevations:?}");
}

#[test]
fn object_displacements_reject_an_invalid_z_without_moving() {
    for command in ["move", "copy"] {
        let mut s = Session::new();
        let text = mk_text(&mut s);
        assert!(s.execute(command, &json!({"handles": [text.hex()], "delta": {"x": 1, "y": 2, "z": "x"}})).is_err());
        let EntityKind::Text(t) = &s.doc().unwrap().entity(text).unwrap().kind else { panic!("not text") };
        assert_eq!(t.insert, Vec3::new(1.0, 2.0, 4.0));
    }
}
