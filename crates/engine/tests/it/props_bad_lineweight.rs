//! `properties.set` rejects a lineweight it cannot parse and leaves the object's lineweight alone.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Lineweight};
use serde_json::json;

fn lineweight(s: &Session, h: Handle) -> Lineweight {
    s.doc().unwrap().entity(h).unwrap().common.lineweight
}

#[test]
fn malformed_lineweight_is_rejected_and_keeps_previous_value() {
    let mut s = Session::new();
    let r = s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let hex = h.hex();

    let before = lineweight(&s, h);
    assert!(s.execute("properties.set", &json!({"handles": [hex], "lineweight": "notanumber"})).is_err());
    assert_eq!(lineweight(&s, h), before);

    s.execute("properties.set", &json!({"handles": [hex], "lineweight": 0.70})).unwrap();
    assert_eq!(lineweight(&s, h), Lineweight::Mm100(70));
    assert!(s.execute("properties.set", &json!({"handles": [hex], "lineweight": "notanumber"})).is_err());
    assert_eq!(lineweight(&s, h), Lineweight::Mm100(70));

    s.execute("properties.set", &json!({"handles": [hex], "lineweight": "0.35 mm"})).unwrap();
    assert_eq!(lineweight(&s, h), Lineweight::Mm100(35));
    s.execute("properties.set", &json!({"handles": [hex], "lineweight": "ByBlock"})).unwrap();
    assert_eq!(lineweight(&s, h), Lineweight::ByBlock);
    assert!(matches!(s.doc().unwrap().entity(h).unwrap().kind, EntityKind::Circle(_)));
}
