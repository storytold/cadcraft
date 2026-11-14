//! A hostile or damaged file can name a handle at the top of the range: an object's own handle,
//! or its `$HANDSEED`. A file's handles keep their identity, so a new object must take a free
//! handle instead of replacing the file's own object (#913).
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::{Common, DimKind, Dimension, Drawing, Entity, EntityKind, Handle, Line, Space};
use cadcraft_geom::Vec3;

const TOP: &str = "FFFFFFFFFFFFFFFF";
const NEAR_TOP: &str = "FFFFFFFFFFFEFFFF";

fn read(text: &str) -> Drawing {
    cadcraft_io::read_dxf(text.as_bytes()).unwrap()
}

fn line(x: f64, y: f64) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(x, y, 0.0), b: Vec3::new(x + 1.0, y, 0.0) })
}

/// One line, with `entity_handle` when given, under a `$HANDSEED` of `seed`.
fn dxf(entity_handle: Option<&str>, seed: &str) -> String {
    let handle = entity_handle.map(|h| format!("5\n{h}\n")).unwrap_or_default();
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n{seed}\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nLINE\n{handle}8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n1.0\n21\n0.0\n31\n0.0\n0\nENDSEC\n0\nEOF\n"
    )
}

fn distinct(d: &Drawing) -> Vec<Handle> {
    d.model.iter().map(|e| e.handle).collect()
}

#[test]
fn an_object_at_the_top_of_the_handle_range_is_not_replaced_by_a_new_one() {
    let mut d = read(&dxf(Some(TOP), TOP));
    assert_eq!(d.model.len(), 1);
    // The file's handle is its own; the new object takes a free handle below it.
    assert!(d.entity(Handle::parse_hex(TOP).unwrap()).is_some(), "the file's handle was renumbered");
    d.add(&Space::Model, Common::default(), line(10.0, 10.0)).unwrap();
    assert_eq!(d.model.len(), 2, "the file's line was replaced by the new one");
    let handles = distinct(&d);
    let unique: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "duplicate handles: {handles:?}");
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Line(ref l) if l.a.x == 0.0)), "the file's line is gone");
}

#[test]
fn a_seed_at_the_top_of_the_handle_range_cannot_be_followed_twice() {
    let mut d = read(&dxf(None, TOP));
    let a = d.add(&Space::Model, Common::default(), line(0.0, 10.0)).unwrap();
    let b = d.add(&Space::Model, Common::default(), line(0.0, 20.0)).unwrap();
    assert_ne!(a, b, "two objects were given the same handle");
    assert_eq!(d.model.len(), 3, "the second line replaced the first");
}

#[test]
fn an_ordinary_high_handle_keeps_its_identity() {
    // Far below the top of the range a file's handles are its own: they stay as they are.
    let h = "FFFFFFFF";
    let mut d = read(&dxf(Some(h), h));
    assert!(d.entity(Handle::parse_hex(h).unwrap()).is_some(), "the file's handle was renumbered");
    let n = d.add(&Space::Model, Common::default(), line(0.0, 5.0)).unwrap();
    assert!(n.0 > Handle::parse_hex(h).unwrap().0, "a new handle below the file's own");
}

#[test]
fn a_handle_just_below_the_top_survives_a_save() {
    // Main keeps a file's handles exactly as written, even near the top of the range: handles
    // are how other objects and other programs refer to an entity (#939's round-trip tests).
    let mut d = read(&dxf(Some(NEAR_TOP), NEAR_TOP));
    let n = d.add(&Space::Model, Common::default(), line(0.0, 5.0)).unwrap();
    let bytes = cadcraft_io::write(&d, "near-top.dxf").unwrap();
    let r = cadcraft_io::read_dxf(&bytes).unwrap();
    assert!(r.entity(Handle::parse_hex(NEAR_TOP).unwrap()).is_some(), "the file's handle did not survive the save");
    assert!(r.entity(n).is_some(), "the new handle did not survive the save");
}

#[test]
fn nine_objects_drawn_after_a_saturated_seed_save_and_reopen_distinct() {
    // The nine-entity case from #2237: a saturated $HANDSEED, ordinary entity handles. Drawing
    // after the load gives every object its own handle, and a save/reopen keeps them apart.
    let mut d = read(&dxf(None, TOP));
    for i in 0..9 {
        d.add(&Space::Model, Common::default(), line(f64::from(i), 40.0)).unwrap();
    }
    let bytes = cadcraft_io::write(&d, "nine.dxf").unwrap();
    let r = cadcraft_io::read_dxf(&bytes).unwrap();
    assert_eq!(r.model.len(), 10, "the file's line and the nine drawn ones");
    let handles = distinct(&r);
    let unique: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "repeated handles on reopen: {handles:?}");
}

/// The first handle in the reserved band above [`HANDLE_LIMIT`].
const BAND: &str = "FFFFFFFF00000000";

#[test]
fn a_seed_in_the_reserved_band_does_not_hand_out_the_files_handle() {
    // #913's remaining case: a file says its next handle is FFFFFFFF00000000 and an object holds
    // that handle. Following the seed once would hand the object's handle to the new object, and
    // `EntityStore` would replace the file's line.
    let mut d = read(&dxf(Some(BAND), BAND));
    assert_eq!(d.model.len(), 1);
    let n = d.add(&Space::Model, Common::default(), line(10.0, 10.0)).unwrap();
    assert_ne!(n, Handle::parse_hex(BAND).unwrap(), "the new object took the file object's handle");
    assert_eq!(d.model.len(), 2, "the file's line was replaced");
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Line(ref l) if l.a.x == 0.0)), "the file's line is gone");

    let bytes = cadcraft_io::write(&d, "band-seed.dxf").unwrap();
    let r = cadcraft_io::read_dxf(&bytes).unwrap();
    let handles = distinct(&r);
    let unique: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "repeated handles on reopen: {handles:?}");
    assert_eq!(r.model.len(), 2, "a line did not survive the save");
}

