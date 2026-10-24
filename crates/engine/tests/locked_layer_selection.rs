//! ERASE, MOVE, ROTATE and SCALE leave objects on locked layers out of "Select objects" and say how
//! many, as AutoCAD does ("10 found" then "6 were on a locked layer.").

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::json;

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({"points": [a, b]})).unwrap();
    Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap()
}

fn start(s: &Session, h: Handle) -> (f64, f64) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.x, l.a.y),
        k => panic!("not a line: {k:?}"),
    }
}

/// One line on layer 0 and `locked` lines on the locked layer L.
fn drawing(locked: usize) -> (Session, Handle, Vec<Handle>) {
    let mut s = Session::new();
    let free = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("layer.new", &json!({"name": "L"})).unwrap();
    s.execute("layer.current", &json!({"name": "L"})).unwrap();
    let held = (0..locked).map(|i| line(&mut s, [0.0, 5.0 + i as f64], [10.0, 5.0 + i as f64])).collect();
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    s.execute("layer.set", &json!({"name": "L", "locked": true})).unwrap();
    (s, free, held)
}

fn logged(s: &Session, line: &str) -> bool {
    s.log.iter().any(|m| m == line)
}

#[test]
fn move_all_says_one_was_on_a_locked_layer() {
    let (mut s, free, held) = drawing(1);
    s.cmdline("move").unwrap();
    s.cmdline("all").unwrap();
    assert!(logged(&s, "2 found, 1 total"), "{:?}", s.log);
    assert!(logged(&s, "1 was on a locked layer."), "{:?}", s.log);
    s.script("\n0,0 3,0\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(start(&s, free), (3.0, 0.0));
    assert_eq!(start(&s, held[0]), (0.0, 5.0));
}

#[test]
fn erase_all_says_how_many_were_on_a_locked_layer() {
    let (mut s, free, held) = drawing(2);
    s.cmdline("erase").unwrap();
    s.cmdline("all").unwrap();
    assert!(logged(&s, "3 found, 1 total"), "{:?}", s.log);
    assert!(logged(&s, "2 were on a locked layer."), "{:?}", s.log);
    s.cmdline("").unwrap();
    let d = s.doc().unwrap();
    assert!(d.entity(free).is_none());
    assert!(held.iter().all(|h| d.entity(*h).is_some()));
}

#[test]
fn rotate_and_scale_count_locked_objects_too() {
    for cmd in ["rotate", "scale"] {
        let (mut s, _, _) = drawing(1);
        s.cmdline(cmd).unwrap();
        s.cmdline("all").unwrap();
        assert!(logged(&s, "1 was on a locked layer."), "{cmd}: {:?}", s.log);
    }
}

/// COPY copies objects on locked layers, so they stay in the selection and nothing is said.
#[test]
fn copy_keeps_objects_on_locked_layers() {
    let (mut s, _, _) = drawing(1);
    s.cmdline("copy").unwrap();
    s.cmdline("all").unwrap();
    assert!(logged(&s, "2 found, 2 total"), "{:?}", s.log);
    assert!(!s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
}

/// Removing objects from the selection never reports locked ones.
#[test]
fn remove_does_not_count_locked_objects() {
    let (mut s, _, _) = drawing(1);
    s.cmdline("move").unwrap();
    s.cmdline("all").unwrap();
    s.log.clear();
    s.cmdline("r").unwrap();
    s.cmdline("all").unwrap();
    assert!(!s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
}

/// MIRROR erasing the source objects mirrors them in place, so the ones on locked layers stay
/// where they are, and the command line says so once the answer is Yes.
#[test]
fn mirror_erasing_sources_says_one_was_on_a_locked_layer() {
    let (mut s, free, held) = drawing(1);
    s.cmdline("mirror").unwrap();
    s.cmdline("all").unwrap();
    assert!(!s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
    s.script("\n0,-10 10,-10\ny\n").unwrap();
    assert!(s.running.is_none());
    assert!(logged(&s, "1 was on a locked layer."), "{:?}", s.log);
    assert_eq!(start(&s, free), (0.0, -20.0));
    assert_eq!(start(&s, held[0]), (0.0, 5.0));
}

/// Keeping the source objects, MIRROR copies the ones on locked layers too and says nothing.
#[test]
fn mirror_keeping_sources_copies_objects_on_locked_layers() {
    let (mut s, free, held) = drawing(2);
    s.cmdline("mirror").unwrap();
    s.cmdline("all").unwrap();
    s.script("\n0,-10 10,-10\nn\n").unwrap();
    assert!(s.running.is_none());
    assert!(!s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
    assert_eq!(start(&s, free), (0.0, 0.0));
    assert!(held.iter().all(|h| start(&s, *h).1 >= 5.0));
    let lines = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!(lines, 6);
}

/// Selected before the command (pickfirst), objects on locked layers are left out the same way:
/// the command line says so and the preview only drags the others.
#[test]
fn move_with_pickfirst_leaves_out_objects_on_locked_layers() {
    let (mut s, free, held) = drawing(2);
    s.set_selection(vec![free, held[0], held[1]]);
    s.cmdline("move").unwrap();
    assert!(logged(&s, "2 were on a locked layer."), "{:?}", s.log);
    assert_eq!(s.selection(), vec![free]);
    s.cmdline("0,0").unwrap();
    let drawn = s.preview(cadcraft_engine::geom::Vec2::new(3.0, 0.0));
    assert!(drawn.iter().any(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.y == 0.0 && l.a.x == 3.0)), "{drawn:?}");
    assert!(!drawn.iter().any(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.y >= 5.0)), "{drawn:?}");
    s.cmdline("3,0").unwrap();
    assert!(s.running.is_none());
    assert_eq!(start(&s, free), (3.0, 0.0));
    assert!(held.iter().enumerate().all(|(i, h)| start(&s, *h) == (0.0, 5.0 + i as f64)));
}

/// A pickfirst selection made only of objects on locked layers leaves nothing to act on, so the
/// command asks for objects.
#[test]
fn erase_with_only_locked_pickfirst_asks_for_objects() {
    let (mut s, _, held) = drawing(1);
    s.set_selection(held.clone());
    s.cmdline("erase").unwrap();
    assert!(logged(&s, "1 was on a locked layer."), "{:?}", s.log);
    assert!(s.running.is_some());
    assert!(s.selection().is_empty());
}

/// COPY keeps a pickfirst selection on locked layers whole.
#[test]
fn copy_with_pickfirst_keeps_objects_on_locked_layers() {
    let (mut s, free, held) = drawing(1);
    s.set_selection(vec![free, held[0]]);
    s.cmdline("copy").unwrap();
    assert!(!s.log.iter().any(|m| m.contains("locked layer")), "{:?}", s.log);
    assert_eq!(s.selection(), vec![free, held[0]]);
}
