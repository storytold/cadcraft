//! What AutoCAD reads from CADCraft's DWG and DXF as it is, without recomputing it: justified
//! text's insertion point (#1050), DIMPOST (#1053), the current dimension style and its header
//! variables (#1051), the colours of a dimension's `*D` block (#1052) and the stored measurement
//! of a rotated linear dimension (#1054) and of angular dimensions.

use cadcraft_color::Color;
use cadcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, EntityKind, HAlign, Space, Text, VAlign};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_dxf, write_with};

fn dwg_roundtrip(d: &Drawing) -> Drawing {
    let dwg = write_with(d, "case.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    read(&dwg, "case.dwg").unwrap()
}

/// `(code, value)` pairs of a DXF, values trimmed.
fn tags(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim().to_string()))).collect()
}

fn text(value: &str, at: Vec3, h: HAlign, v: VAlign) -> EntityKind {
    EntityKind::Text(Text {
        insert: at,
        align_pt: Some(at),
        height: 5.0,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: h,
        valign: v,
    })
}

#[test]
fn justified_text_is_written_with_its_baseline_start_as_insertion_point() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), text("MIDDLE CENTER", Vec3::new(100.0, 50.0, 0.0), HAlign::Center, VAlign::Middle)).unwrap();
    d.add(&Space::Model, Common::default(), text("BOTTOM RIGHT", Vec3::new(100.0, 30.0, 0.0), HAlign::Right, VAlign::Bottom)).unwrap();
    let width = |s: &str| cadcraft_fonts::line_width(s, 5.0, 1.0);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;

    // DXF: group 10 is where the baseline starts, group 11 the alignment point.
    let t = tags(&write_dxf(&d));
    let texts: Vec<usize> = t.iter().enumerate().filter(|(_, (c, v))| *c == 0 && v == "TEXT").map(|(i, _)| i).collect();
    let group = |start: usize, code: i32| -> f64 { t[start..].iter().find(|(c, _)| *c == code).unwrap().1.parse().unwrap() };
    assert_eq!(texts.len(), 2);
    assert!(near(group(texts[0], 10), 100.0 - width("MIDDLE CENTER") / 2.0), "{}", group(texts[0], 10));
    assert!(near(group(texts[0], 11), 100.0));
    assert!(near(group(texts[1], 10), 100.0 - width("BOTTOM RIGHT")), "{}", group(texts[1], 10));

    // DWG: the same, through the bridge.
    let back = dwg_roundtrip(&d);
    let starts: Vec<(f64, f64)> = back
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some((t.insert.x, t.align_pt.map_or(f64::NAN, |a| a.x))),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 2);
    assert!(near(starts[0].0, 100.0 - width("MIDDLE CENTER") / 2.0) && near(starts[0].1, 100.0), "{starts:?}");
    assert!(near(starts[1].0, 100.0 - width("BOTTOM RIGHT")) && near(starts[1].1, 100.0), "{starts:?}");
}

/// A metric drawing whose current style is S25 (text 0.25, 2 decimals, DIMSCALE 25, red lines,
/// green text), with one dimension.
fn s25(kind: DimKind, p13: Vec3, p14: Vec3, defpt: Vec3) -> Drawing {
    let mut d = Drawing::new_metric();
    d.dim_styles.push(DimStyle {
        name: "S25".into(),
        scale: 25.0,
        text_height: 0.25,
        decimals: 2,
        dim_line_color: Color::from_aci(1),
        ext_line_color: Color::from_aci(1),
        text_color: Color::from_aci(3),
        ..DimStyle::default()
    });
    d.header.set_str("DIMSTYLE", "S25");
    let dm = Dimension {
        kind,
        defpt,
        text_mid: Vec3::ZERO,
        p13,
        p14,
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "S25".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    d
}

fn linear(d_rotation: f64) -> Drawing {
    s25(DimKind::Linear { rotation: d_rotation }, Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0), Vec3::new(50.0, -20.0, 0.0))
}

#[test]
fn the_current_dimension_style_and_its_variables_reach_autocad() {
    let d = linear(0.0);
    let t = tags(&write_dxf(&d));
    let var = |name: &str| t.windows(2).find(|w| w[0] == (9, name.to_string())).map(|w| w[1].1.clone());
    assert_eq!(var("$DIMSTYLE").as_deref(), Some("S25"));
    assert_eq!(var("$DIMTXT").and_then(|v| v.parse::<f64>().ok()), Some(0.25));
    assert_eq!(var("$DIMDEC").as_deref(), Some("2"));
    assert_eq!(var("$DIMCLRT").as_deref(), Some("3"));
    // DIMPOST is written even when empty (#1053).
    let back = dwg_roundtrip(&d);
    assert_eq!(back.header.str("DIMSTYLE", ""), "S25", "the DWG keeps the current style");
    for s in &back.dim_styles {
        assert_eq!(s.post, "", "{}: DIMPOST stays empty, not \"<>\"", s.name);
    }
}

