//! Multilines survive DXF save and reopen (#510): the MLINE entity with its vertices, breaks,
//! closed and cap flags, and its MLINESTYLE (elements, caps, joints, fill) in the
//! `ACAD_MLINESTYLE` dictionary, as the DXF Reference describes. Foreign files read the same way,
//! and hostile ones don't panic.

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, EntityKind, MLine, MLineCaps, MLineCut, MLineElement, MLineJust, MLineStyle, Space};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::Vec2;

fn mlines(d: &Drawing) -> Vec<MLine> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::MLine(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

fn wall() -> MLineStyle {
    MLineStyle {
        name: "Wall".into(),
        description: "outer wall".into(),
        elements: vec![
            MLineElement { offset: 1.5, color: Color::Index(1), linetype: "ByLayer".into() },
            MLineElement { offset: 0.0, color: Color::ByBlock, linetype: "Continuous".into() },
            MLineElement { offset: -1.5, color: Color::ByLayer, linetype: "ByLayer".into() },
        ],
        start_caps: MLineCaps { line: true, outer_arc: false, inner_arcs: true, angle: 60f64.to_radians() },
        end_caps: MLineCaps { line: false, outer_arc: true, inner_arcs: false, angle: std::f64::consts::FRAC_PI_2 },
        joints: true,
        fill: Some(Color::Index(3)),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn mlines_and_styles_survive_save_and_reopen() {
    let mut d = Drawing::new_imperial();
    d.mline_styles.push(wall());
    d.header.set_str("CMLSTYLE", "Wall");
    d.header.set_i64("CMLJUST", 1);
    d.header.set_f64("CMLSCALE", 2.5);
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(20.0, 10.0)];
    let mut open = MLine::new(&wall(), &pts, false, MLineJust::Zero, 2.0);
    open.vertices[0].cuts.push(MLineCut { element: 0, start: 4.0, end: 6.5 });
    open.vertices[1].cuts.push(MLineCut { element: 2, start: 7.0, end: 1e12 });
    open.no_end_caps = true;
    let closed =
        MLine::new(&MLineStyle::default(), &[Vec2::new(0.0, 30.0), Vec2::new(10.0, 30.0), Vec2::new(5.0, 40.0)], true, MLineJust::Bottom, -1.0);
    d.add(&Space::Model, Common { layer: "Walls".into(), ..Common::default() }, EntityKind::MLine(open.clone())).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MLine(closed.clone())).unwrap();

    let saved = cadcraft_io::write_dxf(&d);
    let back = cadcraft_io::read_dxf(saved.as_bytes()).unwrap();
    let [a, b] = mlines(&back).try_into().unwrap();
    // The geometry as drawn is the same, breaks included.
    assert_eq!(a.def, wall());
    assert_eq!(
        (a.style.as_str(), a.justification, a.scale, a.closed, a.no_start_caps, a.no_end_caps),
        ("Wall", MLineJust::Zero, 2.0, false, false, true)
    );
    let (ga, go) = (a.geometry(), open.geometry());
    assert_eq!(ga.lines.len(), go.lines.len());
    for ((ea, a1, a2), (eo, o1, o2)) in ga.lines.iter().zip(&go.lines) {
        assert_eq!(ea, eo);
        assert!(a1.near(*o1, 1e-9) && a2.near(*o2, 1e-9), "{a1:?}-{a2:?} vs {o1:?}-{o2:?}");
    }
    assert_eq!((ga.caps.len(), ga.fill.len()), (go.caps.len(), go.fill.len()));
    let cut = a.vertices[0].cuts.first().unwrap();
    assert!(cut.element == 0 && near(cut.start, 4.0) && near(cut.end, 6.5), "{cut:?}");
    assert_eq!(b.def, MLineStyle::default());
    assert_eq!((b.closed, b.justification, b.scale, b.points()), (true, MLineJust::Bottom, -1.0, closed.points()));
    assert_eq!(back.model.iter().next().unwrap().common.layer, "Walls");
    // The styles and current settings.
    assert_eq!(back.mline_style("Wall"), Some(&wall()));
    assert!(back.mline_style("STANDARD").is_some());
    assert_eq!(back.header.str("CMLSTYLE", ""), "Wall");
    assert_eq!(back.header.i64("CMLJUST", 0), 1);
    assert_eq!(back.header.f64("CMLSCALE", 0.0), 2.5);

    // The file links up as the DXF Reference describes.
    let tags = cadcraft_dxf::parse(saved.as_bytes()).unwrap();
    let secs = sections(&tags);
    let recs = |name: &str| records(&secs.iter().find(|s| s.name == name).unwrap().tags);
    let val = |t: &[Tag], c: i32| t.iter().find(|g| g.code == c).map(Tag::str).unwrap_or_default();
    let ents = recs("ENTITIES");
    let (_, ml) = ents.iter().find(|(k, _)| k == "MLINE").unwrap();
    let objs = recs("OBJECTS");
    let (_, st) = objs.iter().find(|(k, t)| k == "MLINESTYLE" && val(t, 2) == "Wall").unwrap();
    assert_eq!(val(ml, 340), val(st, 5));
    assert_eq!(val(ml, 73), "3");
    // Flags: fill, joints, start line and inner arcs, end outer arc.
    assert_eq!(val(st, 70), (1 + 2 + 16 + 32 + 1024).to_string());
    assert!(objs.iter().any(|(k, t)| k == "DICTIONARY" && t.iter().any(|g| g.code == 3 && g.str() == "ACAD_MLINESTYLE")));

    // Saving again is stable.
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&back).as_bytes()).unwrap();
    assert_eq!(mlines(&again).len(), 2);
    assert_eq!(mlines(&again)[0].def, wall());
}

