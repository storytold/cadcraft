//! The headless backend's `control.methods` lists the methods it answers, all of them documented
//! in `docs/control-protocol.md` (the app's list is tested in `crates/ui-egui/tests/it/control_methods.rs`).

use std::collections::BTreeSet;

use cadcraft_mcp::{Backend, Headless};
use serde_json::json;

fn documented() -> BTreeSet<String> {
    let doc = include_str!("../../../../docs/control-protocol.md");
    let table = doc.lines().skip_while(|l| !l.starts_with("| Method |")).skip(2).take_while(|l| l.starts_with('|'));
    table.filter_map(|l| l.split('|').nth(1)).flat_map(|cell| cell.split('`').skip(1).step_by(2).map(str::to_string).collect::<Vec<_>>()).collect()
}

#[test]
fn control_methods_lists_what_headless_answers() {
    let mut b = Headless::default();
    let methods = b.call("control.methods", json!({})).unwrap();
    let methods = methods.as_array().unwrap();
    let doc = documented();
    let missing = std::env::temp_dir().join("cadcraft-843-no-such-dir").join("x.dxf");
    let params = json!({"path": missing.to_string_lossy()});
    for m in methods {
        let name = m["method"].as_str().unwrap();
        assert!(doc.contains(name), "{name} is not in docs/control-protocol.md");
        assert!(m["params"].is_string() && m["description"].as_str().is_some_and(|d| !d.is_empty()), "{m}");
        if let Err(e) = b.call(name, params.clone()) {
            assert!(!e.starts_with("unknown method"), "{name}: {e}");
        }
    }
    let names: BTreeSet<&str> = methods.iter().filter_map(|m| m["method"].as_str()).collect();
    assert!(names.contains("control.methods") && names.contains("ping") && names.contains("engine.execute"), "{names:?}");
    assert!(!names.contains("ui.inspect"), "headless has no UI to inspect");
}

#[test]
fn ping_answers_and_unknown_method_points_to_control_methods() {
    let mut b = Headless::default();
    assert_eq!(b.call("ping", json!({})).unwrap(), json!("pong"));
    let e = b.call("help", json!({})).unwrap_err();
    assert!(e.starts_with("unknown method `help`") && e.contains("control.methods"), "{e}");
}
