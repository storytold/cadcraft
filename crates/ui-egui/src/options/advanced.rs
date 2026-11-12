//! The Options dialog's All settings tab: every setting the OPTIONS JSON form reports, one row
//! each, edited in place, and the whole document as editable JSON. Changes go through the same
//! `options` call as the other tabs, so the values are checked as everywhere else; a refused value
//! is shown here and nothing changes.

use egui::RichText;
use serde_json::{Map, Value};

use crate::CadApp;
use crate::theme::Tokens;

/// The last refused change, kept until a change is accepted (egui temp data).
pub(super) const ERROR: &str = "cc_options_advanced_error";
const FILTER: &str = "cc_options_advanced_filter";
const JSON_TEXT: &str = "cc_options_advanced_json";
const JSON_OPEN: &str = "cc_options_advanced_json_open";

/// The compact text of a value for its edit field.
fn compact(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// What the text of an edit field means for a setting whose value is `old`: a string stays a
/// string, a number or boolean is read as one, an array or object as JSON; `null` takes whatever
/// JSON the text is, else the text. `Err` is the reason it is refused.
pub(super) fn parse_edit(old: &Value, text: &str) -> Result<Value, String> {
    let t = text.trim();
    match old {
        Value::String(_) => Ok(Value::String(text.to_string())),
        Value::Bool(_) => match t.to_ascii_lowercase().as_str() {
            "true" | "1" | "on" | "yes" => Ok(Value::Bool(true)),
            "false" | "0" | "off" | "no" => Ok(Value::Bool(false)),
            _ => Err("expected true or false".into()),
        },
        Value::Number(n) if n.is_i64() || n.is_u64() => {
            t.parse::<i64>().map(Value::from).or_else(|_| t.parse::<f64>().map(Value::from)).map_err(|_| "expected a number".to_string())
        }
        Value::Number(_) => t.parse::<f64>().ok().filter(|x| x.is_finite()).map(Value::from).ok_or_else(|| "expected a number".to_string()),
        Value::Array(_) | Value::Object(_) => serde_json::from_str(t).map_err(|e| format!("not valid JSON: {e}")),
        Value::Null => {
            if t.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(serde_json::from_str(t).unwrap_or_else(|_| Value::String(text.to_string())))
            }
        }
    }
}

/// The settings of `new` that differ from `old` (an object each): what an edited document changes.
pub(super) fn changed(old: &Map<String, Value>, new: &Map<String, Value>) -> Map<String, Value> {
    new.iter()
        .filter(|(k, v)| old.get(*k) != Some(*v))
        .filter_map(|(k, v)| {
            let v = edit_of(k, old.get(k), v);
            (!v.as_object().is_some_and(Map::is_empty)).then(|| (k.clone(), v))
        })
        .collect()
}

/// What an edit of `key` from `old` to `new` sends. `dimStyle` sets the fields it names on the
/// current dimension style and keeps them for new drawings, so only the fields that changed go:
/// the whole reported style would overwrite the saved defaults of fields the user did not touch.
/// Every other setting is sent as edited.
fn edit_of(key: &str, old: Option<&Value>, new: &Value) -> Value {
    match (key, old, new) {
        ("dimStyle", Some(Value::Object(old)), Value::Object(new)) => {
            Value::Object(new.iter().filter(|(k, v)| old.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect())
        }
        _ => new.clone(),
    }
}

/// What a JSON document `new` changes: settings (`report` keys) and interface state (`interface`
/// keys), or the first key that is neither.
pub(super) fn split_edits(
    report: &Map<String, Value>,
    interface: &Map<String, Value>,
    new: &Map<String, Value>,
) -> Result<(Map<String, Value>, Map<String, Value>), String> {
    if let Some(key) = new.keys().find(|k| !report.contains_key(*k) && !interface.contains_key(*k)) {
        return Err(key.clone());
    }
    let mut diff = changed(report, new);
    diff.retain(|k, _| report.contains_key(k));
    let mut ui_diff = changed(interface, new);
    ui_diff.retain(|k, _| interface.contains_key(k));
    Ok((diff, ui_diff))
}

/// A text field that commits when it loses focus or on Enter; `Some(text)` on a change.
fn commit_text(ui: &mut egui::Ui, id: egui::Id, current: &str, width: f32, label: egui::Id) -> Option<String> {
    let mut buf = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(width)).labelled_by(label);
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if buf != current {
            return Some(buf);
        }
    }
    None
}

