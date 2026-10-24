//! The dimension variables added for fit, suppression, fixed extension lines, tolerance
//! justification, angular zeros, arc symbols, leader arrows, linetypes, lineweights and the jog
//! angle survive DXF: in DIMSTYLE records (group codes of the DXF Reference) and as per-dimension
//! `ACAD` DSTYLE overrides. Linetypes are LTYPE handles. DIMJOGANG is in degrees in DIMSTYLE
//! group 50, as AutoCAD reads it, and in radians in xdata and `$DIMJOGANG`; either unit reads.

use cadcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, EntityKind, HVal, Linetype, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read_dxf, write_dxf};

fn styled() -> DimStyle {
    DimStyle {
        name: "Detail".into(),
        suppress_dim1: true,
        suppress_dim2: false,
        fit: 1,
        text_inside: true,
        suppress_outside_arrows: true,
        force_line_inside: true,
        text_move: 2,
        text_vertical_pos: -1.25,
        tol_just: 2,
        angular_zero_suppression: 3,
        arc_symbol: 1,
        leader_arrow_block: "_DOT".into(),
        dim_linetype: "DASHED".into(),
        ext_linetype1: "CENTER".into(),
        ext_linetype2: "DASHED".into(),
        dim_lineweight: 35,
        ext_lineweight: -1,
        fixed_ext: true,
        fixed_ext_length: 0.75,
        jog_angle: 30.0,
        ..DimStyle::default()
    }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.linetypes.push(Linetype::simple("DASHED", "Dashed", &[0.5, -0.25]));
    d.linetypes.push(Linetype::simple("CENTER", "Center", &[1.25, -0.25, 0.25, -0.25]));
    d.dim_styles.push(styled());
    d
}

fn dimension(overrides: serde_json::Value) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 3.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: overrides.as_object().cloned().unwrap_or_default(),
        assoc: Vec::new(),
        marks: Default::default(),
    })
}

#[test]
fn new_dimstyle_variables_roundtrip_in_the_table() {
    let d = drawing();
    let text = write_dxf(&d);
    // Variables newer than R2000 go in the style's CADCRAFT "DIMVARS" xdata (`1070 <code>`, value):
    // DIMJOGANG 30° in radians, DIMLTYPE as an LTYPE handle. R2000 variables stay groups.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let rec = lines.windows(2).position(|w| w == ["2", "Detail"]).unwrap();
    let end = lines[rec..].windows(2).position(|w| w == ["0", "DIMSTYLE"] || w == ["0", "ENDTAB"]).map_or(lines.len(), |e| rec + e);
    let rec = &lines[rec..end];
    assert!(rec.windows(2).any(|w| w == ["289", "1"]), "DIMATFIT as a group");
    assert!(rec.windows(4).any(|w| w == ["1001", "CADCRAFT", "1000", "DIMVARS"]));
    let after = |code: &str| rec.windows(4).find(|w| w[0] == "1070" && w[1] == code).map(|w| (w[2].to_string(), w[3].to_string())).unwrap();
    let (g, jog) = after("50");
    assert!(g == "1040" && (jog.parse::<f64>().unwrap() - 30f64.to_radians()).abs() < 1e-9, "{jog}");
    let (g, lt) = after("345");
    assert_eq!(g, "1005");
    assert!(lines.windows(4).any(|w| w[0] == "LTYPE" && w[1] == "5" && w[2] == lt), "points at an LTYPE record: {lt}");
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.dim_style("Detail").unwrap(), &styled());
    assert_eq!(back.dim_style("Standard").unwrap(), &DimStyle::default());
}

#[test]
fn new_variables_roundtrip_as_dimension_overrides() {
    let mut d = drawing();
    let ov = serde_json::json!({
        "DIMTOFL": 1, "DIMATFIT": 2, "DIMTIX": 1, "DIMSOXD": 1, "DIMTMOVE": 1, "DIMTVP": 0.5,
        "DIMTOLJ": 0, "DIMAZIN": 1, "DIMARCSYM": 2, "DIMLDRBLK": "_DOT", "DIMLTYPE": "DASHED",
        "DIMLTEX1": "CENTER", "DIMLTEX2": "DASHED", "DIMLWD": 50, "DIMLWE": -3, "DIMFXLON": 1,
        "DIMFXL": 0.25, "DIMJOGANG": 60, "DIMSD1": 1, "DIMSD2": 1,
    });
    d.add(&Space::Model, Common::default(), dimension(ov.clone())).unwrap();
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    let dm = back
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .unwrap();
    let want = DimStyle::default().with_overrides(ov.as_object().unwrap());
    assert_eq!(DimStyle::default().with_overrides(&dm.overrides), want);
    assert!((want.jog_angle - 60.0).abs() < 1e-9 && want.dim_linetype == "DASHED" && want.ext_lineweight == -3);
}

