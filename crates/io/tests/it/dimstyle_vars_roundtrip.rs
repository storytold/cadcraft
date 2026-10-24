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
    // Unknown handles and out-of-range values in the style's groups and xdata keep the defaults
    // (the jog angle is in both AutoCAD's xdata and CADCraft's).
    let text = write_dxf(&d);
    let jog = format!("{}", 30f64.to_radians());
    assert_eq!(text.matches(&jog).count(), 2);
    let bad = text.replacen("1070\r\n345\r\n1005\r\n", "1070\r\n345\r\n1005\r\nFFFFF\r\n1000\r\n", 1).replace(&jog, "1e300").replacen(
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

/// The lines of the DIMSTYLE record `name`, from its name to the next record.
fn style_record(lines: &[&str], name: &str) -> Option<std::ops::Range<usize>> {
    let pair = |i: usize, c: &str, v: &str| lines.get(i).is_some_and(|l| l.trim() == c) && lines.get(i + 1).is_some_and(|l| l.trim() == v);
    let table = (0..lines.len()).step_by(2).find(|&i| pair(i, "2", "DIMSTYLE"))?;
    let rec = (table..lines.len()).step_by(2).find(|&i| pair(i, "2", name))?;
    let end = (rec..lines.len()).step_by(2).find(|&i| lines[i].trim() == "0")?;
    Some(rec..end)
}

/// The line holding the value of group `code` in the DIMSTYLE record `name`.
fn style_group(lines: &[&str], name: &str, code: &str) -> Option<usize> {
    style_record(lines, name)?.step_by(2).find(|&i| lines[i].trim() == code).map(|i| i + 1)
}

/// The DIMSTYLE record `name` of `text`, trimmed lines.
fn record_of<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let lines: Vec<&str> = text.lines().collect();
    let r = style_record(&lines, name).unwrap();
    lines[r].iter().map(|l| l.trim()).collect()
}

/// `text` with `groups` (code, value, …) added at the end of the DIMSTYLE record `name`.
fn with_style_xdata(text: &str, name: &str, groups: &[&str]) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    let end = style_record(&lines, name).unwrap().end;
    lines.splice(end..end, groups.iter().copied());
    lines.join("\r\n")
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

/// AutoCAD's own xdata for an R2000 style's jog angle, as AutoCAD LT 2026 saves R2000.
const JOG_APP: &str = "ACAD_DSTYLE_DIMJOGGED_JOGANGLE_SYMBOL";

/// That xdata holding `rad`, with the `1070` marker `marker`.
fn autocad_jog<'a>(marker: &'a str, rad: &'a str) -> [&'a str; 6] {
    ["1001", JOG_APP, "1070", marker, "1040", rad]
}

/// Number of `code`/`value` pairs in DXF text.
fn pairs(text: &str, code: &str, value: &str) -> usize {
    text.lines().map(str::trim).collect::<Vec<_>>().chunks(2).filter(|p| *p == [code, value]).count()
}

fn jog_of(text: &str, style: &str) -> f64 {
    read_dxf(text.as_bytes()).unwrap().dim_style(style).unwrap().jog_angle
}

#[test]
fn r2000_jog_angle_also_goes_in_autocad_xdata() {
    // R2000 DIMSTYLE records have no group 50, so a jog angle other than the default also goes
    // in AutoCAD's own xdata (CADCraft's DIMVARS xdata is for CADCraft only), with its APPID.
    let text = write_dxf(&drawing());
    let rad30 = 30f64.to_radians().to_string();
    let rec = record_of(&text, "Detail");
    assert!(rec.windows(6).any(|w| w == autocad_jog("384", &rad30)), "{rec:?}");
    assert!(!record_of(&text, "Standard").contains(&JOG_APP), "the default isn't written");
    assert_eq!((pairs(&text, "2", JOG_APP), pairs(&text, "1001", JOG_APP)), (1, 1), "one APPID, one record");
    assert_eq!(read_dxf(text.as_bytes()).unwrap().dim_style("Detail").unwrap(), &styled());
    // 2004 and later have group 50 instead; drawings with default jog angles are unchanged.
    assert!(!dxf(&drawing(), Version::R2018).contains(JOG_APP));
    assert!(!write_dxf(&Drawing::new_imperial()).contains(JOG_APP));
    // A jog angle no style takes (set without `apply_fields`) isn't written anywhere: it would be
    // dropped on reading anyway.
    let mut d = drawing();
    d.dim_styles.iter_mut().filter(|s| s.name == "Detail").for_each(|s| s.jog_angle = 100.0);
    for text in [write_dxf(&d), dxf(&d, Version::R2018)] {
        assert!(!record_of(&text, "Detail").contains(&JOG_APP));
        assert_eq!(read_dxf(text.as_bytes()).unwrap().dim_style("Detail").unwrap(), &DimStyle { jog_angle: 45.0, ..styled() });
    }
}