/// Show `text` above the All settings rows until the next edit (`None` clears it): a refused value
/// or JSON document says why.
fn set_error(ui: &egui::Ui, text: Option<String>) {
    let id = egui::Id::unique(ERROR);
    ui.data_mut(|d| {
        match text {
            Some(t) => {
                d.insert_temp(id, t);
            }
            None => d.remove::<String>(id),
        };
    });
}

/// Interface state fields the All settings tab leaves alone: the open dialog and the panel layout
/// (their own commands and dialogs change them).
const INTERFACE_SKIPPED: [&str; 2] = ["dialog", "docking"];

/// The interface state (`ui.set` fields) that the options JSON form does not report already.
fn interface_state(app: &CadApp, report: &Map<String, Value>) -> Map<String, Value> {
    match serde_json::to_value(&app.ui) {
        Ok(Value::Object(m)) => m.into_iter().filter(|(k, _)| !report.contains_key(k) && !INTERFACE_SKIPPED.contains(&k.as_str())).collect(),
        _ => Map::new(),
    }
}

/// `edits` go through the `options` call; `ui_edits` through [`CadApp::set_ui_state`].
pub(super) fn advanced(app: &CadApp, ui: &mut egui::Ui, edits: &mut Map<String, Value>, ui_edits: &mut Map<String, Value>) {
    let t = Tokens::get();
    let report = match super::report(app) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    if let Some(e) = ui.data(|d| d.get_temp::<String>(egui::Id::unique(ERROR))) {
        ui.label(RichText::new(e).color(egui::Color32::from_rgb(0xe0, 0x5a, 0x4a)));
    }
    let mut filter = ui.data(|d| d.get_temp::<String>(egui::Id::unique(FILTER))).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(crate::tl!("Filter"));
        if ui.add(egui::TextEdit::singleline(&mut filter).desired_width(180.0)).changed() {
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(FILTER), filter.clone()));
        }
        ui.label(RichText::new(crate::tf!("{n} settings", n = report.len() + interface_state(app, &report).len())).small().color(t.text_dim));
    });
    let needle = filter.trim().to_ascii_lowercase();
    let interface = interface_state(app, &report);
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("options_advanced").max_height(250.0).auto_shrink([false, true]).show(ui, |ui| {
        super::dialog::grid(ui, "options_advanced_grid", |ui| {
            for (key, value) in &report {
                if !needle.is_empty() && !key.to_ascii_lowercase().contains(&needle) {
                    continue;
                }
                let label = ui.label(RichText::new(key).monospace());
                row(ui, key, label.id, value, edits);
                ui.end_row();
            }
            let shown: Vec<_> = interface.iter().filter(|(k, _)| needle.is_empty() || k.to_ascii_lowercase().contains(&needle)).collect();
            if !shown.is_empty() {
                ui.label(RichText::new(crate::tl!("Interface state")).strong());
                ui.label(RichText::new(crate::tl!("Panels, toolbars and recent files")).small().color(t.text_dim));
                ui.end_row();
                for (key, value) in shown {
                    let label = ui.label(RichText::new(key).monospace());
                    row(ui, key, label.id, value, ui_edits);
                    ui.end_row();
                }
            }
        });
    });
    ui.add_space(4.0);
    json_editor(ui, &report, &interface, edits, ui_edits);
}

