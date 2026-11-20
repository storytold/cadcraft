//! `block` refuses control characters in a block name (the DXF writer would turn them into
//! spaces and the name would change on reopen).

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn block_rejects_control_characters() {
    let mut s = Session::new();
    let r = s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap();
    let h = r["handles"][0].clone();
    for name in ["A\nB", "A\tB", "A\u{0}B"] {
        assert!(s.execute("block", &json!({"name": name, "handles": [h]})).is_err(), "{name:?} was accepted");
    }
    assert!(s.execute("block", &json!({"name": "A B", "handles": [h]})).is_ok());
}
