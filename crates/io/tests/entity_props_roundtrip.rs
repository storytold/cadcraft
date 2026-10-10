//! Entity-level properties that must survive a DXF save and reopen: thickness, POINT angle,
//! hatch origin/background/elevation, xref blocks, the built-in font name, and polyface /
//! polygon mesh POLYLINEs from other programs.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use cadcraft_io::{read_dxf, write_dxf};

fn roundtrip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).expect("reopen")
}

fn add(d: &mut Drawing, thickness: f64, kind: EntityKind) -> Handle {
    let common = Common { thickness, ..Common::default() };
    d.add(&Space::Model, common, kind).expect("add")
}

fn get(d: &Drawing, h: Handle) -> &Entity {
    d.model.get(h).expect("entity kept")
}

fn square(size: f64) -> Vec<PolyVertex> {
    [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)].into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).collect()
}

#[test]
fn thickness_roundtrips_on_every_entity_that_has_one() {
    let mut d = Drawing::new_metric();
    let p = |x, y| Vec3::new(x, y, 0.0);
    let text = Text {
        insert: p(0.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: "T".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    let kinds = vec![
        EntityKind::Line(Line { a: p(0.0, 0.0), b: p(10.0, 0.0) }),
        EntityKind::Circle(Circle { center: p(0.0, 0.0), radius: 5.0 }),
        EntityKind::Arc(Arc { center: p(0.0, 0.0), radius: 5.0, start: 0.0, end: 1.0 }),
        EntityKind::Point(Point { p: p(1.0, 2.0), angle: 0.0 }),
        EntityKind::Text(text),
        EntityKind::LwPolyline(LwPolyline { vertices: square(4.0), closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }),
        EntityKind::Polyline3d(Polyline3d { points: vec![p(0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)], closed: false }),
        EntityKind::Solid(Solid { corners: [p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0), p(1.0, 1.0)] }),
        EntityKind::Trace(Solid { corners: [p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0), p(1.0, 1.0)] }),
    ];
    let handles: Vec<Handle> = kinds.into_iter().enumerate().map(|(i, k)| add(&mut d, 1.5 + i as f64, k)).collect();
    let thin = add(&mut d, 0.0, EntityKind::Line(Line { a: p(0.0, 5.0), b: p(1.0, 5.0) }));
    let r = roundtrip(&d);
    for (i, h) in handles.iter().enumerate() {
        let e = get(&r, *h);
        assert_eq!(e.common.thickness, 1.5 + i as f64, "thickness of {:?}", e.kind);
    }
    assert_eq!(get(&r, thin).common.thickness, 0.0);
}

#[test]
fn point_angle_roundtrips() {
    let mut d = Drawing::new_metric();
    let h = add(&mut d, 0.0, EntityKind::Point(Point { p: Vec3::new(3.0, 4.0, 0.0), angle: 30f64.to_radians() }));
    let r = roundtrip(&d);
    let EntityKind::Point(pt) = &get(&r, h).kind else { panic!("not a point") };
    assert!((pt.angle - 30f64.to_radians()).abs() < 1e-12, "angle {}", pt.angle);
}

fn hatch(pattern: &str, gradient: Option<Gradient>) -> Hatch {
    Hatch {
        pattern: pattern.into(),
        solid: pattern == "SOLID",
        loops: vec![HatchLoop { vertices: square(10.0), outer: true }],
        scale: 2.0,
        angle: 0.25,
        associative: false,
        style: 0,
        elevation: 1.5,
        gradient,
        origin: Vec2::new(1.0, 2.0),
        background: Some(Color::Index(2)),
    }
}