/// One setting's editor, named by its row's `label` for screen readers: a check box for a
/// boolean, a number field for a number, else a text field.
fn row(ui: &mut egui::Ui, key: &str, label: egui::Id, value: &Value, edits: &mut Map<String, Value>) {
    match value {
        Value::Bool(b) => {
            let mut on = *b;
            if ui.checkbox(&mut on, "").labelled_by(label).changed() {
                edits.insert(key.into(), Value::Bool(on));
            }
        }
        Value::Number(n) if n.is_i64() || n.is_u64() => {
            let mut x = n.as_i64().unwrap_or(0);
            if ui.add(egui::DragValue::new(&mut x).speed(1.0)).labelled_by(label).changed() {
                edits.insert(key.into(), Value::from(x));
            }
        }
        Value::Number(n) => {
            let mut x = n.as_f64().unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut x).speed(0.05).max_decimals(6)).labelled_by(label).changed() {
                edits.insert(key.into(), Value::from(x));
            }
        }
        other => {
            let id = egui::Id::unique(("cc_options_advanced_text", key));
            if let Some(text) = commit_text(ui, id, &compact(other), 290.0, label) {
                match parse_edit(other, &text) {
                    Ok(v) => {
                        let v = edit_of(key, Some(other), &v);
                        if !v.as_object().is_some_and(Map::is_empty) {
                            edits.insert(key.into(), v);
                        }
                    }
                    Err(e) => set_error(ui, Some(format!("{key}: {e}"))),
                }
            }
        }
    }
}

