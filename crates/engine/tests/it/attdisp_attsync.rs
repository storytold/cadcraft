//! ATTDISP (ATTMODE, honoured by the display) and ATTSYNC.

use cadcraft_doc::{Attrib, EntityKind, Handle, Space};
use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Make block `name` from what is in model space now (the objects are removed).
fn make_block(s: &mut Session, name: &str) {
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": name, "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
}

fn attdef(s: &mut Session, tag: &str, default: &str, y: f64, height: f64, invisible: bool) {
    s.execute("attdef", &json!({"tag": tag, "default": default, "at": [0, y], "height": height, "invisible": invisible})).unwrap();
}

fn line(s: &mut Session) {
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
}

fn insert(s: &mut Session, x: f64, attribs: Value) -> Handle {
    let r = s.execute("insert", &json!({"name": "B", "at": [x, 0], "attribs": attribs})).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

fn attribs(s: &Session, h: Handle) -> Vec<Attrib> {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.clone()
}

fn segments(s: &Session) -> usize {
    cadcraft_render::build(s.doc().unwrap(), &Space::Model, &cadcraft_render::Options::default()).segment_count()
}

#[test]
fn attdisp_sets_attmode_and_the_display_follows_it() {
    let mut s = Session::new();
    attdef(&mut s, "T", "shown", 1.0, 1.0, false);
    attdef(&mut s, "H", "hidden", 3.0, 1.0, true);
    line(&mut s);
    make_block(&mut s, "B");
    insert(&mut s, 0.0, json!({}));
    let normal = segments(&s);

    s.cmdline("attdisp").unwrap();
    assert!(s.prompt_text().contains("[Normal/ON/OFF] <Normal>"), "{}", s.prompt_text());
    s.cmdline("on").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().header.i64("ATTMODE", 1), 2);
    let on = segments(&s);

    s.cmdline("attdisp off").unwrap();
    assert_eq!(s.doc().unwrap().header.i64("ATTMODE", 1), 0);
    let off = segments(&s);
    assert!(off < normal && normal < on, "off {off}, normal {normal}, on {on}");

    assert_eq!(s.execute("attdisp", &json!({"mode": "normal"})).unwrap(), json!({"attmode": 1}));
    assert_eq!(segments(&s), normal);
    assert!(s.execute("attdisp", &json!({"mode": "sometimes"})).is_err());
}

#[test]
fn view_attribute_display_menu_sets_attmode() {
    let mut s = Session::new();
    for (id, item, mode) in [("attdisp.on", "On", 2), ("attdisp.off", "Off", 0), ("attdisp.normal", "Normal", 1)] {
        let spec = cadcraft_engine::find_command(id).unwrap_or_else(|| panic!("no command {id}"));
        assert_eq!(spec.menu, ["View", "Attribute Display", item]);
        s.start(id).unwrap();
        assert!(s.running.is_none(), "{id}: {}", s.prompt_text());
        assert_eq!(s.doc().unwrap().header.i64("ATTMODE", 1), mode, "{id}");
    }
    assert_eq!(s.execute("attdisp.off", &json!({})).unwrap(), json!({"attmode": 0}));
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().header.i64("ATTMODE", 1), 1);
}

#[test]
fn attdisp_runs_transparently_inside_another_command() {
    let mut s = Session::new();
    s.cmdline("LINE 0,0").unwrap();
    s.cmdline("'ATTDISP").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("attdisp"));
    assert!(s.prompt_text().starts_with(">>"), "{}", s.prompt_text());
    s.cmdline("off").unwrap();
    assert_eq!(s.doc().unwrap().header.i64("ATTMODE", 1), 0);
    // LINE prompts again for its next point.
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("line"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.iter().count(), 1);
}

#[test]
fn attsync_brings_references_in_line_with_the_definitions() {
    let mut s = Session::new();
    attdef(&mut s, "A", "a", 1.0, 0.2, false);
    attdef(&mut s, "OLD", "old", 2.0, 0.2, false);
    line(&mut s);
    make_block(&mut s, "B");
    let r1 = insert(&mut s, 100.0, json!({"A": "kept", "OLD": "x"}));
    let r2 = insert(&mut s, 200.0, json!({}));
    // A reference nested in another block.
    let nested = insert(&mut s, 0.0, json!({"A": "inner"}));
    s.execute("block", &json!({"name": "N", "base": [0, 0], "handles": [nested.hex()], "keep": "delete"})).unwrap();

    // Redefine B: A moves and grows and turns invisible, OLD goes, NEW comes.
    attdef(&mut s, "A", "a", 5.0, 0.5, true);
    attdef(&mut s, "NEW", "new", 7.0, 0.2, false);
    line(&mut s);
    let others: Vec<String> = s.doc().unwrap().model.iter().filter(|e| ![r1, r2].contains(&e.handle)).map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": others, "keep": "delete"})).unwrap();

    // Name option, typed as at the command line.
    s.cmdline("attsync").unwrap();
    assert!(s.prompt_text().contains("[?/Name/Select] <Select>"), "{}", s.prompt_text());
    for l in ["n", "b"] {
        s.cmdline(l).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l == "ATTSYNC block B: 3 references synchronized."), "{:?}", s.log);

    let a = attribs(&s, r1);
    let tags: Vec<&str> = a.iter().map(|a| a.tag.as_str()).collect();
    assert_eq!(tags, ["A", "NEW"]);
    assert_eq!((a[0].text.value.as_str(), a[1].text.value.as_str()), ("kept", "new"));
    assert_eq!((a[0].text.insert.x, a[0].text.insert.y, a[0].text.height), (100.0, 5.0, 0.5));
    assert!(a[0].invisible);
    assert_eq!(attribs(&s, r2)[0].text.value, "a");

    let n = s.doc().unwrap().block("N").unwrap().clone();
    let inner: Vec<Attrib> =
        n.entities.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.attribs.clone()) } else { None }).unwrap();
    assert_eq!(inner.iter().map(|a| (a.tag.as_str(), a.text.value.as_str())).collect::<Vec<_>>(), [("A", "inner"), ("NEW", "new")]);

    // Select option (the default), confirmed with Enter; one undo step per run.
    s.execute("attedit", &json!({"handle": r2.hex(), "values": {"A": "changed"}})).unwrap();
    for l in ["attsync", "", "205,0", ""] {
        s.cmdline(l).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(attribs(&s, r2)[0].text.value, "changed");

    // The JSON form, by a reference's handle.
    assert_eq!(s.execute("attsync", &json!({"handle": r1.hex()})).unwrap(), json!({"block": "B", "references": 3}));
}
