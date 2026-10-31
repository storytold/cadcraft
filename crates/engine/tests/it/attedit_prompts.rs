//! ATTEDIT / -ATTEDIT typed at the command line, and attribute tags matched in any case.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Block B: a line from 0,0 to 10,0 with attributes T (default "one") and U (default "u").
fn with_block() -> Session {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "T", "default": "one", "at": [0, 1]})).unwrap();
    s.execute("attdef", &json!({"tag": "U", "default": "u", "at": [0, 2]})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    s
}

fn insert(s: &mut Session, x: f64, attribs: Value) -> Value {
    s.execute("insert", &json!({"name": "B", "at": [x, 0], "attribs": attribs})).unwrap()
}

fn handle(r: &Value) -> Handle {
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

fn value(s: &Session, h: Handle, tag: &str) -> String {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.iter().find(|a| a.tag == tag).unwrap().text.value.clone()
}

fn type_lines(s: &mut Session, lines: &[&str]) {
    for l in lines {
        s.cmdline(l).unwrap();
    }
}

#[test]
fn attedit_asks_for_a_block_then_each_value() {
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({})));
    s.cmdline("attedit").unwrap();
    assert!(s.prompt_text().contains("Select block reference"), "{}", s.prompt_text());
    s.cmdline("105,0").unwrap();
    assert!(s.prompt_text().contains("Enter value for T <one>"), "{}", s.prompt_text());
    // A value with spaces; Enter keeps U as it was.
    type_lines(&mut s, &["new value", ""]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(value(&s, h, "T"), "new value");
    assert_eq!(value(&s, h, "U"), "u");
    // One undo step.
    s.undo().unwrap();
    assert_eq!(value(&s, h, "T"), "one");

    // EATTEDIT is the same command; a pick on nothing editable re-prompts.
    s.execute("line", &json!({"points": [[0, 50], [10, 50]]})).unwrap();
    type_lines(&mut s, &["eattedit", "5,50"]);
    assert!(s.log.iter().any(|l| l.contains("Select a block reference.")), "{:?}", s.log);
    type_lines(&mut s, &["105,0", "x", "y"]);
    assert!(s.running.is_none());
    assert_eq!((value(&s, h, "T"), value(&s, h, "U")), ("x".into(), "y".into()));
}

#[test]
fn dash_attedit_global_replaces_a_string_in_matching_attributes() {
    let mut s = with_block();
    let a = handle(&insert(&mut s, 100.0, json!({"T": "A-1", "U": "A-9"})));
    let b = handle(&insert(&mut s, 200.0, json!({"T": "A-2"})));
    // Not one at a time, not only visible ones: every reference of B, tag T, any value.
    s.script("-attedit\nn\nn\nb\nt\n\nA-\nX-\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!((value(&s, a, "T"), value(&s, b, "T")), ("X-1".into(), "X-2".into()));
    assert_eq!(value(&s, a, "U"), "A-9", "tag specification filters");
    assert!(s.log.iter().any(|l| l == "2 attributes changed."), "{:?}", s.log);

    // The JSON form does the same.
    let r = s.execute("-attedit", &json!({"tag": "u", "find": "9", "replace": "8"})).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(value(&s, a, "U"), "A-8");
}

#[test]
fn dash_attedit_one_at_a_time_edits_the_selected_attributes() {
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", ""]);
    assert!(s.prompt_text().contains("[Value/Layer/Color/Next] <N>"), "{}", s.prompt_text());
    // Unsupported options say so and keep the prompt.
    s.cmdline("p").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Position is not available yet")), "{:?}", s.log);
    type_lines(&mut s, &["v", "c", "b", "B", "v", "r", "replaced value", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(value(&s, h, "T"), "replaced value");
    assert_eq!(value(&s, h, "U"), "u");
}

#[test]
fn attribute_tags_match_in_any_case_and_unknown_tags_are_reported() {
    let mut s = with_block();
    let r = insert(&mut s, 100.0, json!({"t": "two", "nope": "x"}));
    let h = handle(&r);
    assert_eq!(value(&s, h, "T"), "two");
    assert_eq!(r["unknownTags"], json!(["nope"]));

    let r = s.execute("attedit", &json!({"handle": h.hex(), "values": {"t": "three", "u": 4, "W": "w"}})).unwrap();
    assert_eq!(r["changed"], 2);
    assert_eq!(r["unknownTags"], json!(["W"]));
    assert_eq!((value(&s, h, "T"), value(&s, h, "U")), ("three".into(), "4".into()));

    let r = s.execute("attedit", &json!({"handle": h.hex(), "values": {"T": "four"}})).unwrap();
    assert_eq!(r, json!({"changed": 1}));
}

/// ATTEDIT with several handles writes the values into each reference, reports the tags none of them has,
/// is one undo step, and refuses an empty list.
#[test]
fn attedit_takes_a_selected_block_reference_at_once() {
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({})));
    s.set_selection(vec![h]);
    s.cmdline("attedit").unwrap();
    assert!(s.prompt_text().contains("Enter value for T <one>"), "{}", s.prompt_text());
    type_lines(&mut s, &["picked", ""]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!((value(&s, h, "T"), value(&s, h, "U")), ("picked".into(), "u".into()));
}

#[test]
fn attedit_leaves_constant_attributes_and_locked_references_alone() {
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({})));
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.attribs[1].constant = true;
            }
        })
        .unwrap();
    let rows = cadcraft_engine::attributes(s.doc().unwrap(), h);
    assert_eq!(rows.iter().map(|r| (r.tag.as_str(), r.editable)).collect::<Vec<_>>(), [("T", true), ("U", false)]);

    let r = s.execute("attedit", &json!({"handle": h.hex(), "values": {"T": "t2", "U": "u2"}})).unwrap();
    assert_eq!((r["changed"].clone(), r["constantTags"].clone()), (json!(1), json!(["U"])));
    assert_eq!((value(&s, h, "T"), value(&s, h, "U")), ("t2".into(), "u".into()));
    // One undo step.
    s.undo().unwrap();
    assert_eq!(value(&s, h, "T"), "one");

    // Not a block reference, a value too long, a locked layer.
    let line = s.execute("line", &json!({"points": [[0, 50], [10, 50]]})).unwrap()["handles"][0].clone();
    assert!(s.execute("attedit", &json!({"handle": line, "values": {"T": "x"}})).is_err());
    assert!(s.execute("attedit", &json!({"handle": h.hex(), "values": {"T": "x".repeat(70_000)}})).is_err());
    s.execute("layer.set", &json!({"name": "0", "locked": true})).unwrap();
    let e = s.execute("attedit", &json!({"handle": h.hex(), "values": {"T": "x"}})).unwrap_err().to_string();
    assert!(e.contains("locked"), "{e}");
    assert_eq!(value(&s, h, "T"), "one");
}

