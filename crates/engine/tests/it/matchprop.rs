//! MATCHPROP (#439): the typed source/destination picker with Settings, and the JSON form with a
//! property list.

use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn last(s: &Session) -> Handle {
    s.doc().unwrap().model.last().unwrap().handle
}

/// A red line on layer A (the source) and two plain lines on layer 0.
fn setup() -> (Session, Handle, Handle, Handle) {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "A" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let src = last(&s);
    s.execute("properties.set", &json!({ "handles": [src.hex()], "layer": "A", "color": "red", "ltscale": 2.5 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    let a = last(&s);
    s.execute("line", &json!({ "points": [[0, 9], [10, 9]] })).unwrap();
    let b = last(&s);
    (s, src, a, b)
}

fn props(s: &Session, h: Handle) -> (String, Color, f64) {
    let e = s.doc().unwrap().entity(h).cloned().unwrap();
    (e.common.layer.clone(), e.common.color, e.common.ltscale)
}

#[test]
fn matchprop_json_with_property_list() {
    let (mut s, src, a, b) = setup();
    let r = s.execute("matchprop", &json!({ "source": src.hex(), "targets": [a.hex()] })).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(props(&s, a), ("A".into(), Color::Index(1), 2.5));
    // Only the named properties; `{name: false}` turns single ones off.
    s.execute("matchprop", &json!({ "source": src.hex(), "targets": [b.hex()], "properties": ["color"] })).unwrap();
    assert_eq!(props(&s, b), ("0".into(), Color::Index(1), 1.0));
    s.execute("matchprop", &json!({ "source": src.hex(), "targets": [b.hex()], "properties": { "layer": false } })).unwrap();
    assert_eq!(props(&s, b), ("0".into(), Color::Index(1), 2.5));
    // The source itself is not counted; bad params are refused.
    let r = s.execute("matchprop", &json!({ "source": src.hex(), "targets": [src.hex()] })).unwrap();
    assert_eq!(r["changed"], 0);
    for bad in [
        json!({}),
        json!({ "source": null, "targets": [a.hex()] }),
        json!({ "source": "FFFFF", "targets": [a.hex()] }),
        json!({ "source": src.hex(), "targets": "1A" }),
        json!({ "source": src.hex(), "targets": [a.hex()], "properties": ["colour"] }),
        json!({ "source": src.hex(), "targets": [a.hex()], "properties": { "color": 1 } }),
        json!({ "source": src.hex(), "targets": [a.hex()], "properties": 7 }),
    ] {
        assert!(s.execute("matchprop", &bad).is_err(), "{bad}");
    }
    assert!(s.running.is_none(), "JSON calls never prompt");
}

#[test]
fn matchprop_copies_object_specific_properties() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "height": 5, "text": "src" })).unwrap();
    let t1 = last(&s);
    s.execute("text", &json!({ "at": [0, 20], "height": 1, "text": "dst" })).unwrap();
    let t2 = last(&s);
    s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10]], "width": 2 })).unwrap();
    let p1 = last(&s);
    s.execute("pline", &json!({ "vertices": [[20, 0], [30, 0]] })).unwrap();
    let p2 = last(&s);
    s.execute("matchprop", &json!({ "source": t1.hex(), "targets": [t2.hex()], "properties": ["layer"] })).unwrap();
    let height = |s: &Session, h: Handle| match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Text(t) => t.height,
        _ => f64::NAN,
    };
    assert_eq!(height(&s, t2), 1.0, "text off: height stays");
    s.execute("matchprop", &json!({ "source": t1.hex(), "targets": [t2.hex()] })).unwrap();
    assert_eq!(height(&s, t2), 5.0);
    s.execute("matchprop", &json!({ "source": p1.hex(), "targets": [p2.hex(), t2.hex()] })).unwrap();
    match &s.doc().unwrap().entity(p2).unwrap().kind {
        EntityKind::LwPolyline(p) => assert_eq!(p.const_width, 2.0),
        k => panic!("{k:?}"),
    }
    assert_eq!(height(&s, t2), 5.0, "a polyline source leaves text properties alone");
}

#[test]
fn matchprop_typed_picker_and_settings() {
    let (mut s, src, a, b) = setup();
    let undo = s.state().unwrap().undo.len();
    s.cmdline("ma").unwrap();
    assert!(s.prompt_text().contains("Select source object"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![src])).unwrap();
    assert!(s.log.iter().any(|l| l.starts_with("Current active settings: Color Layer Ltype")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Select destination object(s) or [Settings]"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![a])).unwrap();
    assert_eq!(props(&s, a), ("A".into(), Color::Index(1), 2.5));
    // Settings: turn Layer and Ltscale off; Enter goes back to the destination prompt.
    s.cmdline("s").unwrap();
    assert!(s.prompt_text().contains("[Color/LAyer/LType"), "{}", s.prompt_text());
    s.cmdline("la").unwrap();
    s.cmdline("lts").unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Color Ltype Lineweight")), "{:?}", s.log.last());
    s.cmdline("bogus").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Invalid option keyword")), "{:?}", s.log);
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("Select destination object(s)"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![b])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(props(&s, b), ("0".into(), Color::Index(1), 1.0));
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");
    // Settings are remembered for the next typed MATCHPROP; a preselected object is the source.
    s.set_selection(vec![src]);
    s.cmdline("matchprop").unwrap();
    assert!(s.prompt_text().contains("Select destination"), "{}", s.prompt_text());
    assert!(s.log.last().is_some_and(|l| !l.contains("Layer")), "{:?}", s.log.last());
    s.cmdline("s").unwrap();
    s.cmdline("none").unwrap();
    assert!(s.log.last().is_some_and(|l| l.ends_with("None")), "{:?}", s.log.last());
    s.cmdline("all").unwrap();
    s.cmdline("").unwrap();
    s.cancel();
    // A JSON call ignores the typed settings.
    s.execute("matchprop", &json!({ "source": src.hex(), "targets": [b.hex()] })).unwrap();
    assert_eq!(props(&s, b), ("A".into(), Color::Index(1), 2.5));
}

#[test]
fn matchprop_skips_locked_layers() {
    let (mut s, src, a, _) = setup();
    s.execute("layer.new", &json!({ "name": "L" })).unwrap();
    s.execute("properties.set", &json!({ "handles": [a.hex()], "layer": "L" })).unwrap();
    s.execute("layer.set", &json!({ "name": "L", "locked": true })).unwrap();
    let r = s.execute("matchprop", &json!({ "source": src.hex(), "targets": [a.hex()] })).unwrap();
    assert_eq!(r["changed"], 0);
    assert_eq!(props(&s, a).0, "L");
}
