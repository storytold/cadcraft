//! Setting a block reference's scale through Properties moves and resizes its attributes like SCALE
//! about the insertion point (#481).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::json;

fn setup(at: [f64; 2], scale: f64) -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "TAG", "at": [10, 0], "height": 2.5, "default": "D"})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    let r = s.execute("insert", &json!({"name": "B", "at": at, "scale": scale, "attribs": {"TAG": "hello"}})).unwrap();
    (s, Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

fn attrib(s: &Session, h: Handle) -> (f64, f64, f64, String, f64, f64) {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    let a = &i.attribs[0].text;
    (a.insert.x, a.insert.y, a.height, a.value.clone(), i.scale.x, i.insert.x)
}

#[test]
fn scale_property_moves_and_resizes_attribute() {
    let (mut s, h) = setup([0.0, 0.0], 1.0);
    s.execute("properties.set", &json!({"handles": [h.hex()], "scale": 2})).unwrap();
    let (x, y, height, value, scale, ix) = attrib(&s, h);
    assert_eq!((x, y, height, value.as_str(), scale, ix), (20.0, 0.0, 5.0, "hello", 2.0, 0.0));
}

#[test]
fn scale_property_is_absolute_about_the_insertion_point() {
    let (mut s, h) = setup([7.0, 11.0], 2.0);
    s.execute("properties.set", &json!({"handles": [h.hex()], "scale": 3})).unwrap();
    let (x, y, height, value, scale, ix) = attrib(&s, h);
    assert_eq!((x, y, height, value.as_str(), scale, ix), (37.0, 11.0, 7.5, "hello", 3.0, 7.0));
}
