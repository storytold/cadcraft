//! A reference's attributes take Lock position from their definitions; Verify and Preset stay
//! with the definitions (as AutoCAD saves them).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Attrib, AttribModes, Block, Entity, EntityKind, Text};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

#[test]
fn references_take_lock_position_from_the_definition() {
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let text = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.0,
        value: "X".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    let modes = AttribModes { verify: true, preset: true, lock_position: true };
    let def = Attrib {
        tag: "MARK".into(),
        text,
        invisible: false,
        constant: false,
        prompt: "Mark".into(),
        props: Default::default(),
        multiline: None,
        modes,
    };
    let mut b = Block::new("TAG");
    let h = d.new_handle();
    b.entities.push(Entity::new(h, EntityKind::AttDef(def)));
    d.blocks.insert("TAG".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "tags", None);
    let r = s.execute("insert", &json!({ "name": "TAG", "at": [10, 5] })).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let Some(EntityKind::Insert(i)) = s.doc().unwrap().entity(h).map(|e| e.kind.clone()) else { panic!("no reference") };
    assert_eq!(i.attribs[0].modes, AttribModes { verify: false, preset: false, lock_position: true });
}
