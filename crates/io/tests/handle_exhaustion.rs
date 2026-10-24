//! A hostile or damaged file can carry a handle at the top of the handle range: an object's own
//! handle, or its `$HANDSEED`. Nothing can be allocated above it, so every fresh handle came
//! back as that same top handle — and the store replaces an entity with the same handle, so the
//! next object drawn silently replaced the file's own object (#913).
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::{Common, Drawing, EntityKind, Handle, Line, Space};
use cadcraft_geom::Vec3;

const TOP: &str = "FFFFFFFFFFFFFFFF";

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

#[test]
fn an_object_at_the_top_of_the_handle_range_is_not_replaced_by_a_new_one() {
    let mut d = read(&dxf(Some(TOP), TOP));
    assert_eq!(d.model.len(), 1);
    // Draw a line: a fresh handle, then the entity.
    d.add(&Space::Model, Common::default(), line(10.0, 10.0)).unwrap();
    assert_eq!(d.model.len(), 2, "the file's line was replaced by the new one");
    let handles: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
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
