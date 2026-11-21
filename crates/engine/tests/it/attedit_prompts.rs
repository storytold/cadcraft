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
    // Edit Position, Height, Angle, and Style
    type_lines(&mut s, &["p", "10,20", "h", "2.5", "a", "45", "s", "Standard"]);
    // Edit Value: change and replace
    type_lines(&mut s, &["v", "c", "b", "B", "v", "r", "replaced value", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(value(&s, h, "T"), "replaced value");
    assert_eq!(value(&s, h, "U"), "u");

    let EntityKind::Insert(ins) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    let att = ins.attribs.iter().find(|a| a.tag == "T").unwrap();
    assert!((att.text.insert.x - 10.0).abs() < 1e-6);
    assert!((att.text.insert.y - 20.0).abs() < 1e-6);
    assert!((att.text.height - 2.5).abs() < 1e-6);
    assert!((att.text.rotation.to_degrees() - 45.0).abs() < 1e-4);
    assert_eq!(att.text.style, "Standard");
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

/// -ATTEDIT Position moves an aligned or fit attribute without losing its baseline: both of its
/// points move by the same offset (setting both to the picked point would leave no direction or
/// length to fit the text between).
#[test]
fn dash_attedit_position_keeps_an_aligned_attributes_baseline() {
    use cadcraft_doc::HAlign;
    use cadcraft_geom::Vec3;
    for halign in [HAlign::Aligned, HAlign::Fit] {
        let mut s = with_block();
        let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    for a in i.attribs.iter_mut().filter(|a| a.tag == "T") {
                        a.text.halign = halign;
                        a.text.insert = Vec3::new(100.0, 1.0, 0.0);
                        a.text.align_pt = Some(Vec3::new(104.0, 1.0, 0.0));
                    }
                }
            })
            .unwrap();
        type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", "", "p", "10,20", "n"]);
        assert!(s.running.is_none(), "{}", s.prompt_text());
        let EntityKind::Insert(ins) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
        let t = &ins.attribs.iter().find(|a| a.tag == "T").unwrap().text;
        assert!(t.insert.xy().near(cadcraft_geom::Vec2::new(10.0, 20.0), 1e-9), "{halign:?}: {:?}", t.insert);
        assert!(t.align_pt.is_some_and(|p| p.xy().near(cadcraft_geom::Vec2::new(14.0, 20.0), 1e-9)), "{halign:?}: {:?}", t.align_pt);
    }
}

/// Change attribute T of reference `h` in place.
fn set_t(s: &mut Session, h: Handle, f: impl Fn(&mut cadcraft_doc::Attrib)) {
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                for a in i.attribs.iter_mut().filter(|a| a.tag == "T") {
                    f(a);
                }
            }
        })
        .unwrap();
}

fn attrib_t(s: &Session, h: Handle) -> cadcraft_doc::Attrib {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.iter().find(|a| a.tag == "T").unwrap().clone()
}

/// -ATTEDIT Angle turns an aligned or fit attribute's baseline (which sets its direction) about
/// its insertion point, keeping its length.
#[test]
fn dash_attedit_angle_turns_an_aligned_attributes_baseline() {
    use cadcraft_doc::HAlign;
    use cadcraft_geom::{Vec2, Vec3};
    for halign in [HAlign::Aligned, HAlign::Fit] {
        let mut s = with_block();
        let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
        set_t(&mut s, h, |a| {
            a.text.halign = halign;
            a.text.insert = Vec3::new(100.0, 1.0, 0.0);
            a.text.align_pt = Some(Vec3::new(104.0, 1.0, 0.0));
        });
        type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", "", "a", "90", "n"]);
        assert!(s.running.is_none(), "{}", s.prompt_text());
        let t = attrib_t(&s, h).text;
        assert!(t.insert.xy().near(Vec2::new(100.0, 1.0), 1e-9), "{halign:?}: {:?}", t.insert);
        assert!(t.align_pt.is_some_and(|p| p.xy().near(Vec2::new(100.0, 5.0), 1e-9)), "{halign:?}: {:?}", t.align_pt);
        assert!((t.rotation.to_degrees() - 90.0).abs() < 1e-9, "{halign:?}: {}", t.rotation);
    }
}

