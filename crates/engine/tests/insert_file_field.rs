//! `INSERT name=file` keeps the field of an attribute definition: the definition's extension
//! dictionary and the objects in it come along under new handles, so the attributes the block's
//! references get can carry the field when the drawing is saved.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::foreign::{ForeignExtra, ForeignObject};
use cadcraft_engine::doc::{Attrib, Drawing, Entity, EntityKind, Handle, RawTag, Text};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

fn tag(code: i32, value: &str) -> RawTag {
    RawTag { code, value: value.into() }
}

fn object(handle: u64, kind: &str, tags: Vec<RawTag>) -> ForeignObject {
    ForeignObject { handle: Handle(handle), kind: kind.into(), tags }
}

/// A drawing file in a test format: model space holds a definition NUMBER (handle 0x50) whose
/// extension dictionary 0x60 owns the ACAD_FIELD dictionary 0x61, which owns the field 0x62 with
/// a child field 0x63. Handles 0x61..0x63 stand for the file's own; a second, unrelated object
/// 0x70 is not part of that tree. The bytes `plain` give the same definition without any of it.
fn read(bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let mut d = Drawing::new_imperial();
    let text = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.0,
        value: "TEMPLATE".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    let def = Attrib {
        tag: "NUMBER".into(),
        text,
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    d.model.push(Entity::new(Handle(0x50), EntityKind::AttDef(def)));
    if bytes == b"plain" {
        return Ok(d);
    }
    let f = Arc::make_mut(&mut d.foreign);
    f.entities.insert(Handle(0x50), ForeignExtra { xdict: Some(Handle(0x60)), ..Default::default() });
    f.objects.push(object(0x60, "DICTIONARY", vec![tag(5, "60"), tag(330, "50"), tag(3, "ACAD_FIELD"), tag(360, "61")]));
    f.objects.push(object(0x61, "DICTIONARY", vec![tag(5, "61"), tag(330, "60"), tag(3, "TEXT"), tag(360, "62")]));
    f.objects.push(object(0x62, "FIELD", vec![tag(5, "62"), tag(330, "61"), tag(1, "_text"), tag(360, "63")]));
    f.objects.push(object(0x63, "FIELD", vec![tag(5, "63"), tag(330, "62"), tag(1, "AcVar")]));
    f.objects.push(object(0x70, "XRECORD", vec![tag(5, "70"), tag(330, "0")]));
    f.classes.push(vec![tag(1, "FIELD"), tag(2, "AcDbField")]);
    Ok(d)
}

fn hooks() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
}

#[test]
fn the_definition_of_an_inserted_file_keeps_its_field() {
    hooks();
    let dir = TempDir::new("insert-file-field");
    let path = dir.file("border.dxf", "");
    let mut s = Session::new();
    s.execute("insert", &json!({ "name": "BORDER", "file": path, "at": [0, 0], "attribs": { "NUMBER": "A-1" } })).unwrap();
    let d = s.doc().unwrap();
    let def = d.block("BORDER").and_then(|b| b.entities.iter().find(|e| matches!(e.kind, EntityKind::AttDef(_)))).expect("the definition");
    assert_ne!(def.handle, Handle(0x50), "the definition has a handle of this drawing");
    let root = d.foreign.entities.get(&def.handle).and_then(|x| x.xdict).expect("the definition has an extension dictionary");
    let tree = d.foreign.tree(root);
    assert_eq!(tree.len(), 4, "the dictionaries and both fields, not the unrelated object");
    assert!(tree.iter().all(|h| ![0x60, 0x61, 0x62, 0x63, 0x70].contains(&h.0)), "the copies have new handles");
    // The root's owner is the definition, the others point at each other.
    let owner =
        |h: Handle| d.foreign.objects.iter().find(|o| o.handle == h).and_then(|o| o.tags.iter().find(|t| t.code == 330)).map(|t| t.value.clone());
    assert_eq!(owner(tree[0]), Some(def.handle.hex()));
    assert_eq!(owner(tree[1]), Some(tree[0].hex()));
    assert_eq!(owner(tree[3]), Some(tree[2].hex()));
    assert!(d.foreign.classes.iter().any(|c| c.iter().any(|t| t.code == 1 && t.value == "FIELD")), "the field class comes along");
}

#[test]
fn a_definition_without_a_field_gets_none() {
    hooks();
    let dir = TempDir::new("insert-file-nofield");
    let path = dir.file("plain.dxf", "plain");
    let mut s = Session::new();
    s.execute("insert", &json!({ "name": "PLAIN", "file": path, "at": [0, 0] })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("PLAIN").is_some());
    assert!(d.foreign.is_empty(), "nothing is kept for a drawing without fields");
}
