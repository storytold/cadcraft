//! Panels an external program declares over the control channel, and the
//! ribbon of buttons such a panel shows.
//!
//! PoC for the plug-in UI surface (see docs/customization.md, "Compiled
//! plug-ins… planned"): another program opens the control channel and
//! declares a titled panel with buttons; each button carries a command id
//! and JSON parameters that run when clicked, exactly like
//! `engine.execute`. Optional message lines let the program keep its
//! users posted (last lines shown first). Panels are per-session: they
//! live while the app runs; the declaring program re-declares after a
//! restart, which keeps the app free of hidden third-party state.
//!
//! Bounds keep a hostile or looping caller from exhausting memory: at
//! most 16 panels, 64 buttons per panel, 200 message lines per panel.

use std::collections::{BTreeMap, VecDeque};
use std::sync::RwLock;

use serde_json::{Value, json};

use crate::CadApp;

const MAX_PANELS: usize = 16;
const MAX_BUTTONS: usize = 64;
const MAX_LINES: usize = 200;

/// One button: label as shown, `command` runs through the same `app.run`
/// path the control channel's `engine.execute` uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub command: String,
    pub params: Value,
    pub tooltip: Option<String>,
}

/// Declared panel state (title, buttons and the message feed).
#[derive(Clone, Debug, Default)]
struct PanelSpec {
    title: String,
    buttons: Vec<Button>,
    lines: VecDeque<String>,
    show: bool,
}

static PANELS: RwLock<BTreeMap<String, PanelSpec>> = RwLock::new(BTreeMap::new());

pub fn route(_app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        "ui.panel.register" => Some(register(params)),
        "ui.panel.unregister" => Some(unregister(params)),
        "ui.panel.message" => Some(message(params)),
        "ui.panel.list" => Some(list()),
        _ => None,
    }
}

fn str_param<'a>(params: &'a Value, key: &str, command: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("`{command}` requires `{key}` as a non-empty string"))
}

fn register(params: &Value) -> Result<Value, String> {
    let id = str_param(params, "id", "ui.panel.register")?.to_string();
    let title = str_param(params, "title", "ui.panel.register")?.to_string();
    let show = params.get("show").and_then(Value::as_bool).unwrap_or(true);
    let mut buttons = Vec::new();
    if let Some(list) = params.get("buttons").and_then(Value::as_array) {
        if list.len() > MAX_BUTTONS {
            return Err(format!("`ui.panel.register` allows at most {MAX_BUTTONS} buttons per panel"));
        }
        for (ix, b) in list.iter().enumerate() {
            let label = b
                .get("label")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| format!("button {ix}: `label` is required"))?
                .to_string();
            let command = b
                .get("command")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| format!("button {ix}: `command` is required"))?
                .to_string();
            let params_b = b.get("params").cloned().unwrap_or(Value::Null);
            buttons.push(Button {
                id: b.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                label,
                command,
                params: params_b,
                tooltip: b.get("tooltip").and_then(Value::as_str).map(String::from),
            });
        }
    }
    {
        let mut panels = PANELS.write().unwrap_or_else(|e| e.into_inner());
        if !panels.contains_key(&id) && panels.len() >= MAX_PANELS {
            return Err(format!("`ui.panel.register` allows at most {MAX_PANELS} panels at once"));
        }
        // Re-registering refreshes the spec and keeps the message feed.
        let lines = panels.get(&id).map(|p| p.lines.clone()).unwrap_or_default();
        panels.insert(id.clone(), PanelSpec { title, buttons, lines, show });
    }
    Ok(json!({ "registered": true, "id": id }))
}

fn unregister(params: &Value) -> Result<Value, String> {
    let id = str_param(params, "id", "ui.panel.unregister")?;
    let removed = PANELS.write().unwrap_or_else(|e| e.into_inner()).remove(id).is_some();
    Ok(json!({ "unregistered": removed, "id": id }))
}

fn message(params: &Value) -> Result<Value, String> {
    let id = str_param(params, "id", "ui.panel.message")?;
    let text = str_param(params, "text", "ui.panel.message")?.to_string();
    let mut panels = PANELS.write().unwrap_or_else(|e| e.into_inner());
    let Some(spec) = panels.get_mut(id) else {
        return Err(format!("no panel `{id}` is registered"));
    };
    spec.lines.push_front(text);
    spec.lines.truncate(MAX_LINES);
    Ok(json!({ "delivered": true, "id": id }))
}

fn list() -> Result<Value, String> {
    let panels = PANELS.read().unwrap_or_else(|e| e.into_inner());
    let items: Vec<Value> = panels
        .iter()
        .map(|(id, spec)| {
            json!({
                "id": id,
                "title": spec.title,
                "show": spec.show,
                "buttons": spec.buttons.iter().map(|b| json!({
                    "id": b.id,
                    "label": b.label,
                    "command": b.command,
                    "params": b.params,
                })).collect::<Vec<_>>(),
                "lines": spec.lines.len(),
            })
        })
        .collect();
    Ok(json!({ "panels": items, "count": items.len() }))
}

/// Draw every visible declared panel. Called once per frame from the
/// main [`crate::CadApp`] draw pass.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    // Snapshot (panels, buttons, lines) so the UI works on data, not on
    // the lock held across a repaint.
    let snapshot: BTreeMap<String, (String, Vec<Button>, VecDeque<String>)> = {
        let panels = PANELS.read().unwrap_or_else(|e| e.into_inner());
        panels.iter().filter(|(_, s)| s.show).map(|(id, s)| (id.clone(), (s.title.clone(), s.buttons.clone(), s.lines.clone()))).collect()
    };
    for (id, (title, buttons, lines)) in snapshot {
        egui::Window::new(title).id(egui::Id::unique(format!("plugin-panel-{id}"))).show(ctx, |ui| {
            for b in &buttons {
                let label = egui::RichText::new(&b.label);
                if ui.button(label).clicked()
                    && let Err(e) = app.run(&b.command, b.params.clone())
                {
                    app.session.echo(e);
                }
            }
            if !lines.is_empty() {
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for line in &lines {
                        ui.label(egui::RichText::new(line).weak());
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn register_list_unregister_round_trip() {
        let r = register(&json!({
            "id": "p1",
            "title": "T",
            "buttons": [{"label": "B", "command": "line", "params": {"points": [[0,0],[1,1]]}}]
        }));
        assert!(r.is_ok());
        let l = list().unwrap();
        assert_eq!(l["count"], json!(1));
        assert_eq!(l["panels"][0]["buttons"][0]["command"], json!("line"));
        let u = unregister(&json!({ "id": "p1" })).unwrap();
        assert_eq!(u["unregistered"], json!(true));
        assert!(unregister(&json!({ "id": "p1" })).is_ok());
        assert_eq!(list().unwrap()["count"], json!(0));
    }

    #[test]
    fn register_rejects_missing_fields() {
        assert!(register(&json!({ "title": "T" })).is_err());
        assert!(register(&json!({ "id": "x", "title": "" })).is_err());
        assert!(register(&json!({ "id": "x" })).is_err());
        assert!(register(&json!({ "id": "x", "title": "T", "buttons": [{"command": "line"}] })).is_err());
    }

    #[test]
    fn message_needs_a_registered_panel() {
        assert!(message(&json!({ "id": "nope", "text": "hi" })).is_err());
        register(&json!({ "id": "p2", "title": "T" })).unwrap();
        assert!(message(&json!({ "id": "p2", "text": "hi" })).is_ok());
        unregister(&json!({ "id": "p2" })).unwrap();
    }
}
