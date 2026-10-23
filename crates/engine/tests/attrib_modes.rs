//! A reference's attributes take Lock position from their definitions; Verify and Preset stay
//! with the definitions (as AutoCAD saves them).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Attrib, AttribModes, Block, Entity, EntityKind, MText, MultilineAttrib, Text};
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
    let modes = AttribModes { verify: true, preset: true, lock_position: true, hidden: false };
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
    assert_eq!(i.attribs[0].modes, AttribModes { verify: false, preset: false, lock_position: true, hidden: false });
}

/// A definition at `x` with `modes`, invisible or not, multiline or not.
fn definition(tag: &str, x: f64, modes: AttribModes, invisible: bool, multiline: bool) -> Attrib {
    let text = Text {
        insert: Vec3::new(x, 0.0, 0.0),
        align_pt: None,
        height: 2.0,
        value: tag.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    let multiline = multiline.then(|| {
        let mtext = MText {
            insert: Vec3::new(x, 0.0, 0.0),
            height: 2.0,
            width: 0.0,
            attach: 7,
            rotation: 0.0,
            style: "Standard".into(),
            contents: format!("{tag}\\PTWO"),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        MultilineAttrib { mtext, read_as: format!("{tag}\nTWO") }
    });
    Attrib { tag: tag.into(), text, invisible, constant: false, prompt: tag.into(), props: Default::default(), multiline, modes }
}

#[test]
fn only_shown_unlocked_attributes_have_grips_and_a_multiline_one_moves_its_text() {
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let locked = AttribModes { lock_position: true, ..AttribModes::default() };
    let mut b = Block::new("TAG");
    for a in [
        definition("FREE", 0.0, AttribModes::default(), false, false),
        definition("LOCKED", 10.0, locked, false, false),
        definition("HIDDEN", 20.0, AttribModes::default(), true, false),
        definition("NOTE", 30.0, AttribModes::default(), false, true),
    ] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::AttDef(a)));
    }
    d.blocks.insert("TAG".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "tags", None);
    let r = s.execute("insert", &json!({ "name": "TAG", "at": [100, 0] })).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    // The insertion point, FREE and NOTE: no grip for the locked or the invisible attribute.
    let g = s.grips_of(h).unwrap();
    assert_eq!(g.iter().map(|p| p.x).collect::<Vec<_>>(), [100.0, 100.0, 130.0]);
    // NOTE's grip (index 2) dragged up 5: its multiline text moves with it.
    s.execute("grip.stretch", &json!({ "handle": h.hex(), "index": 2, "to": [130, 5] })).unwrap();
    let Some(EntityKind::Insert(i)) = s.doc().unwrap().entity(h).map(|e| e.kind.clone()) else { panic!("no reference") };
    let note = i.attribs.iter().find(|a| a.tag == "NOTE").unwrap();
    assert_eq!((note.text.insert.y, note.multiline.as_ref().unwrap().mtext.insert.y), (5.0, 5.0));
    let free = i.attribs.iter().find(|a| a.tag == "FREE").unwrap();
    assert_eq!(free.text.insert.y, 0.0, "the others stay");
}
