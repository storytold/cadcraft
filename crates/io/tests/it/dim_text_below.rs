//! #1055: a dimension's `*D` block (what AutoCAD and other readers draw) holds the text after
//! `\X` as a second MTEXT below the dimension line.

use cadcraft_doc::{Common, DimKind, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;

#[test]
fn the_text_below_the_dimension_line_is_a_second_mtext() {
    let mut d = Drawing::new_metric();
    let dm = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(50.0, 20.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(100.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: "30 MIN.\\X50 MAX.".into(),
        style: "ISO-25".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    let dxf = cadcraft_io::write_dxf(&d);
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    let blocks = pairs.iter().position(|(c, v)| *c == "2" && *v == "BLOCKS").unwrap();
    let start = blocks + pairs[blocks..].iter().position(|(c, v)| *c == "2" && v.starts_with("*D")).unwrap();
    let end = start + pairs[start..].iter().position(|(c, v)| *c == "0" && *v == "ENDBLK").unwrap();
    let (mut texts, mut kind, mut y) = (Vec::new(), "", 0.0);
    for (c, v) in &pairs[start..end] {
        match *c {
            "0" => kind = v,
            "20" if kind == "MTEXT" => y = v.parse().unwrap(),
            "1" if kind == "MTEXT" => texts.push((v.to_string(), y)),
            _ => {}
        }
    }
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert_eq!((texts[0].0.as_str(), texts[1].0.as_str()), ("30 MIN.", "50 MAX."));
    assert!(texts[0].1 > 20.0 && texts[1].1 < 20.0, "above and below the line at y = 20: {texts:?}");
}
