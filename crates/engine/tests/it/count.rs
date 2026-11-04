//! COUNT across model space, layouts and nested blocks; objects like a picked one; COUNTLIST and
//! COUNTTABLE (#563).

use std::sync::Arc;
use std::time::Instant;

use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Circle, Common, Entity, EntityKind, Handle, Insert, Space};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn insert(block: &str, x: f64, cols: u32, rows: u32) -> EntityKind {
    EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(x, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols,
        rows,
        col_spacing: 5.0,
        row_spacing: 5.0,
    })
}

fn circle(x: f64, r: f64) -> EntityKind {
    EntityKind::Circle(Circle { center: Vec3::new(x, 0.0, 0.0), radius: r })
}

/// Define block `name` holding `kinds`.
fn define(s: &mut Session, name: &str, kinds: Vec<EntityKind>) {
    let d = s.doc_mut().unwrap();
    let mut b = Block::new(name);
    for k in kinds {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, k));
    }
    d.blocks.insert(name.into(), Arc::new(b));
}

fn put(s: &mut Session, space: &Space, k: EntityKind) -> Handle {
    s.doc_mut().unwrap().add(space, Common::default(), k).unwrap()
}

fn layout() -> Space {
    Space::Paper("Layout1".into())
}

/// BOLT is a circle; PLATE holds four BOLTs. Model space: two PLATEs and one BOLT; Layout1: one
/// PLATE.
fn plates() -> Session {
    let mut s = Session::new();
    define(&mut s, "BOLT", vec![circle(0.0, 1.0)]);
    define(&mut s, "PLATE", (0..4).map(|i| insert("BOLT", f64::from(i), 1, 1)).collect());
    put(&mut s, &Space::Model, insert("PLATE", 0.0, 1, 1));
    put(&mut s, &Space::Model, insert("PLATE", 50.0, 1, 1));
    put(&mut s, &Space::Model, insert("BOLT", 100.0, 1, 1));
    put(&mut s, &layout(), insert("PLATE", 0.0, 1, 1));
    s
}

#[test]
fn counts_nested_references_in_the_current_space() {
    let mut s = plates();
    let r = s.execute("count", &json!({})).unwrap();
    assert_eq!(r["blocks"], json!({"BOLT": 9, "PLATE": 2}), "{r}");
    assert_eq!(r["nested"], json!({"BOLT": 8}), "{r}");
    assert_eq!(r["message"], "Block references in model space:\n  BOLT: 9 (8 nested)\n  PLATE: 2\n  Total: 11");
    let r = s.execute("count", &json!({"block": "bolt"})).unwrap();
    assert_eq!((r["count"].clone(), r["nested"].clone()), (json!(9), json!(8)), "{r}");
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(3), "both plates and the bolt hold a BOLT: {r}");
    assert_eq!(r["message"], "Block BOLT: 9 in model space (8 nested).");
    // Top-level references only.
    let r = s.execute("count", &json!({"nested": false})).unwrap();
    assert_eq!(r["blocks"], json!({"BOLT": 1, "PLATE": 2}), "{r}");
}

#[test]
fn counts_layouts_and_all_spaces() {
    let mut s = plates();
    let r = s.execute("count", &json!({"space": "layout1"})).unwrap();
    assert_eq!(r["blocks"], json!({"BOLT": 4, "PLATE": 1}), "{r}");
    assert!(r["message"].as_str().unwrap().starts_with("Block references in layout Layout1:"), "{r}");
    let r = s.execute("count", &json!({"space": "all"})).unwrap();
    assert_eq!(r["blocks"], json!({"BOLT": 13, "PLATE": 3}), "{r}");
    assert_eq!(r["spaces"]["Model"], json!({"BOLT": 9, "PLATE": 2}), "{r}");
    assert_eq!(r["spaces"]["Layout1"], json!({"BOLT": 4, "PLATE": 1}), "{r}");
    // COUNTLIST lists every space by default.
    let r = s.execute("countlist", &json!({})).unwrap();
    assert_eq!(r["total"], 16, "{r}");
    assert!(s.execute("count", &json!({"space": "NoSuchLayout"})).is_err());
    assert!(s.execute("count", &json!({"space": 7})).is_err());
}

#[test]
fn counts_minsert_copies_and_selections() {
    let mut s = plates();
    let grid = put(&mut s, &Space::Model, insert("BOLT", 200.0, 3, 2));
    let r = s.execute("count", &json!({"block": "BOLT"})).unwrap();
    assert_eq!(r["count"], 15, "a 3 × 2 MINSERT is six bolts: {r}");
    let r = s.execute("count", &json!({"handles": [grid.hex()]})).unwrap();
    assert_eq!(r["blocks"], json!({"BOLT": 6}), "{r}");
    assert!(r["message"].as_str().unwrap().contains("the selection"), "{r}");
}

