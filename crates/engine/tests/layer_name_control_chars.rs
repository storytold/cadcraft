//! `layer.new` refuses control characters in a layer name (the DXF writer would turn them into
//! spaces and the name would change on reopen).

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn layer_new_rejects_control_characters() {
    let mut s = Session::new();
    for name in ["A\nB", "A\tB", "A\u{0}B"] {
        assert!(s.execute("layer.new", &json!({"name": name})).is_err(), "{name:?} was accepted");
    }
    assert!(s.execute("layer.new", &json!({"name": "A B"})).is_ok());
}