#[test]
fn autocad_r2000_jog_xdata_reads() {
    // A style AutoCAD saved as R2000 with a 60° jog angle; CADCraft's own xdata has no jog angle.
    let plain = write_dxf(&{
        let mut d = drawing();
        d.dim_styles.iter_mut().for_each(|s| s.jog_angle = 45.0);
        d
    });
    assert!(!plain.contains(JOG_APP));
    let rad60 = 60f64.to_radians().to_string();
    let at60 = with_style_xdata(&plain, "Detail", &autocad_jog("   384", &rad60));
    assert_eq!(jog_of(&at60, "Detail"), 60.0);
    // Saved and opened again, the style comes back whole.
    let back = read_dxf(write_dxf(&read_dxf(at60.as_bytes()).unwrap()).as_bytes()).unwrap();
    assert_eq!(back.dim_style("Detail").unwrap(), &DimStyle { jog_angle: 60.0, ..styled() });
    // Another marker, no angle a style takes (AutoCAD kept CADCraft 0.6.2's 0.785° as 0.0137 rad;
    // 5.5 is radians there, not 5.5°), NaN or a real of another kind: ignored.
    for bad in [autocad_jog("383", &rad60), autocad_jog("384", "0.0137077838904019"), autocad_jog("384", "5.5"), autocad_jog("384", "NaN")] {
        assert_eq!(jog_of(&with_style_xdata(&plain, "Detail", &bad), "Detail"), 45.0, "{bad:?}");
    }
    let other_real = ["1001", JOG_APP, "1070", "384", "1041", rad60.as_str()];
    assert_eq!(jog_of(&with_style_xdata(&plain, "Detail", &other_real), "Detail"), 45.0);
    // AutoCAD updates its own xdata and leaves CADCraft's as it was: a 60° jog set in AutoCAD wins
    // over the 30° CADCraft wrote in DIMVARS.
    let ours = write_dxf(&drawing());
    let mut lines: Vec<&str> = ours.lines().collect();
    let r = style_record(&lines, "Detail").unwrap();
    let at = r.step_by(2).find(|&i| lines[i].trim() == "1001" && lines[i + 1].trim() == JOG_APP).unwrap() + 5;
    assert_eq!(lines[at].trim(), 30f64.to_radians().to_string());
    lines[at] = &rad60;
    assert_eq!(jog_of(&lines.join("\r\n"), "Detail"), 60.0);
}

#[test]
fn dimstyle_groups_win_over_stale_dimvars() {
    // A CADCraft R2000 file saved again by another program as 2018: its groups are current, the
    // DIMVARS xdata it kept may not be.
    let text = dxf(&drawing(), Version::R2018);
    let stale = ["1001", "CADCRAFT", "1000", "DIMVARS", "1002", "{", "1070", "50", "1040", "1.0471975511965976", "1002", "}"];
    assert_eq!(jog_of(&with_style_xdata(&text, "Detail", &stale), "Detail"), 30.0);
}

#[test]
fn autocad_jog_xdata_is_written_once_and_current() {
    // Read from a file with AutoCAD's xdata (and its APPID), the angle is the style's: saving
    // writes the style's current angle once, not the copy read from the file as well.
    let plain = write_dxf(&{
        let mut d = drawing();
        d.dim_styles.iter_mut().for_each(|s| s.jog_angle = 45.0);
        d
    });
    let rad60 = 60f64.to_radians().to_string();
    let with = with_style_xdata(&plain, "Detail", &autocad_jog("384", &rad60));
    let appid =
        ["  0", "APPID", "  5", "FFF0", "330", "0", "100", "AcDbSymbolTableRecord", "100", "AcDbRegAppTableRecord", "  2", JOG_APP, " 70", "     0"];
    let mut lines: Vec<&str> = with.lines().collect();
    let table = lines.windows(4).position(|w| w[0].trim() == "0" && w[1] == "TABLE" && w[2].trim() == "2" && w[3] == "APPID").unwrap();
    let endtab = table + lines[table..].iter().position(|l| *l == "ENDTAB").unwrap() - 1;
    lines.splice(endtab..endtab, appid);
    let mut d = read_dxf(lines.join("\r\n").as_bytes()).unwrap();
    assert_eq!(d.dim_style("Detail").unwrap().jog_angle, 60.0);
    let again = write_dxf(&d);
    assert_eq!((pairs(&again, "2", JOG_APP), pairs(&again, "1001", JOG_APP)), (1, 1));
    assert!(record_of(&again, "Detail").windows(6).any(|w| w == autocad_jog("384", &rad60)));
    assert_eq!(jog_of(&again, "Detail"), 60.0);
    d.dim_styles.iter_mut().filter(|s| s.name == "Detail").for_each(|s| s.jog_angle = 30.0);
    let edited = write_dxf(&d);
    assert!(record_of(&edited, "Detail").windows(6).any(|w| w == autocad_jog("384", &30f64.to_radians().to_string())));
    assert_eq!(pairs(&edited, "1001", JOG_APP), 1);
    assert_eq!(read_dxf(edited.as_bytes()).unwrap().dim_style("Detail").unwrap(), &styled(), "opened again");
    let newer = dxf(&d, Version::R2018);
    assert!(!newer.contains(JOG_APP), "2018 has group 50 instead");
    assert_eq!(jog_of(&newer, "Detail"), 30.0);
}

#[test]
fn an_unusable_group_leaves_the_variable_to_the_xdata() {
    // An R2000 record (the newer variables in CADCraft's DIMVARS xdata) with DIMARCSYM 7 in group
    // 90, no arc symbol a style takes: the xdata's 1 is read, not the default.
    let text = write_dxf(&drawing());
    let mut lines: Vec<&str> = text.lines().collect();
    let name = style_record(&lines, "Detail").unwrap().start;
    lines.splice(name + 2..name + 2, [" 90", "7"]);
    assert_eq!(read_dxf(lines.join("\r\n").as_bytes()).unwrap().dim_style("Detail").unwrap(), &styled());
}
