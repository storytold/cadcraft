//! A block with a multiline attribute definition: its references get the multiline text placed
//! with them, it moves with them, and an attribute edit replaces it.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Attrib, Block, Common, Entity, EntityKind, Handle, MText, MultilineAttrib, Text};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

/// A session whose block TAG has a multiline definition LABEL ("SITE" / "NAME") at (10, 3).
fn session() -> Session {
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let text = Text {
        insert: Vec3::new(2.0, 2.0, 0.0),
        align_pt: None,
        height: 2.0,
        value: "SITE\nNAME".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    let mtext = MText {
        insert: Vec3::new(10.0, 3.0, 0.0),
        height: 2.0,
        width: 0.0,
        attach: 8,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "SITE\\PNAME".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let def = Attrib {
        tag: "LABEL".into(),
        text,
        invisible: false,
        constant: false,
        prompt: "Site".into(),
        props: Default::default(),
        multiline: Some(MultilineAttrib { mtext, read_as: "SITE\nNAME".into() }),
        modes: Default::default(),
    };
    let mut b = Block::new("TAG");
    let h = d.new_handle();
    b.entities.push(Entity { handle: h, common: Common::default(), kind: EntityKind::AttDef(def) });
    d.blocks.insert("TAG".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "tags", None);
    s
}

fn label(s: &Session, h: Handle) -> Attrib {
    match s.doc().unwrap().entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => i.attribs[0].clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn references_get_the_multiline_text_and_carry_it() {
    let mut s = session();
    let r = s.execute("insert", &json!({ "name": "TAG", "at": [10, 5] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let a = label(&s, h);
    assert_eq!(a.text.value, "SITE\nNAME");
    let m = a.multiline_text().expect("multiline");
    assert_eq!((m.insert.x, m.insert.y, m.contents.as_str()), (20.0, 8.0, "SITE\\PNAME"));

    // MOVE carries it.
    s.execute("move", &json!({ "handles": [h.hex()], "delta": [5, 0] })).unwrap();
    let m = label(&s, h).multiline_text().unwrap();
    assert_eq!((m.insert.x, m.insert.y), (25.0, 8.0));

    // An edited value replaces the multiline text; ATTSYNC keeps the value and the text.
    s.execute("attedit", &json!({ "handle": h.hex(), "values": { "label": "WELL 7" } })).unwrap();
    assert_eq!(label(&s, h).multiline_text().map(|m| m.contents), Some("WELL 7".to_string()));
    s.execute("attsync", &json!({ "name": "TAG" })).unwrap();
    let a = label(&s, h);
    assert_eq!(a.text.value, "WELL 7");
    assert_eq!(a.multiline_text().map(|m| (m.contents, m.insert.x)), Some(("WELL 7".to_string(), 25.0)));
}

#[test]
fn a_value_given_on_insert_replaces_the_definition_text() {
    let mut s = session();
    let r = s.execute("insert", &json!({ "name": "TAG", "at": [0, 0], "attribs": { "LABEL": "NORTH" } })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    assert_eq!(label(&s, h).multiline_text().map(|m| m.contents), Some("NORTH".to_string()));
}
