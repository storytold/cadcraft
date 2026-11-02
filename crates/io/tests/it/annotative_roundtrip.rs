//! Annotative scaling survives DXF (#468): the scale list (ACAD_SCALELIST of SCALE objects), the
//! multileader style's annotative flag (group 296), and CADCraft's own record of CANNOSCALE,
//! ANNOALLVISIBLE, annotative blocks and each object's scales.

use std::sync::Arc;

use cadcraft_doc::{AnnoScale, Block, Common, Drawing, EntityKind, Insert, MLeaderStyle, Space, Text};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn text(value: &str) -> EntityKind {
    EntityKind::Text(Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.5,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    })
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.scales.push(AnnoScale::new("Site plan", 1.0, 250.0));
    d.header.set_str("CANNOSCALE", "1:50");
    d.header.set_i64("ANNOALLVISIBLE", 0);
    let anno = Common { anno_scales: vec![AnnoScale::new("1:50", 1.0, 50.0), AnnoScale::new("Site plan", 1.0, 250.0)], ..Default::default() };
    d.add(&Space::Model, anno, text("ROOM")).unwrap();
    d.add(&Space::Model, Common::default(), text("PLAIN")).unwrap();
    let mut b = Block::new("TAG");
    b.annotative = true;
    b.entities.push(cadcraft_doc::Entity::new(cadcraft_doc::Handle(0x9000), text("T")));
    d.blocks.insert("TAG".into(), Arc::new(b));
    let ins = Insert {
        block: "TAG".into(),
        insert: Vec3::new(10.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let on_paper = Common { anno_scales: vec![AnnoScale::unit()], ..Default::default() };
    d.add(&Space::Paper("Layout1".into()), on_paper, EntityKind::Insert(ins)).unwrap();
    d.mleader_styles.push(MLeaderStyle { name: "Callout".into(), annotative: true, ..Default::default() });
    d
}

fn scales_of(d: &Drawing, value: &str) -> Vec<String> {
    let e = d.model.iter().find(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == value)).unwrap();
    e.common.anno_scales.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn annotative_scaling_round_trips() {
    let d = drawing();
    let text = write_dxf(&d);
    assert!(text.contains("ACAD_SCALELIST") && text.contains("AcDbScale") && text.contains("CADCRAFT_ANNOTATION"));
    let r = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(r.scales, d.scales);
    assert_eq!(r.header.str("CANNOSCALE", ""), "1:50");
    assert_eq!(r.header.i64("ANNOALLVISIBLE", 1), 0);
    assert_eq!(scales_of(&r, "ROOM"), ["1:50", "Site plan"]);
    assert!(scales_of(&r, "PLAIN").is_empty());
    assert!(r.block("TAG").unwrap().annotative);
    let ins = r.layouts.iter().flat_map(|l| l.entities.iter()).find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap();
    assert_eq!(ins.common.anno_scales, [AnnoScale::unit()]);
    assert!(r.mleader_styles.iter().find(|s| s.name == "Callout").unwrap().annotative);
    assert!(!r.mleader_styles.iter().find(|s| s.name == "Standard").unwrap().annotative);
}

#[test]
fn plain_drawings_write_no_annotation_record_and_keep_their_default_list() {
    let d = Drawing::new_metric();
    let text = write_dxf(&d);
    assert!(!text.contains("CADCRAFT_ANNOTATION"));
    let r = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(r.scales, cadcraft_doc::metric_scales());
    assert_eq!(read_dxf(write_dxf(&Drawing::new_imperial()).as_bytes()).unwrap().scales, cadcraft_doc::imperial_scales());
}

#[test]
fn hostile_scales_and_records_are_ignored() {
    let text = write_dxf(&drawing());
    // A SCALE with zero paper units, and a broken annotation record.
    let bad = text.replacen("Site plan\r\n140\r\n1.0\r\n", "Site plan\r\n140\r\n0.0\r\n", 1);
    assert_ne!(bad, text, "the scale was written as expected");
    let r = read_dxf(bad.as_bytes()).unwrap();
    assert!(r.anno_scale("Site plan").is_none() && r.anno_scale("1:50").is_some());
    let start = text.find("{\"version\":1,\"cannoscale\"").unwrap();
    for junk in [
        "{\"version\":2}",
        "{\"version\":1,\"objects\":[{\"handle\":\"ZZ\",\"scales\":[{\"name\":\"x\",\"paperUnits\":-1,\"drawingUnits\":1}]}]}",
        "[[[[",
    ] {
        let end = start + text[start..].find("\r\n").unwrap();
        let t = format!("{}{}{}", &text[..start], junk, &text[end..]);
        let r = read_dxf(t.as_bytes()).unwrap();
        assert!(scales_of(&r, "ROOM").is_empty(), "{junk}");
    }
}
