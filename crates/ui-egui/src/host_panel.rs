//! A parent-owned table. The model is immutable between deliveries; interactions only emit events.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Model {
    pub title: String,
    pub sections: Vec<Section>,
    #[serde(default)]
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub filter: bool,
    #[serde(default)]
    pub empty: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Column {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Row {
    pub id: String,
    #[serde(default)]
    pub group: Option<String>,
    pub cells: BTreeMap<String, Cell>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(try_from = "WireCell")]
pub struct Cell {
    pub kind: Kind,
    pub label: Option<String>,
    pub disabled: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Text(String),
    Edit(String),
    Toggle(bool),
    Button(String),
}

#[derive(Deserialize)]
struct WireCell {
    kind: String,
    text: Option<String>,
    on: Option<bool>,
    label: Option<String>,
    #[serde(default)]
    disabled: bool,
    error: Option<String>,
}

impl TryFrom<WireCell> for Cell {
    type Error = String;

    fn try_from(cell: WireCell) -> Result<Self, String> {
        let kind = match cell.kind.as_str() {
            "text" => Kind::Text(cell.text.ok_or("missing text")?),
            "edit" => Kind::Edit(cell.text.ok_or("missing text")?),
            "button" => {
                if cell.label.is_none() {
                    return Err("missing label".into());
                }
                Kind::Button(cell.text.ok_or("missing text")?)
            }
            "toggle" => {
                if cell.label.is_none() {
                    return Err("missing label".into());
                }
                Kind::Toggle(cell.on.ok_or("missing on")?)
            }
            _ => Kind::Text(cell.text.unwrap_or_default()),
        };
        Ok(Self { kind, label: cell.label, disabled: cell.disabled, error: cell.error })
    }
}

fn bounded_string(value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max { Err(format!("host panel string exceeds {max} characters")) } else { Ok(()) }
}

/// Pure, atomic validation of the v1 parent message. `Ok(None)` removes the panel.
pub fn parse(message: &Value) -> Result<Option<Model>, String> {
    if message.get("type").and_then(Value::as_str) != Some("cadcraft:panel") {
        return Err("not a host panel message".into());
    }
    let panel = message.get("panel").ok_or("missing panel")?;
    if panel.is_null() {
        return Ok(None);
    }
    validate_collections(panel)?;
    let model = Model::deserialize(panel).map_err(|error| error.to_string())?;
    Ok(Some(model))
}

fn validate_collections(panel: &Value) -> Result<(), String> {
    // Check collection bounds before deserializing/copying the model.
    validate_strings(panel, &["title", "notice"])?;
    let sections = panel.get("sections").and_then(Value::as_array).ok_or("missing sections")?;
    if sections.len() > 8 {
        return Err("host panel exceeds 8 sections".into());
    }
    for section in sections {
        validate_strings(section, &["id", "title", "empty"])?;
        let columns = section.get("columns").and_then(Value::as_array).ok_or("missing columns")?;
        let rows = section.get("rows").and_then(Value::as_array).ok_or("missing rows")?;
        if columns.len() > 64 || rows.len() > 2000 {
            return Err("host panel exceeds column or row limit".into());
        }
        for column in columns {
            validate_strings(column, &["id", "label"])?;
        }
        if let Some(actions) = section.get("actions").and_then(Value::as_array) {
            for action in actions {
                validate_strings(action, &["id", "label"])?;
            }
        }
        for row in rows {
            validate_strings(row, &["id", "group"])?;
            let cells = row.get("cells").and_then(Value::as_object).ok_or("missing cells")?;
            if cells.len() > 128 {
                return Err("host panel exceeds 128 cells".into());
            }
            for key in cells.keys() {
                bounded_string(key, 64)?;
            }
            for cell in cells.values() {
                validate_strings(cell, &["kind", "text", "label", "error"])?;
            }
        }
    }
    Ok(())
}

fn validate_strings(value: &Value, fields: &[&str]) -> Result<(), String> {
    for field in fields {
        if let Some(text) = value.get(*field).and_then(Value::as_str) {
            bounded_string(text, if *field == "id" { 64 } else { 512 })?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "cadcraft:panel-edit")]
    Edit { section: String, row: String, column: String, value: Value },
    #[serde(rename = "cadcraft:panel-action")]
    Action { section: String, action: String },
}

type CellKey = (String, String, String);

struct Pending {
    original: String,
    buffer: String,
}

#[derive(Default)]
pub struct State {
    pub enabled: bool,
    pub model: Option<Model>,
    pub events: Vec<Event>,
    presented: bool,
    closed: bool,
    filters: BTreeMap<String, String>,
    edits: BTreeMap<CellKey, Pending>,
}

impl crate::CadApp {
    /// Deliver a validated parent snapshot. A user-closed panel stays closed until a new open.
    pub fn receive_host_panel(&mut self, model: Option<Model>) {
        if !self.host_panel.enabled {
            return;
        }
        if self.host_panel.presented && self.host_panel.model.is_some() {
            self.host_panel.closed = !self.ui.docking.layout.contains(&crate::docking::Panel::Host);
        }
        let first = !self.host_panel.presented && !self.host_panel.closed;
        self.host_panel.model = model;
        if self.host_panel.model.is_none() {
            self.host_panel.presented = false;
            self.host_panel.edits.clear();
            self.host_panel.filters.clear();
            remove(&mut self.ui.docking);
        } else {
            // Discard buffers for rows/columns removed by the parent, while retaining active
            // edits in rows whose values changed in the new snapshot.
            if let Some(model) = &self.host_panel.model {
                self.host_panel.edits.retain(|(section, row, column), _| {
                    model
                        .sections
                        .iter()
                        .find(|item| item.id == *section)
                        .and_then(|section| section.rows.iter().find(|item| item.id == *row))
                        .and_then(|row| row.cells.get(column))
                        .is_some_and(|cell| matches!(cell.kind, Kind::Edit(_)))
                });
                self.host_panel.filters.retain(|id, _| model.sections.iter().any(|section| section.id == *id));
            }
            self.host_panel.presented = true;
            if first {
                let _ = crate::docking::reveal_panel(&mut self.ui.docking, crate::docking::Panel::Host);
            }
        }
    }

    pub fn host_panel_open(&mut self) {
        self.host_panel.presented = false;
        self.host_panel.closed = false;
        self.host_panel.edits.clear();
    }
}

/// Host contents never survive layout persistence or run without an active parent model.
pub fn remove(workspace: &mut crate::docking::Workspace) {
    let panel = crate::docking::Panel::Host;
    let _ = workspace.set_visible(panel, false, None, crate::docking::permissions);
    workspace.hidden.retain(|(id, _)| *id != panel);
}

pub fn show(state: &mut State, ui: &mut egui::Ui) {
    let Some(model) = &state.model else { return };
    let tokens = crate::theme::Tokens::get();
    egui::ScrollArea::both().id_salt("host-panel-scroll").auto_shrink([false, false]).show(ui, |ui| {
        for section in &model.sections {
            ui.push_id(&section.id, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if !section.title.is_empty() {
                        ui.strong(&section.title);
                    }
                    for action in &section.actions {
                        if ui.add_enabled(!action.disabled, egui::Button::new(&action.label)).clicked() {
                            state.events.push(Event::Action { section: section.id.clone(), action: action.id.clone() });
                        }
                    }
                });
                let filter = state.filters.entry(section.id.clone()).or_default();
                if section.filter {
                    ui.add(egui::TextEdit::singleline(filter).hint_text(crate::tl!("Search")).desired_width(ui.available_width()))
                        .accessible_name(crate::tl!("Search"));
                }
                let needle = if section.filter { filter.to_lowercase() } else { String::new() };
                if section.rows.is_empty() {
                    ui.label(&section.empty);
                }
                egui::Grid::new("host-table").min_col_width(24.0).max_col_width(240.0).striped(true).show(ui, |ui| {
                    for column in &section.columns {
                        ui.strong(&column.label);
                    }
                    ui.end_row();
                    let mut group = None;
                    for row in &section.rows {
                        if !needle.is_empty() && !row.cells.values().any(|cell| cell.search_text().to_lowercase().contains(&needle)) {
                            continue;
                        }
                        if row.group.as_deref() != group {
                            group = row.group.as_deref();
                            if let Some(group) = group {
                                ui.colored_label(tokens.text_dim, group);
                                ui.end_row();
                            }
                        }
                        for column in &section.columns {
                            if let Some(cell) = row.cells.get(&column.id) {
                                let key = (section.id.clone(), row.id.clone(), column.id.clone());
                                let value =
                                    ui.push_id((&row.id, &column.id), |ui| render_cell(ui, cell, &column.label, &key, &mut state.edits)).inner;
                                if let Some(value) = value {
                                    state.events.push(Event::Edit {
                                        section: section.id.clone(),
                                        row: row.id.clone(),
                                        column: column.id.clone(),
                                        value,
                                    });
                                }
                            } else {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.separator();
            });
        }
        if let Some(notice) = &model.notice {
            ui.label(notice);
        }
    });
}

impl Cell {
    fn search_text(&self) -> &str {
        match &self.kind {
            Kind::Text(text) | Kind::Edit(text) | Kind::Button(text) => text,
            Kind::Toggle(_) => self.label.as_deref().unwrap_or_default(),
        }
    }
}

fn render_cell(ui: &mut egui::Ui, cell: &Cell, fallback_label: &str, key: &CellKey, edits: &mut BTreeMap<CellKey, Pending>) -> Option<Value> {
    let label = cell.label.as_deref().unwrap_or(fallback_label);
    if cell.disabled {
        edits.remove(key);
    }
    let mut event = None;
    ui.vertical(|ui| {
        ui.add_enabled_ui(!cell.disabled, |ui| match &cell.kind {
            Kind::Text(text) => {
                ui.label(text).on_hover_text(label);
            }
            Kind::Edit(text) => {
                event = edit(ui, text, label, key, edits);
            }
            Kind::Toggle(on) => {
                let mut checked = *on;
                if ui.checkbox(&mut checked, "").accessible_name(label).on_hover_text(label).clicked() {
                    event = Some(Value::Bool(checked));
                }
            }
            Kind::Button(text) => {
                if ui.button(text).accessible_name(label).on_hover_text(label).clicked() {
                    event = Some(Value::Null);
                }
            }
        });
        if let Some(error) = &cell.error {
            ui.colored_label(crate::theme::Tokens::get().error, error);
        }
    });
    event.filter(|_| !cell.disabled)
}

fn edit(ui: &mut egui::Ui, text: &str, label: &str, key: &CellKey, edits: &mut BTreeMap<CellKey, Pending>) -> Option<Value> {
    let id = ui.make_persistent_id("edit");
    // egui may already surrender focus for Escape before this widget runs. A pending
    // buffer still belongs to the field and must be abandoned rather than committed.
    let escape = edits.contains_key(key) && ui.input(|input| input.key_pressed(egui::Key::Escape));
    let pending = edits.entry(key.clone()).or_insert_with(|| Pending { original: text.into(), buffer: text.into() });
    let response = ui.add(egui::TextEdit::singleline(&mut pending.buffer).id(id).desired_width(72.0)).accessible_name(label);
    let commit = response.lost_focus() || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
    let result = (!escape && commit && pending.buffer != pending.original).then(|| Value::String(pending.buffer.clone()));
    if escape || commit || !ui.memory(|memory| memory.has_focus(id)) {
        edits.remove(key);
    }
    result
}
