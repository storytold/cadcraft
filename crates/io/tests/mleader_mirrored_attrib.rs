//! A multileader whose block content is scaled by −1 in X (read back from a DXF file) places its
//! attributes from the block's matrix alone: the length of the default value the ATTDEF carries
//! plays no part, and the text keeps its rotation because the block mirrors.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

/// The first attribute (insert, rotation) of the multileader's block content after a DXF round trip.
fn read_back(default: &str) -> (Vec3, f64) {
    let mut d = Drawing::new_metric();
    let mut blk = Block::new("BUBBLE");
    let tag_text = Text {
        insert: Vec3::new(10.0, 0.0, 0.0),
        align_pt: None,
        height: 5.0,
        value: default.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::default(),
        valign: VAlign::default(),
    };
    let attrib = |text: Text, prompt: &str| Attrib {
        tag: "NUM".into(),
        text,
        invisible: false,
        constant: false,
        prompt: prompt.into(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    blk.entities.push(Entity::new(Handle(0x201), EntityKind::AttDef(attrib(tag_text.clone(), "N"))));
    d.blocks.insert(blk.name.clone(), std::sync::Arc::new(blk));
    let mut placed = tag_text;
    placed.value = "42".into();
    let ins = Insert {
        block: "BUBBLE".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(-1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![attrib(placed, "")],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let m = MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 5.0, 0.0)]],
        landing: Vec3::new(10.0, 3.0, 0.0),
        dogleg: 1.0,
        text: None,
        style: "Standard".into(),
        arrow_size: 0.18,
        spline: false,
        block: Some(ins),
    };
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m)).unwrap();
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2007), binary: false };
    let text = String::from_utf8(cadcraft_io::write_with(&d, "x.dxf", &opts).unwrap()).unwrap();
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let ml = back.model.iter().find_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m.clone()) } else { None }).unwrap();
    let b = ml.block.unwrap();
    assert!(b.scale.x < 0.0, "the content is mirrored: {:?}", b.scale);
    let a = b.attribs.first().unwrap();
    assert_eq!(a.text.value, "42");
    (a.text.insert, a.text.rotation)
}

#[test]
fn mleader_attribute_of_a_mirrored_block_does_not_depend_on_the_definition_default() {
    for default in ["?", "a much much longer default value"] {
        let (at, rot) = read_back(default);
        assert!((at.x + 10.0).abs() < 1e-9 && at.y.abs() < 1e-9, "default {default:?}: attribute at {at:?}");
        assert!(rot.abs() < 1e-9, "default {default:?}: rotation {rot}");
    }
}
