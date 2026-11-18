//! Layer and colour set on one attribute of a block reference: -ATTEDIT Layer/Color, drawn in
//! place of the definition's, reset by ATTSYNC.

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::{Color, Rgb};
use cadcraft_engine::doc::{AttribProps, EntityKind, Handle, Space};
use cadcraft_engine::render::{Options, build};
use serde_json::json;

/// Block B (attributes T and U on layer 0) inserted at 100,0; layer A exists.
fn setup() -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "T", "default": "one", "at": [0, 1]})).unwrap();
    s.execute("attdef", &json!({"tag": "U", "default": "u", "at": [0, 2]})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    let r = s.execute("insert", &json!({"name": "B", "at": [100, 0]})).unwrap();
    s.doc_mut().unwrap().ensure_layer("A");
    (s, Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

fn props(s: &Session, h: Handle, tag: &str) -> AttribProps {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.iter().find(|a| a.tag == tag).unwrap().props.clone()
}

fn drawn_in(s: &Session, rgb: Rgb) -> bool {
    let l = build(s.doc().unwrap(), &Space::Model, &Options::default());
    l.prims.iter().any(|p| p.color == rgb)
}

#[test]
fn dash_attedit_sets_layer_and_colour_of_one_attribute() {
    let (mut s, h) = setup();
    let blue = Rgb(0, 128, 255);
    assert!(!drawn_in(&s, blue));
    for t in ["-attedit", "y", "", "T", "", "105,0", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.prompt_text().contains("[Value/Layer/Color/Next] <N>"), "{}", s.prompt_text());
    s.cmdline("l").unwrap();
    assert!(s.prompt_text().contains("Enter new layer name <0>"), "{}", s.prompt_text());
    // An unknown layer is refused and asked again.
    s.cmdline("nope").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Cannot find layer \"nope\"")), "{:?}", s.log);
    for t in ["a", "c", "1", "c"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.prompt_text().contains("<Red>"), "{}", s.prompt_text());
    for t in ["t", "0,128,255"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.prompt_text().contains("Enter an option"), "{}", s.prompt_text());
    s.cmdline("n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());

    let p = props(&s, h, "T");
    assert_eq!((p.layer.as_deref(), p.color), (Some("A"), Some(Color::True(blue))));
    assert!(props(&s, h, "U").is_empty(), "only the edited attribute changes");
    assert!(drawn_in(&s, blue), "the attribute's own colour is drawn");

    // ATTSYNC brings the attributes back to their definitions, as in AutoCAD.
    s.execute("attsync", &json!({"name": "B"})).unwrap();
    assert!(props(&s, h, "T").is_empty());
    assert!(!drawn_in(&s, blue));
}

#[test]
fn hostile_answers_do_not_panic() {
    let (mut s, h) = setup();
    for t in ["-attedit", "y", "", "", "", "105,0", "", "c", "999", "co", "t", "1,2", "300,0,0", "", "l", "", "n", "n"] {
        let _ = s.cmdline(t);
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(props(&s, h, "T").is_empty() && props(&s, h, "U").is_empty());
}
