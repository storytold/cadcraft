use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use crate::*;

#[path = "../tests/support/pdf_read.rs"]
mod pdf_read;

fn sample() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.layers.push(Layer { name: "Walls".into(), color: Color::Index(1), ..Layer::default() });
    let c = |l: &str| Common { layer: l.into(), ..Common::default() };
    d.add(&Space::Model, c("Walls"), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) })).unwrap();
    d.add(&Space::Model, c("0"), EntityKind::Circle(Circle { center: Vec3::new(3.0, 4.0, 0.0), radius: 2.5 })).unwrap();
    d.add(&Space::Model, c("0"), EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 1.0, start: 0.5, end: 2.0 })).unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::with_bulge(Vec2::new(4.0, 0.0), 0.5), PolyVertex::new(Vec2::new(4.0, 3.0))],
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Text(Text {
            insert: Vec3::new(1.0, 1.0, 0.0),
            align_pt: None,
            height: 0.25,
            value: "Hello Café".into(),
            rotation: 0.3,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::MText(MText {
            insert: Vec3::new(5.0, 5.0, 0.0),
            height: 0.2,
            width: 3.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "line one\\Pline two".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Ellipse(Ellipse {
            center: Vec3::new(8.0, 8.0, 0.0),
            major: Vec3::new(3.0, 0.0, 0.0),
            ratio: 0.5,
            start: 0.0,
            end: std::f64::consts::TAU,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Spline(cadcraft_geom::Spline::from_fit_points(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 1.0), Vec2::new(4.0, 3.0)])),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Dimension(Dimension {
            kind: DimKind::Linear { rotation: 0.0 },
            defpt: Vec3::new(0.0, -1.0, 0.0),
            text_mid: Vec3::ZERO,
            p13: Vec3::ZERO,
            p14: Vec3::new(10.0, 0.0, 0.0),
            p15: Vec3::ZERO,
            p16: Vec3::ZERO,
            text: String::new(),
            style: "Standard".into(),
            measurement: 10.0,
            text_rotation: 0.0,
            user_text_pos: false,
            block: None,
            overrides: Default::default(),
            assoc: Vec::new(),
            marks: Default::default(),
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Hatch(Hatch {
            pattern: "ANSI31".into(),
            solid: false,
            loops: vec![HatchLoop {
                vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::new(Vec2::new(2.0, 0.0)), PolyVertex::new(Vec2::new(2.0, 2.0))],
                outer: true,
                sources: Vec::new(),
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        }),
    )
    .unwrap();
    let mut b = Block::new("Bolt");
    b.entities.push(Entity::new(Handle(0x50), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.25 })));
    d.blocks.insert("Bolt".into(), std::sync::Arc::new(b));
    d.add(
        &Space::Model,
        c("0"),
        EntityKind::Insert(Insert {
            block: "Bolt".into(),
            insert: Vec3::new(2.0, 2.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    )
    .unwrap();
    d.add(&Space::Paper("Layout1".into()), c("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })).unwrap();
    d
}

#[test]
fn dxf_roundtrip_preserves_entities() {
    let d = sample();
    let text = write_dxf(&d);
    let back = read_dxf(text.as_bytes()).unwrap();
    let kinds = |d: &Drawing| d.model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>();
    assert_eq!(kinds(&back), kinds(&d));
    assert_eq!(back.layer("Walls").unwrap().color, Color::Index(1));
    assert!(back.block("Bolt").is_some());
    assert_eq!(back.layout("Layout1").unwrap().entities.len(), 1);
    // Geometry survives.
    for (a, b) in d.model.iter().zip(back.model.iter()) {
        match (&a.kind, &b.kind) {
            (EntityKind::Line(x), EntityKind::Line(y)) => assert_eq!(x, y),
            (EntityKind::Circle(x), EntityKind::Circle(y)) => assert_eq!(x, y),
            (EntityKind::LwPolyline(x), EntityKind::LwPolyline(y)) => assert_eq!(x.vertices, y.vertices),
            (EntityKind::Text(x), EntityKind::Text(y)) => {
                assert_eq!(x.value, y.value);
                assert!((x.rotation - y.rotation).abs() < 1e-9);
            }
            (EntityKind::MText(x), EntityKind::MText(y)) => assert_eq!(x.contents, y.contents),
            (EntityKind::Hatch(x), EntityKind::Hatch(y)) => assert_eq!(x.loops, y.loops),
            _ => {}
        }
        assert_eq!(a.handle, b.handle);
        assert_eq!(a.common.layer, b.common.layer);
    }
    // Handles stay unique after reading.
    let mut back = back;
    let h = back.new_handle();
    assert!(back.entity(h).is_none());
}

#[test]
fn lwpolyline_elevation_roundtrips() {
    for elevation in [5.0, -2.5, 0.0] {
        let mut d = Drawing::new_metric();
        d.add(
            &Space::Model,
            Common::default(),
            EntityKind::LwPolyline(LwPolyline {
                vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::new(Vec2::new(10.0, 0.0)), PolyVertex::new(Vec2::new(10.0, 10.0))],
                closed: true,
                const_width: 0.0,
                elevation,
                plinegen: false,
            }),
        )
        .unwrap();
        let back = roundtrip(&d);
        let again = roundtrip(&back);
        for doc in [&back, &again] {
            let p = first(doc, |k| if let EntityKind::LwPolyline(p) = k { Some(p.clone()) } else { None });
            assert_eq!(p.elevation, elevation);
            assert_eq!(p.vertices.len(), 3);
        }
    }
}

#[test]
fn second_roundtrip_is_stable() {
    let d = sample();
    let t1 = write_dxf(&d);
    let d2 = read_dxf(t1.as_bytes()).unwrap();
    let d3 = read_dxf(write_dxf(&d2).as_bytes()).unwrap();
    assert_eq!(d2.model.len(), d3.model.len());
}

#[test]
fn layer_transparency_and_description_roundtrip() {
    // Issues #96 and #97: both are LAYER xdata (AcCmTransparency, AcAecLayerStandard).
    let layers = [("T0", 0, ""), ("T1", 1, "One"), ("T40", 40, "Existing brick walls to remain"), ("T90", 90, "Ångström – ünïcode")];
    let mut d = Drawing::new_imperial();
    for (name, transparency, description) in layers {
        d.layers.push(Layer {
            name: name.into(),
            transparency,
            description: description.into(),
            color: Color::Index(1),
            plot: false,
            ..Layer::default()
        });
    }
    let long = "é".repeat(200);
    d.layers.push(Layer { name: "Long".into(), description: long.clone(), ..Layer::default() });
    let text = write_dxf(&d);
    let back = read_dxf(text.as_bytes()).unwrap();
    for (name, transparency, description) in layers {
        let l = back.layer(name).unwrap();
        assert_eq!((l.transparency, l.description.as_str(), l.color, l.plot), (transparency, description, Color::Index(1), false), "{name}");
    }
    // An xdata string holds at most 255 bytes; the cut falls on a character boundary.
    assert_eq!(back.layer("Long").unwrap().description, long[..254]);
    // AutoCAD's encoding: the alpha with the "by alpha" flag, under registered applications.
    assert!(text.contains("AcCmTransparency\r\n1071\r\n33554585\r\n"), "40% is alpha 153");
    assert_eq!(text.matches("AcDbRegAppTableRecord\r\n  2\r\nAcCmTransparency").count(), 1);
    assert!(text.contains("AcDbRegAppTableRecord\r\n  2\r\nAcAecLayerStandard"));
    // Values that aren't "by alpha" (ByLayer, ByBlock, junk) leave the layer opaque.
    for v in [0, 0x0100_0000, -1, i64::MAX] {
        assert_eq!(crate::dxf_ext::transparency_from_dxf(v), None, "{v:#x}");
    }
    assert_eq!(crate::dxf_ext::transparency_from_dxf(0x0200_00ff), Some(0));
    assert_eq!(crate::dxf_ext::transparency_from_dxf(0x0200_0000), Some(90));
}

#[test]
fn block_definition_properties_and_constant_attdefs_roundtrip() {
    let mut d = Drawing::new_metric();
    let mut b = Block::new("Title");
    (b.description, b.units, b.explodable) = ("Drawing title block".into(), 4, false);
    let text = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.5,
        value: "ACME".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    let attdef = |tag: &str, invisible: bool, constant: bool| Attrib {
        tag: tag.into(),
        text: text.clone(),
        invisible,
        constant,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    for (i, a) in [attdef("COMPANY", false, true), attdef("SECRET", true, true), attdef("SHEET", false, false)].into_iter().enumerate() {
        b.entities.push(Entity::new(Handle(0x500 + i as u64), EntityKind::AttDef(a)));
    }
    d.blocks.insert("Title".into(), std::sync::Arc::new(b));
    d.blocks.insert("Plain".into(), std::sync::Arc::new(Block::new("Plain")));
    d.bump_handseed(Handle(0x510));
    let back = roundtrip(&d);
    let t = back.block("Title").unwrap();
    assert_eq!((t.description.as_str(), t.units, t.explodable), ("Drawing title block", 4, false));
    let flags: Vec<(String, bool, bool)> = t
        .entities
        .iter()
        .filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some((a.tag.clone(), a.invisible, a.constant)) } else { None })
        .collect();
    assert_eq!(flags, [("COMPANY".into(), false, true), ("SECRET".into(), true, true), ("SHEET".into(), false, false)]);
    let p = back.block("Plain").unwrap();
    assert_eq!((p.description.as_str(), p.units, p.explodable), ("", 0, true));
}

#[test]
fn reads_r12_style_polyline_and_paper_flag() {
    let text = "0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n8\n0\n66\n1\n70\n1\n0\nVERTEX\n8\n0\n10\n0\n20\n0\n0\nVERTEX\n8\n0\n10\n5\n20\n0\n42\n1\n0\nVERTEX\n8\n0\n10\n5\n20\n5\n0\nSEQEND\n0\nLINE\n67\n1\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "a.dxf").unwrap();
    assert_eq!(d.model.len(), 1);
    match &d.model.iter().next().unwrap().kind {
        EntityKind::LwPolyline(p) => {
            assert_eq!(p.vertices.len(), 3);
            assert!(p.closed);
            assert_eq!(p.vertices[1].bulge, 1.0);
        }
        _ => panic!(),
    }
    assert_eq!(d.layouts[0].entities.len(), 1);
}

#[test]
fn entity_transparency_roundtrips_as_group_440() {
    // Issue #132: ByLayer writes nothing, ByBlock 0x01000000, a percentage its alpha | 0x02000000.
    let ts = [Transparency::ByLayer, Transparency::ByBlock, Transparency::Percent(0), Transparency::Percent(40), Transparency::Percent(90)];
    let mut d = Drawing::new_imperial();
    for (i, t) in ts.iter().enumerate() {
        let y = i as f64;
        let line = EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(1.0, y, 0.0) });
        d.add(&Space::Model, Common { transparency: *t, ..Common::default() }, line).unwrap();
    }
    let text = write_dxf(&d);
    assert_eq!(text.matches("\r\n440\r\n").count(), 4, "ByLayer writes no 440");
    assert!(text.contains("\r\n440\r\n16777216\r\n"), "ByBlock");
    assert!(text.contains("\r\n440\r\n33554585\r\n"), "40% is alpha 153");
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.model.iter().map(|e| e.common.transparency).collect::<Vec<_>>(), ts);
    // Values that are neither ByBlock nor "by alpha" read as ByLayer.
    let text = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n440\n-1\n10\n0\n20\n0\n11\n1\n21\n1\n0\nLINE\n8\n0\n440\n33554432\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "a.dxf").unwrap();
    assert_eq!(d.model.iter().map(|e| e.common.transparency).collect::<Vec<_>>(), [Transparency::ByLayer, Transparency::Percent(90)]);
}

#[test]
fn hostile_dxf_does_not_panic() {
    for t in [
        "0\nSECTION\n2\nENTITIES\n0\nHATCH\n91\n999999999\n92\n2\n93\n99999\n0\nENDSEC\n0\nEOF\n",
        "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n99\n40\n1\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n",
        "0\nSECTION\n2\nENTITIES\n0\nINSERT\n66\n1\n2\nX\n0\nATTRIB\n0\nENDSEC\n",
        "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nINSERT\n2\nA\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nENDSEC\n0\nEOF\n",
        "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n42\n5\n20\n1\n0\nENDSEC\n0\nEOF\n",
    ] {
        if let Ok(d) = read(t.as_bytes(), "x.dxf") {
            let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
            let _ = d.extents(&Space::Model);
        }
    }
}

#[test]
fn svg_and_png_export() {
    let d = sample();
    let svg = String::from_utf8(write(&d, "a.svg").unwrap()).unwrap();
    assert!(svg.starts_with("<svg") && svg.contains("polyline"));
    let png = write(&d, "a.png").unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

fn view_box(svg: &str) -> String {
    let i = svg.find("viewBox=\"").map(|i| i + 9).unwrap_or(0);
    svg[i..].split('"').next().unwrap_or("").to_string()
}

#[test]
fn image_export_frames_window() {
    use cadcraft_geom::Bounds2;
    let d = sample();
    let win = Bounds2::new(Vec2::new(1.0, 2.0), Vec2::new(31.0, 22.0));
    // SVG: the viewBox is exactly the window (no margin); the default still fits the extents.
    let framed = String::from_utf8(write_framed(&d, "a.svg", Some(win)).unwrap()).unwrap();
    let plain = String::from_utf8(write(&d, "a.svg").unwrap()).unwrap();
    assert_eq!(view_box(&framed), "0 0 30.000000 20.000000");
    assert_ne!(view_box(&framed), view_box(&plain));
    assert_eq!(plain, String::from_utf8(write_framed(&d, "a.svg", None).unwrap()).unwrap());
    let ext = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default()).bounds;
    let m = ext.width().max(ext.height()) * 0.02;
    assert_eq!(view_box(&plain), format!("0 0 {:.6} {:.6}", ext.width() + 2.0 * m, ext.height() + 2.0 * m));
    // PNG: the window fills the 3:2 image exactly, centred.
    let v = png_view(&ext, 2400, 1600, Some(win));
    assert_eq!(v.center, Vec2::new(16.0, 12.0));
    assert!((v.scale - 80.0).abs() < 1e-9);
    assert_eq!(png_view(&ext, 2400, 1600, None), cadcraft_render::raster::View::fit(&ext, 2400, 1600, 0.05));
    let png_framed = write_framed(&d, "a.png", Some(win)).unwrap();
    assert_eq!(&png_framed[1..4], b"PNG");
    assert_ne!(png_framed, write(&d, "a.png").unwrap());
    // PDF: the window is fitted to the sheet instead of the extents.
    let pdf_framed = write_framed(&d, "a.pdf", Some(win)).unwrap();
    check_pdf(&pdf_framed);
    assert_ne!(pdf_framed, write(&d, "a.pdf").unwrap());
    // Non-image formats ignore the window.
    assert_eq!(write_framed(&d, "a.dxf", Some(win)).unwrap(), write(&d, "a.dxf").unwrap());
}

#[test]
fn hostile_export_window_is_an_error() {
    use cadcraft_geom::Bounds2;
    let d = sample();
    let raw = |x0: f64, y0: f64, x1: f64, y1: f64| Bounds2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) };
    let bad = [
        raw(f64::NAN, 0.0, 1.0, 1.0),
        raw(0.0, 0.0, f64::INFINITY, 1.0),
        raw(0.0, 0.0, 0.0, 1.0),                   // zero width
        raw(0.0, 0.0, 1.0, 0.0),                   // zero height
        raw(5.0, 0.0, 1.0, 1.0),                   // inverted
        raw(-1e300, -1e300, 1e300, 1e300),         // huge
        raw(1e11, 1e11, 1e11 + 1e-6, 1e11 + 1e-6), // vanishingly small next to its coordinates
        Bounds2::EMPTY,
    ];
    for w in bad {
        assert!(check_window(&w).is_err(), "{w:?}");
        for name in ["a.png", "a.svg", "a.pdf"] {
            assert!(write_framed(&d, name, Some(w)).is_err(), "{name} {w:?}");
        }
    }
    assert!(check_window(&raw(-1e6, -1e6, 1e6, 1e6)).is_ok());
    assert!(check_window(&raw(0.0, 0.0, 1e-3, 1e-3)).is_ok());
}

/// Read a PDF back (its structure checked by the reader) and return the first page's decoded
/// content.
fn check_pdf(bytes: &[u8]) -> String {
    assert!(String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64)..]).trim_end().ends_with("%%EOF"));
    let doc = pdf_read::read(bytes);
    assert!(!doc.pages.is_empty());
    doc.content().to_string()
}

fn media_box(bytes: &[u8]) -> (f64, f64) {
    pdf_read::read(bytes).media()
}

/// The line widths (`w`) the content sets.
fn widths(content: &str) -> Vec<f64> {
    pdf_read::ops(content).iter().filter(|o| o.op == "w").filter_map(|o| o.nums.first().copied()).collect()
}