#[test]
fn a_dimension_block_takes_the_style_colours() {
    let d = linear(0.0);
    let t = tags(&write_dxf(&d));
    let blocks = t.iter().position(|(c, v)| *c == 2 && v == "BLOCKS").unwrap();
    let start = blocks + t[blocks..].iter().position(|(c, v)| *c == 2 && v.starts_with("*D")).unwrap();
    let end = start + t[start..].iter().position(|(c, v)| *c == 0 && v == "ENDBLK").unwrap();
    let mut colors: Vec<(String, i64)> = Vec::new();
    let mut kind = String::new();
    for (c, v) in &t[start..end] {
        match c {
            0 => kind = v.clone(),
            62 => colors.push((kind.clone(), v.parse().unwrap())),
            _ => {}
        }
    }
    assert!(colors.iter().any(|(k, c)| k == "LINE" && *c == 1), "{colors:?}");
    assert!(colors.iter().any(|(k, c)| k == "MTEXT" && *c == 3), "{colors:?}");
    assert!(!colors.iter().any(|(_, c)| *c == 0), "nothing ByBlock: {colors:?}");
}

#[test]
fn a_rotated_linear_dimension_stores_its_projected_length() {
    let d = s25(
        DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 },
        Vec3::new(-5.0, 0.0, 0.0),
        Vec3::new(0.0, 210.0, 0.0),
        Vec3::new(-40.0, 105.0, 0.0),
    );
    let dwg = write_with(&d, "case.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let dxf = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let t = tags(&dxf);
    let dim = t.iter().position(|(c, v)| *c == 0 && v == "DIMENSION").unwrap();
    let m: f64 = t[dim..].iter().find(|(c, _)| *c == 42).unwrap().1.parse().unwrap();
    assert!((m - 210.0).abs() < 1e-9, "group 42 = {m}");
}

/// Group 10 and the value of every TEXT in a DXF, in file order.
fn text_starts(dxf: &str) -> Vec<(String, cadcraft_geom::Vec2)> {
    let t = tags(dxf);
    let mut out = Vec::new();
    for (i, (c, v)) in t.iter().enumerate() {
        if *c == 0 && v == "TEXT" {
            let end = t[i + 1..].iter().position(|(c, _)| *c == 0).map_or(t.len(), |k| i + 1 + k);
            let g = |code: i32| t[i + 1..end].iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
            let num = |code: i32| g(code).and_then(|v| v.parse::<f64>().ok()).unwrap_or(f64::NAN);
            out.push((g(1).unwrap_or_default(), cadcraft_geom::Vec2::new(num(10), num(20))));
        }
    }
    out
}

