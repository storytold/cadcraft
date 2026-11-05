//! A hatch's background colour set from the Properties palette (properties.set).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Hatch};
use serde_json::{Value, json};

fn hatch(s: &Session, h: &str) -> Hatch {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

/// properties.set refuses a hatch background that is no colour, as HATCH and HATCHEDIT do,
/// instead of reporting a change it didn't make (#2330 review).
#[test]
fn properties_refuse_a_bad_hatch_background() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "ANSI31" })).unwrap());
    for bad in [json!("blu"), json!(true), json!({})] {
        assert!(s.execute("properties.set", &json!({ "handles": [h], "background": bad })).is_err(), "{bad}");
    }
    s.execute("properties.set", &json!({ "handles": [h], "background": 3 })).unwrap();
    assert!(hatch(&s, &h).background.is_some());
    s.execute("properties.set", &json!({ "handles": [h], "background": "None" })).unwrap();
    assert!(hatch(&s, &h).background.is_none());
}
