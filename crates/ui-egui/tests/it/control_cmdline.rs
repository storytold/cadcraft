//! The app's control channel answers `cmdline.input` and `app.open` with the same reply shape as
//! the headless MCP backend (`cadcraft_mcp::Headless`, tested in `crates/mcp/tests/it/cmdline_parity.rs`).

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

const STATE_KEYS: [&str; 7] = ["prompt", "running", "keywords", "accept", "buffer", "history", "historyExpanded"];

fn call(app: &mut CadApp, method: &str, params: Value) -> Value {
    let (req, _reply) = ControlRequest::new(method, params);
    match handle(app, &egui::Context::default(), &req) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => Value::Null,
    }
}

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

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

#[test]
fn refused_input_is_echoed_to_output_and_reported_in_error() {
    let mut app = app();
    let v = call(&mut app, "cmdline.input", json!({"text": "nosuchcommand"}));
    assert_eq!(v["ok"], true, "{v}");
    let r = &v["result"];
    assert_eq!(keys(r), want(&["output", "error"]), "{r}");
    let e = r["error"].as_str().unwrap();
    assert!(!e.is_empty());
    assert!(r["output"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
    assert!(r["history"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
}

#[test]
fn accepted_input_has_no_error_and_reports_the_prompt() {
    let mut app = app();
    let v = call(&mut app, "cmdline.input", json!({"text": "circle"}));
    let r = &v["result"];
    assert_eq!(keys(r), want(&["output"]), "{r}");
    assert_eq!(r["running"], "circle");
    assert!(!r["accept"].is_null(), "{r}");
    assert_eq!(keys(&call(&mut app, "cmdline.state", json!({}))["result"]), want(&[]));
}

#[test]
fn open_without_a_path_is_refused_before_the_engine() {
    let mut app = app();
    assert_eq!(call(&mut app, "app.open", json!({})), json!({"ok": false, "error": "missing path"}));
    assert_eq!(call(&mut app, "app.open", json!({"path": 3})), json!({"ok": false, "error": "missing path"}));
}