/// A multiline as another program writes it: style `Duo` (offsets 0.25 and -0.25, the first red)
/// and a two-segment multiline whose first element is broken from 2 to 3 on the first segment.
fn foreign_dxf() -> String {
    let g: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "MLINE"),
        (5, "40"),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbMline"),
        (2, "Duo"),
        (340, "50"),
        (40, "4"),
        (70, "1"),
        (71, "1"),
        (72, "3"),
        (73, "2"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (210, "0"),
        (220, "0"),
        (230, "1"),
        // Vertex 1: segment east, miter north.
        (11, "0"),
        (21, "0"),
        (31, "0"),
        (12, "1"),
        (22, "0"),
        (32, "0"),
        (13, "0"),
        (23, "1"),
        (33, "0"),
        (74, "4"),
        (41, "1"),
        (41, "0"),
        (41, "2"),
        (41, "3"),
        (75, "0"),
        (74, "2"),
        (41, "-1"),
        (41, "0"),
        (75, "0"),
        // Vertex 2.
        (11, "10"),
        (21, "0"),
        (31, "0"),
        (12, "0"),
        (22, "1"),
        (32, "0"),
        (13, "-0.7071067811865475"),
        (23, "0.7071067811865475"),
        (33, "0"),
        (74, "2"),
        (41, "1.4142135623730951"),
        (41, "0"),
        (75, "0"),
        (74, "2"),
        (41, "-1.4142135623730951"),
        (41, "0"),
        (75, "0"),
        // Vertex 3.
        (11, "10"),
        (21, "10"),
        (31, "0"),
        (12, "0"),
        (22, "1"),
        (32, "0"),
        (13, "-1"),
        (23, "0"),
        (33, "0"),
        (74, "2"),
        (41, "1"),
        (41, "0"),
        (75, "0"),
        (74, "2"),
        (41, "-1"),
        (41, "0"),
        (75, "0"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "OBJECTS"),
        (0, "DICTIONARY"),
        (5, "C"),
        (330, "0"),
        (100, "AcDbDictionary"),
        (3, "ACAD_MLINESTYLE"),
        (350, "17"),
        (0, "DICTIONARY"),
        (5, "17"),
        (330, "C"),
        (100, "AcDbDictionary"),
        (3, "Duo"),
        (350, "50"),
        (0, "MLINESTYLE"),
        (5, "50"),
        (330, "17"),
        (100, "AcDbMlineStyle"),
        (2, "Duo"),
        (70, "0"),
        (3, ""),
        (62, "256"),
        (51, "90"),
        (52, "90"),
        (71, "2"),
        (49, "0.25"),
        (62, "1"),
        (6, "BYLAYER"),
        (49, "-0.25"),
        (62, "256"),
        (6, "BYLAYER"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    g.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

#[test]
fn foreign_mline_reads_with_its_style() {
    let d = cadcraft_io::read_dxf(foreign_dxf().as_bytes()).unwrap();
    let [m] = mlines(&d).try_into().unwrap();
    assert_eq!(m.style, "Duo");
    assert_eq!(m.def.elements.iter().map(|e| (e.offset, e.color)).collect::<Vec<_>>(), vec![(0.25, Color::Index(1)), (-0.25, Color::ByLayer)]);
    assert_eq!((m.scale, m.justification), (4.0, MLineJust::Zero));
    let mut offs = m.offsets();
    offs.sort_by(f64::total_cmp);
    assert_eq!(offs, vec![-1.0, 1.0]);
    // The first element: broken from 2 to 3 along the first segment.
    let g = m.geometry();
    let first: Vec<(Vec2, Vec2)> = g.lines.iter().filter(|(e, _, _)| *e == 0).map(|(_, a, b)| (*a, *b)).collect();
    assert_eq!(first.len(), 3, "{first:?}");
    assert!(first[0].0.near(Vec2::new(0.0, 1.0), 1e-9) && first[0].1.near(Vec2::new(2.0, 1.0), 1e-9));
    assert!(first[1].0.near(Vec2::new(3.0, 1.0), 1e-9) && first[1].1.near(Vec2::new(9.0, 1.0), 1e-9));
    assert!(d.mline_style("Duo").is_some());
}

#[test]
fn mline_without_its_style_keeps_its_offsets() {
    // The style object is missing: the elements come from the vertex data.
    let text = foreign_dxf();
    let cut = text.find("0\nSECTION\n2\nOBJECTS").unwrap();
    let text = format!("{}0\nEOF\n", &text[..cut]);
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let [m] = mlines(&d).try_into().unwrap();
    let mut offs = m.offsets();
    offs.sort_by(f64::total_cmp);
    assert!(near(offs[0], -1.0) && near(offs[1], 1.0), "{offs:?}");
    // And it saves with a style of its own.
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert!(back.mline_style("Duo").is_some());
    assert_eq!(mlines(&back)[0].geometry().lines.len(), m.geometry().lines.len());
}

#[test]
fn hostile_mline_does_not_panic() {
    let mut huge = String::from("0\nSECTION\n2\nENTITIES\n0\nMLINE\n100\nAcDbMline\n2\nX\n40\nnan\n72\n999999999\n73\n999999\n");
    for i in 0..3000 {
        huge.push_str(&format!("11\n{i}\n21\n1e308\n13\nnan\n23\ninf\n74\n99999\n41\n1e308\n41\n-1e308\n41\nnan\n"));
    }
    for _ in 0..200 {
        huge.push_str("74\n5\n41\n0\n41\n1\n41\n2\n");
    }
    huge.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nMLINESTYLE\n5\n9\n100\nAcDbMlineStyle\n2\nX\n70\n-1\n51\nnan\n71\n-5\n");
    for _ in 0..100 {
        huge.push_str("49\nnan\n62\n99999\n6\n\n");
    }
    huge.push_str("0\nENDSEC\n0\nEOF\n");
    let d = cadcraft_io::read_dxf(huge.as_bytes()).unwrap();
    for m in mlines(&d) {
        assert!(m.def.elements.len() <= cadcraft_doc::MAX_MLINE_ELEMENTS);
        let _ = m.geometry();
    }
    let _ = cadcraft_io::write_dxf(&d);
    let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn mline_survives_dwg_save_and_reopen() {
    let mut d = Drawing::new_imperial();
    d.mline_styles.push(wall());
    let mut m = MLine::new(&wall(), &[Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0), Vec2::new(8.0, 6.0)], false, MLineJust::Top, 1.5);
    m.vertices[0].cuts.push(MLineCut { element: 1, start: 2.0, end: 3.0 });
    d.add(&Space::Model, Common::default(), EntityKind::MLine(m.clone())).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let [b] = mlines(&back).try_into().unwrap();
    assert_eq!((b.style.as_str(), b.def.elements.len(), b.justification, b.points()), ("Wall", 3, MLineJust::Top, m.points()));
    assert_eq!(b.geometry().lines.len(), m.geometry().lines.len());
}
