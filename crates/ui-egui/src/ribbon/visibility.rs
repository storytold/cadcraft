//! Validated ribbon visibility preferences and command-backed customization.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::TABS;
use crate::CadApp;

/// Canonical tab ids and English panel titles identify hidden items, never translated labels.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Visibility {
    pub hidden_tabs: Vec<String>,
    pub hidden_panels: Vec<(String, String)>,
}

impl Visibility {
    /// Reject unknown, duplicate and excessive entries atomically.
    pub fn validate(&self) -> Result<(), String> {
        if self.hidden_tabs.len() > TABS.len() || self.hidden_panels.len() > TABS.iter().map(|t| t.panels.len()).sum::<usize>() {
            return Err("ribbon visibility exceeds the catalog size".into());
        }
        for (i, id) in self.hidden_tabs.iter().enumerate() {
            if !TABS.iter().any(|t| t.id == id) || self.hidden_tabs[..i].contains(id) {
                return Err("unknown or duplicate ribbon tab".into());
            }
        }
        for (i, key) in self.hidden_panels.iter().enumerate() {
            if !TABS.iter().any(|t| t.id == key.0 && t.panels.iter().any(|p| p.title == key.1)) || self.hidden_panels[..i].contains(key) {
                return Err("unknown or duplicate ribbon panel".into());
            }
        }
        Ok(())
    }

    /// Whether a catalog tab is available in the ribbon tab row.
    pub fn tab_visible(&self, id: &str) -> bool {
        !self.hidden_tabs.iter().any(|hidden| hidden == id)
    }

    /// Whether a panel is available within its owning tab.
    pub fn panel_visible(&self, tab: &str, title: &str) -> bool {
        !self.hidden_panels.iter().any(|(t, p)| t == tab && p == title)
    }
}

/// Handle UI-only visibility commands without starting or cancelling an engine command.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id == "ui.ribbon.visibility.reset" {
        app.ui.ribbon_visibility = Visibility::default();
        return Some(Ok(Value::Null));
    }
    if id != "ui.ribbon.visibility" {
        return None;
    }
    Some((|| {
        let tab = params.get("tab").and_then(Value::as_str).ok_or("tab is required")?;
        let on = params.get("on").and_then(Value::as_bool).ok_or("on must be a boolean")?;
        let mut next = app.ui.ribbon_visibility.clone();
        if let Some(panel) = params.get("panel") {
            let panel = panel.as_str().ok_or("panel must be a string")?;
            let key = (tab.to_string(), panel.to_string());
            next.hidden_panels.retain(|hidden| hidden != &key);
            // Validate even a request to show an unknown panel.
            if !TABS.iter().any(|t| t.id == tab && t.panels.iter().any(|p| p.title == panel)) {
                return Err("unknown ribbon panel".into());
            }
            if !on {
                next.hidden_panels.push(key);
            }
        } else {
            if !TABS.iter().any(|t| t.id == tab) {
                return Err("unknown ribbon tab".into());
            }
            next.hidden_tabs.retain(|hidden| hidden != tab);
            if !on {
                next.hidden_tabs.push(tab.to_string());
            }
        }
        next.validate()?;
        app.ui.ribbon_visibility = next;
        Ok(Value::Null)
    })())
}

/// The same menu serves the ribbon background and its always-visible recovery button.
pub fn menu(app: &mut CadApp, ui: &mut egui::Ui, active: Option<&str>) {
    if ui.button(crate::tl!("Show All Ribbon Items")).clicked() {
        let _ = app.run("ui.ribbon.visibility.reset", json!({}));
    }
    ui.menu_button(crate::tl!("Show Tabs"), |ui| {
        for tab in TABS {
            let mut on = app.ui.ribbon_visibility.tab_visible(tab.id);
            if ui.checkbox(&mut on, super::tab_label(tab.title)).changed() {
                let _ = app.run("ui.ribbon.visibility", json!({"tab":tab.id,"on":on}));
            }
        }
    });
    if let Some(tab) = TABS.iter().find(|tab| Some(tab.id) == active) {
        ui.menu_button(crate::tl!("Show Panels"), |ui| {
            for panel in tab.panels {
                let mut on = app.ui.ribbon_visibility.panel_visible(tab.id, panel.title);
                if ui.checkbox(&mut on, crate::i18n::ribbon(panel.title)).changed() {
                    let _ = app.run("ui.ribbon.visibility", json!({"tab":tab.id,"panel":panel.title,"on":on}));
                }
            }
        });
    }
}