#[test]
fn header_jog_angle_is_read_in_degrees_and_hostile_values_are_dropped() {
    let d = drawing();
    let text = write_dxf(&d).replacen("HEADER\r\n", "HEADER\r\n  9\r\n$DIMJOGANG\r\n 40\r\n0.7853981633974483\r\n", 1);
    let back = read_dxf(text.as_bytes()).unwrap();
    assert!((back.header.f64("DIMJOGANG", 0.0) - 45.0).abs() < 1e-9);
    assert!(back.dim_overrides().is_empty(), "{:?}", back.dim_overrides());
    // Unknown handles and out-of-range values in the style's groups and xdata keep the defaults.
    let text = write_dxf(&d);
    let jog = format!("{}", 30f64.to_radians());
    let bad = text.replacen("1070\r\n345\r\n1005\r\n", "1070\r\n345\r\n1005\r\nFFFFF\r\n1000\r\n", 1).replacen(&jog, "1e300", 1).replacen(
        "\r\n289\r\n1\r\n",
        "\r\n289\r\n99\r\n",
        1,
    );
    assert_ne!(bad, text);
    let back = read_dxf(bad.as_bytes()).unwrap();
    let st = back.dim_style("Detail").unwrap();
    assert_eq!((st.dim_linetype.as_str(), st.jog_angle, st.fit), ("", 45.0, 3), "{st:?}");
    assert_eq!(st.ext_linetype1, "CENTER");
}

fn dxf(d: &Drawing, v: Version) -> String {
    String::from_utf8(cadcraft_io::write_with(d, "x.dxf", &SaveOptions { version: Some(v), binary: false }).unwrap()).unwrap()
}

/// `d` saved (2018) and opened again.
fn reopened(d: &Drawing) -> Drawing {
    read_dxf(dxf(d, Version::R2018).as_bytes()).unwrap()
}

/// The line holding the value of group `code` in the DIMSTYLE record `name`.
fn style_group(lines: &[&str], name: &str, code: &str) -> Option<usize> {
    let pair = |i: usize, c: &str, v: &str| lines.get(i).is_some_and(|l| l.trim() == c) && lines.get(i + 1).is_some_and(|l| l.trim() == v);
    let table = (0..lines.len()).step_by(2).find(|&i| pair(i, "2", "DIMSTYLE"))?;
    let rec = (table..lines.len()).step_by(2).find(|&i| pair(i, "2", name))?;
    (rec..lines.len()).step_by(2).take_while(|&i| lines[i].trim() != "0").find(|&i| lines[i].trim() == code).map(|i| i + 1)
}

/// `text` with the value of group `code` in the DIMSTYLE record `name` replaced by `value`.
fn with_style_group(text: &str, name: &str, code: &str, value: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    let at = style_group(&lines, name, code).unwrap();
    lines[at] = value;
    lines.join("\r\n")
}

#[test]
fn jog_angle_is_degrees_in_dimstyle_groups() {
    // AutoCAD reads group 50 as degrees: given 0.7853981633974483 (45° in radians), it took the
    // jog angle as 0.785°.
    let d = drawing();
    for v in [Version::R2004, Version::R2018] {
        let text = dxf(&d, v);
        let lines: Vec<&str> = text.lines().collect();
        for (name, want) in [("Standard", "45.0"), ("Detail", "30.0")] {
            let at = style_group(&lines, name, "50").unwrap();
            assert_eq!(lines[at].trim(), want, "{v:?} {name}");
        }
        let back = read_dxf(text.as_bytes()).unwrap();
        assert_eq!(back.dim_style("Standard").unwrap(), &DimStyle::default(), "{v:?}");
        assert_eq!(back.dim_style("Detail").unwrap(), &styled(), "{v:?}");
    }
}

