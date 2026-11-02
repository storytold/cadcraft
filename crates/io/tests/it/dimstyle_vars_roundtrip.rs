//! The dimension variables added for fit, suppression, fixed extension lines, tolerance
//! justification, angular zeros, arc symbols, leader arrows, linetypes, lineweights and the jog
//! angle survive DXF: in DIMSTYLE records (group codes of the DXF Reference) and as per-dimension
//! `ACAD` DSTYLE overrides. Linetypes are LTYPE handles; DIMJOGANG is in radians in the file.

use cadcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, EntityKind, Linetype, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

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