#[test]
fn hatch_origin_background_and_elevation_roundtrip() {
    let mut d = Drawing::new_metric();
    let patterned = add(&mut d, 0.0, EntityKind::Hatch(hatch("ANSI31", None)));
    let g = Gradient { name: "CYLINDER".into(), color1: Color::Index(1), color2: Color::True(Rgb(10, 20, 30)), angle: 0.5, centered: false };
    let mut with_gradient = hatch("SOLID", Some(g.clone()));
    with_gradient.background = Some(Color::True(Rgb(1, 2, 3)));
    let graded = add(&mut d, 0.0, EntityKind::Hatch(with_gradient));
    let mut plain = hatch("ANSI31", None);
    (plain.origin, plain.background, plain.elevation) = (Vec2::ZERO, None, 0.0);
    let plain_h = add(&mut d, 0.0, EntityKind::Hatch(plain));

    let text = write_dxf(&d);
    let r = read_dxf(text.as_bytes()).expect("reopen");
    let hatch_of = |h| match &get(&r, h).kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    };
    let a = hatch_of(patterned);
    assert!(a.origin.near(Vec2::new(1.0, 2.0), 1e-12), "origin {:?}", a.origin);
    assert_eq!(a.background, Some(Color::Index(2)));
    assert_eq!(a.elevation, 1.5);
    let b = hatch_of(graded);
    assert!(b.origin.near(Vec2::new(1.0, 2.0), 1e-12));
    assert_eq!(b.background, Some(Color::True(Rgb(1, 2, 3))));
    assert_eq!(b.gradient, Some(g), "gradient xdata still read next to the hatch xdata");
    let c = hatch_of(plain_h);
    assert_eq!((c.origin, c.background, c.elevation), (Vec2::ZERO, None, 0.0));

    // Other readers see the origin in the pattern lines' base points (43/44): ANSI31's line
    // passes through (0,0), so the first base point is the origin itself.
    let g = groups(&text);
    let in_hatch = g.iter().skip_while(|(c, v)| !(*c == 0 && v == "HATCH"));
    let first_43 = in_hatch.clone().find(|(c, _)| *c == 43).map(|(_, v)| v.parse::<f64>().expect("43 value"));
    let first_44 = in_hatch.clone().find(|(c, _)| *c == 44).map(|(_, v)| v.parse::<f64>().expect("44 value"));
    assert_eq!((first_43, first_44), (Some(1.0), Some(2.0)));
}

#[test]
fn xref_block_flag_and_path_roundtrip() {
    let mut d = Drawing::new_metric();
    let mut x = Block::new("Site");
    x.xref_path = Some("plans/site.dxf".into());
    d.blocks.insert(x.name.clone(), std::sync::Arc::new(x));
    let mut local = Block::new("Door");
    local.entities.push(Entity::new(Handle(0x50), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })));
    d.blocks.insert(local.name.clone(), std::sync::Arc::new(local));
    let ins = Insert {
        block: "Site".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let h = add(&mut d, 0.0, EntityKind::Insert(ins));

    let r = roundtrip(&d);
    assert_eq!(r.blocks.get("Site").and_then(|b| b.xref_path.clone()), Some("plans/site.dxf".into()));
    assert_eq!(r.blocks.get("Door").map(|b| b.xref_path.clone()), Some(None));
    let EntityKind::Insert(i) = &get(&r, h).kind else { panic!("not an insert") };
    assert_eq!((i.block.as_str(), i.cols, i.rows), ("Site", 1, 1), "the INSERT naming the xref is untouched");
}

#[test]
fn builtin_font_name_roundtrips_and_other_readers_get_txt() {
    let mut d = Drawing::new_metric();
    d.text_styles.push(TextStyle { name: "Notes".into(), font: "romans.shx".into(), ..TextStyle::default() });
    let text = write_dxf(&d);
    let g = groups(&text);
    // The STYLE table's "Standard" entry (other tables have a "Standard" too).
    let table = g.iter().position(|(c, v)| *c == 2 && v == "STYLE").expect("STYLE table");
    let standard = table + g.iter().skip(table).position(|(c, v)| *c == 2 && v == "Standard").expect("Standard style");
    let file = g.iter().skip(standard).find(|(c, _)| *c == 3).map(|(_, v)| v.as_str());
    assert_eq!(file, Some("txt"), "font file group stays a file other readers have");
    let r = read_dxf(text.as_bytes()).expect("reopen");
    let font = |n: &str| r.text_styles.iter().find(|s| s.name == n).map(|s| s.font.clone());
    assert_eq!(font("Standard").as_deref(), Some("CADCraft Stroke"));
    assert_eq!(font("Notes").as_deref(), Some("romans.shx"));

    // Another program that changed the font file wins over the stale xdata.
    let edited = text.replacen("  3\r\ntxt\r\n", "  3\r\nisocp.shx\r\n", 1);
    assert_ne!(edited, text);
    let r = read_dxf(edited.as_bytes()).expect("reopen edited");
    assert_eq!(r.text_styles.iter().find(|s| s.name == "Standard").map(|s| s.font.as_str()), Some("isocp.shx"));
}

/// The (code, value) pairs of a DXF text.
fn groups(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.chunks(2).filter_map(|p| Some((p.first()?.parse().ok()?, (*p.get(1)?).to_string()))).collect()
}

