//! The headless backend answers `cmdline.key` like `cmdline.input` and like the app's control
//! channel (`crates/ui-egui/tests/it/control_cmdline_key.rs`): a refused Enter is echoed and reported
//! in `error`, and the call itself succeeds.

use cadcraft_mcp::{Backend, Headless};
use serde_json::{Value, json};

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    k.sort();
    k
}

/// The `cmdline.state` fields plus `extra`.
fn want(b: &mut Headless, extra: &[&str]) -> Vec<String> {
    let mut k = keys(&b.call("cmdline.state", json!({})).unwrap());
    k.extend(extra.iter().map(ToString::to_string));
    k.sort();
    k
}

#[test]
fn refused_enter_is_echoed_and_reported_in_error() {
    // Enter repeats CLOSE, which needs an open drawing.
    let mut b = Headless::default();
    b.call("cmdline.input", json!({"text": "close"})).unwrap();
    let r = b.call("cmdline.key", json!({"key": "enter"})).unwrap();
    assert_eq!(keys(&r), want(&mut b, &["output", "error"]), "{r}");
    let e = r["error"].as_str().unwrap();
    assert!(!e.is_empty());
    assert!(r["output"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
    assert!(r["history"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
}

#[test]
fn accepted_keys_have_output_and_no_error() {
    let mut b = Headless::default();
    for key in ["enter", "escape"] {
        b.call("cmdline.input", json!({"text": "circle"})).unwrap();
        let r = b.call("cmdline.key", json!({ "key": key })).unwrap();
        assert_eq!(keys(&r), want(&mut b, &["output"]), "{key}: {r}");
        assert!(r["running"].is_null(), "{key}: {r}");
        assert!(r["output"].as_array().unwrap().iter().any(|l| l.as_str() == Some("*Cancel*")), "{key}: {r}");
    }
}