#[test]
fn counts_objects_like_a_picked_one_and_highlights_them() {
    let mut s = Session::new();
    let a = put(&mut s, &Space::Model, circle(0.0, 1.0));
    put(&mut s, &Space::Model, circle(10.0, 1.0));
    put(&mut s, &Space::Model, circle(20.0, 1.0));
    put(&mut s, &Space::Model, circle(30.0, 2.0));
    let r = s.execute("count", &json!({"object": a.hex(), "highlight": true})).unwrap();
    assert_eq!(r["count"], 3, "the larger circle is not the same shape: {r}");
    assert_eq!(s.selection().len(), 3);
    assert!(s.execute("count", &json!({"object": "zz"})).is_err());
    assert!(s.execute("count", &json!({"object": "FFFFFF"})).is_err());
}

#[test]
fn count_at_the_command_line() {
    let mut s = plates();
    let plate = s.doc().unwrap().model.iter().find(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "PLATE")).unwrap().handle;
    s.cmdline("count").unwrap();
    s.input(Input::Pick(vec![plate])).unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Block PLATE: 2 in model space."));
    assert_eq!(s.selection().len(), 2, "both plates are highlighted");
    s.cmdline("count").unwrap();
    s.cmdline("s").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("  Total: 16"));
    assert!(s.running.is_none());
}

#[test]
fn counttable_inserts_a_table() {
    let mut s = plates();
    let r = s.execute("counttable", &json!({"at": [0, -20]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Table(t) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not a table") };
    let text: Vec<Vec<&str>> = t.cells.iter().map(|row| row.iter().map(|c| c.text.as_str()).collect()).collect();
    assert!(text.contains(&vec!["BOLT", "9"]) && text.contains(&vec!["PLATE", "2"]), "{text:?}");
    s.execute("undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().entity(h).is_none(), "one undo step removes the table");
    assert!(Session::new().execute("counttable", &json!({"at": [0, 0]})).is_err(), "nothing to count");
}

/// Self-referencing blocks and a ten-way fan-out twenty levels deep finish at once and never
/// overflow.
#[test]
fn hostile_nesting_is_bounded() {
    let mut s = Session::new();
    define(&mut s, "LOOP", vec![insert("LOOP", 0.0, 1, 1)]);
    for level in 0..20 {
        define(&mut s, &format!("L{level}"), (0..10).map(|i| insert(&format!("L{}", level + 1), f64::from(i), 1000, 1000)).collect());
    }
    put(&mut s, &Space::Model, insert("LOOP", 0.0, 1, 1));
    put(&mut s, &Space::Model, insert("L0", 0.0, u32::MAX, u32::MAX));
    let t = Instant::now();
    let r: Value = s.execute("count", &json!({"space": "all"})).unwrap();
    assert!(t.elapsed().as_secs() < 5);
    assert_eq!(r["blocks"]["L15"], json!(u64::MAX), "counts saturate: {r}");
    assert!(r["blocks"]["LOOP"].as_u64().unwrap() >= 1, "{r}");
    assert!(r["blocks"].get("L16").is_none(), "nesting stops at MAX_BLOCK_DEPTH: {r}");
}

/// #1440: COUNT says which objects it scanned, which it counted, and why the rest were skipped.
#[test]
fn self_check_says_what_count_covered() {
    let mut s = plates();
    put(&mut s, &Space::Model, circle(300.0, 1.0));
    let r = s.execute("count", &json!({})).unwrap();
    let sc = &r["selfCheck"];
    assert_eq!(sc["schema"], 1);
    assert_eq!(sc["found"], 4, "three block references and one circle: {r}");
    assert_eq!(sc["analyzed"], 3);
    assert_eq!(sc["skipped"]["Circle"], json!({ "count": 1, "reason": "not a block reference" }));
    assert_eq!(sc["caps"], json!({}));
    assert_eq!(sc["implausible"], json!([]));

    // Counting the circles compares only circles, and says so.
    let h = put(&mut s, &Space::Model, circle(310.0, 1.0));
    let r = s.execute("count", &json!({"object": h.hex()})).unwrap();
    let sc = &r["selfCheck"];
    assert_eq!((sc["found"].clone(), sc["analyzed"].clone()), (json!(5), json!(2)), "{r}");
    assert_eq!(sc["skipped"]["Block Reference"]["reason"], "a different type");

    // When nested expansion hits the work cap, the cap and its value are reported.
    let mut s = Session::new();
    define(&mut s, "LOOP", vec![insert("LOOP", 0.0, 1, 1)]);
    put(&mut s, &Space::Model, insert("LOOP", 0.0, 1, 1));
    let r: Value = s.execute("count", &json!({"space": "all"})).unwrap();
    assert_eq!(r["selfCheck"]["caps"], json!({ "blockDepth": 16 }), "the infinite LOOP hits the depth cap: {r}");
}