#[test]
fn pdf_model_fitted_to_sheet() {
    let d = sample();
    let bytes = plot(&d, &Space::Model, &serde_json::json!({"paper": "A4", "landscape": true})).unwrap();
    let content = check_pdf(&bytes);
    let (w, h) = media_box(&bytes);
    assert!((w - 841.89).abs() < 0.01 && (h - 595.276).abs() < 0.01, "A4 landscape in points: {w} x {h}");
    assert!(pdf_read::count(&content, "m") > 0 && pdf_read::count(&content, "l") > 0 && pdf_read::count(&content, "S") > 0);
    // Walls layer is red; colour 7 prints black.
    assert!(content.contains("1 0 0 RG"));
    assert!(content.contains("0 0 0 RG"));
    // Uncompressed output is plain text and also valid.
    let raw = plot(&d, &Space::Model, &serde_json::json!({"paper": "Letter", "compress": false, "landscape": false})).unwrap();
    let c2 = check_pdf(&raw);
    assert!(!pdf_read::read(&raw).pages[0].compressed);
    assert!(pdf_read::read(&bytes).pages[0].compressed);
    assert_eq!(media_box(&raw), (612.0, 792.0));
    assert!(pdf_read::clip_rect(&c2).is_some(), "clipped to the plot area: {c2}");
    // write() picks PDF by extension.
    assert!(write(&d, "x.pdf").unwrap().starts_with(b"%PDF"));
}

#[test]
fn pdf_layout_one_to_one_with_viewport() {
    let mut d = sample();
    let paper = Space::Paper("Layout1".into());
    d.layouts[0].page.lineweights = true;
    d.add(
        &paper,
        Common::default(),
        EntityKind::Viewport(Viewport {
            center: Vec3::new(5.0, 4.0, 0.0),
            width: 8.0,
            height: 6.0,
            view_center: Vec2::new(5.0, 2.5),
            view_height: 12.0,
            id: 2,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: None,
            on: true,
        }),
    )
    .unwrap();
    let bytes = plot(&d, &paper, &serde_json::json!({})).unwrap();
    let content = check_pdf(&bytes);
    // ANSI A landscape (inches → points).
    assert_eq!(media_box(&bytes), (792.0, 612.0));
    // Viewport border at 1:1: x from 1in to 9in = 72pt..648pt.
    let corner = |o: &pdf_read::Op| matches!(o.op.as_str(), "m" | "l") && (o.nums[0] - 72.0).abs() < 1e-6 && (o.nums[1] - 72.0).abs() < 1e-6;
    assert!(pdf_read::drawn(&content).iter().any(corner), "{content}");
    // Lineweights on: default 0.25 mm = 0.709 pt.
    assert!(widths(&content).iter().any(|w| (w - 0.709).abs() < 0.001), "{content}");
    let off = plot(&d, &paper, &serde_json::json!({"lineweights": false})).unwrap();
    assert!(widths(&check_pdf(&off)).contains(&0.0));
    assert!(plot(&d, &Space::Paper("Nope".into()), &serde_json::json!({})).is_err());
    assert!(plot(&d, &paper, &serde_json::json!({"paper": "Z9"})).is_err());
}

#[test]
fn pdf_hostile_options_and_empty() {
    let d = Drawing::new_metric();
    let bytes = plot(&d, &Space::Model, &serde_json::json!({})).unwrap();
    check_pdf(&bytes);
    let (w, h) = media_box(&bytes);
    assert!((w - 841.89).abs() < 0.01 && (h - 595.276).abs() < 0.01, "metric default is A4 landscape");
    let s = sample();
    for o in [
        serde_json::json!({"width": 1e308, "height": -5}),
        serde_json::json!({"width": 1e308, "height": 1e308, "scale": 1e308, "fit": false}),
        serde_json::json!({"scale": -1, "fit": false, "title": "a(b)\\c\u{e9}"}),
        serde_json::json!(null),
        serde_json::json!([1, 2]),
    ] {
        check_pdf(&plot(&s, &Space::Model, &o).unwrap());
    }
}

#[test]
fn dxf_roundtrips_page_setup() {
    let mut d = sample();
    let a3 = cadcraft_render::paper_size("A3").unwrap();
    {
        let p = &mut d.layouts[0].page;
        p.paper = a3.name.into();
        p.width_mm = a3.width_mm;
        p.height_mm = a3.height_mm;
        p.landscape = false;
        p.margins_mm = [5.0, 6.0, 7.0, 8.0];
    }
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    let p = &back.layout("Layout1").unwrap().page;
    assert_eq!((p.width_mm, p.height_mm, p.landscape), (297.0, 420.0, false));
    assert_eq!(p.margins_mm, [5.0, 6.0, 7.0, 8.0]);
    assert_eq!(p.paper, a3.name);
}

// ---------------------------------------------------------------------------------------------
// Round trips of styles, overrides, associativity, constraints and tables.

fn dim(kind: DimKind, p13: Vec3, p14: Vec3) -> Dimension {
    Dimension {
        kind,
        defpt: Vec3::new(p13.x, p13.y - 2.0, 0.0),
        text_mid: Vec3::ZERO,
        p13,
        p14,
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

fn first<T>(d: &Drawing, f: impl Fn(&EntityKind) -> Option<T>) -> T {
    d.model.iter().find_map(|e| f(&e.kind)).expect("entity")
}

fn roundtrip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).unwrap()
}

#[test]
fn viewport_frozen_layers_roundtrip() {
    let mut d = sample();
    let paper = Space::Paper("Layout1".into());
    let vp = |id: u32, frozen: Vec<String>| {
        EntityKind::Viewport(Viewport {
            center: Vec3::new(5.0, 4.0, 0.0),
            width: 8.0,
            height: 6.0,
            view_center: Vec2::new(5.0, 2.5),
            view_height: 12.0,
            id,
            locked: true,
            frozen_layers: frozen,
            layer_colors: Vec::new(),
            clip: None,
            on: true,
        })
    };
    d.add(&paper, Common::default(), vp(2, vec!["Walls".into(), "A B".into()])).unwrap();
    d.add(&paper, Common::default(), vp(3, Vec::new())).unwrap();
    let back = roundtrip(&d);
    let frozen: Vec<(u32, Vec<String>)> = back
        .layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) => Some((v.id, v.frozen_layers.clone())),
            _ => None,
        })
        .collect();
    assert!(frozen.contains(&(2, vec!["Walls".to_string(), "A B".to_string()])), "{frozen:?}");
    assert!(frozen.contains(&(3, Vec::new())), "{frozen:?}");
}

/// A layout with viewport 2 frozen on `Walls` and `A B` (both real layers), and its written DXF as
/// (code, value) pairs.
fn frozen_vp_dxf() -> (Drawing, Vec<(String, String)>) {
    let mut d = sample();
    d.layers.push(Layer { name: "A B".into(), ..Layer::default() });
    let paper = Space::Paper("Layout1".into());
    let vp = EntityKind::Viewport(Viewport {
        center: Vec3::new(5.0, 4.0, 0.0),
        width: 8.0,
        height: 6.0,
        view_center: Vec2::new(5.0, 2.5),
        view_height: 12.0,
        id: 2,
        locked: false,
        frozen_layers: vec!["Walls".into(), "A B".into()],
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    });
    d.add(&paper, Common::default(), vp).unwrap();
    let text = write_dxf(&d);
    let lines: Vec<&str> = text.lines().collect();
    let pairs = lines.chunks(2).map(|c| (c[0].trim().to_string(), c.get(1).copied().unwrap_or("").to_string())).collect();
    (d, pairs)
}

/// The pair range of the VIEWPORT record whose group 69 is `id`.
fn vp_record(p: &[(String, String)], id: &str) -> std::ops::Range<usize> {
    let starts: Vec<usize> = (0..p.len()).filter(|&i| p[i].0 == "0").collect();
    for w in starts.windows(2) {
        let r = w[0]..w[1];
        if p[w[0]].1 == "VIEWPORT" && p[r.clone()].iter().any(|(c, v)| c == "69" && v.trim() == id) {
            return r;
        }
    }
    panic!("no VIEWPORT {id}");
}

fn layer_handle(p: &[(String, String)], name: &str) -> String {
    let starts: Vec<usize> = (0..p.len()).filter(|&i| p[i].0 == "0").collect();
    for w in starts.windows(2) {
        let rec = &p[w[0]..w[1]];
        if rec[0].1 == "LAYER" && rec.iter().any(|(c, v)| c == "2" && v == name) {
            return rec.iter().find(|(c, _)| c == "5").unwrap().1.clone();
        }
    }
    panic!("no LAYER {name}");
}

fn viewport_frozen(d: &Drawing, id: u32) -> Vec<String> {
    d.layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .find_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id == id => Some(v.frozen_layers.clone()),
            _ => None,
        })
        .expect("viewport")
}

fn join(p: &[(String, String)]) -> String {
    p.iter()
        .map(|(c, v)| {
            format!(
                "{c}
{v}
"
            )
        })
        .collect()
}

#[test]
fn viewport_frozen_layers_written_as_341_handles() {
    let (_, p) = frozen_vp_dxf();
    let r = vp_record(&p, "2");
    let rec = &p[r];
    let h341: Vec<&str> = rec.iter().filter(|(c, _)| c == "341").map(|(_, v)| v.as_str()).collect();
    assert_eq!(h341, vec![layer_handle(&p, "Walls").as_str(), layer_handle(&p, "A B").as_str()]);
    let last341 = rec.iter().rposition(|(c, _)| c == "341").unwrap();
    let x = rec.iter().position(|(c, _)| c == "1001").expect("xdata");
    assert!(last341 < x, "341 must precede the xdata");
}

#[test]
fn viewport_frozen_layers_read_from_341_without_xdata() {
    let (_, p) = frozen_vp_dxf();
    let r = vp_record(&p, "2");
    // Strip the CADCraft xdata, and add a 341 to a handle that names no layer.
    let x = p[r.clone()].iter().position(|(c, _)| c == "1001").unwrap() + r.start;
    let mut q: Vec<(String, String)> = p[..r.start].to_vec();
    let mut head = p[r.start..x].to_vec();
    head.push(("341".into(), "FFFFF".into()));
    q.extend(head);
    q.extend(p[r.end..].iter().cloned());
    let back = read_dxf(join(&q).as_bytes()).unwrap();
    assert_eq!(viewport_frozen(&back, 2), vec!["Walls".to_string(), "A B".to_string()]);
}

#[test]
fn viewport_frozen_xdata_wins_over_341() {
    let (_, p) = frozen_vp_dxf();
    let r = vp_record(&p, "2");
    // Point every 341 at layer 0: the xdata list must still be what is read.
    let zero = layer_handle(&p, "0");
    let mut q = p.clone();
    for t in &mut q[r] {
        if t.0 == "341" {
            t.1 = zero.clone();
        }
    }
    let back = read_dxf(join(&q).as_bytes()).unwrap();
    assert_eq!(viewport_frozen(&back, 2), vec!["Walls".to_string(), "A B".to_string()]);
}

#[test]
fn viewport_frozen_layers_survive_dwg() {
    let (d, _) = frozen_vp_dxf();
    let dwg = write(&d, "t.dwg").unwrap();
    let back = read(&dwg, "t.dwg").unwrap();
    assert_eq!(viewport_frozen(&back, 2), vec!["Walls".to_string(), "A B".to_string()]);
    // Survives via the CADCraft xdata: the DWG converter does not yet carry group 341 itself.
}

#[test]
fn viewport_layer_colors_roundtrip() {
    let mut d = sample();
    let paper = Space::Paper("Layout1".into());
    let vp = |id: u32, frozen: Vec<String>, colors: Vec<(String, Color)>| {
        EntityKind::Viewport(Viewport {
            center: Vec3::new(5.0, 4.0, 0.0),
            width: 8.0,
            height: 6.0,
            view_center: Vec2::new(5.0, 2.5),
            view_height: 12.0,
            id,
            locked: false,
            frozen_layers: frozen,
            layer_colors: colors,
            clip: None,
            on: true,
        })
    };
    let colors = vec![("Walls".to_string(), Color::Index(5)), ("A B".to_string(), Color::True(cadcraft_color::Rgb(255, 128, 0)))];
    d.add(&paper, Common::default(), vp(2, Vec::new(), colors.clone())).unwrap();
    d.add(&paper, Common::default(), vp(3, Vec::new(), Vec::new())).unwrap();
    // Frozen layers and colour overrides share one CADCraft xdata group.
    d.add(&paper, Common::default(), vp(4, vec!["Walls".into()], colors.clone())).unwrap();
    let back = roundtrip(&d);
    let found: Vec<(u32, Vec<String>, Vec<(String, Color)>)> = back
        .layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) => Some((v.id, v.frozen_layers.clone(), v.layer_colors.clone())),
            _ => None,
        })
        .collect();
    assert!(found.contains(&(2, Vec::new(), colors.clone())), "{found:?}");
    assert!(found.contains(&(3, Vec::new(), Vec::new())), "{found:?}");
    assert!(found.contains(&(4, vec!["Walls".to_string()], colors)), "{found:?}");
}

fn full_dim_style() -> DimStyle {
    DimStyle {
        name: "Mech".into(),
        scale: 2.5,
        arrow_size: 0.25,
        ext_offset: 0.1,
        ext_extend: 0.2,
        text_height: 0.3,
        text_gap: 0.05,
        decimals: 3,
        angular_decimals: 2,
        linear_factor: 0.5,
        text_above: 1,
        text_inside_horizontal: false,
        text_outside_horizontal: false,
        arrow_block: "_ArchTick".into(),
        tick_size: 0.15,
        dim_line_color: Color::Index(1),
        ext_line_color: Color::ByLayer,
        text_color: Color::Index(5),
        text_style: "Romans".into(),
        post: "<> mm".into(),
        center_mark: -0.1,
        zero_suppression: 12,
        linear_unit: 4,
        tolerance: true,
        tol_plus: 0.02,
        tol_minus: 0.01,
        baseline_spacing: 0.5,
        annotative: true,
        arrow_block1: "_DOT".into(),
        arrow_block2: "_Open".into(),
        dim_line_extend: 0.125,
        text_just: 2,
        round: 0.25,
        decimal_separator: ",".into(),
        fraction_format: 1,
        limits: true,
        tol_decimals: 2,
        tol_scale: 0.75,
        tol_zero_suppression: 8,
        alt: true,
        alt_factor: 0.03937,
        alt_decimals: 3,
        alt_post: "[<>]".into(),
        alt_round: 0.005,
        alt_unit: 6,
        alt_zero_suppression: 4,
        angular_unit: 1,
        suppress_ext1: true,
        suppress_ext2: true,
        ..DimStyle::default()
    }
}

#[test]
fn dimstyle_roundtrips_every_field() {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), ..TextStyle::default() });
    d.dim_styles.push(full_dim_style());
    let text = write_dxf(&d);
    // Arrowheads are blocks referenced by handle (342/343/344) with DIMSAH set.
    assert!(text.contains("_ArchTick") && text.contains("_DOT") && text.contains("_Open"));
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.dim_style("Mech").unwrap(), &full_dim_style());
    assert_eq!(back.dim_style("Standard").unwrap(), &DimStyle::default());
    // Generated arrowhead blocks are not imported as user blocks, so they never pile up.
    assert!(back.blocks.keys().all(|k| !k.starts_with('_')), "{:?}", back.blocks.keys().collect::<Vec<_>>());
    let again = roundtrip(&back);
    assert_eq!(again.dim_styles, back.dim_styles);
    assert_eq!(write_dxf(&again).matches("AcDbBlockBegin").count(), write_dxf(&back).matches("AcDbBlockBegin").count());
}

#[test]
fn user_block_arrowheads_are_referenced_not_regenerated() {
    let mut d = Drawing::new_imperial();
    let mut b = Block::new("MyArrow");
    b.entities.push(Entity::new(Handle(0x60), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.5 })));
    d.blocks.insert("MyArrow".into(), std::sync::Arc::new(b));
    d.dim_styles.push(DimStyle { name: "U".into(), arrow_block: "MyArrow".into(), ..DimStyle::default() });
    let back = roundtrip(&d);
    assert_eq!(back.dim_style("U").unwrap().arrow_block, "MyArrow");
    assert!(back.block("MyArrow").is_some());
    assert_eq!(back.blocks.len(), d.blocks.len());
}

/// Every standard arrowhead CADCraft generates reads back as its own geometry, through DXF and
/// DWG, so reopening a drawing CADCraft wrote never turns them into user blocks.
#[test]
fn generated_arrowhead_blocks_are_not_kept_as_user_blocks() {
    let mut d = Drawing::new_metric();
    for (i, (_, name)) in crate::dxf_ext::ARROWS.iter().enumerate() {
        d.dim_styles.push(DimStyle { name: format!("S{i}"), arrow_block: (*name).into(), ..DimStyle::default() });
    }
    let dxf = write_dxf(&d);
    for (_, name) in crate::dxf_ext::ARROWS.iter().filter(|(k, _)| *k != cadcraft_render::Arrowhead::ClosedFilled) {
        assert!(dxf.contains(&format!("\r\n{name}\r\n")), "{name} is written");
    }
    let dwg = cadcraft_dwg::dxf_to_dwg(dxf.as_bytes()).unwrap();
    let from_dwg = read_dxf(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    for (back, what) in [(read_dxf(dxf.as_bytes()).unwrap(), "DXF"), (from_dwg, "DWG")] {
        assert!(back.blocks.keys().all(|k| !k.starts_with('_')), "{what}: {:?}", back.blocks.keys().collect::<Vec<_>>());
        assert!(roundtrip(&back).blocks.keys().all(|k| !k.starts_with('_')), "{what}: second save");
    }
}

/// A file's own arrowhead block with a standard name (AutoCAD's `_Dot` is a closed polyline
/// with width, not CADCraft's disc of solids) is kept when only a dimension style names it, and
/// saved as it was, through DXF and DWG, instead of being replaced by CADCraft's geometry.
#[test]
fn file_arrowhead_blocks_with_standard_names_are_kept() {
    let mut d = Drawing::new_metric();
    let donut = LwPolyline {
        vertices: vec![
            PolyVertex { p: Vec2::new(-0.25, 0.0), bulge: 1.0, start_width: 0.5, end_width: 0.5 },
            PolyVertex { p: Vec2::new(0.25, 0.0), bulge: 1.0, start_width: 0.5, end_width: 0.5 },
        ],
        closed: true,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    };
    for (name, kind) in [("_Dot", EntityKind::LwPolyline(donut)), ("_DotSmall", EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.0625 }))] {
        let mut b = Block::new(name);
        b.entities.push(Entity::new(Handle(0), kind));
        d.blocks.insert(name.into(), std::sync::Arc::new(b));
    }
    d.dim_styles.push(DimStyle { name: "Dots".into(), arrow_block1: "_Dot".into(), arrow_block2: "_DotSmall".into(), ..DimStyle::default() });
    let kinds = |d: &Drawing, name: &str| d.block(name).map(|b| b.entities.iter().map(|e| e.kind.clone()).collect::<Vec<_>>()).unwrap_or_default();
    let dxf = write_dxf(&d);
    let dwg = cadcraft_dwg::dxf_to_dwg(dxf.as_bytes()).unwrap();
    let from_dwg = read_dxf(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    for (back, what) in [(read_dxf(dxf.as_bytes()).unwrap(), "DXF"), (from_dwg, "DWG")] {
        for name in ["_Dot", "_DotSmall"] {
            assert_eq!(kinds(&back, name), kinds(&d, name), "{what}: {name}");
        }
        let again = write_dxf(&back);
        assert!(!again.contains("\nSOLID\r"), "{what}: no arrowhead geometry of CADCraft's is written");
        assert_eq!(kinds(&roundtrip(&back), "_Dot"), kinds(&d, "_Dot"), "{what}: a second save keeps it too");
    }
}

