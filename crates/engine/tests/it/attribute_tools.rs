//! Attribute tools: BATTMAN editing (definitions edited, removed, reordered; references follow).

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Block B with attribute definitions A, B, C (defaults a, b, c) and a line, plus two references.
fn drawing() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    for (i, t) in ["A", "B", "C"].iter().enumerate() {
        let y = i as f64 * 2.0;
        s.execute("attdef", &json!({"tag": t, "prompt": format!("{t}?"), "default": t.to_ascii_lowercase(), "at": [0, y], "height": 1.0})).unwrap();
    }
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    let r1 = insert(&mut s, 0.0, json!({"A": "one", "B": "two"}));
    let r2 = insert(&mut s, 50.0, json!({}));
    (s, r1, r2)
}

fn insert(s: &mut Session, x: f64, attribs: Value) -> Handle {
    let r = s.execute("insert", &json!({"name": "B", "at": [x, 0], "attribs": attribs})).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

/// (tag, value, invisible) of a reference's attributes, in order.
fn attribs(s: &Session, h: Handle) -> Vec<(String, String, bool)> {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone(), a.invisible)).collect()
}

fn tags(r: &Value) -> Vec<&str> {
    r["attributes"].as_array().unwrap().iter().map(|a| a["tag"].as_str().unwrap()).collect()
}

fn battman(s: &mut Session, p: Value) -> Value {
    s.execute("battman", &p).unwrap()
}

#[test]
fn battman_lists_definitions_and_references() {
    let (mut s, ..) = drawing();
    let r = battman(&mut s, json!({"name": "b"}));
    assert_eq!(r["block"], "B");
    assert_eq!(r["references"], 2);
    assert_eq!(tags(&r), ["A", "B", "C"]);
    assert_eq!(
        r["attributes"][0],
        json!({"tag": "A", "prompt": "A?", "default": "a", "invisible": false, "constant": false, "height": 1.0, "layer": "0"})
    );
    // Listing changes nothing, so it leaves no undo step.
    let undo = s.state().unwrap().undo.len();
    battman(&mut s, json!({"name": "B", "action": "list"}));
    assert_eq!(s.state().unwrap().undo.len(), undo);
}

#[test]
fn battman_edit_renames_a_tag_and_references_keep_their_values() {
    let (mut s, r1, r2) = drawing();
    let r = battman(
        &mut s,
        json!({"name": "B", "action": "edit", "tag": "a", "newTag": "part", "prompt": "Part number", "default": "P-0", "invisible": true}),
    );
    assert_eq!(r["synced"], 2);
    assert_eq!(
        r["attributes"][0],
        json!({"tag": "PART", "prompt": "Part number", "default": "P-0", "invisible": true, "constant": false, "height": 1.0, "layer": "0"})
    );
    let a = attribs(&s, r1);
    assert_eq!(a[0], ("PART".into(), "one".into(), true));
    assert_eq!(attribs(&s, r2)[0], ("PART".into(), "a".into(), true));
    // A new reference gets the new default.
    let r3 = insert(&mut s, 100.0, json!({}));
    assert_eq!(attribs(&s, r3)[0].1, "P-0");
    // One undo step (after the INSERT's) puts everything back.
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(attribs(&s, r1)[0], ("A".into(), "one".into(), false));
}

#[test]
fn battman_without_sync_leaves_references_alone() {
    let (mut s, r1, _) = drawing();
    let r = battman(&mut s, json!({"name": "B", "action": "edit", "tag": "B", "invisible": true, "sync": false}));
    assert!(r.get("synced").is_none());
    assert_eq!(r["attributes"][1]["invisible"], true);
    assert_eq!(attribs(&s, r1)[1], ("B".into(), "two".into(), false));
    // Sync applies it later.
    assert_eq!(battman(&mut s, json!({"name": "B", "action": "sync"}))["synced"], 2);
    assert!(attribs(&s, r1)[1].2);
}