/// A height or angle picked at -ATTEDIT is measured from the point the text is placed by: the
/// alignment point of justified text.
#[test]
fn dash_attedit_picked_height_and_angle_start_at_the_justification_point() {
    use cadcraft_doc::HAlign;
    use cadcraft_geom::Vec3;
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
    set_t(&mut s, h, |a| {
        a.text.halign = HAlign::Center;
        a.text.insert = Vec3::new(100.0, 1.0, 0.0);
        a.text.align_pt = Some(Vec3::new(110.0, 1.0, 0.0));
    });
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", "", "h", "110,6", "a", "110,6", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = attrib_t(&s, h).text;
    assert!((t.height - 5.0).abs() < 1e-9, "{}", t.height);
    assert!((t.rotation.to_degrees() - 90.0).abs() < 1e-9, "{}", t.rotation.to_degrees());
}

/// A multiline attribute is drawn as its multiline text: -ATTEDIT Position, Height and Angle
/// change that text, and a picked angle starts at its insertion point.
#[test]
fn dash_attedit_edits_a_multiline_attributes_text() {
    use cadcraft_doc::{MText, MultilineAttrib};
    use cadcraft_geom::{Vec2, Vec3};
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
    let mtext = MText {
        insert: Vec3::new(120.0, 3.0, 0.0),
        height: 2.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "abc".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    set_t(&mut s, h, |a| a.multiline = Some(MultilineAttrib { mtext: mtext.clone(), read_as: "abc".into() }));
    let before = attrib_t(&s, h).text.insert;
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", "", "p", "10,20", "h", "2.5", "a", "10,30", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let a = attrib_t(&s, h);
    let m = a.multiline.as_ref().unwrap().mtext.clone();
    assert!(m.insert.xy().near(Vec2::new(10.0, 20.0), 1e-9), "{:?}", m.insert);
    // The single-line text moves with it.
    assert!((a.text.insert - before).xy().near(Vec2::new(-110.0, 17.0), 1e-9), "{:?}", a.text.insert);
    assert!((m.height - 2.5).abs() < 1e-9 && (a.text.height - 2.5).abs() < 1e-9, "{} {}", m.height, a.text.height);
    assert!((m.rotation.to_degrees() - 90.0).abs() < 1e-9, "{}", m.rotation.to_degrees());
}

/// The Height, Angle and Style defaults of a multiline attribute are its multiline text's, which
/// is what is drawn; a single-line attribute's are its text's.
#[test]
fn dash_attedit_defaults_of_a_multiline_attribute_come_from_its_multiline_text() {
    use cadcraft_doc::{MText, MultilineAttrib};
    use cadcraft_geom::Vec3;
    let mut s = with_block();
    let h = handle(&insert(&mut s, 100.0, json!({"T": "abc"})));
    let mtext = MText {
        insert: Vec3::new(120.0, 3.0, 0.0),
        height: 3.5,
        width: 0.0,
        attach: 1,
        rotation: 30f64.to_radians(),
        style: "NOTES".into(),
        contents: "abc".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    set_t(&mut s, h, |a| {
        a.text.height = 1.25;
        a.text.rotation = 0.0;
        a.text.style = "Standard".into();
        a.multiline = Some(MultilineAttrib { mtext: mtext.clone(), read_as: "abc".into() });
    });
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "105,0", "", "h"]);
    assert!(s.prompt_text().contains("<3.5000>"), "{}", s.prompt_text());
    type_lines(&mut s, &["", "a"]);
    assert!(s.prompt_text().contains("<30.0000>"), "{}", s.prompt_text());
    type_lines(&mut s, &["", "s"]);
    assert!(s.prompt_text().contains("<NOTES>"), "{}", s.prompt_text());
    type_lines(&mut s, &["", "n"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // A single-line attribute keeps its text's.
    let g = handle(&insert(&mut s, 200.0, json!({"T": "abc"})));
    set_t(&mut s, g, |a| {
        a.text.height = 1.25;
        a.text.style = "Standard".into();
    });
    type_lines(&mut s, &["-attedit", "y", "", "T", "", "205,0", "", "h"]);
    assert!(s.prompt_text().contains("<1.2500>"), "{}", s.prompt_text());
    type_lines(&mut s, &["", "s"]);
    assert!(s.prompt_text().contains("<Standard>"), "{}", s.prompt_text());
}
