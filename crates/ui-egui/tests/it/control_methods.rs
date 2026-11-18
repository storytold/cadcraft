//! `control.methods` lists what the control channel answers, matching the table in
//! `docs/control-protocol.md`; `ping` answers; an unknown method points to `control.methods` (#843).

use std::collections::BTreeSet;

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

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// The method names in the first column of the doc's method table.
fn documented() -> BTreeSet<String> {
    let doc = include_str!("../../../../docs/control-protocol.md");
    let table = doc.lines().skip_while(|l| !l.starts_with("| Method |")).skip(2).take_while(|l| l.starts_with('|'));
    table.filter_map(|l| l.split('|').nth(1)).flat_map(|cell| cell.split('`').skip(1).step_by(2).map(str::to_string).collect::<Vec<_>>()).collect()
}

fn listed(app: &mut CadApp) -> Vec<Value> {
    let v = call(app, "control.methods", json!({}));
    assert_eq!(v["ok"], true, "{v}");
    v["result"].as_array().cloned().unwrap_or_default()
}

#[test]
fn control_methods_matches_the_documented_table() {
    let mut app = app();
    let methods = listed(&mut app);
    for m in &methods {
        assert!(m["method"].is_string() && m["params"].is_string() && m["description"].as_str().is_some_and(|d| !d.is_empty()), "{m}");
    }
    let names: BTreeSet<String> = methods.iter().filter_map(|m| m["method"].as_str().map(str::to_string)).collect();
    assert_eq!(names.len(), methods.len(), "a method is listed twice");
    let doc = documented();
    assert!(doc.contains("control.methods") && doc.contains("ping"), "{doc:?}");
    assert_eq!(names, doc);
}

#[test]
fn every_listed_method_is_answered() {
    let mut app = app();
    let missing = std::env::temp_dir().join("cadcraft-843-no-such-dir").join("x.dxf");
    let params = json!({"path": missing.to_string_lossy()});
    for m in listed(&mut app) {
        let name = m["method"].as_str().unwrap_or_default();
        let r = call(&mut app, name, params.clone());
        assert!(!r["error"].as_str().is_some_and(|e| e.starts_with("unknown method")), "{name}: {r}");
    }
}

#[test]
fn ping_answers() {
    assert_eq!(call(&mut app(), "ping", json!({})), json!({"ok": true, "result": "pong"}));
}

#[test]
fn unknown_method_points_to_control_methods() {
    let r = call(&mut app(), "help", json!({}));
    assert_eq!(r["ok"], false);
    let e = r["error"].as_str().unwrap_or_default();
    assert!(e.starts_with("unknown method `help`") && e.contains("control.methods"), "{e}");
}
