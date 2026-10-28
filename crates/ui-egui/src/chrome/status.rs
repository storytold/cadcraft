//! Status-item visibility is an application preference, independent of drafting settings.
use crate::CadApp;
use serde_json::{Value, json};

/// Stable preference IDs paired with translated status-item labels.
pub const ITEMS: &[(&str, &str)] = &[
    ("coordinates", "Coordinates"),
    ("paperSpace", "Paper Space"),
    ("annotationScale", "Annotation scale"),
    ("viewportScale", "Viewport scale"),
    ("grid", "Grid (F7)"),
    ("snap", "Snap Mode (F9)"),
    ("ortho", "Ortho Mode (F8)"),
    ("polar", "Polar Tracking (F10)"),
    ("isodraft", "Isometric Drafting"),
    ("otrack", "Object Snap Tracking (F11)"),
    ("osnap", "Object Snap (F3)"),
    ("lwdisplay", "Show/Hide Lineweight"),
    ("transparencydisplay", "Show/Hide Transparency"),
    ("dynmode", "Dynamic Input (F12)"),
    ("qpmode", "Quick Properties"),
    ("annotationVisibility", "Annotation Visibility"),
    ("workspace", "Workspace Switching"),
    ("draftingSettings", "Drafting Settings"),
];
/// Reject unknown or duplicate IDs before replacing stored preferences.
pub fn validate(hidden: &[String]) -> Result<(), String> {
    if hidden.len() > ITEMS.len()
        || hidden.iter().enumerate().any(|(index, id)| !ITEMS.iter().any(|(known, _)| id == known) || hidden[..index].contains(id))
    {
        return Err("Invalid status bar items".into());
    }
    Ok(())
}
/// Whether a status item is shown, independently of its drafting setting.
pub fn visible(app: &CadApp, id: &str) -> bool {
    !app.ui.hidden_status_items.iter().any(|hidden| hidden == id)
}
pub(super) fn command_id(command: &str) -> &str {
    match command {
        "annovisible.toggle" => "annotationVisibility",
        "ui.noop" => "workspace",
        "ui.dialog.dsettings" => "draftingSettings",
        _ => command,
    }
}
pub(crate) fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id == "ui.status.reset" {
        app.ui.hidden_status_items.clear();
        return Some(Ok(json!({"hiddenStatusItems":[]})));
    }
    if id != "ui.status.item" {
        return None;
    }
    Some((|| {
        let item =
            params.get("item").and_then(Value::as_str).filter(|id| ITEMS.iter().any(|(known, _)| known == id)).ok_or("Unknown status bar item")?;
        let shown = match params.get("visible") {
            None => !visible(app, item),
            Some(v) => v.as_bool().ok_or("visible must be a boolean")?,
        };
        let mut hidden = app.ui.hidden_status_items.clone();
        hidden.retain(|id| id != item);
        if !shown {
            hidden.push(item.to_string());
        }
        validate(&hidden)?;
        app.ui.hidden_status_items = hidden;
        Ok(json!({"item":item,"visible":shown}))
    })())
}
pub(crate) fn menu(app: &mut CadApp, ui: &mut egui::Ui) {
    if ui.button(crate::tl!("Show All Status Items")).clicked() {
        let _ = app.run("ui.status.reset", json!({}));
    }
    ui.separator();
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for (id, label) in ITEMS {
            let mut shown = visible(app, id);
            if ui.checkbox(&mut shown, crate::i18n::t(label)).changed() {
                let _ = app.run("ui.status.item", json!({"item":id,"visible":shown}));
            }
        }
    });
}