/// Justified TEXT made in CADCraft (insertion point = alignment point) is written with its
/// baseline start in group 10, for every justification and rotated too: placed baseline-left at
/// the written group 10, each text covers what CADCraft draws for the justified one. Left Middle
/// and Left Bottom were kept at the alignment point, half or a third of a height off, because
/// that point passed for AutoCAD's own (test by @lee-cyrille-maskell, from their fix for #1050).
#[test]
fn justified_text_made_here_writes_its_baseline_start_for_every_justification() {
    use HAlign::{Center, Left, Right};
    use VAlign::{Baseline, Bottom, Top};
    let just = [
        (Left, Baseline),
        (Center, Baseline),
        (Right, Baseline),
        (HAlign::Middle, Baseline),
        (Left, Top),
        (Center, Top),
        (Right, Top),
        (Left, VAlign::Middle),
        (Center, VAlign::Middle),
        (Right, VAlign::Middle),
        (Left, Bottom),
        (Center, Bottom),
        (Right, Bottom),
    ];
    let mut d = Drawing::new_imperial();
    let mut made = Vec::new();
    for (i, &(h, v)) in just.iter().enumerate() {
        for rotation in [0.0, 0.5] {
            let at = Vec3::new(100.0, 400.0 - 22.0 * i as f64, 0.0);
            let left = h == Left && v == Baseline;
            let t = Text {
                insert: at,
                align_pt: if left { None } else { Some(at) },
                height: 5.0,
                value: format!("{h:?} {v:?} {rotation} Hxg"),
                rotation,
                width_factor: 0.8,
                oblique: 0.0,
                style: "Standard".into(),
                halign: h,
                valign: v,
            };
            d.add(&Space::Model, Common::default(), EntityKind::Text(t.clone())).unwrap();
            made.push(t);
        }
    }
    let dxf = write_dxf(&d);
    let starts = text_starts(&dxf);
    assert_eq!(starts.len(), made.len());
    let near = |a: cadcraft_geom::Vec2, b: cadcraft_geom::Vec2| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6;
    for (t, (value, p)) in made.iter().zip(&starts) {
        assert_eq!(&t.value, value);
        let (_, want) = cadcraft_render::place_text_entity(&d, t, &t.value);
        let start = Text { insert: Vec3::new(p.x, p.y, 0.0), align_pt: None, halign: Left, valign: Baseline, ..t.clone() };
        let (_, got) = cadcraft_render::place_text_entity(&d, &start, &start.value);
        assert!(near(got.min, want.min) && near(got.max, want.max), "{value}: drawn {want:?}, from group 10 {got:?}");
    }
    // Read back and saved again, group 10 stays put.
    let back = read(dxf.as_bytes(), "t.dxf").unwrap();
    assert_eq!(text_starts(&write_dxf(&back)), starts);
}

/// Group 42 of a dimension in the DXF AutoCAD reads from `d`'s DWG.
fn stored_measurement(d: &Drawing) -> f64 {
    let dwg = write_with(d, "case.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let dxf = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let t = tags(&dxf);
    let dim = t.iter().position(|(c, v)| *c == 0 && v == "DIMENSION").unwrap();
    t[dim..].iter().find(|(c, _)| *c == 42).unwrap().1.parse().unwrap()
}

fn polar(deg: f64, r: f64) -> Vec3 {
    let a = deg.to_radians();
    Vec3::new(r * a.cos(), r * a.sin(), 0.0)
}

/// Angular dimensions store their angle too, in radians as AutoCAD does (0.0 in 0.6.2).
#[test]
fn an_angular_dimension_stores_its_angle_in_radians() {
    // Three points: vertex (p15) at the origin, legs through p13 and p14, the arc through defpt.
    for deg in [60.0, 174.0] {
        let d = s25(DimKind::Angular3P, polar(0.0, 20.0), polar(deg, 20.0), polar(deg / 2.0, 15.0));
        let m = stored_measurement(&d);
        assert!((m - f64::to_radians(deg)).abs() < 1e-9, "{deg}°: group 42 = {m}");
    }
    // Two lines, 120° apart: p13-p14 and defpt-p15, the arc through p16.
    let mut d = s25(DimKind::Angular, polar(0.0, 5.0), polar(0.0, 20.0), polar(120.0, 5.0));
    let h = d.model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    d.modify_entity(h, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.p15 = polar(120.0, 20.0);
            dm.p16 = polar(60.0, 15.0);
        }
    })
    .unwrap();
    let m = stored_measurement(&d);
    assert!((m - f64::to_radians(120.0)).abs() < 1e-9, "two lines: group 42 = {m}");
}

/// A justified text read from AutoCAD keeps AutoCAD's insertion point (exact for its own fonts)
/// when it is near CADCraft's; only a stale one (the alignment point, or a text that has since
/// changed length) is replaced.
#[test]
fn an_insertion_point_from_autocad_is_kept() {
    let mut d = Drawing::new_metric();
    let at = Vec3::new(100.0, 50.0, 0.0);
    let ours = 100.0 - cadcraft_fonts::line_width("MIDDLE CENTER", 5.0, 1.0) / 2.0;
    let mut k = text("MIDDLE CENTER", at, HAlign::Center, VAlign::Middle);
    if let EntityKind::Text(t) = &mut k {
        // AutoCAD's font is a little narrower than CADCraft's.
        t.insert = Vec3::new(ours + 2.5, 47.5, 0.0);
    }
    d.add(&Space::Model, Common::default(), k).unwrap();
    let t = tags(&write_dxf(&d));
    let i = t.iter().position(|(c, v)| *c == 0 && v == "TEXT").unwrap();
    let x: f64 = t[i..].iter().find(|(c, _)| *c == 10).unwrap().1.parse().unwrap();
    assert!((x - (ours + 2.5)).abs() < 1e-9, "kept: {x}");
}
