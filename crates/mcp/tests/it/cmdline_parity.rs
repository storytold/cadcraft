//! The headless backend answers `cmdline.input` and `app.open` with the same reply shape as the
//! app's control channel (`cadcraft_ui_egui::control`, tested in `crates/ui-egui/tests/it/control_cmdline.rs`).

use cadcraft_mcp::{Backend, Headless};
use serde_json::{Value, json};

const STATE_KEYS: [&str; 7] = ["prompt", "running", "keywords", "accept", "buffer", "history", "historyExpanded"];

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    k.sort();
    k
}

fn want(extra: &[&str]) -> Vec<String> {
    let mut k: Vec<String> = STATE_KEYS.iter().chain(extra).map(ToString::to_string).collect();
    k.sort();
    k
}

#[test]
fn refused_input_is_echoed_to_output_and_reported_in_error() {
    let mut b = Headless::default();
    let r = b.call("cmdline.input", json!({"text": "nosuchcommand"})).unwrap();
    assert_eq!(keys(&r), want(&["output", "error"]), "{r}");
    let e = r["error"].as_str().unwrap();
    assert!(!e.is_empty());
    assert!(r["output"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
    assert!(r["history"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
}

#[test]
fn accepted_input_has_no_error_and_reports_the_prompt() {
    let mut b = Headless::default();
    let r = b.call("cmdline.input", json!({"text": "circle"})).unwrap();
    assert_eq!(keys(&r), want(&["output"]), "{r}");
    assert_eq!(r["running"], "circle");
    assert!(!r["accept"].is_null(), "{r}");
    assert_eq!(r["buffer"], "");
    assert_eq!(r["historyExpanded"], false);
    assert_eq!(keys(&b.call("cmdline.state", json!({})).unwrap()), want(&[]));
}

#[test]
fn open_without_a_path_is_refused_before_the_engine() {
    let mut b = Headless::default();
    assert_eq!(b.call("app.open", json!({})).unwrap_err(), "missing path");
    assert_eq!(b.call("app.open", json!({"path": 3})).unwrap_err(), "missing path");
}
