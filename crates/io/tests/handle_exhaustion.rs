//! A hostile or damaged file can carry a handle at the top of the handle range: an object's own
//! handle, or its `$HANDSEED`. Nothing can be allocated above it, so every fresh handle came
//! back as that same top handle — and the store replaces an entity with the same handle, so the
//! next object drawn silently replaced the file's own object (#913).
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::{Common, Drawing, EntityKind, Group, Handle, Line, Space};
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

/// A group member whose entity the reader renumbered keeps its place in the group: the member
/// reference names the file's handle, and it follows the entity to the fresh one.
fn group_case(binary: bool) {
    let mut d = Drawing::new_metric();
    let line = |x| EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 1.0, 2.0, 0.0) });
    let a = d.add(&Space::Model, Common::default(), line(0.0)).unwrap();
    let b = d.add(&Space::Model, Common::default(), line(10.0)).unwrap();
    d.groups.push(Group { name: "Pair".into(), description: String::new(), selectable: true, members: vec![a, b] });
    let mut tags = cadcraft_dxf::parse(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    // A damaged file names both the entity and its group reference at the top of the range.
    for t in &mut tags {
        if matches!(t.code, 5 | 340) && t.str() == a.hex() {
            *t = cadcraft_dxf::Tag::s(t.code, TOP);
        }
    }
    let bytes = if binary { cadcraft_dxf::write_binary(&tags, false) } else { cadcraft_dxf::write_ascii(&tags).into_bytes() };
    let r = cadcraft_io::read_dxf(&bytes).unwrap();
    assert_eq!(r.model.len(), 2);
    let first = r.model.iter().find(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x == 0.0)).unwrap().handle;
    let second = r.model.iter().find(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x == 10.0)).unwrap().handle;
    assert_ne!(first, Handle::parse_hex(TOP).unwrap(), "the file's top handle was kept");
    let group = r.groups.iter().find(|g| g.name == "Pair").unwrap();
    assert_eq!(group.members, vec![first, second], "the group lost a member");
}

#[test]
fn a_renumbered_member_keeps_its_group_ascii() {
    group_case(false);
}

#[test]
fn a_renumbered_member_keeps_its_group_binary() {
    group_case(true);
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
    let handles: Vec<Handle> = r.model.iter().map(|e| e.handle).collect();
    let unique: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "repeated handles on reopen: {handles:?}");
}

#[test]
fn a_saturated_seed_refuses_to_save() {
    // A drawing whose seed is at the top of the range would write the same handle for every
    // object it still has to name: saving refuses instead of writing that file.
    let mut d = Drawing::new_metric();
    d.handseed = u64::MAX;
    let err = cadcraft_io::write(&d, "full.dxf").unwrap_err().to_string();
    assert!(err.contains("exhausted"), "{err}");
}
