//! The Object Grouping dialog (CLASSICGROUP): the drawing's groups with their selectable flag,
//! and creating, renaming, describing, adding to, removing from, highlighting and exploding
//! them. The objects come from the selection made before (as with GROUP's pickfirst
//! selection). A thin view: it acts through `groups.list`, `group`, `groupedit` and `ungroup`.

use egui::RichText;
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

#[derive(Clone, Default)]
struct State {
    selected: Option<String>,
    name: String,
    description: String,
    unnamed: bool,
    selectable: bool,
    include_unnamed: bool,
}

fn id() -> egui::Id {
    egui::Id::unique("object-grouping")
}

/// What a button asks for, run once the dialog is drawn.
enum Act {
    Run(&'static str, Value),
    Highlight(Vec<String>),
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let mut st: State = ctx.data_mut(|d| d.get_temp(id())).unwrap_or(State { selectable: true, ..State::default() });
    let groups: Vec<Value> = app.session.execute("groups.list", &json!({})).ok().and_then(|v| v["groups"].as_array().cloned()).unwrap_or_default();
    let sel: Vec<String> = app.session.selection().iter().map(|h| h.hex()).collect();
    let mut act = None;
    egui::Window::new(crate::tl!("Object Grouping")).id(id()).open(open).resizable(true).default_width(460.0).show(ctx, |ui| {
        egui::ScrollArea::vertical().id_salt("og-list").max_height(180.0).show(ui, |ui| {
            egui::Grid::new("og-grid").num_columns(3).striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                for h in [crate::tl!("Group Name"), crate::tl!("Selectable"), crate::tl!("Objects")] {
                    ui.label(RichText::new(h).color(t.text_dim));
                }
                ui.end_row();
                for g in groups.iter().filter(|g| st.include_unnamed || !g["unnamed"].as_bool().unwrap_or(false)) {
                    let name = g["name"].as_str().unwrap_or_default().to_string();
                    let on = st.selected.as_deref() == Some(name.as_str());
                    if ui.selectable_label(on, &name).clicked() {
                        st.selected = Some(name.clone());
                        st.name = name.clone();
                        st.description = g["description"].as_str().unwrap_or_default().to_string();
                        st.selectable = g["selectable"].as_bool().unwrap_or(true);
                    }
                    ui.label(crate::i18n::t(if g["selectable"].as_bool().unwrap_or(true) { "Yes" } else { "No" }));
                    ui.label(g["members"].as_array().map_or(0, Vec::len).to_string());
                    ui.end_row();
                }
            });
        });
        ui.checkbox(&mut st.include_unnamed, crate::tl!("Include Unnamed"));
        ui.separator();
        ui.heading(crate::tl!("Group Identification"));
        egui::Grid::new("og-ident").num_columns(2).show(ui, |ui| {
            let name_caption = ui.label(crate::tl!("Group Name"));
            ui.add_enabled(!st.unnamed, egui::TextEdit::singleline(&mut st.name).desired_width(220.0)).labelled_by(name_caption.id);
            ui.end_row();
            let description_caption = ui.label(crate::tl!("Description"));
            ui.add(egui::TextEdit::singleline(&mut st.description).desired_width(220.0)).labelled_by(description_caption.id);
            ui.end_row();
        });
        ui.separator();
        ui.heading(crate::tl!("Create Group"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut st.selectable, crate::tl!("Selectable"));
            ui.checkbox(&mut st.unnamed, crate::tl!("Unnamed"));
        });
        ui.horizontal(|ui| {
            let can = !sel.is_empty() && (st.unnamed || !st.name.trim().is_empty());
            if ui
                .add_enabled(can, egui::Button::new(crate::tl!("New Group")))
                .on_hover_text(crate::tl!("Groups the objects selected in the drawing"))
                .clicked()
            {
                let name = if st.unnamed { Value::Null } else { json!(st.name.trim()) };
                act = Some(Act::Run("group", json!({ "name": name, "description": st.description, "selectable": st.selectable, "handles": sel })));
            }
            if sel.is_empty() {
                ui.label(RichText::new(crate::tl!("Select the objects in the drawing first.")).color(t.text_dim));
            }
        });
        ui.separator();
        ui.heading(crate::tl!("Change Group"));
        let chosen = st.selected.clone();
        ui.add_enabled_ui(chosen.is_some(), |ui| {
            let name = chosen.clone().unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!sel.is_empty(), egui::Button::new(crate::tl!("Add")))
                    .on_hover_text(crate::tl!("Adds the selected objects"))
                    .clicked()
                {
                    act = Some(Act::Run("groupedit", json!({ "name": name, "add": sel })));
                }
                if ui
                    .add_enabled(!sel.is_empty(), egui::Button::new(crate::tl!("Remove")))
                    .on_hover_text(crate::tl!("Removes the selected objects"))
                    .clicked()
                {
                    act = Some(Act::Run("groupedit", json!({ "name": name, "remove": sel })));
                }
                if ui.button(crate::tl!("Rename")).clicked() && !st.name.trim().is_empty() && st.name.trim() != name {
                    act = Some(Act::Run("groupedit", json!({ "name": name, "rename": st.name.trim() })));
                }
                if ui.button(crate::tl!("Description")).clicked() {
                    act = Some(Act::Run("groupedit", json!({ "name": name, "description": st.description })));
                }
                if ui.button(crate::tl!("Selectable")).on_hover_text(crate::tl!("Turns whole-group selection on or off")).clicked() {
                    let now = groups.iter().find(|g| g["name"] == name.as_str()).and_then(|g| g["selectable"].as_bool()).unwrap_or(true);
                    act = Some(Act::Run("groupedit", json!({ "name": name, "selectable": !now })));
                }
                if ui.button(crate::tl!("Highlight")).clicked() {
                    let members =
                        groups.iter().find(|g| g["name"] == name.as_str()).and_then(|g| g["members"].as_array().cloned()).unwrap_or_default();
                    act = Some(Act::Highlight(members.iter().filter_map(|m| m.as_str().map(str::to_string)).collect()));
                }
                if ui.button(crate::tl!("Explode")).on_hover_text(crate::tl!("Removes the group; its objects stay")).clicked() {
                    act = Some(Act::Run("ungroup", json!({ "name": name })));
                }
            });
        });
    });
    match act {
        Some(Act::Run(cmd, p)) => {
            let renamed = p.get("rename").and_then(Value::as_str).map(str::to_string);
            match app.run(cmd, p) {
                Ok(r) => {
                    if cmd == "group" {
                        st.selected = r["name"].as_str().map(str::to_string);
                    } else if cmd == "ungroup" {
                        st.selected = None;
                    } else if let Some(n) = renamed {
                        st.selected = Some(n);
                    }
                }
                Err(e) => app.set_status(e),
            }
        }
        Some(Act::Highlight(hs)) => {
            let hs = hs.iter().filter_map(|h| cadcraft_engine::doc::Handle::parse_hex(h)).collect();
            app.session.set_selection(hs);
        }
        None => {}
    }
    ctx.data_mut(|d| d.insert_temp(id(), st));
}