#[test]
fn dimension_overrides_roundtrip_as_dstyle_xdata() {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), ..TextStyle::default() });
    let mut dm = dim(DimKind::Linear { rotation: 0.0 }, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
    let raw = serde_json::json!({
        "arrowSize": 0.5,
        "dimLineColor": 3,
        "textStyle": "Romans",
        "arrowBlock1": "_Dot",
        "arrowBlock": "",
        "decimalSeparator": ",",
        "suppressExt1": true,
        "post": "<> mm",
        "decimals": 2,
        "textJust": 1,
        "altFactor": 25.4,
    });
    dm.overrides = raw.as_object().unwrap().clone();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("DSTYLE"));
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = first(&back, |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    // Values come back canonical (colours as colour objects); the effective style is identical.
    assert_eq!(got.overrides.len(), dm.overrides.len(), "{:?}", got.overrides);
    let base = DimStyle::default();
    assert_eq!(base.with_overrides(&got.overrides), base.with_overrides(&dm.overrides));
    assert_eq!(got.overrides.get("dimLineColor"), Some(&serde_json::to_value(Color::Index(3)).unwrap()));
    // Canonical values round-trip exactly.
    let again = first(&roundtrip(&back), |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    assert_eq!(again.overrides, got.overrides);
}

/// A drawing with `n` lines, circles and polylines on a grid (issue #57's tests).
fn many_entities(n: usize) -> Drawing {
    let mut d = Drawing::new_imperial();
    for i in 0..n {
        let (x, y) = ((i % 300) as f64 * 10.0, (i / 300) as f64 * 10.0);
        let kind = match i % 3 {
            0 => EntityKind::Line(Line { a: Vec3::new(x, y, 0.0), b: Vec3::new(x + 4.0, y + (i % 7) as f64, 0.0) }),
            1 => EntityKind::Circle(Circle { center: Vec3::new(x, y, 0.0), radius: 1.0 + (i % 5) as f64 }),
            _ => EntityKind::LwPolyline(LwPolyline {
                vertices: (0..8).map(|k| PolyVertex::new(Vec2::new(x + f64::from(k), y + (k % 3) as f64))).collect(),
                closed: false,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
        };
        d.add(&Space::Model, Common::default(), kind).unwrap();
    }
    d
}

/// An AutoCAD 2010 (AC1024) DWG of `n` entities, made through the DWG bridge.
#[cfg(not(target_arch = "wasm32"))]
fn ac1024_dwg(n: usize) -> Vec<u8> {
    let dxf = write_dxf(&many_entities(n)).replacen("AC1015", "AC1024", 1);
    let dwg = cadcraft_dwg::dxf_to_dwg(dxf.as_bytes()).unwrap();
    assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some("AC1024"));
    dwg
}

/// Issue #57: an AutoCAD 2010 DWG with tens of thousands of entities opens completely, and its
/// DXF rendition stays far inside the limits (about 6 bytes of DXF and 0.4 group codes per DWG
/// byte), so a 10 MB file is nowhere near them.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ac1024_dwg_with_many_entities_opens() {
    let n = 30_000;
    let dwg = ac1024_dwg(n);
    let dxf = cadcraft_dwg::dwg_to_dxf(&dwg).unwrap();
    let tags = cadcraft_dxf::parse(&dxf).unwrap().len();
    assert!(dxf.len() < dwg.len() * 20, "DXF {} bytes from {} DWG bytes", dxf.len(), dwg.len());
    assert!(tags < dwg.len(), "{tags} group codes from {} DWG bytes", dwg.len());
    let d = read(&dwg, "big.dwg").unwrap();
    assert_eq!(d.entity_count(), n);
}

/// Issue #57: oversized input is rejected early, with a message naming the limit, never a panic.
/// A conversion that runs away (a damaged or misread DWG can make the reader produce a drawing
/// thousands of times larger than the file) stops at the DXF size limit instead of filling memory
/// and failing later with "file too large"; the DWG size is checked before reading; the DXF
/// group-code count before parsing.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn oversized_dwg_and_dxf_are_rejected_early() {
    let dwg = ac1024_dwg(3000);
    let full = cadcraft_dwg::dwg_to_dxf(&dwg).unwrap().len();
    assert!(full > 400_000, "{full}");
    let limits = cadcraft_dwg::Limits { max_dxf_bytes: 300_000, ..cadcraft_dwg::Limits::DEFAULT };
    let e = cadcraft_dwg::dwg_to_dxf_with(&dwg, limits).unwrap_err();
    assert!(e.starts_with("DWG: converting this 0."), "{e}");
    assert!(e.contains("MB file produced more than 0.3 MB of drawing data (the limit)"), "{e}");

    let limits = cadcraft_dwg::Limits { max_dwg_bytes: 100_000, ..cadcraft_dwg::Limits::DEFAULT };
    let e = cadcraft_dwg::dwg_to_dxf_with(&dwg, limits).unwrap_err();
    assert!(e.starts_with("DWG: the file is 0.") && e.ends_with("MB, larger than the 0.1 MB CADCraft opens"), "{e}");

    // Lines that aren't even group codes: the size is reported before anything is parsed.
    let text = "not a group code\nx\n".repeat(60);
    let e = cadcraft_dxf::parse_with_limit(text.as_bytes(), 50).unwrap_err();
    assert_eq!(e.to_string(), "drawing too large: 60 DXF group codes, more than the 50 CADCraft reads");
}

/// Issue #800: one unreadable object record doesn't fail the whole DWG. The reader skips it, the
/// rest opens, and the drawing's read notes say how many records were left out and of which
/// types, so a partial read never passes for a complete one. A complete read has no notes.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn damaged_dwg_opens_partially_and_says_what_it_left_out() {
    let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..200 {
        dxf += &format!("0\nLINE\n8\n0\n10\n{i}\n20\n0\n11\n{i}\n21\n10\n");
    }
    dxf += "0\nENDSEC\n0\nEOF\n";
    let d = read(dxf.as_bytes(), "lines.dxf").unwrap();
    let dwg = crate::write_with(&d, "lines.dwg", &crate::SaveOptions { version: Some(crate::Version::R2000), binary: false }).unwrap();
    let whole = read(&dwg, "lines.dwg").unwrap();
    assert_eq!((whole.entity_count(), whole.read_notes.len()), (200, 0));

    // Damage a few bytes at a time across the objects area: every damaged copy that still reads
    // as a DWG either reads whole or notes what it left out.
    let mut partial = 0;
    for at in (dwg.len() / 4..dwg.len() * 3 / 5).step_by(97) {
        let mut b = dwg.clone();
        b[at..at + 6].iter_mut().for_each(|x| *x ^= 0xA5);
        let Ok(d) = read(&b, "damaged.dwg") else { continue };
        if d.entity_count() < 200 {
            assert_eq!(d.read_notes.len(), 1, "{} lines read, no note", d.entity_count());
        }
        if let [note] = d.read_notes.as_slice() {
            assert!(note.starts_with("DWG: ") && note.contains(" couldn't be read and "), "{note}");
            assert!(note.ends_with("saving over the original file would lose it") || note.ends_with("would lose them"), "{note}");
            partial += 1;
        }
    }
    assert!(partial > 0, "no damaged copy read partially");
}

/// A dimension's standard DIMASSOC objects carry the same associations as CADCraft's xdata, so
/// a file from another writer (xdata stripped) still follows its geometry: a linear dimension's
/// two origins, a polyline's vertex named by its segment (#1049), and since #1651 a radial
/// dimension's circle, whose centre comes back as a `Center` link and its rim point as
/// `OnCircle`.
#[test]
fn dimension_associativity_roundtrips() {
    let mut d = Drawing::new_imperial();
    let c = Common::default;
    let l1 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 5.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(10.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) })).unwrap();
    let ci = d.add(&Space::Model, c(), EntityKind::Circle(Circle { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0 })).unwrap();
    let pl = d
        .add(
            &Space::Model,
            c(),
            EntityKind::LwPolyline(LwPolyline {
                vertices: vec![PolyVertex::new(Vec2::new(30.0, 0.0)), PolyVertex::new(Vec2::new(32.0, 0.0)), PolyVertex::new(Vec2::new(32.0, 3.0))],
                closed: false,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
        )
        .unwrap();
    let mut lin = dim(DimKind::Linear { rotation: 0.0 }, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
    lin.assoc = vec![
        DimAssoc { point: "p13".into(), handle: l1, snap: AssocSnap::Start },
        DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::Start },
    ];
    let mut ali = dim(DimKind::Aligned, Vec3::new(22.0, 0.0, 0.0), Vec3::new(32.0, 3.0, 0.0));
    ali.assoc = vec![
        DimAssoc { point: "p13".into(), handle: ci, snap: AssocSnap::OnCircle { angle: 0.0 } },
        DimAssoc { point: "p14".into(), handle: pl, snap: AssocSnap::Vertex { index: 2 } },
        DimAssoc { point: "defpt".into(), handle: l1, snap: AssocSnap::Intersection { other: l2 } },
    ];
    let mut rad = dim(DimKind::Radius, Vec3::new(20.0, 0.0, 0.0), Vec3::ZERO);
    rad.assoc = vec![DimAssoc { point: "defpt".into(), handle: ci, snap: AssocSnap::Center }];
    let hl = d.add(&Space::Model, c(), EntityKind::Dimension(lin.clone())).unwrap();
    let ha = d.add(&Space::Model, c(), EntityKind::Dimension(ali.clone())).unwrap();
    let hr = d.add(&Space::Model, c(), EntityKind::Dimension(rad.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("DIMASSOC") && text.contains("ACAD_DIMASSOC") && text.contains("AcDbOsnapPointRef"));
    let back = read_dxf(text.as_bytes()).unwrap();
    let assoc_of = |d: &Drawing, h: Handle| match &d.entity(h).unwrap().kind {
        EntityKind::Dimension(x) => x.assoc.clone(),
        _ => panic!(),
    };
    assert_eq!(assoc_of(&back, hl), lin.assoc);
    assert_eq!(assoc_of(&back, ha), ali.assoc);
    assert_eq!(assoc_of(&back, hr), rad.assoc);
    // Entities owning reactors still land in model space.
    assert_eq!(back.model.len(), d.model.len());
    assert!(back.layouts.iter().all(|l| l.entities.is_empty()));

    // Without CADCraft's xdata (a file from another writer), the standard DIMASSOC objects
    // still give the extension-line links.
    let tags = cadcraft_dxf::parse(text.as_bytes()).unwrap();
    let mut stripped = Vec::new();
    let mut skipping = false;
    for t in tags {
        if t.code == 1001 {
            skipping = t.str() == "CADCRAFT";
        } else if t.code < 1000 {
            skipping = false;
        }
        if !skipping {
            stripped.push(t);
        }
    }
    let foreign = read_dxf(cadcraft_dxf::write_ascii(&stripped).as_bytes()).unwrap();
    assert_eq!(assoc_of(&foreign, hl), lin.assoc);
    let fa = assoc_of(&foreign, ha);
    assert_eq!(fa.len(), 2);
    assert_eq!(fa[1], ali.assoc[1], "the polyline vertex, named by its segment (#1049)");
    assert!(matches!(fa[0].snap, AssocSnap::OnCircle { angle } if angle.abs() < 1e-9));
    let fr = assoc_of(&foreign, hr);
    assert_eq!(fr.len(), 2, "a radial dimension's circle comes from the standard DIMASSOC");
    assert_eq!(fr[0], rad.assoc[0], "the centre");
    assert!(matches!(fr[1].snap, AssocSnap::OnCircle { angle } if angle.is_finite()));
}

/// #1049: a polyline vertex is referenced as AutoCAD does: an end of one of its segments,
/// numbered from 1 (subentity type 73 = 2, segment in 91, osnap 1 end / 13 start, parameter 1 /
/// 0), preferring a straight segment. The cases are the ones observed in AutoCAD 2026 (black box)
/// in the PR review.
#[test]
fn polyline_vertex_references_name_the_segment() {
    let poly = |pts: &[(f64, f64)], closed: bool, bulges: &[(usize, f64)]| LwPolyline {
        vertices: pts
            .iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let b = bulges.iter().find(|(j, _)| *j == i).map_or(0.0, |(_, b)| *b);
                PolyVertex::with_bulge(Vec2::new(*x, *y), b)
            })
            .collect(),
        closed,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    };
    let refs = |p: &LwPolyline| (0..p.vertices.len()).map(|i| crate::dxf_write::polyline_vertex_ref(p, i)).collect::<Vec<_>>();
    // Open: vertex 0 is the start of segment 1, vertex k the end of segment k.
    let open = poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (40.0, 60.0)], false, &[]);
    assert_eq!(refs(&open), [Some((13, 0.0, 1)), Some((1, 1.0, 1)), Some((1, 1.0, 2)), Some((1, 1.0, 3))]);
    // Closed (clockwise): the same; the closing segment is the 4th.
    let closed = poly(&[(0.0, 0.0), (0.0, 60.0), (100.0, 60.0), (100.0, 0.0)], true, &[]);
    assert_eq!(refs(&closed), [Some((13, 0.0, 1)), Some((1, 1.0, 1)), Some((1, 1.0, 2)), Some((1, 1.0, 3))]);
    // An arc as segment 3: its end, vertex 3, is the start of the straight segment 4 instead.
    let arc = poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (40.0, 60.0), (0.0, 30.0)], true, &[(2, 0.5)]);
    assert_eq!(refs(&arc)[3], Some((13, 0.0, 4)));
    assert_eq!(refs(&arc)[2], Some((1, 1.0, 2)));
    // Between two arcs there is no straight segment: no reference.
    let arcs = poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0)], false, &[(0, 0.5), (1, 0.5)]);
    assert_eq!(refs(&arcs)[1], None);
}

/// #1049: dimensions on the vertices of a polyline are written with segment references, and a
/// file with only those (another writer's, whose reference points AutoCAD leaves at 0,0) is read
/// back to the same vertices.
#[test]
fn polyline_vertex_dimensions_round_trip_by_segment() {
    let mut d = Drawing::new_metric();
    let c = Common::default;
    let pts = [(0.0, 0.0), (160.0, 0.0), (160.0, 210.0), (0.0, 210.0)];
    let pl = d
        .add(
            &Space::Model,
            c(),
            EntityKind::LwPolyline(LwPolyline {
                vertices: pts.iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(),
                closed: true,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
        )
        .unwrap();
    let mut on_pl = dim(DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 }, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 210.0, 0.0));
    on_pl.assoc = vec![
        DimAssoc { point: "p13".into(), handle: pl, snap: AssocSnap::Vertex { index: 0 } },
        DimAssoc { point: "p14".into(), handle: pl, snap: AssocSnap::Vertex { index: 3 } },
    ];
    let hp = d.add(&Space::Model, c(), EntityKind::Dimension(on_pl.clone())).unwrap();
    let text = write_dxf(&d);
    let tags = cadcraft_dxf::parse(text.as_bytes()).unwrap();
    // Each reference to the polyline: (72, 73, 91).
    let refs: Vec<(i64, i64, i64)> = tags
        .iter()
        .enumerate()
        .filter(|(_, t)| t.code == 331 && t.str() == pl.hex())
        .map(|(i, _)| {
            let back = |code: i32| tags[..i].iter().rev().find(|t| t.code == code).and_then(|t| t.str().parse().ok()).unwrap_or(-1);
            let fwd = |code: i32| tags[i..].iter().find(|t| t.code == code).and_then(|t| t.str().parse().ok()).unwrap_or(-1);
            (back(72), fwd(73), fwd(91))
        })
        .collect();
    assert_eq!(refs, [(13, 2, 1), (1, 2, 3)]);

    // Without CADCraft's xdata and with the reference points at 0,0 (as AutoCAD writes them),
    // the segments still give the vertices.
    let mut stripped = Vec::new();
    let mut skipping = false;
    let mut in_ref = false;
    for mut t in tags {
        if t.code == 1001 {
            skipping = t.str() == "CADCRAFT";
        } else if t.code < 1000 {
            skipping = false;
        }
        if t.code == 1 {
            in_ref = t.str() == "AcDbOsnapPointRef";
        }
        if in_ref && matches!(t.code, 10 | 20) {
            t = cadcraft_dxf::Tag::new(t.code, cadcraft_dxf::Value::Real(0.0));
        }
        if !skipping {
            stripped.push(t);
        }
    }
    let foreign = read_dxf(cadcraft_dxf::write_ascii(&stripped).as_bytes()).unwrap();
    match &foreign.entity(hp).unwrap().kind {
        EntityKind::Dimension(x) => assert_eq!(x.assoc, on_pl.assoc),
        _ => panic!(),
    }
}

