//! TEXT generation flags (71: backwards, upside down) and the MTEXT Text frame (90 bit 16)
//! round-trip through DXF.

use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, MText, Space, Text, VAlign};
use cadcraft_geom::Vec3;

#[test]
fn text_flags_and_mtext_frame_roundtrip() {
    let mut d = Drawing::new_metric();
    let t = Text {
        insert: Vec3::new(1.0, 2.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: "AB".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
        backwards: true,
        upside_down: true,
        formula: Some("=1+1".into()),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Text(t)).unwrap();
    let m = MText {
        formula: None,
        frame: true,
        insert: Vec3::new(0.0, 10.0, 0.0),
        height: 2.5,
        width: 40.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "framed".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(m)).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let kinds: Vec<EntityKind> = back.model.iter().map(|e| e.kind.clone()).collect();
    assert!(kinds.iter().any(|k| matches!(k, EntityKind::Text(t) if t.backwards && t.upside_down && t.formula.as_deref() == Some("=1+1"))));
    assert!(kinds.iter().any(|k| matches!(k, EntityKind::MText(m) if m.frame)));
}
