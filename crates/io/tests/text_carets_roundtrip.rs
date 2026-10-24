//! Carets in TEXT, ATTDEF and DIMENSION strings are written as `^ ` and read back: AutoCAD
//! reads a bare `^2` in `10^2` as a control character.

use cadcraft_doc::{Attrib, Common, DimKind, Dimension, Drawing, EntityKind, HAlign, Space, Text, VAlign};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn text(value: &str) -> Text {
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

fn dimension(label: &str) -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: label.into(),
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

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Text(text("10^2 m"))).unwrap();
    let attdef = Attrib {
        tag: "AREA".into(),
        text: text("1^3"),
        invisible: false,
        constant: false,
        prompt: "Area in m^2".into(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::AttDef(attdef)).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dimension("<> x10^3"))).unwrap();
    d
}

fn strings(d: &Drawing) -> Vec<String> {
    d.model
        .iter()
        .flat_map(|e| match &e.kind {
            EntityKind::Text(t) => vec![t.value.clone()],
            EntityKind::AttDef(a) => vec![a.text.value.clone(), a.prompt.clone()],
            EntityKind::Dimension(dm) => vec![dm.text.clone()],
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn carets_are_written_escaped_and_read_back() {
    let dxf = write_dxf(&drawing());
    for escaped in ["10^ 2 m", "1^ 3", "Area in m^ 2", "<> x10^ 3"] {
        assert!(dxf.contains(escaped), "{escaped} written with the caret escaped");
    }
    let back = read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(strings(&back), ["10^2 m", "1^3", "Area in m^2", "<> x10^3"]);
}

#[test]
fn bare_carets_from_older_files_are_kept() {
    let dxf = write_dxf(&drawing()).replace("10^ 2 m", "10^2 m");
    let back = read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(strings(&back).first().map(String::as_str), Some("10^2 m"));
}