/// A minimal ENTITIES-only DXF from group/value pairs.
fn dxf(records: &[&[(i32, &str)]]) -> String {
    let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
    for r in records {
        for (c, v) in *r {
            s.push_str(&format!("{c}\n{v}\n"));
        }
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

fn vertex(x: &str, y: &str, z: &str, flags: &str) -> Vec<(i32, &'static str)> {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    vec![(0, "VERTEX"), (8, "0"), (10, leak(x)), (20, leak(y)), (30, leak(z)), (70, leak(flags))]
}

fn face(idx: [&'static str; 4]) -> Vec<(i32, &'static str)> {
    vec![(0, "VERTEX"), (8, "0"), (10, "0"), (20, "0"), (30, "0"), (70, "128"), (71, idx[0]), (72, idx[1]), (73, idx[2]), (74, idx[3])]
}

fn faces(d: &Drawing) -> Vec<Face3d> {
    d.model
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Face3d(f) => f.clone(),
            k => panic!("expected only 3D faces, got {k:?}"),
        })
        .collect()
}

#[test]
fn polyface_and_polygon_mesh_polylines_become_faces_not_bogus_vertices() {
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    // Polyface: four vertices, a quad, a triangle whose third edge is hidden, and faces with an
    // out-of-range index or only two vertices (skipped).
    let mut recs: Vec<Vec<(i32, &str)>> =
        vec![vec![(0, "POLYLINE"), (8, "0"), (66, "1"), (10, "0"), (20, "0"), (30, "0"), (70, "64"), (71, "4"), (72, "4")]];
    for (x, y) in [("0", "0"), ("1", "0"), ("1", "1"), ("0", "1")] {
        recs.push(vertex(x, y, "2", "192"));
    }
    recs.push(face(["1", "2", "3", "4"]));
    recs.push(face(["1", "3", "-4", "0"]));
    recs.push(face(["1", "2", "99", "0"]));
    recs.push(face(["1", "2", "0", "0"]));
    recs.push(vec![(0, "SEQEND"), (8, "0")]);
    let refs: Vec<&[(i32, &str)]> = recs.iter().map(Vec::as_slice).collect();
    let d = read_dxf(dxf(&refs).as_bytes()).expect("read polyface");
    let fs = faces(&d);
    assert_eq!(fs.len(), 2, "{fs:?}");
    assert_eq!(fs[0].corners, [p(0.0, 0.0, 2.0), p(1.0, 0.0, 2.0), p(1.0, 1.0, 2.0), p(0.0, 1.0, 2.0)]);
    assert_eq!(fs[0].hidden_edges, 0);
    assert_eq!(fs[1].corners, [p(0.0, 0.0, 2.0), p(1.0, 1.0, 2.0), p(0.0, 1.0, 2.0), p(0.0, 1.0, 2.0)]);
    assert_eq!(fs[1].hidden_edges, 4 | 8, "the triangle's closing edge is hidden");
    // The faces survive a save and reopen as 3DFACEs.
    assert_eq!(faces(&roundtrip(&d)), fs);

    // 3×2 polygon mesh: two quads.
    let mut recs: Vec<Vec<(i32, &str)>> =
        vec![vec![(0, "POLYLINE"), (8, "0"), (66, "1"), (10, "0"), (20, "0"), (30, "0"), (70, "16"), (71, "3"), (72, "2")]];
    for (x, y) in [("0", "0"), ("0", "1"), ("1", "0"), ("1", "1"), ("2", "0"), ("2", "1")] {
        recs.push(vertex(x, y, "0", "64"));
    }
    recs.push(vec![(0, "SEQEND"), (8, "0")]);
    let refs: Vec<&[(i32, &str)]> = recs.iter().map(Vec::as_slice).collect();
    let d = read_dxf(dxf(&refs).as_bytes()).expect("read mesh");
    let fs = faces(&d);
    assert_eq!(fs.len(), 2);
    assert_eq!(fs[1].corners, [p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(2.0, 1.0, 0.0), p(2.0, 0.0, 0.0)]);

    // Hostile counts: a mesh claiming more vertices than it has yields nothing, without panicking.
    let recs: Vec<Vec<(i32, &str)>> = vec![
        vec![(0, "POLYLINE"), (8, "0"), (66, "1"), (70, "17"), (71, "9223372036854775807"), (72, "-5")],
        vertex("0", "0", "0", "64"),
        vec![(0, "SEQEND")],
    ];
    let refs: Vec<&[(i32, &str)]> = recs.iter().map(Vec::as_slice).collect();
    let d = read_dxf(dxf(&refs).as_bytes()).expect("read hostile mesh");
    assert_eq!(d.model.iter().count(), 0);
}