fn parametric_sample(d: &mut Drawing) -> (Vec<Constraint>, Parametric) {
    let l1 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(4.0, 0.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(4.0, 0.0, 0.0), b: Vec3::new(4.0, 3.0, 0.0) })).unwrap();
    let constraints = vec![
        Constraint { id: 1, kind: ConstraintKind::Horizontal, refs: vec![GeomRef::whole(l1)], name: String::new(), expr: String::new() },
        Constraint {
            id: 2,
            kind: ConstraintKind::Coincident,
            refs: vec![GeomRef::new(l1, Sub::End), GeomRef::new(l2, Sub::Start)],
            name: String::new(),
            expr: String::new(),
        },
        Constraint {
            id: 3,
            kind: ConstraintKind::Distance(DistAxis::Vertical),
            refs: vec![GeomRef::new(l2, Sub::Start), GeomRef::new(l2, Sub::End)],
            name: "d1".into(),
            expr: "width/2 + 1".into(),
        },
        Constraint {
            id: 4,
            kind: ConstraintKind::Angular,
            refs: vec![GeomRef::whole(l1), GeomRef::whole(l2)],
            name: "ang1".into(),
            expr: "90".into(),
        },
        Constraint {
            id: 5,
            kind: ConstraintKind::Fix,
            refs: vec![GeomRef::new(l2, Sub::Vertex(7)), GeomRef::new(l2, Sub::Segment(3))],
            name: String::new(),
            expr: String::new(),
        },
    ];
    // Long text (chunk boundaries inside strings, spaces at the edges), backslashes that
    // would look like DXF `\U+` escapes, and non-ASCII text.
    let long = format!("  {}  \\U+0041 \\\\ \"quoted\" Ünïcødé ✓ {}  ", "word ".repeat(80), "x".repeat(300));
    let parametric = Parametric {
        parameters: vec![
            Parameter { name: "width".into(), expr: "12.5".into(), description: long },
            Parameter { name: "h".into(), expr: "width*0.4".into(), description: String::new() },
        ],
        settings: ParametricSettings {
            infer: true,
            distance_tolerance: 0.125,
            angle_tolerance: 2.5,
            auto_types: vec!["Parallel".into(), "Tangent".into()],
            bars_visible: false,
            bar_exceptions: vec![l1],
            dims_visible: false,
            dim_exceptions: vec![l2],
            bar_transparency: 30,
        },
    };
    d.constraints = constraints.clone();
    d.parametric = parametric.clone();
    (constraints, parametric)
}

#[test]
fn constraints_and_parameters_roundtrip_exactly() {
    let mut d = Drawing::new_imperial();
    let (constraints, parametric) = parametric_sample(&mut d);
    let text = write_dxf(&d);
    assert!(text.contains("CADCRAFT_CONSTRAINTS") && text.contains("XRECORD"));
    assert!(!text.contains("\\U+0041"), "no DXF unicode escape may appear in the payload");
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.constraints, constraints);
    assert_eq!(back.parametric, parametric);
    let again = roundtrip(&back);
    assert_eq!(again.constraints, constraints);
    assert_eq!(again.parametric, parametric);
    // Drawings without parametric data carry no record.
    assert!(!write_dxf(&Drawing::new_imperial()).contains("CADCRAFT_CONSTRAINTS"));
}

#[test]
fn layer_states_roundtrip_next_to_constraints() {
    let mut d = Drawing::new_imperial();
    let (constraints, _) = parametric_sample(&mut d);
    d.layers.push(Layer {
        name: "Wände".into(),
        color: Color::True(cadcraft_color::Rgb(200, 10, 40)),
        transparency: 30,
        description: "walls \\ hidden".into(),
        ..Layer::default()
    });
    d.layer_states.push(LayerState { name: "Plot".into(), layers: d.layers.clone() });
    let mut off = d.layers.clone();
    for l in &mut off {
        (l.on, l.frozen, l.locked, l.plot) = (false, true, true, false);
    }
    d.layer_states.push(LayerState { name: "All off".into(), layers: off });
    let text = write_dxf(&d);
    assert!(text.contains("CADCRAFT_LAYERSTATES"));
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.layer_states, d.layer_states);
    assert_eq!(back.constraints, constraints);
    assert_eq!(roundtrip(&back).layer_states, d.layer_states);
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(read(&write(&d, "x.dwg").unwrap(), "x.dwg").unwrap().layer_states, d.layer_states);
    // Drawings without saved layer states carry no record.
    assert!(!write_dxf(&Drawing::new_imperial()).contains("CADCRAFT_LAYERSTATES"));
}

fn table_sample() -> Table {
    let cell = |t: &str| TableCell { text: t.into(), ..Default::default() };
    let mut rows = vec![
        vec![cell("Door schedule"), cell(""), cell("")],
        vec![cell("Mark"), cell("Size"), cell("Notes")],
        vec![cell("D1"), cell("900 x 2100"), cell(&"long note ".repeat(40))],
        vec![cell("D2"), cell("800 x 2100"), cell("")],
    ];
    rows[0][0].merged = Some((1, 3));
    rows[2][0].merged = Some((2, 1));
    Table {
        insert: Vec3::new(5.0, 20.0, 0.0),
        col_widths: vec![1.5, 2.5, 4.0],
        row_heights: vec![0.5, 0.4, 0.4, 0.4],
        cells: rows,
        style: "Schedule".into(),
        text_height: 0.2,
        title: true,
        header: true,
        ..Table::default()
    }
}

#[test]
fn tables_roundtrip_with_title_header_and_merges() {
    let mut d = Drawing::new_imperial();
    d.table_styles.push(TableStyle { name: "Schedule".into(), text_height: 0.25, margin: 0.1, title: false, header: true, ..TableStyle::default() });
    let t = table_sample();
    d.add(&Space::Model, Common::default(), EntityKind::Table(t.clone())).unwrap();
    let text = write_dxf(&d);
    assert!(text.contains("ACAD_TABLE") && text.contains("TABLESTYLE") && text.contains("*T1"));
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None });
    assert_eq!(got, t);
    assert_eq!(back.table_styles, d.table_styles);
    assert!(back.block("*T1").is_none(), "table blocks are regenerated, not imported");
    let again = roundtrip(&back);
    assert_eq!(first(&again, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None }), t);
    assert_eq!(again.blocks.len(), back.blocks.len());
}

#[test]
fn table_block_is_relative_to_the_insertion_point() {
    let d = Drawing::new_imperial();
    let t = table_sample();
    let ents = crate::dxf_write::table_block_entities(&d, &t, "0");
    let mut xs = Vec::new();
    for e in &ents {
        if let EntityKind::Line(l) = &e.kind {
            xs.extend([l.a.x, l.b.x]);
        }
    }
    assert!(!xs.is_empty());
    let (min, max) = xs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    // Columns total 8 wide; the block starts at its own origin, not at the insert x of 5.
    assert!(min.abs() < 1e-9 && (max - 8.0).abs() < 1e-9, "block x range {min}..{max}");
}

#[test]
fn text_style_flags_roundtrip() {
    let mut d = Drawing::new_imperial();
    let st = TextStyle {
        name: "Mirror".into(),
        font: "romans.shx".into(),
        big_font: "bigfont.shx".into(),
        height: 0.0,
        width_factor: 0.8,
        oblique: 15f64.to_radians(),
        backwards: true,
        upside_down: true,
        vertical: true,
        annotative: true,
    };
    d.text_styles.push(st.clone());
    d.text_styles.push(TextStyle { name: "Plain".into(), font: "arial.ttf".into(), backwards: true, ..TextStyle::default() });
    let back = roundtrip(&d);
    let got = back.text_style("Mirror").unwrap();
    assert_eq!((got.backwards, got.upside_down, got.vertical, got.annotative), (true, true, true, true));
    assert_eq!(got.big_font, "bigfont.shx");
    assert!((got.oblique - st.oblique).abs() < 1e-12);
    let plain = back.text_style("Plain").unwrap();
    assert_eq!((plain.backwards, plain.upside_down, plain.vertical, plain.annotative), (true, false, false, false));
}

/// Non-ASCII text in every kind of string the writer emits: CJK, Latin-1 and an astral
/// character (issue #164).
const INTL: &str = "图号 Ä°ø Ø 😀";

fn intl_text(value: &str) -> Text {
    Text {
        insert: Vec3::new(1.0, 1.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    }
}

fn intl_sample() -> Drawing {
    let mut d = Drawing::new_metric();
    let layer = format!("层 {INTL}");
    let block = format!("块 {INTL}");
    d.layers.push(Layer { name: layer.clone(), color: Color::Index(3), ..Layer::default() });
    let on = Common { layer: layer.clone(), ..Common::default() };
    d.add(&Space::Model, on.clone(), EntityKind::Text(intl_text(INTL))).unwrap();
    d.add(
        &Space::Model,
        on.clone(),
        EntityKind::MText(MText {
            insert: Vec3::new(5.0, 5.0, 0.0),
            height: 2.5,
            width: 30.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: format!("{INTL}\\P第二行 Ünïcødé"),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
    )
    .unwrap();
    let def = Attrib {
        tag: "图号".into(),
        text: intl_text("默认"),
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    let mut b = Block::new(&block);
    b.entities.push(Entity::new(Handle(0x50), EntityKind::AttDef(def)));
    d.blocks.insert(block.clone(), std::sync::Arc::new(b));
    let att = Attrib {
        tag: "图号".into(),
        text: intl_text(INTL),
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    d.add(
        &Space::Model,
        on,
        EntityKind::Insert(Insert {
            block,
            insert: Vec3::new(2.0, 2.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![att],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    )
    .unwrap();
    d
}

fn assert_intl(back: &Drawing) {
    let layer = format!("层 {INTL}");
    let block = format!("块 {INTL}");
    assert_eq!(back.layer(&layer).map(|l| l.color), Some(Color::Index(3)));
    let text = first(back, |k| if let EntityKind::Text(t) = k { Some(t.value.clone()) } else { None });
    assert_eq!(text, INTL);
    let mtext = first(back, |k| if let EntityKind::MText(t) = k { Some(t.contents.clone()) } else { None });
    assert_eq!(mtext, format!("{INTL}\\P第二行 Ünïcødé"));
    let ins = first(back, |k| if let EntityKind::Insert(i) = k { Some(i.clone()) } else { None });
    assert_eq!(ins.block, block);
    assert_eq!(ins.attribs.len(), 1);
    assert_eq!(ins.attribs[0].tag, "图号");
    assert_eq!(ins.attribs[0].text.value, INTL);
    let b = back.block(&block).unwrap();
    let def = b.entities.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).unwrap();
    assert_eq!(def.tag, "图号");
    assert_eq!(def.text.value, "默认");
    assert!(back.model.iter().all(|e| e.common.layer == layer));
}

/// The writer declares R2000 (`AC1015`, `ANSI_1252`), where text is in the code page and not
/// UTF-8, so every non-ASCII character must be a `\U+XXXX` escape and the file pure ASCII.
#[test]
fn non_ascii_text_is_escaped_for_r2000_and_roundtrips() {
    let d = intl_sample();
    let text = write_dxf(&d);
    assert!(text.is_ascii(), "an AC1015 DXF must not contain UTF-8 bytes");
    let tags = cadcraft_dxf::parse(text.as_bytes()).unwrap();
    let header = |name: &str| tags.windows(2).find(|w| w[0].code == 9 && w[0].str() == name).map(|w| w[1].str());
    assert_eq!(header("$ACADVER").as_deref(), Some("AC1015"));
    assert_eq!(header("$DWGCODEPAGE").as_deref(), Some("ANSI_1252"));
    // CJK, Latin-1 and a surrogate pair for the astral character.
    assert!(text.contains("\\U+56FE\\U+53F7 \\U+00C4\\U+00B0\\U+00F8 \\U+00D8 \\U+D83D\\U+DE00"));
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_intl(&back);
    // A second save writes the same text.
    assert_eq!(write_dxf(&back).matches("\\U+").count(), text.matches("\\U+").count());
}

/// Real R2007+ files are UTF-8 and older ones are in their code page; both still read.
#[test]
fn reads_utf8_and_code_page_text() {
    let file = |ver: &str, value: &[u8]| {
        let mut b =
            format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{ver}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n1\n1\n")
                .into_bytes();
        b.extend_from_slice(value);
        b.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        b
    };
    let value = |bytes: &[u8]| first(&read_dxf(bytes).unwrap(), |k| if let EntityKind::Text(t) = k { Some(t.value.clone()) } else { None });
    assert_eq!(value(&file("AC1032", INTL.as_bytes())), INTL);
    assert_eq!(value(&file("AC1015", b"Caf\xe9 \xd8 \\U+56FE\\U+53F7")), "Café Ø 图号");
}

/// Saving as DWG goes through this DXF and acadrust, which decodes the escapes.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_roundtrip_keeps_non_ascii_text() {
    let back = read(&write(&intl_sample(), "x.dwg").unwrap(), "x.dwg").unwrap();
    assert_intl(&back);
}

#[test]
fn hostile_extension_data_never_panics() {
    let ent = |body: &str| format!("0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n");
    let obj = |body: &str| format!("0\nSECTION\n2\nOBJECTS\n{body}0\nENDSEC\n0\nEOF\n");
    let cases = [
        // DSTYLE: unbalanced, odd counts, unknown codes, huge values, bad handles.
        ent("0\nDIMENSION\n5\nA0\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n{\n1070\n"),
        ent("0\nDIMENSION\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n{\n1070\n40\n1000\nnot a number\n1070\n271\n1070\n32767\n1070\n342\n1005\nZZZZ\n1070\n278\n1070\n-5\n1070\n176\n1070\n-32000\n1070\n9999\n1040\n1e308\n1002\n}\n"),
        ent("0\nDIMENSION\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n}\n1002\n{\n1002\n{\n"),
        // ASSOC: malformed links, missing handles, huge vertex index, nested braces.
        ent("0\nDIMENSION\n70\n1\n1001\nCADCRAFT\n1000\nASSOC\n1002\n{\n1002\n{\n1000\np13\n1070\n6\n1071\n-1\n1002\n}\n1002\n{\n1000\nbogus\n1005\n1\n1070\n0\n1002\n}\n1002\n{\n1000\np14\n1005\nFFFFFFFFFFFFFFFFFFFF\n1070\n4\n1002\n}\n1002\n{\n1000\np14\n1005\n2A\n1070\n6\n1071\n2147483647\n1002\n}\n"),
        ent("0\nDIMENSION\n70\n0\n1001\nCADCRAFT\n1000\nASSOC\n1002\n{\n1002\n{\n1002\n{\n1002\n{\n"),
        // Tables: enormous or negative counts, cells without bodies, spans past the edge.
        ent("0\nACAD_TABLE\n2\n*T1\n100\nAcDbTable\n91\n999999999\n92\n999999999\n171\n1\n0\nACAD_TABLE\n100\nAcDbTable\n91\n-4\n92\n3\n171\n"),
        ent("0\nACAD_TABLE\n100\nAcDbTable\n91\n2\n92\n2\n141\nnan\n142\n-1\n171\n1\n173\n1\n175\n99999\n176\n-3\n1\nx\n171\n171\n171\n171\n171\n2\nzzz\n1001\nCADCRAFT\n1000\nTABLE\n1040\n-1\n"),
        // Cell text heights (140): junk, huge, unbalanced value block.
        ent("0\nACAD_TABLE\n100\nAcDbTable\n91\n1\n92\n3\n171\n1\n140\nnan\n171\n1\n140\n-5\n301\nCELL_VALUE\n140\n1e308\n171\n1\n304\nACVALUE_END\n140\n1e999\n1\nx\n"),
        // DIMASSOC pointing at nothing, or at a non-dimension.
        format!(
            "{}{}",
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n10\n10\n0\n20\n0\n11\n1\n21\n0\n0\nDIMENSION\n5\n11\n70\n0\n0\nENDSEC\n",
            "0\nSECTION\n2\nOBJECTS\n0\nDIMASSOC\n100\nAcDbDimAssoc\n330\n11\n90\n-1\n1\nAcDbOsnapPointRef\n72\n1\n331\n10\n1\nAcDbOsnapPointRef\n72\n6\n331\n10\n332\n99\n1\n1\n1\n0\nDIMASSOC\n100\nAcDbDimAssoc\n330\n10\n90\n3\n0\nDIMASSOC\n0\nENDSEC\n0\nEOF\n"
        ),
        // Constraint record: garbage, wrong version, deep nesting, unterminated.
        obj("0\nDICTIONARY\n5\nC\n3\nCADCRAFT_CONSTRAINTS\n350\nD\n0\nXRECORD\n5\nD\n1\n{\"version\":1,\"constraints\":[{\"id\":1,\"kind\":\"nope\"}]}\n"),
        obj(&format!("0\nDICTIONARY\n3\nCADCRAFT_CONSTRAINTS\n350\nD\n0\nXRECORD\n5\nD\n1\n{}\n", "[".repeat(5000))),
        obj("0\nDICTIONARY\n3\nCADCRAFT_CONSTRAINTS\n350\nD\n3\nX\n0\nXRECORD\n5\nD\n1\n{\"version\":99}\n"),
        obj("0\nDICTIONARY\n3\nCADCRAFT_CONSTRAINTS\n0\nXRECORD\n1\n{\"version\":1,\n"),
        // Table styles with junk numbers; DIMSTYLE with junk and dangling handles.
        obj("0\nDICTIONARY\n3\nS\n350\nE\n0\nTABLESTYLE\n5\nE\n40\n-7\n140\n0\n280\n9\n"),
        "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nDIMSTYLE\n0\nDIMSTYLE\n2\nX\n271\n99999\n340\nBAD\n342\n0\n343\nFFFF\n176\n70000\n278\n0\n278\n1114112\n77\n-1\n1001\nAcadAnnotative\n1000\nAnnotativeData\n1002\n{\n0\nSTYLE\n2\nS\n70\n-1\n71\n99999\n0\nENDTAB\n0\nENDSEC\n0\nEOF\n".to_string(),
    ];
    for t in &cases {
        if let Ok(d) = read(t.as_bytes(), "x.dxf") {
            let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
            let _ = write_dxf(&d);
        }
    }
    // Huge but well-formed override lists are capped, not trusted.
    let mut big = String::from("0\nSECTION\n2\nENTITIES\n0\nDIMENSION\n70\n0\n1001\nACAD\n1000\nDSTYLE\n1002\n{\n");
    for _ in 0..50_000 {
        big.push_str("1070\n41\n1040\n0.5\n");
    }
    big.push_str("1002\n}\n0\nENDSEC\n0\nEOF\n");
    let d = read(big.as_bytes(), "x.dxf").unwrap();
    let dm = first(&d, |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    assert_eq!(dm.overrides.len(), 1);
}

/// Everything above in one drawing (also used for external validation).
fn extension_sample() -> Drawing {
    let mut d = sample();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), backwards: true, annotative: true, ..TextStyle::default() });
    d.dim_styles.push(full_dim_style());
    d.table_styles.push(TableStyle { name: "Schedule".into(), text_height: 0.25, margin: 0.1, title: true, header: true, ..TableStyle::default() });
    d.add(&Space::Model, Common::default(), EntityKind::Table(table_sample())).unwrap();
    let l1 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 30.0, 0.0), b: Vec3::new(0.0, 35.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(8.0, 30.0, 0.0), b: Vec3::new(8.0, 35.0, 0.0) })).unwrap();
    let mut dm = dim(DimKind::Linear { rotation: 0.0 }, Vec3::new(0.0, 30.0, 0.0), Vec3::new(8.0, 30.0, 0.0));
    dm.style = "Mech".into();
    dm.assoc = vec![
        DimAssoc { point: "p13".into(), handle: l1, snap: AssocSnap::Start },
        DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::Start },
    ];
    dm.overrides = serde_json::json!({"arrowBlock2": "_BoxFilled", "textColor": 2, "decimals": 1}).as_object().unwrap().clone();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    let ci = d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(20.0, 30.0, 0.0), radius: 2.0 })).unwrap();
    let mut al = dim(DimKind::Aligned, Vec3::new(22.0, 30.0, 0.0), Vec3::new(8.0, 35.0, 0.0));
    al.assoc = vec![
        DimAssoc { point: "p13".into(), handle: ci, snap: AssocSnap::OnCircle { angle: 0.0 } },
        DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::Intersection { other: l1 } },
    ];
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(al)).unwrap();
    parametric_sample(&mut d);
    d
}

