//! The anonymous `*D` block written for a dimension (what other programs draw) holds its text at
//! the height it is drawn at: the dimension's overrides and DIMSCALE (0 = the drawing's) count
//! (issue #367). It holds the style's DIMPOST on angular dimensions too (issue #1653).

use cadcraft_doc::{Common, DimKind, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version};

/// The group 40 (height) of the first MTEXT in the BLOCKS section.
fn block_mtext_height(dxf: &str) -> f64 {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let blocks = lines.windows(2).position(|w| w == ["2", "BLOCKS"]).unwrap();
    let mtext = blocks + lines[blocks..].windows(2).position(|w| w == ["0", "MTEXT"]).unwrap();
    let h = mtext + lines[mtext..].chunks(2).position(|w| w[0] == "40").unwrap() * 2;
    lines[h + 1].parse().unwrap()
}

fn height_written(style_scale: f64, overrides: serde_json::Value) -> f64 {
    let mut d = Drawing::new_imperial();
    d.dim_styles.iter_mut().for_each(|s| s.scale = style_scale);
    let dm = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(15.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(30.0, 0.0, 0.0),
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
    };
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    block_mtext_height(&cadcraft_io::write_dxf(&d))
}

#[test]
fn dimension_block_text_has_the_drawn_height() {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let plain = height_written(1.0, serde_json::json!({}));
    assert!(near(plain, 0.18), "{plain}");
    let scaled = height_written(1.0, serde_json::json!({"scale": 10.0}));
    assert!(near(scaled, 1.8), "a DIMSCALE override scales the block text: {scaled}");
    let txt = height_written(1.0, serde_json::json!({"DIMTXT": 0.5}));
    assert!(near(txt, 0.5), "a DIMTXT override sets the block text height: {txt}");
    let zero = height_written(0.0, serde_json::json!({}));
    assert!(near(zero, 0.18), "DIMSCALE 0 in model space is scale 1, not ~0: {zero}");
}

/// The group 1 (contents) values of every MTEXT in the BLOCKS section, in file order.
fn block_mtext_contents(dxf: &str) -> Vec<String> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let t: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    let blocks = t.iter().position(|&(c, v)| c == "2" && v == "BLOCKS").unwrap();
    let end_blocks = blocks + t[blocks..].iter().position(|&(c, v)| c == "0" && v == "ENDSEC").unwrap();
    let mut out = Vec::new();
    let mut i = blocks;
    while let Some(k) = t[i..end_blocks].iter().position(|&(c, v)| c == "0" && v == "MTEXT") {
        let start = i + k;
        let end = t[start + 1..end_blocks].iter().position(|&(c, _)| c == "0").map_or(end_blocks, |n| start + 1 + n);
        // Contents are group 1 (the last chunk) after any 250-char group 3 chunks.
        out.push(t[start + 1..end].iter().filter(|&&(c, _)| c == "3" || c == "1").map(|&(_, v)| v).collect());
        i = end;
    }
    out
}

/// A dimension whose style sets DIMPOST `post` and one decimal place for angles.
fn angular_drawing(kind: DimKind, points: [Vec3; 5], post: &str) -> Drawing {
    let mut d = Drawing::new_metric();
    d.dim_styles.iter_mut().for_each(|s| {
        s.post = post.into();
        s.angular_decimals = 1;
    });
    let [p13, p14, p15, p16, defpt] = points;
    let dm = Dimension {
        kind,
        defpt,
        text_mid: Vec3::ZERO,
        p13,
        p14,
        p15,
        p16,
        text: String::new(),
        style: "Standard".into(),
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

/// The block text of the drawing's one dimension, as written to DXF and to DWG.
fn dimension_block_text(d: &Drawing) -> (String, String) {
    let mut dxf = block_mtext_contents(&cadcraft_io::write_dxf(d));
    let dwg = cadcraft_io::write_with(d, "case.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let from_dwg = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let mut dwg = block_mtext_contents(&from_dwg);
    assert_eq!(dxf.len(), 1, "{dxf:?}");
    assert_eq!(dwg.len(), 1, "{dwg:?}");
    (dxf.remove(0), dwg.remove(0))
}

#[test]
fn dimpost_reaches_a_three_point_angular_dimension_block() {
    // Vertex p15 at the origin, legs through p13 (0°) and p14 (90°), the arc through defpt.
    let d = angular_drawing(
        DimKind::Angular3P,
        [Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, Vec3::ZERO, Vec3::new(3.0, 3.0, 0.0)],
        "<> m",
    );
    let (dxf, dwg) = dimension_block_text(&d);
    // The DXF is ASCII: the degree sign is its `\U+00B0` escape.
    assert_eq!(dxf, "90.0\\U+00B0 m");
    assert_eq!(dwg, "90.0° m");
}

#[test]
fn dimpost_reaches_a_two_line_angular_dimension_block() {
    // Lines p13-p14 (along +X) and defpt-p15 (45°), the arc through p16.
    let d = angular_drawing(
        DimKind::Angular,
        [Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(15.0, 15.0, 0.0), Vec3::new(4.0, 1.5, 0.0), Vec3::new(-5.0, -5.0, 0.0)],
        "<> m",
    );
    let (dxf, dwg) = dimension_block_text(&d);
    assert_eq!(dxf, "45.0\\U+00B0 m");
    assert_eq!(dwg, "45.0° m");
}
