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
    assert!(s.prompt_text().contains("[Value/Position/Height/Angle/Style/Layer/Color/Next] <N>"), "{}", s.prompt_text());
    type_lines(&mut s, &["v", "c", "b", "B", "v", "r", "replaced value", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(value(&s, h, "T"), "replaced value");
    assert_eq!(value(&s, h, "U"), "u");
}

fn text(s: &Session, h: Handle, tag: &str) -> cadcraft_doc::Text {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.iter().find(|a| a.tag == tag).unwrap().text.clone()
}

#[test]
fn dash_attedit_one_at_a_time_changes_position_height_angle_and_style() {
    let mut s = with_block();
    s.execute("style", &json!({ "name": "Notes" })).unwrap();
    let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
    let (before, u) = (text(&s, h, "T"), text(&s, h, "U"));
    assert_eq!((before.insert.x, before.insert.y), (100.0, 1.0));
    let undo = s.state().unwrap().undo.len();
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", ""]);
    // Position: Enter keeps it, a point moves it there.
    s.cmdline("p").unwrap();
    assert!(s.prompt_text().contains("Specify new text insertion point <no change>"), "{}", s.prompt_text());
    type_lines(&mut s, &["", "p", "120,30"]);
    // Height: a refused answer asks again; a distance is taken.
    s.cmdline("h").unwrap();
    assert!(s.prompt_text().contains("Specify new height <"), "{}", s.prompt_text());
    type_lines(&mut s, &["0", "-2", "x"]);
    assert!(s.prompt_text().contains("Specify new height"), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l.contains("Requires a positive height.")), "{:?}", s.log);
    s.cmdline("4.5").unwrap();
    // Angle, as AUNITS shows it.
    s.cmdline("a").unwrap();
    assert!(s.prompt_text().contains("Specify new rotation angle <0>"), "{}", s.prompt_text());
    s.cmdline("30").unwrap();
    // Style: an unknown one is refused and asked again.
    s.cmdline("s").unwrap();
    assert!(s.prompt_text().contains("Enter new text style <Standard>"), "{}", s.prompt_text());
    type_lines(&mut s, &["Nope", "notes", "n"]);
    assert!(s.log.iter().any(|l| l.contains("Cannot find text style \"Nope\".")), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = text(&s, h, "T");
    assert_eq!((t.insert.x, t.insert.y, t.height, t.style.as_str(), t.value.as_str()), (120.0, 30.0, 4.5, "Notes", "abc"));
    assert!((t.rotation - 30f64.to_radians()).abs() < 1e-12, "{}", t.rotation);
    assert_eq!(text(&s, h, "U"), u, "only the attribute being edited");
    // The whole -ATTEDIT is one undo step.
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    s.undo().unwrap();
    assert_eq!(text(&s, h, "T"), before);

    // Written and read back.
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", "", "h", "3", "a", "90", "p", "110,20", "s", "Notes", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    for name in ["a.dxf", "a.dwg"] {
        let d = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), name).unwrap(), name).unwrap();
        let t = d
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Insert(i) => i.attribs.iter().find(|a| a.tag == "T").map(|a| a.text.clone()),
                _ => None,
            })
            .unwrap();
        assert!((t.insert.x - 110.0).abs() < 1e-9 && (t.insert.y - 20.0).abs() < 1e-9, "{name}: {t:?}");
        assert!((t.height - 3.0).abs() < 1e-9 && (t.rotation - 90f64.to_radians()).abs() < 1e-9, "{name}: {t:?}");
        assert!(t.style.eq_ignore_ascii_case("Notes"), "{name}: {t:?}");
    }
}

#[test]
fn dash_attedit_moves_justified_text_by_its_alignment_point() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    // A middle-centre attribute definition, from -ATTDEF.
    type_lines(&mut s, &["-attdef", "", "MC", "p", "v", "j", "mc", "5,5", "1", "0"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "J", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    let h = handle(&s.execute("insert", &json!({"name": "J", "at": [100, 0], "attribs": {"MC": "v"}})).unwrap());
    type_lines(&mut s, &["-attedit", "y", "", "", "", "105,0", "", "p", "200,50", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = text(&s, h, "MC");
    assert_eq!((t.justify_point().x, t.justify_point().y), (200.0, 50.0));
}

#[test]
fn dash_attedit_json_changes_position_height_angle_and_style() {
    let mut s = with_block();
    s.execute("style", &json!({ "name": "Notes" })).unwrap();
    let a = handle(&insert(&mut s, 100.0, json!({"T": "x"})));
    let b = handle(&insert(&mut s, 200.0, json!({"T": "y"})));
    let b_height = text(&s, b, "T").height;
    let undo = s.state().unwrap().undo.len();
    let r =
        s.execute("-attedit", &json!({"tag": "t", "handles": [a.hex()], "position": [7, 8], "height": 2.5, "angle": 45, "style": "notes"})).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    let t = text(&s, a, "T");
    assert_eq!((t.insert.x, t.insert.y, t.height, t.style.as_str()), (7.0, 8.0, 2.5, "Notes"));
    assert!((t.rotation - 45f64.to_radians()).abs() < 1e-12);
    assert_eq!(text(&s, b, "T").height, b_height, "only the given reference");
    // Every reference of the block, with a value replacement in the same call.
    let r = s.execute("-attedit", &json!({"tag": "T", "height": 9, "find": "y", "replace": "z"})).unwrap();
    assert_eq!(r["changed"], 2);
    assert_eq!((text(&s, a, "T").height, text(&s, b, "T").height, value(&s, b, "T")), (9.0, 9.0, "z".to_string()));
    // Hostile or missing parameters are refused and change nothing.
    for bad in [
        json!({"height": 0}),
        json!({"height": -1}),
        json!({"height": "big"}),
        json!({"angle": "x"}),
        json!({"position": [1]}),
        json!({"position": "nowhere"}),
        json!({"style": "Nope"}),
        json!({"style": 3}),
        json!({}),
        json!({"find": ""}),
    ] {
        assert!(s.execute("-attedit", &bad).is_err(), "{bad}");
    }
    assert_eq!(text(&s, a, "T").height, 9.0);
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