#[test]
fn extension_sample_roundtrips_and_can_be_exported() {
    let d = extension_sample();
    let text = write_dxf(&d);
    if let Ok(path) = std::env::var("CADCRAFT_DXF_OUT") {
        std::fs::write(path, &text).unwrap();
    }
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.model.len(), d.model.len());
    assert_eq!(back.constraints, d.constraints);
    assert_eq!(back.dim_style("Mech"), d.dim_style("Mech"));
    let kinds = |d: &Drawing| d.model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>();
    assert_eq!(kinds(&back), kinds(&d));
}

/// DXF → DWG (acadrust) → DXF keeps what the DWG bridge supports: dimension styles with
/// arrow blocks, override and associativity xdata, the constraint XRECORD, tables and text
/// style flags. (TABLESTYLE objects do not survive the bridge.)
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_roundtrip_keeps_extension_data() {
    let d = extension_sample();
    let back = read(&write(&d, "x.dwg").unwrap(), "x.dwg").unwrap();
    // The bridge loses DIMFXL and DIMJOGANG (0 and its 5° minimum come back).
    let norm = |s: Option<&DimStyle>| s.cloned().map(|s| DimStyle { fixed_ext_length: 1.0, jog_angle: 45.0, ..s });
    assert_eq!(norm(back.dim_style("Mech")), norm(d.dim_style("Mech")));
    assert_eq!(back.constraints, d.constraints);
    assert_eq!(back.parametric, d.parametric);
    let dims =
        |d: &Drawing| d.model.iter().filter_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some(x.clone()) } else { None }).collect::<Vec<_>>();
    let (a, b) = (dims(&d), dims(&back));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.assoc, y.assoc);
        let st = DimStyle::default();
        assert_eq!(st.with_overrides(&x.overrides), st.with_overrides(&y.overrides));
    }
    assert_eq!(first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None }), table_sample());
    let r = back.text_style("Romans").unwrap();
    assert!(r.backwards && r.annotative);
}

#[test]
fn mleader_is_written_as_multileader() {
    let mut d = Drawing::new_metric();
    let text = cadcraft_doc::MText {
        insert: Vec3::new(12.0, 5.0, 0.0),
        height: 2.5,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        contents: "Note".into(),
        style: "Standard".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let m = cadcraft_doc::MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
        landing: Vec3::new(10.0, 5.0, 0.0),
        dogleg: 2.0,
        text: Some(text),
        style: "Standard".into(),
        arrow_size: 2.5,
        spline: false,
        block: None,
    };
    d.add(&Space::Model, Default::default(), EntityKind::MLeader(m.clone())).unwrap();
    // MULTILEADER is a 2007 entity (R2000/2004 files get LEADER + MTEXT).
    let back =
        crate::read(&crate::write_with(&d, "x.dxf", &crate::SaveOptions { version: Some(crate::Version::R2007), binary: false }).unwrap(), "x.dxf")
            .unwrap();
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::MLeader(x) if *x == m)));
    assert!(!back.model.iter().any(|e| matches!(e.kind, EntityKind::Leader(_) | EntityKind::MText(_))));
}

#[test]
fn attdef_prompt_survives_dxf_roundtrip() {
    let mut d = Drawing::new_metric();
    let text = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.0,
        value: "X".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    let attdef = |prompt: &str| {
        EntityKind::AttDef(Attrib {
            tag: "TAG1".into(),
            text: text.clone(),
            invisible: false,
            constant: false,
            prompt: prompt.into(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        })
    };
    d.add(&Space::Model, Default::default(), attdef("Enter value")).unwrap();
    d.add(&Space::Model, Default::default(), attdef("")).unwrap();
    let back = roundtrip(&d);
    let prompts: Vec<&str> =
        back.model.iter().filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.prompt.as_str()) } else { None }).collect();
    assert_eq!(prompts, vec!["Enter value", ""]);
}

#[test]
fn dimension_text_rotation_roundtrips() {
    let mut d = Drawing::new_imperial();
    let mut dm = dim(DimKind::Linear { rotation: 0.0 }, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
    dm.text_rotation = 30.0_f64.to_radians();
    let h = d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm.clone())).unwrap();
    let back = roundtrip(&d);
    let EntityKind::Dimension(x) = &back.entity(h).unwrap().kind else { panic!("dimension expected") };
    assert!((x.text_rotation - dm.text_rotation).abs() < 1e-9, "got {}", x.text_rotation);
    // The anonymous block that renders the reopened dimension carries the same text angle.
    let b = x.block.as_deref().and_then(|n| back.block(n)).expect("dimension block");
    let rot = b.entities.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.rotation) } else { None });
    assert!(rot.is_some_and(|r| (r - dm.text_rotation).abs() < 1e-9), "block text rotation {rot:?}");
}

/// Wipeout clip vertices are in image pixel space (origin top-left, y down): an asymmetric
/// boundary must keep its orientation, both through our reader and in the file itself.
#[test]
fn wipeout_boundary_keeps_its_orientation() {
    let mut d = Drawing::new_imperial();
    // A triangle with its apex at the top.
    let boundary = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 4.0)];
    d.add(&Space::Model, Common::default(), EntityKind::Wipeout(Wipeout { boundary: boundary.clone() })).unwrap();
    let text = write_dxf(&d);
    // The apex (0, 4) is at the top-left corner of the image: pixel (-0.5, -0.5).
    let pts: Vec<(f64, f64)> = {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let mut v = Vec::new();
        for i in 0..lines.len().saturating_sub(3) {
            if lines[i] == "14" && lines[i + 2] == "24" {
                v.push((lines[i + 1].parse().unwrap(), lines[i + 3].parse().unwrap()));
            }
        }
        v
    };
    assert!(pts.iter().any(|&(x, y)| (x + 0.5).abs() < 1e-9 && (y + 0.5).abs() < 1e-9), "{pts:?}");
    let back = roundtrip(&d);
    let got = first(&back, |k| if let EntityKind::Wipeout(w) = k { Some(w.boundary.clone()) } else { None });
    assert_eq!(&got[..3], &boundary[..]);
}

#[test]
fn arc_length_dimension_keeps_its_kind_and_centre_on_roundtrip() {
    let mut d = Drawing::new_metric();
    // Arc of radius 10 centred off the origin, so a missing centre would read back as zero.
    let mut al = dim(DimKind::ArcLength, Vec3::new(15.0, 5.0, 0.0), Vec3::new(5.0, 15.0, 0.0));
    al.p15 = Vec3::new(5.0, 5.0, 0.0);
    let h = d.add(&Space::Model, Default::default(), EntityKind::Dimension(al)).unwrap();
    let text = write_dxf(&d);
    // A standard aligned DIMENSION record (type 1 | 32), so other programs can read it.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.windows(2).any(|w| w == ["70", "33"]) && !lines.windows(2).any(|w| w == ["70", "40"]), "{text}");
    let back = read_dxf(text.as_bytes()).unwrap();
    match &back.entity(h).unwrap().kind {
        EntityKind::Dimension(x) => {
            assert!(matches!(x.kind, DimKind::ArcLength));
            assert_eq!(x.p15, Vec3::new(5.0, 5.0, 0.0));
        }
        _ => panic!("not a dimension"),
    }
    // And through DWG.
    let dwg = cadcraft_dwg::dxf_to_dwg(text.as_bytes()).unwrap();
    let back = read_dxf(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let dims: Vec<_> = back.model.iter().filter_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some(x.clone()) } else { None }).collect();
    assert_eq!(dims.len(), 1);
    assert!(matches!(dims[0].kind, DimKind::ArcLength), "{:?}", dims[0].kind);
    assert!((dims[0].p15 - Vec3::new(5.0, 5.0, 0.0)).len() < 1e-9);
}

/// Two gradient hatches (true colours, shifted, rotated; index colours, centered) and a plain
/// solid hatch over the same square.
fn gradient_sample() -> (Drawing, Vec<Option<Gradient>>) {
    let mut d = Drawing::new_metric();
    let square = vec![
        PolyVertex::new(Vec2::ZERO),
        PolyVertex::new(Vec2::new(10.0, 0.0)),
        PolyVertex::new(Vec2::new(10.0, 10.0)),
        PolyVertex::new(Vec2::new(0.0, 10.0)),
    ];
    let gradients = vec![
        Some(Gradient {
            name: "LINEAR".into(),
            color1: Color::True(cadcraft_color::Rgb(255, 0, 0)),
            color2: Color::True(cadcraft_color::Rgb(0, 0, 255)),
            angle: 30f64.to_radians(),
            centered: false,
        }),
        Some(Gradient { name: "CURVED".into(), color1: Color::Index(5), color2: Color::ByLayer, angle: 0.0, centered: true }),
        None,
    ];
    for g in &gradients {
        let h = Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops: vec![HatchLoop { vertices: square.clone(), outer: true, sources: Vec::new() }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: g.clone(),
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        };
        d.add(&Space::Model, Common::default(), EntityKind::Hatch(h)).unwrap();
    }
    (d, gradients)
}

fn hatches(d: &Drawing) -> Vec<Hatch> {
    d.model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).collect()
}

/// Issue #80: a DXF save/reopen keeps each hatch's gradient (name, both colours, angle and
/// centered setting) as native HATCH groups 450–470, and a plain solid stays plain.
#[test]
fn gradient_hatch_roundtrips_through_dxf() {
    let (d, gradients) = gradient_sample();
    let text = write_dxf(&d);
    for code in ["450", "451", "452", "453", "460", "461", "462", "463", "421", "470"] {
        assert!(text.lines().any(|l| l.trim() == code), "missing group {code}");
    }
    let back = read_dxf(text.as_bytes()).unwrap();
    let (a, b) = (hatches(&d), hatches(&back));
    assert_eq!(b.len(), gradients.len());
    for ((x, y), g) in a.iter().zip(&b).zip(&gradients) {
        assert_eq!(&y.gradient, g);
        assert_eq!(x.loops, y.loops);
    }
    assert_eq!(roundtrip(&back).model.iter().map(|e| &e.kind).collect::<Vec<_>>(), back.model.iter().map(|e| &e.kind).collect::<Vec<_>>());
}

/// R2000 readers and DWG conversions drop the native groups; CADCraft's xdata copy restores the
/// gradient. When native groups are present (another program may have edited them) they win.
#[test]
fn gradient_falls_back_to_xdata_and_native_groups_win() {
    let (d, gradients) = gradient_sample();
    let text = write_dxf(&d);
    // Strip groups 450–470 and 63/421 (pairs of lines) from every entity.
    let lines: Vec<&str> = text.lines().collect();
    let mut stripped = String::new();
    for pair in lines.chunks(2) {
        let code = pair.first().map(|c| c.trim().parse::<i32>().unwrap_or(-1)).unwrap_or(-1);
        if (450..=470).contains(&code) || code == 63 || code == 421 {
            continue;
        }
        for l in pair {
            stripped.push_str(l);
            stripped.push('\n');
        }
    }
    assert!(!stripped.lines().any(|l| l.trim() == "450"));
    let back = read_dxf(stripped.as_bytes()).unwrap();
    assert_eq!(hatches(&back).into_iter().map(|h| h.gradient).collect::<Vec<_>>(), gradients);

    // Native groups edited elsewhere (new angle, colour, name) take precedence over stale xdata.
    let edited = text.replacen("470\r\nLINEAR", "470\r\nSPHERICAL", 1).replacen("421\r\n16711680", "421\r\n65280", 1);
    assert_ne!(edited, text);
    let back = read_dxf(edited.as_bytes()).unwrap();
    let g = hatches(&back).into_iter().next().and_then(|h| h.gradient).unwrap();
    assert_eq!(g.name, "SPHERICAL");
    assert_eq!(g.color1, Color::True(cadcraft_color::Rgb(0, 255, 0)));
}

/// Issue #80: DWG saves (R2000, which has no native gradient fields) keep the gradient too.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gradient_hatch_roundtrips_through_dwg() {
    let (d, gradients) = gradient_sample();
    let back = read(&write(&d, "x.dwg").unwrap(), "x.dwg").unwrap();
    let hs = hatches(&back);
    assert_eq!(hs.iter().map(|h| h.gradient.clone()).collect::<Vec<_>>(), gradients);
    assert_eq!(hs.iter().map(|h| h.loops.clone()).collect::<Vec<_>>(), hatches(&d).into_iter().map(|h| h.loops).collect::<Vec<_>>());
}

#[test]
fn hostile_gradient_groups_never_panic() {
    let hatch = |tail: &str| {
        format!(
            "0\nSECTION\n2\nENTITIES\n0\nHATCH\n2\nSOLID\n70\n1\n91\n1\n92\n2\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n1\n20\n0\n10\n1\n20\n1\n98\n0\n{tail}0\nENDSEC\n0\nEOF\n"
        )
    };
    let cases = [
        // Native: absurd counts, colour records past two, out-of-range ACI/true colours, NaN.
        hatch(
            "450\n1\n453\n2147483647\n460\nnan\n461\ninf\n463\n0\n63\n99999\n421\n-1\n463\n1\n63\n-32768\n421\n99999999999\n463\n2\n63\n3\n463\n7\n470\n\n",
        ),
        // Native flag only, or a non-gradient flag, or groups out of order.
        hatch("450\n1\n"),
        hatch("450\n-7\n470\nX\n"),
        hatch("421\n255\n63\n1\n463\n0\n450\n1\n470\nLINEAR\n"),
        // Xdata: missing values, junk colour names, huge numbers.
        hatch("1001\nCADCRAFT\n1000\nGRADIENT\n"),
        hatch("1001\nCADCRAFT\n1000\nGRADIENT\n1000\nX\n1040\n1e308\n1070\n-1\n1000\nnot a colour\n1000\n999,999,999\n"),
        hatch("1001\nCADCRAFT\n1000\nGRADIENT\n1040\nnan\n1001\nACAD\n1000\n\n"),
    ];
    for t in &cases {
        let d = read(t.as_bytes(), "x.dxf").unwrap();
        for h in hatches(&d) {
            if let Some(g) = &h.gradient {
                assert!(g.angle.is_finite());
            }
        }
        let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
        let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
        assert_eq!(
            hatches(&back).into_iter().map(|h| h.gradient).collect::<Vec<_>>(),
            hatches(&d).into_iter().map(|h| h.gradient).collect::<Vec<_>>()
        );
    }
}

