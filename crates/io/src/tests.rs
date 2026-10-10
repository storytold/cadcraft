use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use crate::*;

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
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
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
fn second_roundtrip_is_stable() {
    let d = sample();
    let t1 = write_dxf(&d);
    let d2 = read_dxf(t1.as_bytes()).unwrap();
    let d3 = read_dxf(write_dxf(&d2).as_bytes()).unwrap();
    assert_eq!(d2.model.len(), d3.model.len());
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

/// Structural sanity check of a PDF: header, object count, xref offsets pointing at `n 0 obj`.
fn check_pdf(bytes: &[u8]) -> String {
    assert!(bytes.starts_with(b"%PDF-1.4"));
    let find = |pat: &[u8]| bytes.windows(pat.len()).position(|w| w == pat);
    let rfind = |pat: &[u8]| bytes.windows(pat.len()).rposition(|w| w == pat);
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64)..]).to_string();
    assert!(tail.trim_end().ends_with("%%EOF"));
    let sx = rfind(b"startxref\n").unwrap();
    let after = String::from_utf8_lossy(&bytes[sx + 10..]).to_string();
    let xref_off: usize = after.lines().next().unwrap().trim().parse().unwrap();
    assert!(bytes[xref_off..].starts_with(b"xref\n"));
    let xref = String::from_utf8_lossy(&bytes[xref_off..]).to_string();
    let mut lines = xref.lines().skip(1);
    let count: usize = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let objs = bytes.windows(7).filter(|w| w == b" 0 obj\n").count();
    assert_eq!(count, objs + 1, "xref size = objects + free entry");
    assert_eq!(bytes.windows(6).filter(|w| w == b"endobj").count(), objs);
    let entries: Vec<&str> = lines.take(count).collect();
    assert!(entries[0].starts_with("0000000000 65535 f"));
    for (i, e) in entries.iter().enumerate().skip(1) {
        let off: usize = e[..10].parse().unwrap();
        assert!(bytes[off..].starts_with(format!("{i} 0 obj").as_bytes()), "object {i} offset");
    }
    assert!(xref.contains(&format!("/Size {count}")));
    // The content stream's /Length matches its data.
    let sp = find(b"/Length ").unwrap();
    let after = String::from_utf8_lossy(&bytes[sp + 8..sp + 30]).to_string();
    let len: usize = after.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
    let start = find(b"stream\n").unwrap() + 7;
    assert!(bytes[start + len..].starts_with(b"\nendstream"));
    let compressed = find(b"/FlateDecode").is_some();
    let data = &bytes[start..start + len];
    if compressed {
        String::from_utf8(miniz_oxide::inflate::decompress_to_vec_zlib(data).unwrap()).unwrap()
    } else {
        String::from_utf8(data.to_vec()).unwrap()
    }
}

fn media_box(bytes: &[u8]) -> (f64, f64) {
    let text = String::from_utf8_lossy(bytes).to_string();
    let i = text.find("/MediaBox [0 0 ").unwrap() + 15;
    let v: Vec<f64> = text[i..].split(']').next().unwrap().split_whitespace().map(|x| x.parse().unwrap()).collect();
    (v[0], v[1])
}

#[test]
fn pdf_model_fitted_to_sheet() {
    let d = sample();
    let bytes = plot(&d, &Space::Model, &serde_json::json!({"paper": "A4", "landscape": true})).unwrap();
    let content = check_pdf(&bytes);
    let (w, h) = media_box(&bytes);
    assert!((w - 841.89).abs() < 0.01 && (h - 595.276).abs() < 0.01, "A4 landscape in points: {w} x {h}");
    assert!(content.contains(" m\n") && content.contains(" l\n") && content.contains("S\n"));
    // Walls layer is red; colour 7 prints black.
    assert!(content.contains("1 0 0 RG"));
    assert!(content.contains("0 0 0 RG"));
    // Uncompressed output is plain text and also valid.
    let raw = plot(&d, &Space::Model, &serde_json::json!({"paper": "Letter", "compress": false, "landscape": false})).unwrap();
    let c2 = check_pdf(&raw);
    assert!(!String::from_utf8_lossy(&raw).contains("FlateDecode"));
    assert_eq!(media_box(&raw), (612.0, 792.0));
    assert!(c2.contains("re W n"));
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
        }),
    )
    .unwrap();
    let bytes = plot(&d, &paper, &serde_json::json!({})).unwrap();
    let content = check_pdf(&bytes);
    // ANSI A landscape (inches → points).
    assert_eq!(media_box(&bytes), (792.0, 612.0));
    // Viewport border at 1:1: x from 1in to 9in = 72pt..648pt.
    assert!(content.contains("72 72 m") || content.contains("72 72 l"), "{content}");
    // Lineweights on: default 0.25 mm = 0.709 pt.
    assert!(content.contains("0.709 w"));
    let off = plot(&d, &paper, &serde_json::json!({"lineweights": false})).unwrap();
    assert!(check_pdf(&off).contains("0 w"));
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
    }
}

fn first<T>(d: &Drawing, f: impl Fn(&EntityKind) -> Option<T>) -> T {
    d.model.iter().find_map(|e| f(&e.kind)).expect("entity")
}

fn roundtrip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).unwrap()
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
        alt: true,
        alt_factor: 0.03937,
        alt_decimals: 3,
        alt_post: "[<>]".into(),
        angular_unit: 1,
        suppress_ext1: true,
        suppress_ext2: true,
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
    assert_eq!(fa[1], ali.assoc[1]);
    assert!(matches!(fa[0].snap, AssocSnap::OnCircle { angle } if angle.abs() < 1e-9));
    assert!(assoc_of(&foreign, hr).is_empty(), "radial links are CADCraft-only");
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

fn table_sample() -> Table {
    let cell = |t: &str| TableCell { text: t.into(), merged: None };
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
    }
}

#[test]
fn tables_roundtrip_with_title_header_and_merges() {
    let mut d = Drawing::new_imperial();
    d.table_styles.push(TableStyle { name: "Schedule".into(), text_height: 0.25, margin: 0.1, title: false, header: true });
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
    d.table_styles.push(TableStyle { name: "Schedule".into(), text_height: 0.25, margin: 0.1, title: true, header: true });
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
    assert_eq!(back.dim_style("Mech"), d.dim_style("Mech"));
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
fn mleader_is_written_as_leader_and_mtext() {
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
    };
    let m = cadcraft_doc::MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
        landing: Vec3::new(10.0, 5.0, 0.0),
        dogleg: 2.0,
        text: Some(text),
        style: "Standard".into(),
        arrow_size: 2.5,
    };
    d.add(&Space::Model, Default::default(), EntityKind::MLeader(m)).unwrap();
    let back = roundtrip(&d);
    assert!(back.model.iter().any(|e| matches!(e.kind, EntityKind::Leader(_))));
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::MText(t) if t.contents == "Note")));
}
