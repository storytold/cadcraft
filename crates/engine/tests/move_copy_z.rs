//! #1407: MOVE/COPY carry a Z delta; planar ROTATE/SCALE/MIRROR preserve Z.
use cadcraft_doc::{
    Attrib, Block, Common, Entity, EntityKind, Handle, Hatch, HatchLoop, Insert, LwPolyline, MLeader, MText, MultilineAttrib, Space, Text,
};
use cadcraft_engine::Session;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
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

    // A null delta leaves Z to from/to, the same source XY comes from.
    let d = mk_poly(&mut s);
    s.execute("move", &json!({"handles": [d.hex()], "delta": null, "from": [0, 0, 1], "to": [2, 3, 4]})).unwrap();
    assert_eq!(poly_z(&s, d)[0], Vec3::new(2.0, 3.0, 3.0));

    // Object form: an absent z is zero, a present non-numeric z is an error.
    let e = mk_poly(&mut s);
    s.execute("move", &json!({"handles": [e.hex()], "delta": {"x": 1, "y": 2}})).unwrap();
    assert_eq!(poly_z(&s, e)[0], Vec3::new(1.0, 2.0, 0.0));
    assert!(s.execute("move", &json!({"handles": [e.hex()], "delta": {"x": 0, "y": 0, "z": "3"}})).is_err());
    assert_eq!(poly_z(&s, e)[0], Vec3::new(1.0, 2.0, 0.0), "the refused MOVE changes nothing");
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

/// Text and an insert's attributes move by `dz` too: a block reference moved in Z used to leave
/// its attributes behind, and a plain text entity silently dropped the offset (#1407).
#[test]
fn move_carries_z_to_text_and_insert_attributes() {
    let mut s = Session::new();
    let t = h(&s.execute("text", &json!({"at": [0, 10], "height": 1, "text": "Note"})).unwrap());
    // The text command places text in the plane; give it an elevation the way a 3D file would.
    s.doc_mut().unwrap().model.modify(t, |e| {
        if let EntityKind::Text(x) = &mut e.kind {
            x.insert.z = 2.0;
        }
    });
    let ins = {
        let d = s.doc_mut().unwrap();
        let attrib = |tag: &str, z: f64, align: bool| Attrib {
            tag: tag.into(),
            text: Text {
                insert: Vec3::new(0.0, 0.0, z),
                align_pt: align.then_some(Vec3::new(0.0, 0.0, z)),
                height: 1.0,
                value: "P-1".into(),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: Default::default(),
                valign: Default::default(),
            },
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        };
        let mut blk = Block::new("TITLE");
        blk.entities.push(Entity::new(Handle(0x900), EntityKind::AttDef(attrib("PART", 0.0, false))));
        d.blocks.insert("TITLE".into(), std::sync::Arc::new(blk));
        let e = Entity::new(
            Handle(0x901),
            EntityKind::Insert(Insert {
                block: "TITLE".into(),
                insert: Vec3::new(0.0, 0.0, 1.0),
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: vec![attrib("PART", 1.0, true)],
                clip: None,
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            }),
        );
        d.add(&Space::Model, Common::default(), e.kind).unwrap()
    };
    s.execute("move", &json!({"handles": [t.hex(), ins.hex()], "delta": [1.0, 2.0, 3.0]})).unwrap();
    let d = s.doc().unwrap();
    let text_z = match &d.entity(t).unwrap().kind {
        EntityKind::Text(x) => x.insert.z,
        k => panic!("not text: {k:?}"),
    };
    assert_eq!(text_z, 5.0, "a text entity keeps the Z delta");
    match &d.entity(ins).unwrap().kind {
        EntityKind::Insert(i) => {
            assert_eq!(i.insert.z, 4.0, "the insert moves");
            assert_eq!(i.attribs[0].text.insert.z, 4.0, "its attribute moves with it, not left behind");
            assert_eq!(i.attribs[0].text.align_pt.unwrap().z, 4.0, "its alignment point keeps the Z delta too");
        }
        k => panic!("not an insert: {k:?}"),
    }
}

