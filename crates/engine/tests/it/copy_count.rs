//! JSON COPY refuses a `count` that is not an integer from 1 to 10000 instead of clamping it.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn entities(s: &Session) -> usize {
    s.doc().map(|d| d.model.iter().count()).unwrap_or(0)
}

#[test]
fn invalid_copy_count_is_refused() {
    let mut s = Session::new();
    let r = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let h = r["handles"][0].clone();
    let before = entities(&s);
    let bad: [Value; 7] = [json!(0), json!(-3), json!(2.5), json!("abc"), json!(null), json!(10001), json!(50000)];
    for c in bad {
        let r = s.execute("copy", &json!({"handles": [h], "delta": [0, 5], "count": c}));
        assert!(r.is_err(), "count {c} should be refused");
        assert_eq!(entities(&s), before, "count {c} changed the drawing");
    }
    s.execute("copy", &json!({"handles": [h], "delta": [0, 5], "count": 3})).unwrap();
    assert_eq!(entities(&s), before + 3);
}