#[test]
fn battman_removes_and_reorders_definitions() {
    let (mut s, r1, _) = drawing();
    let r = battman(&mut s, json!({"name": "B", "action": "down", "tag": "a"}));
    assert_eq!(tags(&r), ["B", "A", "C"]);
    assert_eq!(attribs(&s, r1).iter().map(|a| a.1.as_str()).collect::<Vec<_>>(), ["two", "one", "c"]);
    // Already at the top: nothing moves.
    assert_eq!(tags(&battman(&mut s, json!({"name": "B", "action": "up", "tag": "B"}))), ["B", "A", "C"]);
    assert_eq!(tags(&battman(&mut s, json!({"name": "B", "action": "order", "order": ["c", "a", "b"]}))), ["C", "A", "B"]);
    let r = battman(&mut s, json!({"name": "B", "action": "remove", "tag": "A"}));
    assert_eq!(tags(&r), ["C", "B"]);
    assert_eq!(attribs(&s, r1).iter().map(|a| a.0.as_str()).collect::<Vec<_>>(), ["C", "B"]);
    // The block's other objects are still there.
    let d = s.doc().unwrap();
    assert!(d.block("B").unwrap().entities.iter().any(|e| matches!(e.kind, EntityKind::Line(_))));
}

#[test]
fn battman_constant_definitions_leave_references() {
    let (mut s, r1, _) = drawing();
    battman(&mut s, json!({"name": "B", "action": "edit", "tag": "C", "constant": true}));
    assert_eq!(attribs(&s, r1).len(), 2);
    battman(&mut s, json!({"name": "B", "action": "edit", "tag": "C", "constant": false}));
    assert_eq!(attribs(&s, r1)[2], ("C".into(), "c".into(), false));
}

#[test]
fn battman_refuses_bad_edits_without_changing_anything() {
    let (mut s, ..) = drawing();
    let before = s.doc().unwrap().clone();
    for p in [
        json!({}),
        json!({"name": "NOPE"}),
        json!({"name": "B", "action": "explode"}),
        json!({"name": "B", "action": "edit"}),
        json!({"name": "B", "action": "edit", "tag": "Z"}),
        json!({"name": "B", "action": "edit", "tag": "A", "newTag": "b"}),
        json!({"name": "B", "action": "edit", "tag": "A", "newTag": "two words"}),
        json!({"name": "B", "action": "edit", "tag": "A", "newTag": ""}),
        json!({"name": "B", "action": "edit", "tag": "A", "height": -1}),
        json!({"name": "B", "action": "edit", "tag": "A", "height": "tall"}),
        json!({"name": "B", "action": "edit", "tag": "A", "layer": "NOPE"}),
        json!({"name": "B", "action": "order", "order": ["A", "B"]}),
        json!({"name": "B", "action": "order", "order": ["A", "A", "B", "C"]}),
        json!({"name": "B", "action": "order", "order": vec!["A"; 100_000]}),
        json!({"name": "B", "action": "order", "order": [1, 2, 3]}),
        json!({"name": "B", "action": "remove", "tag": "Z"}),
    ] {
        assert!(s.execute("battman", &p).is_err(), "{p}");
        assert_eq!(*s.doc().unwrap(), before, "{p}");
    }
}

#[test]
fn battman_at_the_command_line_edits_removes_and_moves() {
    let (mut s, r1, _) = drawing();
    s.cmdline("battman").unwrap();
    assert!(s.prompt_text().contains("Enter name of block to manage or [?]:"), "{}", s.prompt_text());
    s.cmdline("b").unwrap();
    assert!(s.log.iter().any(|l| l == "Block B: 2 references in the drawing."), "{:?}", s.log);
    assert!(s.prompt_text().contains("[Edit/Remove/moveUp/moveDown/Sync/List/eXit] <eXit>"), "{}", s.prompt_text());
    for l in ["e", "a", "t", "ITEM", "p", "Item name", "i", "y", "h", "2", "x"] {
        s.cmdline(l).unwrap();
    }
    let r = battman(&mut s, json!({"name": "B"}));
    assert_eq!(
        r["attributes"][0],
        json!({"tag": "ITEM", "prompt": "Item name", "default": "a", "invisible": true, "constant": false, "height": 2.0, "layer": "0"})
    );
    assert_eq!(attribs(&s, r1)[0], ("ITEM".into(), "one".into(), true));
    for l in ["d", "item", "r", "c"] {
        s.cmdline(l).unwrap();
    }
    assert_eq!(attribs(&s, r1).iter().map(|a| a.0.as_str()).collect::<Vec<_>>(), ["B", "ITEM"]);
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn battman_starts_from_a_selected_reference() {
    let (mut s, r1, _) = drawing();
    s.set_selection(vec![r1]);
    s.cmdline("battman").unwrap();
    assert!(s.prompt_text().contains("Block B: enter an option"), "{}", s.prompt_text());
    s.cmdline("x").unwrap();
    assert!(s.running.is_none());
}