/// A polyline's and a hatch's elevation is their world Z, so MOVE carries the delta there; a
/// mirrored plan polyline stores its elevation in the opposite OCS direction.
#[test]
fn move_carries_z_to_planar_elevations() {
    let mut s = Session::new();
    let pline = |elevation: f64| {
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex::new(Vec2::new(0.0, 0.0)), PolyVertex::new(Vec2::new(10.0, 0.0))],
            closed: false,
            const_width: 0.0,
            elevation,
            plinegen: false,
        })
    };
    let (a, b, c) = {
        let d = s.doc_mut().unwrap();
        let a = d.add(&Space::Model, Common::default(), pline(2.0)).unwrap();
        let mirrored = Common { extrusion: Vec3::new(0.0, 0.0, -1.0), ..Common::default() };
        let b = d.add(&Space::Model, mirrored, pline(2.0)).unwrap();
        let hatch = EntityKind::Hatch(Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops: vec![HatchLoop {
                vertices: vec![PolyVertex::new(Vec2::new(0.0, 0.0)), PolyVertex::new(Vec2::new(10.0, 0.0)), PolyVertex::new(Vec2::new(10.0, 10.0))],
                outer: true,
                sources: Vec::new(),
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 2.0,
            gradient: None,
            origin: Vec2::new(0.0, 0.0),
            background: None,
            pattern_lines: Vec::new(),
        });
        let c = d.add(&Space::Model, Common::default(), hatch).unwrap();
        (a, b, c)
    };
    s.execute("move", &json!({"handles": [a.hex(), b.hex(), c.hex()], "delta": [0, 0, 3]})).unwrap();
    let d = s.doc().unwrap();
    let elevation = |h: Handle| match &d.entity(h).unwrap().kind {
        EntityKind::LwPolyline(p) => p.elevation,
        EntityKind::Hatch(x) => x.elevation,
        k => panic!("not planar: {k:?}"),
    };
    assert_eq!(elevation(a), 5.0, "a polyline's elevation is its world Z");
    assert_eq!(elevation(b), -1.0, "a mirrored plan polyline still rises in world Z: -(-1) = 2 + 3");
    assert_eq!(elevation(c), 5.0, "a hatch's elevation is its world Z");
}

/// An attribute's multiline text and a multileader's block content move by `dz` with the rest.
#[test]
fn move_carries_z_to_multiline_attributes_and_a_multileader_block() {
    let mut s = Session::new();
    let attdef = Attrib {
        tag: "NOTE".into(),
        text: Text {
            insert: Vec3::new(0.0, 0.0, 1.0),
            align_pt: None,
            height: 1.0,
            value: "NOTE".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        },
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: Some(MultilineAttrib {
            mtext: MText {
                insert: Vec3::new(0.0, 0.0, 1.0),
                height: 1.0,
                width: 0.0,
                attach: 1,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "NOTE".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            },
            read_as: "NOTE".into(),
        }),
        modes: Default::default(),
    };
    let mleader = EntityKind::MLeader(MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)]],
        landing: Vec3::new(1.0, 1.0, 0.0),
        dogleg: 0.5,
        text: None,
        style: "Standard".into(),
        arrow_size: 1.0,
        spline: false,
        block: Some(Insert {
            block: "TITLE".into(),
            insert: Vec3::new(0.0, 0.0, 1.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            clip: None,
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    });
    let (a, b) = {
        let d = s.doc_mut().unwrap();
        let a = d.add(&Space::Model, Common::default(), EntityKind::AttDef(attdef)).unwrap();
        let b = d.add(&Space::Model, Common::default(), mleader).unwrap();
        (a, b)
    };
    s.execute("move", &json!({"handles": [a.hex(), b.hex()], "delta": [0, 0, 3]})).unwrap();
    let d = s.doc().unwrap();
    match &d.entity(a).unwrap().kind {
        EntityKind::AttDef(x) => {
            assert_eq!(x.text.insert.z, 4.0);
            assert_eq!(x.multiline.as_ref().unwrap().mtext.insert.z, 4.0, "multiline text keeps the delta");
        }
        k => panic!("not an attdef: {k:?}"),
    }
    match &d.entity(b).unwrap().kind {
        EntityKind::MLeader(x) => {
            assert_eq!(x.landing.z, 3.0);
            assert_eq!(x.block.as_ref().unwrap().insert.z, 4.0, "block content keeps the delta");
        }
        k => panic!("not a multileader: {k:?}"),
    }
}