#[test]
fn attedit_with_several_handles_edits_each_in_one_undo_step() {
    let mut s = with_block();
    let (a, b) = (handle(&insert(&mut s, 100.0, json!({"t": "one"}))), handle(&insert(&mut s, 200.0, json!({"t": "two"}))));
    let r = s.execute("attedit", &json!({"handles": [a.hex(), b.hex()], "values": {"T": "same", "nope": "x"}})).unwrap();
    assert_eq!(r["changed"], 2);
    assert_eq!(r["unknownTags"], json!(["nope"]));
    assert_eq!((value(&s, a, "T"), value(&s, b, "T")), ("same".into(), "same".into()));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!((value(&s, a, "T"), value(&s, b, "T")), ("one".into(), "two".into()), "one undo restores both");
    assert!(s.execute("attedit", &json!({"handles": [], "values": {"T": "x"}})).is_err(), "no targets is an error");
}

/// A handle no object has is refused before anything is edited, even when it comes after valid ones.
#[test]
fn attedit_with_a_missing_handle_changes_nothing() {
    let mut s = with_block();
    let a = handle(&insert(&mut s, 100.0, json!({"t": "one"})));
    let undo_before = s.state().unwrap().undo.len();
    let r = s.execute("attedit", &json!({"handles": [a.hex(), "FFFFF"], "values": {"T": "changed"}}));
    assert!(r.is_err(), "a missing handle is an error: {r:?}");
    assert_eq!(value(&s, a, "T"), "one", "the valid reference was not edited");
    assert_eq!(s.state().unwrap().undo.len(), undo_before, "no undo step for a refused request");
}

/// With several handles the rules hold for every target: a constant attribute is named once and left
/// alone, and one target on a locked layer refuses the whole request before anything changes.
#[test]
fn attedit_with_several_handles_keeps_the_rules_for_every_target() {
    let mut s = with_block();
    let a = handle(&insert(&mut s, 100.0, json!({"t": "one"})));
    s.execute("layer.new", &json!({"name": "L2", "current": true})).unwrap();
    let b = handle(&insert(&mut s, 200.0, json!({"t": "two"})));
    for h in [a, b] {
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.attribs[1].constant = true;
                }
            })
            .unwrap();
    }
    let r = s.execute("attedit", &json!({"handles": [a.hex(), b.hex()], "values": {"T": "same", "U": "x"}})).unwrap();
    assert_eq!((r["changed"].clone(), r["constantTags"].clone()), (json!(2), json!(["U"])));
    assert_eq!((value(&s, a, "U"), value(&s, b, "U")), ("u".into(), "u".into()), "constant attributes keep their value");

    // `a` is on layer 0, locked: `b` is not, and still is not edited.
    s.execute("layer.set", &json!({"name": "0", "locked": true})).unwrap();
    let e = s.execute("attedit", &json!({"handles": [b.hex(), a.hex()], "values": {"T": "later"}})).unwrap_err().to_string();
    assert!(e.contains("locked"), "{e}");
    assert_eq!((value(&s, a, "T"), value(&s, b, "T")), ("same".into(), "same".into()));
}