/// The whole document as JSON to edit, settings and interface state: Apply sends what changed
/// through `options` (settings) or `ui_edits` (interface state); an unknown key changes nothing.
fn json_editor(
    ui: &mut egui::Ui,
    report: &Map<String, Value>,
    interface: &Map<String, Value>,
    edits: &mut Map<String, Value>,
    ui_edits: &mut Map<String, Value>,
) {
    let open_id = egui::Id::unique(JSON_OPEN);
    let text_id = egui::Id::unique(JSON_TEXT);
    let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let r = egui::CollapsingHeader::new(crate::tl!("Edit as JSON")).id_salt("options_advanced_json").default_open(open).show(ui, |ui| {
        let pretty = || {
            let mut all = report.clone();
            all.extend(interface.clone());
            serde_json::to_string_pretty(&Value::Object(all)).unwrap_or_default()
        };
        let mut text = ui.data(|d| d.get_temp::<String>(text_id)).unwrap_or_else(pretty);
        let edit = egui::TextEdit::multiline(&mut text).code_editor().desired_rows(6).desired_width(f32::INFINITY);
        if ui.add(edit).changed() {
            ui.data_mut(|d| d.insert_temp(text_id, text.clone()));
        }
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("Apply")).clicked() {
                match serde_json::from_str::<Value>(&text) {
                    Ok(Value::Object(new)) => match split_edits(report, interface, &new) {
                        Ok((diff, ui_diff)) => {
                            set_error(ui, None);
                            edits.extend(diff);
                            ui_edits.extend(ui_diff);
                            ui.data_mut(|d| d.remove::<String>(text_id));
                        }
                        Err(key) => set_error(ui, Some(format!("{}: {key}", crate::tl!("Unknown setting")))),
                    },
                    Ok(_) => set_error(ui, Some(crate::tl!("The settings must be a JSON object.").to_string())),
                    Err(e) => set_error(ui, Some(format!("{}: {e}", crate::tl!("Not valid JSON")))),
                }
            }
            if ui.button(crate::tl!("Reload")).clicked() {
                ui.data_mut(|d| d.remove::<String>(text_id));
                set_error(ui, None);
            }
            if ui.button(crate::tl!("Copy as JSON")).clicked() {
                ui.ctx().copy_text(pretty());
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(open_id, r.openness > 0.5));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A text field's edit becomes the same kind of JSON value as the setting it edits (bool,
    /// number, list), or an error the tab shows.
    #[test]
    fn edited_text_is_read_as_the_kind_of_value_it_replaces() {
        assert_eq!(parse_edit(&json!("dark"), "light"), Ok(json!("light")));
        assert_eq!(parse_edit(&json!("a"), " spaced "), Ok(json!(" spaced ")), "a string is kept as typed");
        assert_eq!(parse_edit(&json!(true), "OFF"), Ok(json!(false)));
        assert!(parse_edit(&json!(true), "maybe").is_err());
        assert_eq!(parse_edit(&json!(7), "12"), Ok(json!(12)));
        assert_eq!(parse_edit(&json!(7), "1.5"), Ok(json!(1.5)), "the engine decides whether a whole number is needed");
        assert!(parse_edit(&json!(7), "x").is_err());
        assert_eq!(parse_edit(&json!(0.5), "2"), Ok(json!(2.0)));
        assert!(parse_edit(&json!(0.5), "NaN").is_err() && parse_edit(&json!(0.5), "inf").is_err());
        assert_eq!(parse_edit(&json!([1.0, 1.0]), "[2, 3]"), Ok(json!([2, 3])));
        assert!(parse_edit(&json!([1.0, 1.0]), "[2,").is_err());
        assert_eq!(parse_edit(&Value::Null, ""), Ok(Value::Null), "empty means the default");
        assert_eq!(parse_edit(&Value::Null, "#ff0000"), Ok(json!("#ff0000")));
        assert_eq!(parse_edit(&Value::Null, "12"), Ok(json!(12)));
    }

    /// Apply sends only what changed, so an unedited document re-sets nothing.
    #[test]
    fn only_the_settings_that_differ_are_sent() {
        let old = json!({ "a": 1, "b": "x", "c": null }).as_object().cloned().unwrap_or_default();
        let new = json!({ "a": 1, "b": "y", "c": null, "d": true }).as_object().cloned().unwrap_or_default();
        assert_eq!(Value::Object(changed(&old, &new)), json!({ "b": "y", "d": true }));
        assert!(changed(&old, &old).is_empty());
    }

    /// An edit of the current dimension style sends only the fields that changed, so the saved
    /// defaults of the others (`dimDefaults`) stay as they were; other objects go whole.
    #[test]
    fn a_dim_style_edit_sends_only_the_changed_fields() {
        let map = |v: Value| v.as_object().cloned().unwrap_or_default();
        let old = map(json!({ "dimStyle": { "DIMDEC": 2, "DIMTXT": 2.5 }, "other": { "a": 1, "b": 2 } }));
        let new = map(json!({ "dimStyle": { "DIMDEC": 2, "DIMTXT": 3.5 }, "other": { "a": 1, "b": 3 } }));
        assert_eq!(Value::Object(changed(&old, &new)), json!({ "dimStyle": { "DIMTXT": 3.5 }, "other": { "a": 1, "b": 3 } }));
        assert_eq!(edit_of("dimStyle", old.get("dimStyle"), &json!({ "DIMDEC": 2, "DIMTXT": 2.5 })), json!({}));
    }

    /// Settings go to `options`, interface keys to `set_ui_state`, and a key that is neither
    /// changes nothing.
    #[test]
    fn the_json_document_routes_settings_and_interface_state_and_refuses_unknown_keys() {
        let map = |v: Value| v.as_object().cloned().unwrap_or_default();
        let report = map(json!({ "a": 1, "b": "x" }));
        let interface = map(json!({ "showToolbar": true }));
        let (settings, ui) = split_edits(&report, &interface, &map(json!({ "a": 2, "b": "x", "showToolbar": false }))).unwrap();
        assert_eq!((Value::Object(settings), Value::Object(ui)), (json!({ "a": 2 }), json!({ "showToolbar": false })));
        assert_eq!(split_edits(&report, &interface, &map(json!({ "nope": 1 }))), Err("nope".to_string()));
    }
}
