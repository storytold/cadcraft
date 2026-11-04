//! Browser panel layout storage, independent of drawings and native workspace files.

use cadcraft_ui_egui::{UiState, docking};
use serde_json::{Value, json};

#[cfg(target_arch = "wasm32")]
pub const KEY: &str = "cadcraft.panel-layout";
const MAX_BYTES: usize = 64 * 1024;

/// Serialize only validated panel placement and visibility, with a bounded record size.
pub fn save(ui: &UiState) -> Result<String, String> {
    docking::validate(&ui.docking)?;
    let saved = json!({
        "version": docking::LAYOUT_VERSION,
        "docking": ui.docking,
        "showToolsets": ui.show_toolsets,
        "showPalettes": ui.show_palettes,
    })
    .to_string();
    if saved.len() > MAX_BYTES {
        return Err("panel layout exceeds 64 KiB".into());
    }
    Ok(saved)
}

/// Validate the entire optional record before changing any UI field.
pub fn restore(ui: &mut UiState, saved: &str) -> Result<(), String> {
    if saved.len() > MAX_BYTES {
        return Err("panel layout exceeds 64 KiB".into());
    }
    let value: Value = serde_json::from_str(saved).map_err(|error| error.to_string())?;
    if value.get("version").and_then(Value::as_u64) != Some(u64::from(docking::LAYOUT_VERSION)) {
        return Err("unsupported panel layout version".into());
    }
    let workspace: docking::Workspace =
        serde_json::from_value(value.get("docking").cloned().ok_or("missing panel layout")?).map_err(|error| error.to_string())?;
    docking::validate(&workspace)?;
    let tools = value.get("showToolsets").and_then(Value::as_bool).ok_or("invalid Tool Sets visibility")?;
    let palettes = value.get("showPalettes").and_then(Value::as_bool).ok_or("invalid palette visibility")?;
    ui.docking = workspace;
    ui.show_toolsets = tools;
    ui.show_palettes = palettes;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};

    #[test]
    fn floating_and_hidden_panels_survive_storage_without_replacing_drawing_state() {
        let mut source = CadApp::new(Session::default(), Services::default());
        source.run("ui.dock", json!({"operation":"float","panel":"toolSets","rect":[120,90,340,410]})).unwrap();
        source.run("ui.dock", json!({"operation":"close","panel":"properties"})).unwrap();
        let saved = save(&source.ui).unwrap();
        let mut destination = CadApp::new(Session::default(), Services::default());
        destination.session.start("line").unwrap();
        let before = destination.session.doc().unwrap().clone();
        restore(&mut destination.ui, &saved).unwrap();
        assert_eq!(destination.ui.docking, source.ui.docking);
        assert_eq!(destination.ui.show_toolsets, source.ui.show_toolsets);
        assert_eq!(destination.ui.show_palettes, source.ui.show_palettes);
        assert_eq!(destination.session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
        assert_eq!(destination.session.doc().unwrap(), &before);
    }

    #[test]
    fn malformed_or_unsupported_storage_is_rejected_atomically() {
        let mut ui = UiState::default();
        let before = serde_json::to_value(&ui).unwrap();
        let valid: Value = serde_json::from_str(&save(&ui).unwrap()).unwrap();
        for (field, bad) in [
            ("version", json!(0)),
            ("version", json!(docking::LAYOUT_VERSION + 1)),
            ("docking", json!({})),
            ("showToolsets", json!("false")),
            ("showPalettes", Value::Null),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = bad;
            assert!(restore(&mut ui, &invalid.to_string()).is_err());
            assert_eq!(serde_json::to_value(&ui).unwrap(), before);
        }
        for invalid in ["not json".to_string(), " ".repeat(MAX_BYTES + 1)] {
            assert!(restore(&mut ui, &invalid).is_err());
            assert_eq!(serde_json::to_value(&ui).unwrap(), before);
        }
    }

    #[test]
    fn storage_cannot_remove_the_document_view() {
        let mut ui = UiState::default();
        let before = serde_json::to_value(&ui).unwrap();
        let mut invalid = ui.clone();
        invalid.docking.layout.root = None;
        assert!(save(&invalid).is_err());
        let mut record: Value = serde_json::from_str(&save(&ui).unwrap()).unwrap();
        record["docking"] = serde_json::to_value(&invalid.docking).unwrap();
        assert!(restore(&mut ui, &record.to_string()).is_err());
        assert_eq!(serde_json::to_value(&ui).unwrap(), before);
    }
}