#[test]
fn jog_angle_in_radians_in_a_group_still_reads() {
    // CADCraft 0.7.0 and earlier wrote group 50 in radians. 5° to 90° in radians are below the 5°
    // a style takes, so they can't be degrees and read as radians.
    let text = dxf(&drawing(), Version::R2018);
    for (file, want) in [("0.7853981633974483", 45.0), ("1.0471975511965976", 60.0), ("0.08726646259971647", 5.0), ("1.5707963267948966", 90.0)] {
        let back = read_dxf(with_style_group(&text, "Detail", "50", file).as_bytes()).unwrap();
        assert_eq!(back.dim_style("Detail").unwrap().jog_angle, want, "{file}");
        // Saved again, it's degrees: the same style comes back, its linetypes too.
        assert_eq!(reopened(&back).dim_style("Detail").unwrap(), &DimStyle { jog_angle: want, ..styled() }, "saved again: {file}");
    }
}

#[test]
fn jog_angle_no_style_takes_keeps_the_default() {
    let text = dxf(&drawing(), Version::R2018);
    for file in ["0.0", "0.05", "2.0", "4.99", "90.5", "-30.0", "1e300", "-1e300", "NaN", "inf"] {
        let back = read_dxf(with_style_group(&text, "Detail", "50", file).as_bytes()).unwrap();
        let st = back.dim_style("Detail").unwrap();
        assert_eq!(st.jog_angle, 45.0, "{file}");
        assert_eq!(DimStyle { jog_angle: 30.0, ..st.clone() }, styled(), "the rest of the style reads: {file}");
        assert_eq!(reopened(&back).dim_style("Detail").unwrap(), &DimStyle { jog_angle: 45.0, ..styled() }, "saved again: {file}");
    }
}

#[test]
fn jog_angle_overrides_stay_radians_in_xdata() {
    // Xdata keeps radians, as CADCraft's DSTYLE overrides and R2000 DIMVARS always have (AutoCAD's
    // own R2000 save keeps the style's jog angle in xdata in radians too). Degrees read as well.
    let mut d = drawing();
    d.add(&Space::Model, Common::default(), dimension(serde_json::json!({ "DIMJOGANG": 60 }))).unwrap();
    let text = dxf(&d, Version::R2018);
    let rad = 60f64.to_radians().to_string();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.windows(4).any(|w| w == ["1070", "50", "1040", rad.as_str()]), "DSTYLE: 1070 50, 1040 {rad}");
    let jog = |text: &str| {
        read_dxf(text.as_bytes()).unwrap().model.iter().find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(DimStyle::default().with_overrides(&dm.overrides).jog_angle),
            _ => None,
        })
    };
    assert_eq!(jog(&text), Some(60.0));
    let degrees = text.replacen(&format!("1040\r\n{rad}\r\n"), "1040\r\n60.0\r\n", 1);
    assert_ne!(degrees, text);
    assert_eq!(jog(&degrees), Some(60.0));
}

#[test]
fn header_jog_angle_takes_degrees_too_and_drops_unusable_values() {
    let d = drawing();
    // The header carries the current style's jog angle, in radians.
    let text = write_dxf(&d);
    let lines: Vec<&str> = text.lines().collect();
    let at = lines.iter().position(|l| l.trim() == "$DIMJOGANG").expect("$DIMJOGANG written") + 2;
    assert!((lines[at].trim().parse::<f64>().unwrap() - 45f64.to_radians()).abs() < 1e-9, "{}", lines[at]);
    let jog = |v: &str| {
        let mut lines = lines.clone();
        lines[at] = v;
        read_dxf(lines.join("\r\n").as_bytes()).unwrap().header.get("DIMJOGANG").and_then(HVal::as_f64)
    };
    assert_eq!(jog("0.5235987755982988"), Some(30.0));
    assert_eq!(jog("30.0"), Some(30.0));
    for v in ["0.0", "120.0", "1e300", "NaN"] {
        assert_eq!(jog(v), None, "{v}");
    }
}

/// The DWG bridge (acadrust) reads and writes DIMSTYLE group 50 in degrees too, so a DWG whose
/// style jogs at 30° (made here by the bridge from our 2018 DXF) opens at 30°. Read as radians,
/// it was 1719° and the style fell back to 45°.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_jog_angle_reads_in_degrees() {
    let dwg = cadcraft_dwg::dxf_to_dwg_as(dxf(&drawing(), Version::R2018).as_bytes(), "AC1032").unwrap();
    let text = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let jog: f64 = lines[style_group(&lines, "Detail", "50").unwrap()].trim().parse().unwrap();
    assert!((jog - 30.0).abs() < 1e-9, "the bridge's DXF has degrees: {jog}");
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(back.dim_style("Detail").unwrap().jog_angle, 30.0);
}