/// The native 470 group keeps a long name whole; the xdata copy is cut to 255 bytes at a
/// character boundary.
#[test]
fn long_gradient_name_is_cut_only_in_xdata() {
    let name = "Ä".repeat(200);
    let mut d = Drawing::new_metric();
    let mut h = hatches(&gradient_sample().0).into_iter().next().unwrap();
    if let Some(g) = &mut h.gradient {
        g.name = name.clone();
    }
    d.add(&Space::Model, Common::default(), EntityKind::Hatch(h)).unwrap();
    let text = write_dxf(&d);
    let lines: Vec<&str> = text.lines().collect();
    // Non-ASCII text is written as \U+ escapes (R2000 files are ANSI).
    assert!(lines.contains(&"\\U+00C4".repeat(200).as_str()));
    assert!(lines.contains(&"\\U+00C4".repeat(127).as_str()));
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(hatches(&back).into_iter().next().and_then(|h| h.gradient).unwrap().name, name);
}

#[test]
fn wipeout_survives_dxf_roundtrip() {
    let mut d = Drawing::new_imperial();
    let boundary = vec![Vec2::new(2.0, 3.0), Vec2::new(12.0, 3.0), Vec2::new(12.0, 13.0), Vec2::new(2.0, 13.0)];
    d.add(&Space::Model, Common::default(), EntityKind::Wipeout(Wipeout { boundary: boundary.clone() })).unwrap();
    let back = roundtrip(&d);
    let got = first(&back, |k| if let EntityKind::Wipeout(w) = k { Some(w.boundary.clone()) } else { None });
    // The DXF polygon is closed, so the first vertex is repeated at the end.
    assert_eq!(got.len(), 5);
    assert_eq!(&got[..4], &boundary[..]);
    assert_eq!(got[4], boundary[0]);
}

#[test]
fn autocad_header_dim_variables_are_not_overrides() {
    // AutoCAD writes the current style's values as $DIM* header variables; reading them back must
    // not turn them into style overrides for new dimensions (issue #66).
    use crate::dxf_ext::{K, dim_code};
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Romans".into(), font: "romans.shx".into(), ..TextStyle::default() });
    d.dim_styles.push(full_dim_style());
    d.header.set_str("DIMSTYLE", "Mech");
    d.header.set_f64("DIMSCALE", full_dim_style().scale);
    let st = serde_json::to_value(full_dim_style()).unwrap();
    let mut vars = String::new();
    for (var, field) in DIMVARS {
        let Some((_, kind)) = dim_code(field) else { continue };
        let v = &st[*field];
        let (code, val) = match kind {
            K::Real => (40, v.as_f64().unwrap().to_string()),
            K::Int => (70, v.as_i64().unwrap().to_string()),
            K::Bool => (70, i64::from(v.as_bool().unwrap()).to_string()),
            K::Color => (70, serde_json::from_value::<Color>(v.clone()).unwrap().to_aci().to_string()),
            K::Char => (70, u32::from(v.as_str().unwrap().chars().next().unwrap()).to_string()),
            K::Str => (1, v.as_str().unwrap().to_string()),
            // Names as AutoCAD may spell them.
            K::Block => (1, v.as_str().unwrap().to_ascii_uppercase()),
            K::TextStyle => (7, v.as_str().unwrap().to_ascii_uppercase()),
            K::Linetype => (6, v.as_str().unwrap().to_string()),
            K::Angle => (40, v.as_f64().unwrap().to_radians().to_string()),
        };
        vars.push_str(&format!("  9\r\n${var}\r\n{code}\r\n{val}\r\n"));
    }
    let text = write_dxf(&d).replacen("HEADER\r\n", &format!("HEADER\r\n{vars}"), 1);
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.header.str("DIMDSEP", ""), ",");
    assert!(back.header.get("DIMTXT").is_some() && back.header.get("DIMCLRD").is_some());
    assert_eq!(back.dim_overrides(), serde_json::Map::new());
    // A variable that really differs from the style is an override. CADCraft writes the current
    // style's variables in the header too (#1051), so both copies change.
    let text = text.replacen("$DIMDEC\r\n70\r\n3\r\n", "$DIMDEC\r\n70\r\n5\r\n", 1).replacen("$DIMDEC\r\n 70\r\n3\r\n", "$DIMDEC\r\n 70\r\n5\r\n", 1);
    assert_eq!(read_dxf(text.as_bytes()).unwrap().dim_overrides().get("decimals"), Some(&serde_json::json!(5)));
}

#[test]
fn mirrored_insert_extrusion_maps_to_negative_x_scale() {
    // A block reference mirrored in AutoCAD is stored with extrusion (0,0,-1); its insertion point,
    // scale and rotation are in that OCS, where x runs the other way: OCS (-10,5) is WCS (10,5).
    let text = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n2\n21\n0\n0\nENDBLK\n0\nENDSEC\n\
                0\nSECTION\n2\nENTITIES\n0\nINSERT\n8\n0\n2\nB\n10\n-10\n20\n5\n50\n90\n210\n0\n220\n0\n230\n-1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "m.dxf").unwrap();
    let ins = first(&d, |k| if let EntityKind::Insert(i) = k { Some(i.clone()) } else { None });
    assert!((ins.insert.x - 10.0).abs() < 1e-9 && (ins.insert.y - 5.0).abs() < 1e-9, "{:?}", ins.insert);
    assert_eq!((ins.scale.x, ins.scale.y), (-1.0, 1.0));
    assert!((ins.rotation + std::f64::consts::FRAC_PI_2).abs() < 1e-12, "{}", ins.rotation);
    // In the OCS the line (0,0)-(2,0) turned by 90° runs from (-10,5) to (-10,7); mirrored into the WCS
    // that is (10,5)-(10,7). The same follows from T(10,5)·R(-90°)·S(-1,1): (2,0) → (-2,0) → (0,2) → (10,7).
    let b = d.extents(&Space::Model);
    for (got, want) in [(b.min.x, 10.0), (b.min.y, 5.0), (b.max.x, 10.0), (b.max.y, 7.0)] {
        assert!((got - want).abs() < 1e-9, "extents {b:?}");
    }
    // Written back in the WCS (negative x scale, no extrusion), it reads back in the same place.
    let b2 = roundtrip(&d).extents(&Space::Model);
    assert!((b2.min - b.min).len() < 1e-9 && (b2.max - b.max).len() < 1e-9, "{b2:?} vs {b:?}");
}

#[test]
fn mirrored_ellipse_extrusion_reverses_parameters() {
    // Extrusion (0,0,-1) flips the minor axis (extrusion × major): parameters 0..90° of the ellipse
    // centred at the origin with major (2,0) and ratio 0.5 run from (2,0) to (0,-1), not (0,1).
    let text = "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n8\n0\n10\n0\n20\n0\n11\n2\n21\n0\n40\n0.5\n41\n0\n42\n1.5707963267948966\n\
                210\n0\n220\n0\n230\n-1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "e.dxf").unwrap();
    let b = d.extents(&Space::Model);
    for (got, want) in [(b.min.x, 0.0), (b.min.y, -1.0), (b.max.x, 2.0), (b.max.y, 0.0)] {
        assert!((got - want).abs() < 1e-6, "extents {b:?}");
    }
}

#[test]
fn mirrored_2d_polyline_extrusion_flips_x_and_bulge() {
    // R12-style 2D POLYLINE vertices are OCS like LWPOLYLINE: with extrusion (0,0,-1), OCS (1,0) is WCS
    // (-1,0) and a counter-clockwise bulge turns clockwise.
    let text = "0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n8\n0\n66\n1\n70\n0\n210\n0\n220\n0\n230\n-1\n\
                0\nVERTEX\n8\n0\n10\n1\n20\n0\n42\n1\n0\nVERTEX\n8\n0\n10\n3\n20\n0\n0\nSEQEND\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "p.dxf").unwrap();
    let p = first(&d, |k| if let EntityKind::LwPolyline(p) = k { Some(p.clone()) } else { None });
    let got: Vec<(f64, f64, f64)> = p.vertices.iter().map(|v| (v.p.x, v.p.y, v.bulge)).collect();
    assert_eq!(got, vec![(-1.0, 0.0, -1.0), (-3.0, 0.0, 0.0)]);
}

#[test]
fn mirrored_hatch_extrusion_flips_boundary() {
    // HATCH boundaries are OCS: the square (1,0)..(3,2) under extrusion (0,0,-1) lies at (-3,0)..(-1,2).
    let text = "0\nSECTION\n2\nENTITIES\n0\nHATCH\n8\n0\n10\n0\n20\n0\n30\n0\n210\n0\n220\n0\n230\n-1\n2\nSOLID\n70\n1\n71\n0\n91\n1\n\
                92\n2\n72\n0\n73\n1\n93\n4\n10\n1\n20\n0\n10\n3\n20\n0\n10\n3\n20\n2\n10\n1\n20\n2\n97\n0\n75\n0\n76\n1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "h.dxf").unwrap();
    let b = d.extents(&Space::Model);
    for (got, want) in [(b.min.x, -3.0), (b.min.y, 0.0), (b.max.x, -1.0), (b.max.y, 2.0)] {
        assert!((got - want).abs() < 1e-9, "extents {b:?}");
    }
}

/// The model extents of a one-loop solid HATCH whose edge boundary is `edges` (group 72 onwards).
fn hatch_edge_extents(edges: &str) -> Bounds2 {
    let n = edges.matches("72\n").count();
    let text = format!(
        "0\nSECTION\n2\nENTITIES\n0\nHATCH\n8\n0\n10\n0\n20\n0\n30\n0\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n\
         91\n1\n92\n1\n93\n{n}\n{edges}97\n0\n75\n0\n76\n1\n98\n0\n0\nENDSEC\n0\nEOF\n"
    );
    read(text.as_bytes(), "h.dxf").unwrap().extents(&Space::Model)
}

/// Issues #492, #493, #494: a full-circle arc edge, spline edges (knots come before the control
/// points) and clockwise ellipse edges all keep their curve.
#[test]
fn hatch_edges_keep_full_circles_splines_and_clockwise_ellipses() {
    let near = |b: Bounds2, want: [f64; 4]| {
        let got = [b.min.x, b.min.y, b.max.x, b.max.y];
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-3), "extents {got:?}, want {want:?}");
    };
    // #492: one arc edge from 0 to 360.
    near(hatch_edge_extents("72\n2\n10\n0\n20\n0\n40\n5\n50\n0\n51\n360\n73\n1\n"), [-5.0, -5.0, 5.0, 5.0]);
    // #493: a cubic Bezier through (0,0)..(4,0) peaking at y = 3, closed by a line.
    let spline = "72\n4\n94\n3\n73\n0\n74\n0\n95\n8\n96\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
                  10\n0\n20\n0\n10\n0\n20\n4\n10\n4\n20\n4\n10\n4\n20\n0\n97\n0\n";
    near(hatch_edge_extents(&format!("{spline}72\n1\n10\n4\n20\n0\n11\n0\n21\n0\n")), [0.0, 0.0, 4.0, 3.0]);
    // #494: the clockwise half-ellipse from 0 to 180 is the lower half, closed by its diameter.
    let ellipse = |ccw| format!("72\n3\n10\n0\n20\n0\n11\n5\n21\n0\n40\n0.5\n50\n0\n51\n180\n73\n{ccw}\n72\n1\n10\n-5\n20\n0\n11\n5\n21\n0\n");
    near(hatch_edge_extents(&ellipse(0)), [-5.0, -2.5, 5.0, 0.0]);
    near(hatch_edge_extents(&ellipse(1)), [-5.0, 0.0, 5.0, 2.5]);
}

/// Issue #923: a HATCH spline edge caps its degree at 10 like the SPLINE reader does, both when the
/// knots are valid for the stated degree and when the reader falls back to the control polygon.
#[test]
fn hatch_spline_edge_caps_degree_like_spline() {
    let control: Vec<Vec2> = (0..13).map(|i| Vec2::new(f64::from(i), f64::from(i % 2) * 10.0)).collect();
    let points: String = control.iter().map(|c| format!("10\n{}\n20\n{}\n", c.x, c.y)).collect();
    let close = "72\n1\n10\n12\n20\n0\n11\n0\n21\n0\n";
    // The degree-10 curve over the same controls, which is what the capped edge must produce.
    let want = cadcraft_geom::Spline::from_control(control, 10).tessellate(1e-3);
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for p in &want {
        (lo, hi) = (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)));
    }
    // Degree 12 with 13 controls: a valid knot vector has 26 knots; the second edge gives none.
    let knots = format!("{}{}", "40\n0\n".repeat(13), "40\n1\n".repeat(13));
    for (nk, knots) in [(26, knots.as_str()), (0, "")] {
        let edge = format!("72\n4\n94\n12\n73\n0\n74\n0\n95\n{nk}\n96\n13\n{knots}{points}97\n0\n{close}");
        let b = hatch_edge_extents(&edge);
        for (got, want) in [(b.min.x, lo.x), (b.min.y, lo.y), (b.max.x, hi.x), (b.max.y, hi.y)] {
            assert!((got - want).abs() < 1e-6, "extents {b:?}, want {lo:?} {hi:?} (knots {nk})");
        }
    }
}

#[test]
fn mirrored_text_extrusion_keeps_its_footprint() {
    // TEXT at OCS (-10,5) under extrusion (0,0,-1) starts at WCS (10,5) and runs towards -x (with
    // mirrored glyphs). Drawn readable, it becomes right-justified at (10,5) with the same footprint.
    let text = "0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n10\n-10\n20\n5\n40\n1\n1\nAB\n210\n0\n220\n0\n230\n-1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "t.dxf").unwrap();
    let t = first(&d, |k| if let EntityKind::Text(t) = k { Some(t.clone()) } else { None });
    assert_eq!(t.halign, HAlign::Right);
    assert!(t.rotation.abs() < 1e-12, "{}", t.rotation);
    let a = t.align_pt.expect("right-justified text has an alignment point");
    assert!((a.x - 10.0).abs() < 1e-9 && (a.y - 5.0).abs() < 1e-9, "{a:?}");
    let list = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
    assert!((list.bounds.max.x - 10.0).abs() < 1e-6 && list.bounds.min.x < 9.5, "drawn left of (10,5): {:?}", list.bounds);
}

#[test]
fn mirrored_mtext_extrusion_keeps_its_footprint() {
    // MTEXT at (0,0) with direction (1,0), top-left attachment and extrusion (0,0,-1): the line height
    // runs along extrusion × direction = (0,-1), so the text box lies above the insertion point
    // (glyphs upside down). Drawn readable: turned round, top-right attachment, same box.
    let text =
        "0\nSECTION\n2\nENTITIES\n0\nMTEXT\n8\n0\n10\n0\n20\n0\n40\n1\n71\n1\n1\nAB\n11\n1\n21\n0\n210\n0\n220\n0\n230\n-1\n0\nENDSEC\n0\nEOF\n";
    let d = read(text.as_bytes(), "mt.dxf").unwrap();
    let t = first(&d, |k| if let EntityKind::MText(t) = k { Some(t.clone()) } else { None });
    assert_eq!(t.attach, 3);
    assert!((t.rotation - std::f64::consts::PI).abs() < 1e-12, "{}", t.rotation);
    let b = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default()).bounds;
    assert!(b.min.x > -0.2 && b.max.x > 0.5 && b.min.y > -0.2 && b.max.y > 0.5, "box right of and above (0,0): {b:?}");
}

// --- SVG import ---

/// One 20mm×10mm page whose user units are millimetres; the reader flips Y, so a point at
/// SVG y lands at `10 - y` in the drawing.
const SVG_PAGE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10">"#;

fn kinds(d: &Drawing) -> Vec<&'static str> {
    d.model.iter().map(|e| e.kind.type_name()).collect()
}