#[test]
fn a_save_does_not_give_a_structural_record_an_objects_handle() {
    // The writer numbers its own records from `seed + 0x1000`; with the seed at the bottom of the
    // reserved band that lands on the file's own handle. The file must keep one record per handle.
    let object = "FFFFFFFF00001000";
    let d = read(&dxf(Some(object), "FFFFFFFEFFFFFFFF"));
    let bytes = cadcraft_io::write(&d, "band-structural.dxf").unwrap();
    let tags = craft_dxf::parse(&bytes).unwrap();
    // Record handles start with the TABLES section; before it the HEADER's `$HANDSEED` is the
    // next free handle, not a record's.
    let after_header = tags.iter().position(|t| t.code == 2 && t.str() == "TABLES").unwrap();
    let written: Vec<String> = tags[after_header..].iter().filter(|t| t.code == 5 || t.code == 105).map(|t| t.str().to_string()).collect();
    let unique: std::collections::HashSet<&String> = written.iter().collect();
    assert_eq!(unique.len(), written.len(), "the save repeats a handle: {written:?}");
    assert_eq!(written.iter().filter(|h| h.as_str() == object).count(), 1, "the object's handle was not written exactly once");

    let r = cadcraft_io::read_dxf(&bytes).unwrap();
    assert!(r.entity(Handle::parse_hex(object).unwrap()).is_some(), "the file's object is gone");
}

/// A CADCraft-marked file with a handle-less block entity and an `ENTITIES` record that names
/// the handle the reader would give it. The marker means the reader skips the handle pre-scan,
/// so the two sections must still reserve against each other.
fn marked_file_with_handle_less_block_entity() -> String {
    "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n1000\n0\nENDSEC\n\
     0\nSECTION\n2\nBLOCKS\n\
     0\nBLOCK\n8\n0\n2\n*MODEL_SPACE\n70\n0\n10\n0.0\n20\n0.0\n30\n0.0\n3\n*MODEL_SPACE\n1\n\n\
     0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n1.0\n21\n0.0\n31\n0.0\n\
     0\nENDBLK\n8\n0\n\
     0\nENDSEC\n\
     0\nSECTION\n2\nENTITIES\n\
     0\nLINE\n5\n100\n8\n0\n10\n10.0\n20\n0.0\n30\n0.0\n11\n11.0\n21\n0.0\n31\n0.0\n\
     0\nENDSEC\n\
     0\nSECTION\n2\nOBJECTS\n\
     0\nXRECORD\n5\n300\n100\nAcDbXrecord\n3\nCADCRAFT_HANDSEED\n1\n1000\n\
     0\nENDSEC\n0\nEOF\n"
        .to_string()
}

#[test]
fn a_marked_files_sections_reserve_handles_against_each_other() {
    // The block entity has no handle of its own, so the reader gives it one; a later ENTITIES
    // record must not be given the same handle, or one entity silently replaces the other.
    let d = read(&marked_file_with_handle_less_block_entity());
    assert_eq!(d.model.len(), 2, "one entity replaced the other");
    let handles = distinct(&d);
    let unique: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "duplicate handles: {handles:?}");
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Line(ref l) if l.a.x == 0.0)), "the block entity is gone");
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Line(ref l) if l.a.x == 10.0)), "the ENTITIES line is gone");
}

#[test]
fn generated_geometry_never_takes_a_handle_an_object_holds() {
    // The drawing's own handles sit in the reserved band and the seed is at the top of the
    // range, so the writer's structural numbering wraps into the band. Every other band handle
    // is taken, so without a check the generated dimension block's entities land on an occupied
    // one: the saved file would repeat a handle.
    let mut d = Drawing::new_imperial();
    d.handseed = u64::MAX - 1;
    {
        let model = d.space_mut(&Space::Model).unwrap();
        for i in 0..200u64 {
            let handle = Handle(cadcraft_doc::HANDLE_LIMIT + 1 + 2 * i);
            model.push(Entity { handle, common: Common::default(), kind: line(i as f64, 0.0) });
        }
        model.push(Entity {
            handle: Handle(0x200),
            common: Common::default(),
            kind: EntityKind::Dimension(Dimension {
                kind: DimKind::Linear { rotation: 0.0 },
                defpt: Vec3::new(0.0, 1.0, 0.0),
                text_mid: Vec3::new(1.0, 1.0, 0.0),
                p13: Vec3::ZERO,
                p14: Vec3::new(2.0, 0.0, 0.0),
                p15: Vec3::ZERO,
                p16: Vec3::ZERO,
                text: String::new(),
                style: "Standard".into(),
                measurement: 2.0,
                text_rotation: 0.0,
                user_text_pos: false,
                block: None,
                insertion_point: Vec3::ZERO,
                overrides: serde_json::Map::new(),
                assoc: Vec::new(),
                marks: Default::default(),
            }),
        });
    }
    let bytes = cadcraft_io::write(&d, "generated.dxf").unwrap();
    let tags = craft_dxf::parse(&bytes).unwrap();
    let after_header = tags.iter().position(|t| t.code == 2 && t.str() == "TABLES").unwrap();
    let written: Vec<String> = tags[after_header..].iter().filter(|t| t.code == 5 || t.code == 105).map(|t| t.str().to_string()).collect();
    let unique: std::collections::HashSet<&String> = written.iter().collect();
    assert_eq!(unique.len(), written.len(), "the save repeats a handle");
}
