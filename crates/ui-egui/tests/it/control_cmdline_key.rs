//! The app's control channel answers `cmdline.key` like `cmdline.input` and like the headless MCP
//! backend (`crates/mcp/tests/it/cmdline_key.rs`): a refused Enter is echoed and reported in `error`.

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

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

/// The `cmdline.state` fields plus `extra`.
fn want(app: &mut CadApp, extra: &[&str]) -> Vec<String> {
    let mut k = keys(&call(app, "cmdline.state", json!({}))["result"]);
    k.extend(extra.iter().map(ToString::to_string));
    k.sort();
    k
}

#[test]
fn refused_enter_is_echoed_and_reported_in_error() {
    // Enter repeats CLOSE, which needs an open drawing.
    let mut app = CadApp::new(Session::new(), Services::default());
    app.session.cmdline("close").unwrap();
    let v = call(&mut app, "cmdline.key", json!({"key": "enter"}));
    assert_eq!(v["ok"], true, "{v}");
    let r = &v["result"];
    assert_eq!(keys(r), want(&mut app, &["output", "error"]), "{r}");
    let e = r["error"].as_str().unwrap();
    assert!(!e.is_empty());
    assert!(r["output"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
    assert!(r["history"].as_array().unwrap().iter().any(|l| l.as_str() == Some(e)), "{r}");
}

#[test]
fn accepted_keys_have_output_and_no_error() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for key in ["enter", "escape"] {
        app.session.cmdline("circle").unwrap();
        let v = call(&mut app, "cmdline.key", json!({ "key": key }));
        let r = &v["result"];
        assert_eq!(keys(r), want(&mut app, &["output"]), "{key}: {r}");
        assert!(r["running"].is_null(), "{key}: {r}");
        assert!(r["output"].as_array().unwrap().iter().any(|l| l.as_str() == Some("*Cancel*")), "{key}: {r}");
    }
}