fn gzip_of(data: &[u8]) -> Vec<u8> {
    let mut crc: u32 = 0xffff_ffff;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & 0u32.wrapping_sub(crc & 1));
        }
    }
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 6));
    out.extend_from_slice(&(!crc).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

#[test]
fn svg_shapes_become_entities_in_millimetres() {
    let svg = format!(
        "{SVG_PAGE}<rect x=\"1\" y=\"1\" width=\"4\" height=\"3\"/><circle cx=\"10\" cy=\"5\" r=\"2\"/>\
         <ellipse cx=\"10\" cy=\"5\" rx=\"4\" ry=\"2\"/><line x1=\"0\" y1=\"0\" x2=\"20\" y2=\"10\"/>\
         <polyline points=\"0,0 5,0 5,5\"/><polygon points=\"0,0 2,0 1,1\"/></svg>"
    );
    let d = read(svg.as_bytes(), "shapes.svg").unwrap();
    assert_eq!(kinds(&d), ["Polyline", "Circle", "Ellipse", "Line", "Polyline", "Polyline"]);
    let r = first(&d, |k| if let EntityKind::LwPolyline(p) = k { Some(p.clone()) } else { None });
    assert!(r.closed);
    let got: Vec<Vec2> = r.vertices.iter().map(|v| v.p).collect();
    let want = [Vec2::new(1.0, 9.0), Vec2::new(5.0, 9.0), Vec2::new(5.0, 6.0), Vec2::new(1.0, 6.0)];
    for (g, w) in got.iter().zip(want.iter()) {
        assert!((*g - *w).len() < 1e-9, "{g:?} vs {w:?}");
    }
    let c = first(&d, |k| if let EntityKind::Circle(c) = k { Some(c.clone()) } else { None });
    assert!((c.radius - 2.0).abs() < 1e-9 && (c.center.x - 10.0).abs() < 1e-9 && (c.center.y - 5.0).abs() < 1e-9, "{c:?}");
    let e = first(&d, |k| if let EntityKind::Ellipse(e) = k { Some(e.clone()) } else { None });
    let major = Vec2::new(e.major.x - e.center.x, e.major.y - e.center.y);
    assert!((major.len() - 4.0).abs() < 1e-9 && (e.ratio - 0.5).abs() < 1e-9, "{e:?}");
    let l = first(&d, |k| if let EntityKind::Line(l) = k { Some(l.clone()) } else { None });
    assert!((l.a.y - 10.0).abs() < 1e-9 && (l.b.y).abs() < 1e-9, "{l:?}");
}

#[test]
fn svg_path_lines_curves_and_arcs() {
    let svg = format!("{SVG_PAGE}<path d=\"M1 1 L5 1 C6 1 7 2 8 3\"/><path d=\"M0 0 A5 5 0 0 1 10 0 Z\"/></svg>");
    let d = read(svg.as_bytes(), "paths.svg").unwrap();
    assert_eq!(kinds(&d), ["Polyline", "Spline", "Polyline"]);
    let line_run = first(&d, |k| if let EntityKind::LwPolyline(p) = k { Some(p.clone()) } else { None });
    assert!(!line_run.closed && line_run.vertices.len() == 2, "{line_run:?}");
    let spline = first(&d, |k| if let EntityKind::Spline(s) = k { Some(s.clone()) } else { None });
    assert!(spline.is_valid(), "{spline:?}");
    let arc = d
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::LwPolyline(p) if p.vertices.first().is_some_and(|v| v.bulge != 0.0) => Some(p.clone()),
            _ => None,
        })
        .expect("an arc becomes a bulge vertex");
    assert!(arc.closed && arc.vertices.len() == 2, "{arc:?}");
    let bulge = arc.vertices[0].bulge;
    assert!((bulge.abs() - 1.0).abs() < 1e-6, "semicircle bulge is ±1, got {bulge}");
}

#[test]
fn svg_group_transforms_apply() {
    let svg = format!("{SVG_PAGE}<g transform=\"translate(5,2)\"><line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\"/></g></svg>");
    let d = read(svg.as_bytes(), "t.svg").unwrap();
    let l = first(&d, |k| if let EntityKind::Line(l) = k { Some(l.clone()) } else { None });
    assert!((l.a - Vec3::new(5.0, 8.0, 0.0)).len() < 1e-9, "{l:?}");
    assert!((l.b - Vec3::new(15.0, 8.0, 0.0)).len() < 1e-9, "{l:?}");
}

#[test]
fn svg_viewbox_scaling_and_mm_units() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 200 100"><circle cx="100" cy="50" r="50"/></svg>"#;
    let d = read(svg.as_bytes(), "vb.svg").unwrap();
    let c = first(&d, |k| if let EntityKind::Circle(c) = k { Some(c.clone()) } else { None });
    assert!((c.radius - 5.0).abs() < 1e-9, "{c:?}");
    assert!((c.center.x - 10.0).abs() < 1e-9 && (c.center.y - 5.0).abs() < 1e-9, "{c:?}");
}

#[test]
fn svg_viewbox_with_px_or_unitless_root_size_uses_reference_pixel() {
    for w in ["800", "800px"] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="600" viewBox="0 0 400 300"><line x1="0" y1="0" x2="400" y2="0"/></svg>"#
        );
        let d = read(svg.as_bytes(), "vb.svg").unwrap();
        let l = first(&d, |k| if let EntityKind::Line(l) = k { Some(l.clone()) } else { None });
        assert!((l.b.x - 800.0 * 25.4 / 96.0).abs() < 1e-9, "{w}: {l:?}");
    }
}

#[test]
fn svg_inkscape_layers_are_kept() {
    let svg = format!(
        "{SVG_PAGE}<g inkscape:groupmode=\"layer\" inkscape:label=\"Cut\"><rect x=\"0\" y=\"0\" width=\"2\" height=\"2\"/></g>\
         <g inkscape:groupmode=\"layer\" inkscape:label=\"Engrave\"><circle cx=\"5\" cy=\"5\" r=\"1\"/></g></svg>",
        SVG_PAGE = SVG_PAGE.replace("<svg ", "<svg xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" ")
    );
    let d = read(svg.as_bytes(), "layers.svg").unwrap();
    let layers: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(layers.contains(&"Cut") && layers.contains(&"Engrave"), "{layers:?}");
    let per_layer = |name: &str| d.model.iter().filter(|e| e.common.layer == name).count();
    assert_eq!(per_layer("Cut"), 1);
    assert_eq!(per_layer("Engrave"), 1);
}

#[test]
fn svg_hidden_and_defs_subtrees_are_skipped() {
    let svg = format!(
        "{SVG_PAGE}<defs><rect x=\"0\" y=\"0\" width=\"9\" height=\"9\"/></defs><text x=\"0\" y=\"0\">hi</text>\
         <rect x=\"0\" y=\"0\" width=\"1\" height=\"1\" display=\"none\"/><rect x=\"0\" y=\"0\" width=\"1\" height=\"1\" style=\"fill:red;display:none\"/>\
         <rect x=\"1\" y=\"1\" width=\"2\" height=\"2\"/></svg>"
    );
    let d = read(svg.as_bytes(), "skip.svg").unwrap();
    assert_eq!(kinds(&d), ["Polyline"]);
}

/// #627: what the reader doesn't read yet (text, raster images, `<use>` copies, style sheets,
/// rounded corners) is named in a read note instead of being left out silently; what isn't drawn
/// (in `<defs>` or hidden) isn't counted.
#[test]
fn svg_left_out_content_is_noted() {
    let svg = format!(
        "{SVG_PAGE}<defs><style>.a {{ stroke: red }}</style><text>in defs</text><rect id=\"r\" width=\"1\" height=\"1\"/></defs>\
         <text x=\"0\" y=\"5\">one</text><text x=\"0\" y=\"8\"><tspan>two</tspan></text><text display=\"none\">hidden</text>\
         <image href=\"a.png\" width=\"2\" height=\"2\"/><use href=\"#r\"/><use href=\"#r\" x=\"3\"></use>\
         <rect x=\"1\" y=\"1\" width=\"4\" height=\"3\" rx=\"1\"/><rect x=\"6\" y=\"1\" width=\"4\" height=\"3\"/></svg>"
    );
    let d = read(svg.as_bytes(), "left-out.svg").unwrap();
    assert_eq!(kinds(&d), ["Polyline", "Polyline"]);
    assert_eq!(
        d.read_notes,
        ["SVG: CADCraft doesn't read these yet, so they were left out: 2 texts, 1 raster image, 2 `<use>` copies, \
          1 `<style>` sheet (its class rules aren't applied), 1 rectangle with rounded corners (drawn square)"]
    );
    let plain = read(format!("{SVG_PAGE}<rect x=\"1\" y=\"1\" width=\"4\" height=\"3\"/></svg>").as_bytes(), "a.svg").unwrap();
    assert!(plain.read_notes.is_empty(), "{:?}", plain.read_notes);
}

#[test]
fn svgz_gunzips_to_the_same_drawing() {
    let svg = format!("{SVG_PAGE}<rect x=\"1\" y=\"1\" width=\"4\" height=\"3\"/></svg>");
    let plain = read(svg.as_bytes(), "a.svg").unwrap();
    let packed = read(&gzip_of(svg.as_bytes()), "a.svgz").unwrap();
    assert_eq!(kinds(&plain), kinds(&packed));
    assert_eq!(plain.model.iter().map(|e| &e.kind).collect::<Vec<_>>(), packed.model.iter().map(|e| &e.kind).collect::<Vec<_>>());
}

#[test]
fn svg_read_dispatches_by_extension() {
    let svg = format!("{SVG_PAGE}<circle cx=\"5\" cy=\"5\" r=\"1\"/></svg>");
    assert!(read(svg.as_bytes(), "a.svg").is_ok());
    assert!(read(&gzip_of(svg.as_bytes()), "a.svgz").is_ok());
}

#[test]
fn svg_hostile_input_errors_without_panicking() {
    let deep = format!("{SVG_PAGE}{}<line x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"/>{}</svg>", "<g>".repeat(200), "</g>".repeat(200));
    for (bytes, name) in [
        (b"not xml at all".to_vec(), "a.svg"),
        (Vec::new(), "a.svg"),
        (b"<svg><rect x=\"1e309\" width=\"NaN\" height=\"inf\"/></svg>".to_vec(), "a.svg"),
        (deep.clone().into_bytes(), "a.svg"),
        (b"<svg><path d=\"M0 0 X5 5\"/></svg>".to_vec(), "a.svg"),
        (b"<svg><path d=\"M0 0 L\"/></svg>".to_vec(), "a.svg"),
        (b"<svg><path d=\"M0 0 A1 1 0 5 1 2 2\"/></svg>".to_vec(), "a.svg"),
        (b"<svg><polyline points=\"1 2 3\"/></svg>".to_vec(), "a.svg"),
        (b"<svg><g transform=\"bogus(1)\"><rect width=\"1\" height=\"1\"/></g></svg>".to_vec(), "a.svg"),
        (vec![0x1f, 0x8b, 8, 0, b'j', b'u', b'n', b'k', 0, 0, 0, 0, 0, 0, 0, 0], "a.svgz"),
        (vec![0xff, 0xfe, 0x00, 0x01, 0x02, 0x03], "a.svg"),
    ] {
        let _ = read(&bytes, name);
    }
    let deep = read(deep.as_bytes(), "a.svg");
    assert!(deep.is_err(), "200 nested groups are refused, not walked");
}

#[test]
fn svg_no_geometry_yields_an_empty_drawing() {
    let svg = format!("{SVG_PAGE}</svg>");
    let d = read(svg.as_bytes(), "empty.svg").unwrap();
    assert!(kinds(&d).is_empty());
}

/// An application data group inside a kept XRECORD — the AcDs thumbnail schema's
/// `{ATTRRECORD 341 h 2 name 280 1 291 1 ATTRRECORD}` — is written back as read. It used to be
/// collapsed into a handle list: the handle came out under group 291 (a bool) and the name and
/// flags were dropped, which strict DXF readers (ezdxf) reject outright.
#[test]
fn kept_xrecord_application_group_roundtrips_as_read() {
    let dxf = "0\nSECTION\n2\nOBJECTS\n\
0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nTD_SCHEMA\n350\n20\n\
0\nXRECORD\n5\n20\n330\nC\n100\nAcDbXrecord\n280\n1\n1\nAcDb_Thumbnail_Schema\n\
102\n{ATTRRECORD\n341\n21\n2\nAcDbDs::TreatedAsObjectData\n280\n1\n291\n1\n102\nATTRRECORD}\n\
0\nXRECORD\n5\n21\n330\n20\n100\nAcDbXrecord\n280\n1\n1\nTarget\n\
0\nENDSEC\n0\nEOF\n";
    let d = read_dxf(dxf.as_bytes()).expect("read");
    let out = write_dxf(&d);
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let at = lines.iter().position(|l| *l == "{ATTRRECORD").expect("group kept");
    // The group keeps its own shape: a reference under 341, then the name, flag and bool.
    assert_eq!(lines[at + 1], "341", "reference group code: {:?}", &lines[at..at + 12]);
    assert!(lines[at + 2] != "0" && !lines[at + 2].is_empty());
    assert_eq!(&lines[at + 3..at + 5], &["2", "AcDbDs::TreatedAsObjectData"]);
    assert_eq!(&lines[at + 5..at + 7], &["280", "1"]);
    assert_eq!(&lines[at + 7..at + 9], &["291", "1"]);
    assert_eq!(&lines[at + 9..at + 11], &["102", "ATTRRECORD}"]);
    // And no handle is written under a bool code anywhere.
    assert!(!out.contains("291\n2"), "no handle may be written under a bool code");
}

// ---------- plot transparency ----------

/// A black line under a red solid at `percent` transparency (drawn after it, so on top).
fn transparent_over_line(percent: u8) -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(-10.0, 30.0, 0.0), b: Vec3::new(110.0, 30.0, 0.0) })).unwrap();
    let corners = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, 60.0, 0.0), Vec3::new(100.0, 60.0, 0.0)];
    let red = Common { color: Color::Index(1), transparency: Transparency::Percent(percent), ..Common::default() };
    d.add(&Space::Model, red, EntityKind::Solid(Solid { corners })).unwrap();
    d
}

#[test]
fn pdf_plots_transparency_only_when_asked() {
    let d = transparent_over_line(50);
    let opts = |extra: serde_json::Value| {
        let mut o = serde_json::json!({"paper": "A4", "compress": false});
        if let (Some(o), Some(e)) = (o.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        o
    };
    let on = plot(&d, &Space::Model, &opts(serde_json::json!({"plotTransparency": true}))).unwrap();
    let content = check_pdf(&on);
    // One graphics state: fill alpha 0.5 (strokes stay opaque).
    assert_eq!(pdf_read::read(&on).pages[0].alphas, [(1.0, 0.5)]);
    // The line is opaque, the solid at half alpha.
    let ops = pdf_read::ops(&content);
    assert!(ops.windows(2).any(|w| w[0].op == "gs" && w[1].op == "rg" && w[1].nums == [1.0, 0.0, 0.0]), "{content}");
    assert_eq!(pdf_read::count(&content, "gs"), 1, "{content}");
    // Off (the default, and explicitly): no graphics states, and the same bytes either way.
    let off = plot(&d, &Space::Model, &opts(serde_json::json!({}))).unwrap();
    assert_eq!(off, plot(&d, &Space::Model, &opts(serde_json::json!({"plotTransparency": false}))).unwrap());
    assert!(pdf_read::read(&off).pages[0].alphas.is_empty() && pdf_read::count(&check_pdf(&off), "gs") == 0);
    // On, but nothing transparent: the same bytes as off.
    let opaque = transparent_over_line(0);
    assert_eq!(
        plot(&opaque, &Space::Model, &opts(serde_json::json!({"plotTransparency": true}))).unwrap(),
        plot(&opaque, &Space::Model, &opts(serde_json::json!({}))).unwrap()
    );
    // A hostile flag is ignored (off).
    assert_eq!(plot(&d, &Space::Model, &opts(serde_json::json!({"plotTransparency": "yes"}))).unwrap(), off);
}

#[test]
fn layout_page_setup_plot_transparency_is_used() {
    let mut d = transparent_over_line(90);
    let paper = Space::Paper(d.layouts[0].name.clone());
    let corners = [Vec3::new(1.0, 1.0, 0.0), Vec3::new(5.0, 1.0, 0.0), Vec3::new(1.0, 4.0, 0.0), Vec3::new(5.0, 4.0, 0.0)];
    let faint = Common { color: Color::Index(5), transparency: Transparency::Percent(90), ..Common::default() };
    d.add(&paper, faint, EntityKind::Solid(Solid { corners })).unwrap();
    let raw = serde_json::json!({"compress": false});
    let alphas = |d: &Drawing, o: &serde_json::Value| pdf_read::read(&plot(d, &paper, o).unwrap()).pages[0].alphas.clone();
    assert!(alphas(&d, &raw).is_empty());
    d.layouts[0].page.plot_transparency = true;
    let on = alphas(&d, &raw);
    assert!(on.iter().any(|(_, f)| (f - 0.1).abs() < 1e-6), "the faint solid fills at alpha 0.1: {on:?}");
    // The plot's own option wins over the page setup.
    assert!(alphas(&d, &serde_json::json!({"compress": false, "plotTransparency": false})).is_empty());
}

#[test]
fn png_and_svg_plots_blend_transparency_when_asked() {
    let d = transparent_over_line(50);
    let png = |on: Option<bool>| {
        let mut o = serde_json::json!({"output": "png", "resolution": 50, "width": 210, "height": 297, "landscape": true});
        if let (Some(on), Some(o)) = (on, o.as_object_mut()) {
            o.insert("plotTransparency".into(), serde_json::json!(on));
        }
        plotter::plot(&d, &Space::Model, &o).unwrap()
    };
    let decode = |bytes: &[u8]| tiny_skia::Pixmap::decode_png(bytes).unwrap();
    // The centre of the sheet is inside the solid (the drawing is fitted to the paper).
    let centre = |pm: &tiny_skia::Pixmap| {
        let c = pm.pixel(pm.width() / 2, pm.height() / 2 + 20).unwrap();
        (c.red(), c.green(), c.blue())
    };
    assert_eq!(centre(&decode(&png(None))), (255, 0, 0), "off: opaque red");
    assert_eq!(png(None), png(Some(false)));
    let (r, g, b) = centre(&decode(&png(Some(true))));
    assert!(r == 255 && g.abs_diff(127) <= 2 && b.abs_diff(127) <= 2, "half red on white paper: {:?}", (r, g, b));
    let svg = |on: bool| {
        let o = serde_json::json!({"output": "svg", "plotTransparency": on});
        String::from_utf8(plotter::plot(&d, &Space::Model, &o).unwrap()).unwrap()
    };
    assert!(svg(true).contains(r##"fill="#ff0000" opacity="0.50""##), "{}", svg(true));
    assert!(!svg(false).contains("opacity"));
}

#[test]
fn view_exports_draw_transparency() {
    let d = transparent_over_line(50);
    let svg = String::from_utf8(write(&d, "a.svg").unwrap()).unwrap();
    assert!(svg.contains(r#"opacity="0.50""#));
    assert_ne!(write(&d, "a.png").unwrap(), write(&transparent_over_line(0), "a.png").unwrap());
}

#[test]
fn pdf_strokes_overlapping_transparent_objects_one_by_one() {
    // Two crossing lines, ByLayer on layer `layer_t` percent transparent.
    let crossing = |layer_t: u8| {
        let mut d = Drawing::new_metric();
        d.layers.push(Layer { name: "Glass".into(), color: Color::Index(5), transparency: layer_t, ..Layer::default() });
        let on = || Common { layer: "Glass".into(), ..Common::default() };
        d.add(&Space::Model, on(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 100.0, 0.0) })).unwrap();
        d.add(&Space::Model, on(), EntityKind::Line(Line { a: Vec3::new(0.0, 100.0, 0.0), b: Vec3::new(100.0, 0.0, 0.0) })).unwrap();
        d
    };
    let content = |d: &Drawing, on: bool| {
        check_pdf(&plot(d, &Space::Model, &serde_json::json!({"paper": "A4", "compress": false, "plotTransparency": on})).unwrap())
    };
    // 50 %: each line is its own stroke, so they build up where they cross.
    let glass = content(&crossing(50), true);
    assert!(pdf_read::count(&glass, "gs") > 0, "{glass}");
    let page =
        pdf_read::read(&plot(&crossing(50), &Space::Model, &serde_json::json!({"paper": "A4", "plotTransparency": true})).unwrap()).pages.remove(0);
    assert!(page.alphas.contains(&(0.5, 1.0)), "strokes at half alpha: {:?}", page.alphas);
    assert_eq!(pdf_read::count(&glass, "S"), 2, "{glass}");
    // Opaque: one path as before, whether the option is on or off.
    let opaque = content(&crossing(0), true);
    assert_eq!(pdf_read::count(&opaque, "S"), 1, "{opaque}");
    assert_eq!(opaque, content(&crossing(0), false));
    // Transparency off: the transparent layer plots as before too.
    assert_eq!(content(&crossing(50), false), opaque);
}

#[test]
fn pdf_strokes_each_transparent_primitive_inside_a_block() {
    // One block reference whose two children cross: they share the reference's handle.
    let crossing_block = |t: u8| {
        let mut d = Drawing::new_metric();
        let mut blk = Block::new("X");
        for (i, (a, b)) in [((0.0, 0.0), (100.0, 100.0)), ((0.0, 100.0), (100.0, 0.0))].into_iter().enumerate() {
            let line = Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) };
            let mut e = Entity::new(Handle(i as u64 + 1), EntityKind::Line(line));
            e.common = Common { color: Color::Index(1), transparency: Transparency::ByBlock, ..Common::default() };
            blk.entities.push(e);
        }
        d.blocks.insert("X".into(), std::sync::Arc::new(blk));
        let ins = Insert {
            block: "X".into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        };
        d.add(&Space::Model, Common { transparency: Transparency::Percent(t), ..Common::default() }, EntityKind::Insert(ins)).unwrap();
        d
    };
    let content = |d: &Drawing, on: bool| {
        check_pdf(&plot(d, &Space::Model, &serde_json::json!({"paper": "A4", "compress": false, "plotTransparency": on})).unwrap())
    };
    let half = content(&crossing_block(50), true);
    assert!(pdf_read::count(&half, "gs") > 0, "{half}");
    let page = pdf_read::read(&plot(&crossing_block(50), &Space::Model, &serde_json::json!({"paper": "A4", "plotTransparency": true})).unwrap())
        .pages
        .remove(0);
    assert!(page.alphas.contains(&(0.5, 1.0)), "strokes at half alpha: {:?}", page.alphas);
    assert_eq!(pdf_read::count(&half, "S"), 2, "{half}");
    // Opaque: one path, the same with the option on or off.
    let opaque = content(&crossing_block(0), true);
    assert_eq!(pdf_read::count(&opaque, "S"), 1, "{opaque}");
    assert_eq!(opaque, content(&crossing_block(0), false));
    assert_eq!(content(&crossing_block(50), false), opaque);
}

#[test]
fn foreign_table_reads_cell_text_height_and_keeps_style_scale() {
    // A table from another writer: no CADCraft xdata; cell 1 sets 140 before its value block
    // (the cell's text height), cell 2 only has a 140 inside the value block (a number).
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nACAD_TABLE\n100\nAcDbTable\n91\n1\n92\n2\n141\n300\n142\n900\n142\n900\n\
        171\n1\n140\n150\n301\nCELL_VALUE\n1\nA\n304\nACVALUE_END\n\
        171\n1\n301\nCELL_VALUE\n140\n42\n1\nB\n304\nACVALUE_END\n0\nENDSEC\n0\nEOF\n";
    let d = read_dxf(dxf.as_bytes()).unwrap();
    let t = first(&d, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None });
    assert_eq!(t.cells[0][0].text_height, Some(150.0));
    assert_eq!(t.cells[0][1].text_height, None);
    // No scale of its own: the table takes its style's height.
    assert_eq!(t.text_height, TableStyle::default().text_height);
    // Written back and read again.
    let back = roundtrip(&d);
    let t2 = first(&back, |k| if let EntityKind::Table(x) = k { Some(x.clone()) } else { None });
    assert_eq!(t2.cells[0][0].text_height, Some(150.0));
    assert_eq!(t2.cells[0][1].text_height, None);
}

/// Issue #1209: a file that declares no units ($INSUNITS absent or 0) stays unitless when written
/// again; only a new drawing takes its template's units.
#[test]
fn drawing_without_insunits_stays_unitless() {
    let r12 = |extra: &str| {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n{extra}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n"
        )
    };
    let insunits = |text: &str| {
        text.lines().map(str::trim).collect::<Vec<_>>().windows(3).find(|w| w[0] == "$INSUNITS" && w[1] == "70").map(|w| w[2].to_string())
    };
    let rewritten = |extra: &str| insunits(&write_dxf(&read_dxf(r12(extra).as_bytes()).unwrap()));
    assert_eq!(rewritten(""), Some("0".into()), "absent");
    assert_eq!(rewritten("9\n$INSUNITS\n70\n0\n"), Some("0".into()), "declared unitless");
    assert_eq!(rewritten("9\n$INSUNITS\n70\n6\n"), Some("6".into()), "declared metres");
    assert_eq!(insunits(&write_dxf(&Drawing::new_metric())), Some("4".into()));
    assert_eq!(insunits(&write_dxf(&Drawing::new_imperial())), Some("1".into()));
}

#[test]
fn pdf_clips_a_primitive_beyond_the_coordinate_range() {
    let mut d = Drawing::new_metric();
    let paper = Space::Paper("Layout1".into());
    let v = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    // 4e7 mm at 1:1 is about 1.1e8 pt, past the range the writer's f32 geometry takes.
    d.add(&paper, Common::default(), EntityKind::Line(Line { a: v(10.0, 20.0), b: v(4.0e7, 20.0) })).unwrap();
    d.add(&paper, Common::default(), EntityKind::Solid(Solid { corners: [v(10.0, 50.0), v(4.0e7, 50.0), v(10.0, 60.0), v(10.0, 60.0)] })).unwrap();
    let bytes = plot(&d, &paper, &serde_json::json!({"fit": false, "scale": 1.0, "compress": false})).unwrap();
    let content = check_pdf(&bytes);
    let clip = pdf_read::clip_rect(&content).unwrap();
    // The x extent of what each paint operator paints.
    let (mut xs, mut painted) = (Vec::new(), Vec::new());
    for o in pdf_read::drawn(&content) {
        match o.op.as_str() {
            "m" | "l" => xs.push(o.nums[0]),
            "S" | "f" => painted.push((o.op, xs.drain(..).fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x))))),
            _ => {}
        }
    }
    let (stroke, fill) = (painted.iter().find(|p| p.0 == "S"), painted.iter().find(|p| p.0 == "f"));
    // Both run from inside the plot area to its right edge: the stroke to just past it.
    let (_, (lo, hi)) = stroke.expect("the line is drawn");
    assert!(*lo < clip[2] && *hi > clip[2] && *hi < clip[2] + 5.0, "line from {lo} to {hi}, clip {clip:?}");
    let (_, (lo, hi)) = fill.expect("the solid is filled");
    assert!(*lo < clip[2] && (*hi - clip[2]).abs() < 1e-3, "fill from {lo} to {hi}, clip {clip:?}");
}

#[test]
fn pdf_plots_a_zero_length_line_as_a_dot() {
    let mut d = Drawing::new_metric();
    let red = |lw| Common { color: Color::Index(1), lineweight: lw, ..Common::default() };
    // Points at 1:1 on A4, so the dot sits away from the clip edges.
    let at = Vec3::new(50.0, 50.0, 0.0);
    d.add(&Space::Model, red(Lineweight::Mm100(100)), EntityKind::Line(Line { a: at, b: at })).unwrap();
    // The x extent of what the content fills in red: the dot's diameter.
    let red_fill_width = |o: serde_json::Value| {
        let content = check_pdf(&plot(&d, &Space::Model, &o).unwrap());
        let ops = pdf_read::ops(&content);
        let start = ops.iter().position(|o| o.op == "rg" && o.nums == [1.0, 0.0, 0.0]).expect("a red fill");
        let end = start + ops[start..].iter().position(|o| o.op == "f").expect("filled");
        assert!(ops[start..end].iter().filter(|o| o.op == "c").count() == 4, "{content}");
        let xs: Vec<f64> =
            ops[start..end].iter().filter(|o| matches!(o.op.as_str(), "m" | "c")).flat_map(|o| o.nums.iter().step_by(2).copied()).collect();
        xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min)
    };
    let opts = |lineweights: bool| serde_json::json!({"paper": "A4", "fit": false, "scale": 1.0, "compress": false, "lineweights": lineweights});
    // 1 mm lineweight: a dot 1 mm across.
    let w = red_fill_width(opts(true));
    assert!((w - 72.0 / 25.4).abs() < 0.01, "{w}");
    // A hairline still leaves a mark.
    let w = red_fill_width(opts(false));
    assert!((w - 0.25).abs() < 0.01, "{w}");
}

/// DXF text as (code, value) pairs.
fn dxf_pairs(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].trim().to_string(), c[1].trim().to_string())).collect()
}

/// The records of type `kind` in DXF pairs, each as its groups after the type.
fn dxf_records<'a>(pairs: &'a [(String, String)], kind: &str) -> Vec<&'a [(String, String)]> {
    let mut out = Vec::new();
    for (i, p) in pairs.iter().enumerate() {
        if p.0 == "0" && p.1 == kind {
            let rest = &pairs[i + 1..];
            let end = rest.iter().position(|q| q.0 == "0").unwrap_or(rest.len());
            out.push(&rest[..end]);
        }
    }
    out
}

fn group<'a>(rec: &'a [(String, String)], code: &str) -> Option<&'a str> {
    rec.iter().find(|p| p.0 == code).map(|p| p.1.as_str())
}

/// #1812: a file whose CLASSES start with VISUALSTYLE (500) and WIDGETX (501), with proxy objects
/// of both classes and of a number past its last class, and a proxy entity of WIDGETX.
fn proxy_file() -> Vec<u8> {
    let text = String::from_utf8(write(&Drawing::new_metric(), "p.dxf").unwrap()).unwrap();
    let mut pairs = dxf_pairs(&text);
    let p = |c: &str, v: &str| (c.to_string(), v.to_string());
    let class = |name: &str, cpp: &str| {
        vec![p("0", "CLASS"), p("1", name), p("2", cpp), p("3", "ObjectDBX Classes"), p("90", "4095"), p("280", "1"), p("281", "0")]
    };
    let classes = pairs.iter().position(|q| q.0 == "2" && q.1 == "CLASSES").unwrap() + 1;
    let mut injected = class("VISUALSTYLE", "AcDbVisualStyle");
    injected.extend(class("WIDGETX", "AcDbWidgetX"));
    pairs.splice(classes..classes, injected);
    let objects = pairs.iter().position(|q| q.0 == "2" && q.1 == "OBJECTS").unwrap();
    let nod = objects + 1;
    let nod_handle = pairs[nod + 1].1.clone();
    let after = nod + pairs[nod..].iter().position(|q| q.0 == "281").unwrap() + 1;
    pairs.splice(after..after, [p("3", "ACAD_VISUALSTYLE"), p("350", "AAA0")]);
    let proxy = |h: &str, class: &str, data: &str| {
        vec![
            p("0", "ACAD_PROXY_OBJECT"),
            p("5", h),
            // An application's 102 group before the proxy's own groups holds a 91 of its own.
            p("102", "{MYAPP"),
            p("91", "7"),
            p("102", "}"),
            p("330", "AAA0"),
            p("100", "AcDbProxyObject"),
            p("90", "499"),
            p("91", class),
            p("95", "0"),
            p("70", "0"),
            p("92", "8"),
            p("310", data),
            p("94", "0"),
        ]
    };
    let mut objs = vec![
        p("0", "DICTIONARY"),
        p("5", "AAA0"),
        p("330", &nod_handle),
        p("100", "AcDbDictionary"),
        p("281", "1"),
        p("3", "Realistic"),
        p("350", "AAA1"),
        p("3", "Widget"),
        p("350", "AAA2"),
        p("3", "Stray"),
        p("350", "AAA3"),
    ];
    objs.extend(proxy("AAA1", "500", "AA"));
    objs.extend(proxy("AAA2", "501", "BB"));
    objs.extend(proxy("AAA3", "537", "CC"));
    let end = objects + pairs[objects..].iter().position(|q| q.0 == "0" && q.1 == "ENDSEC").unwrap();
    pairs.splice(end..end, objs);
    let ms = {
        let recs = dxf_records(&pairs, "BLOCK_RECORD");
        group(recs.iter().find(|r| group(r, "2") == Some("*Model_Space")).unwrap(), "5").unwrap().to_string()
    };
    let ents = pairs.iter().position(|q| q.0 == "2" && q.1 == "ENTITIES").unwrap() + 1;
    pairs.splice(
        ents..ents,
        [
            p("0", "ACAD_PROXY_ENTITY"),
            p("5", "AAA4"),
            p("330", &ms),
            p("100", "AcDbEntity"),
            p("8", "0"),
            p("100", "AcDbProxyEntity"),
            p("90", "498"),
            p("91", "501"),
            p("92", "0"),
            p("93", "0"),
            p("94", "0"),
            p("95", "0"),
            p("70", "0"),
        ],
    );
    pairs.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>().into_bytes()
}

#[test]
fn proxies_keep_their_class_when_classes_are_rewritten() {
    let d = read(&proxy_file(), "p.dxf").unwrap();
    let text = String::from_utf8(write(&d, "out.dxf").unwrap()).unwrap();
    let pairs = dxf_pairs(&text);
    let classes: Vec<String> = dxf_records(&pairs, "CLASS").iter().map(|r| group(r, "1").unwrap().to_string()).collect();
    let class_of = |n: &str| classes.get(n.parse::<usize>().unwrap() - 500).cloned();
    // The proxy's own 91 (after its subclass marker); the application group's stays as it was.
    let own_91 = |r: &[(String, String)]| r.iter().skip_while(|q| q.1 != "AcDbProxyObject").find(|q| q.0 == "91").map(|q| q.1.clone()).unwrap();
    let objects: Vec<(String, Option<String>)> =
        dxf_records(&pairs, "ACAD_PROXY_OBJECT").iter().map(|r| (group(r, "310").unwrap().to_string(), class_of(&own_91(r)))).collect();
    assert!(dxf_records(&pairs, "ACAD_PROXY_OBJECT").iter().all(|r| group(r, "91") == Some("7")), "the application's 91 changed");
    assert_eq!(
        objects,
        [("AA".to_string(), Some("VISUALSTYLE".to_string())), ("BB".to_string(), Some("WIDGETX".to_string()))],
        "classes {classes:?}"
    );
    let entities: Vec<Option<String>> = dxf_records(&pairs, "ACAD_PROXY_ENTITY").iter().map(|r| class_of(group(r, "91").unwrap())).collect();
    assert_eq!(entities, [Some("WIDGETX".to_string())]);
    // The proxy whose number named no class is gone, with its dictionary entry.
    assert!(!text.contains("\nStray\n"));
    // And it stays so through another save.
    let again = String::from_utf8(write(&read(text.as_bytes(), "out.dxf").unwrap(), "again.dxf").unwrap()).unwrap();
    let pairs = dxf_pairs(&again);
    let classes: Vec<String> = dxf_records(&pairs, "CLASS").iter().map(|r| group(r, "1").unwrap().to_string()).collect();
    let names: Vec<String> =
        dxf_records(&pairs, "ACAD_PROXY_OBJECT").iter().map(|r| classes[own_91(r).parse::<usize>().unwrap() - 500].clone()).collect();
    assert_eq!(names, ["VISUALSTYLE", "WIDGETX"]);
}

/// A name that isn't valid Unicode still picks its format by extension.
#[cfg(unix)]
#[test]
fn non_unicode_names_pick_the_format_by_extension() {
    use std::os::unix::ffi::OsStringExt as _;
    let name = |ext: &str| std::path::PathBuf::from(std::ffi::OsString::from_vec([b"caf\xe9.".as_slice(), ext.as_bytes()].concat()));
    assert!(is_image(name("png")) && !is_image(name("dxf")));
    let d = sample();
    let bytes = write(&d, name("dxf")).unwrap();
    assert!(read(&bytes, name("dxf")).is_ok());
    assert!(write(&d, name("png")).unwrap().starts_with(b"\x89PNG"));
    assert!(write(&d, name("dwt")).is_ok(), "a template is written as DWG");
}
