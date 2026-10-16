//! The Command Aliases dialog (ALIASEDIT typed or from a menu): the user's aliases next to the
//! built-in ones, Add, Edit and Remove, then OK or Apply. Built-in aliases are read-only; Edit on
//! one adds a user alias of the same name, which wins over it. The dialog edits a copy and sends
//! only the differences to ALIASEDIT, so it never forgets the alias file REINIT reads.

use std::collections::BTreeMap;

use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The Add or Edit form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub alias: String,
    pub command: String,
    /// The user alias Edit was opened on (`None` when adding, or overriding a built-in).
    pub editing: Option<String>,
}

/// The dialog while open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AliasDlgState {
    /// The user aliases as edited: lower-case alias → command id.
    pub user: BTreeMap<String, String>,
    /// The user aliases ALIASEDIT holds now (OK and Apply send the difference).
    pub applied: BTreeMap<String, String>,
    /// The alias file loaded last, which REINIT reads again.
    pub file: Option<String>,
    pub selected: Option<String>,
    pub filter: String,
    pub form: Option<Form>,
    pub error: Option<String>,
    /// ALIASEDIT typed again while open: bring the window to the front on the next frame.
    pub raise: bool,
}

/// The user aliases and alias file ALIASEDIT holds now.
fn current(app: &mut CadApp) -> (BTreeMap<String, String>, Option<String>) {
    let listing = app.session.execute("aliasedit", &json!({})).unwrap_or_default();
    let aliases = listing
        .get("aliases")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(a, c)| Some((a.clone(), c.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    (aliases, listing.get("file").and_then(Value::as_str).map(str::to_string))
}

/// The built-in aliases: lower-case alias → command id, the first command naming it winning as
/// it does when typed. Aliases that are also command names never reach their command and are left out.
fn built_in() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for c in cadcraft_engine::command_specs() {
        for a in c.aliases {
            let a = a.to_ascii_lowercase();
            if cadcraft_engine::find_command(&a).is_none() {
                m.entry(a).or_insert_with(|| c.id.to_string());
            }
        }
    }
    m
}

/// Open the dialog on the aliases ALIASEDIT holds now.
pub fn open(app: &mut CadApp) {
    // Already open: keep the edits not applied yet.
    if app.ui.dialog.as_deref() == Some("aliasedit")
        && let Some(st) = app.alias_dlg.as_mut()
    {
        st.raise = true;
        return;
    }
    let (user, file) = current(app);
    app.alias_dlg = Some(AliasDlgState { applied: user.clone(), user, file, ..Default::default() });
    app.ui.dialog = Some("aliasedit".into());
}

/// The alias and command a form names, with the rules ALIASEDIT applies, or why not.
fn check(form: &Form, user: &BTreeMap<String, String>) -> Result<(String, String), String> {
    if form.alias.trim().is_empty() {
        return Err(crate::tl!("Type an alias name.").into());
    }
    let name = cadcraft_engine::cmd::file::alias_name(&form.alias)?;
    if form.command.trim().trim_start_matches('*').trim().is_empty() {
        return Err(crate::tl!("Choose the command the alias starts.").into());
    }
    let Some(id) = cadcraft_engine::cmd::file::command_id(&form.command) else {
        return Err(crate::tf!("{command} is an unknown command.", command = form.command.trim().to_ascii_uppercase()));
    };
    let renamed_from = form.editing.as_ref().filter(|old| **old != name);
    if user.contains_key(&name) && form.editing.as_deref() != Some(name.as_str()) {
        return Err(crate::tf!("{alias} is already defined; select it and use Edit.", alias = name.to_ascii_uppercase()));
    }
    let after = user.len().saturating_add(usize::from(!user.contains_key(&name))).saturating_sub(usize::from(renamed_from.is_some()));
    if after > cadcraft_engine::cmd::file::MAX_ALIASES {
        return Err(crate::tf!("More than {n} aliases.", n = cadcraft_engine::cmd::file::MAX_ALIASES));
    }
    Ok((name, id))
}

/// Save alias: put the form's alias in the list, or say why not (the form stays open).
pub fn save_form(st: &mut AliasDlgState) {
    let Some(form) = st.form.clone() else { return };
    match check(&form, &st.user) {
        Ok((name, id)) => {
            if let Some(old) = &form.editing {
                st.user.remove(old);
            }
            st.user.insert(name.clone(), id);
            st.selected = Some(name);
            st.form = None;
            st.error = None;
        }
        Err(e) => st.error = Some(e),
    }
}

/// Send the changes to ALIASEDIT; true when it took them all. ALIASEDIT applies aliases one by
/// one, so after a refusal the dialog reads back what it holds instead of assuming nothing changed.
pub fn apply(app: &mut CadApp) -> bool {
    let Some(st) = app.alias_dlg.as_ref() else { return true };
    let set: Map<String, Value> = st.user.iter().filter(|(a, c)| st.applied.get(*a) != Some(*c)).map(|(a, c)| (a.clone(), json!(c))).collect();
    let remove: Vec<&String> = st.applied.keys().filter(|a| !st.user.contains_key(*a)).collect();
    if set.is_empty() && remove.is_empty() {
        return true;
    }
    let r = app.run("aliasedit", json!({ "set": set, "remove": remove }));
    let (applied, file) = current(app);
    let Some(st) = app.alias_dlg.as_mut() else { return r.is_ok() };
    st.applied = applied;
    st.file = file;
    match r {
        Ok(_) => {
            st.error = None;
            true
        }
        Err(e) => {
            st.error = Some(e);
            false
        }
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.alias_dlg.is_none() {
        crate::aliasdlg::open(app);
    }
    let built = built_in();
    let warn = Tokens::get().warn;
    let (mut ok, mut cancel, mut apply_clicked) = (false, false, false);
    let Some(st) = app.alias_dlg.as_mut() else { return };
    let id = egui::Id::new("aliasedit_dialog");
    if std::mem::take(&mut st.raise) {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    }
    egui::Window::new(crate::tl!("Command Aliases")).id(id).open(open).collapsible(false).default_size(vec2(480.0, 460.0)).show(ctx, |ui| {
        match &st.file {
            Some(f) => ui.label(crate::tf!("Alias file: {file}", file = f)),
            None => ui.label(crate::tl!("No alias file loaded. REINIT replaces these aliases with a file's.")),
        };
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Filter"));
            ui.text_edit_singleline(&mut st.filter);
        });
        let f = st.filter.trim().to_ascii_lowercase();
        let mut names: Vec<&String> = st.user.keys().chain(built.keys().filter(|a| !st.user.contains_key(*a))).collect();
        names.sort();
        // Edit and Remove act only on a selected row the filter shows.
        let mut selected_shown = false;
        egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("alias_rows").striped(true).num_columns(3).show(ui, |ui| {
                ui.strong(crate::tl!("Alias"));
                ui.strong(crate::tl!("Command"));
                ui.label("");
                ui.end_row();
                for name in names {
                    let (command, mine) = match st.user.get(name) {
                        Some(c) => (c, true),
                        None => (built.get(name).unwrap_or(name), false),
                    };
                    if !f.is_empty() && !name.contains(&f) && !command.contains(&f) {
                        continue;
                    }
                    let selected = st.selected.as_deref() == Some(name.as_str());
                    selected_shown |= selected;
                    let text = RichText::new(name.to_ascii_uppercase());
                    if ui.selectable_label(selected, if mine { text } else { text.weak() }).clicked() {
                        st.selected = Some(name.clone());
                    }
                    ui.label(if mine { RichText::new(command.to_ascii_uppercase()) } else { RichText::new(command.to_ascii_uppercase()).weak() });
                    let note = match (mine, built.contains_key(name)) {
                        (true, true) => crate::tl!("overrides built-in"),
                        (true, false) => "",
                        (false, _) => crate::tl!("built-in"),
                    };
                    ui.label(RichText::new(note).small());
                    ui.end_row();
                }
            });
        });
        let sel = st.selected.clone().filter(|_| selected_shown);
        let sel_mine = sel.as_ref().is_some_and(|s| st.user.contains_key(s));
        let sel_known = sel.as_ref().is_some_and(|s| sel_mine || built.contains_key(s));
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("Add…")).clicked() {
                st.form = Some(Form::default());
                st.error = None;
            }
            if ui.add_enabled(sel_known, egui::Button::new(crate::tl!("Edit…"))).clicked()
                && let Some(s) = &sel
            {
                let command = st.user.get(s).or_else(|| built.get(s)).cloned().unwrap_or_default();
                st.form = Some(Form { alias: s.to_ascii_uppercase(), command: command.to_ascii_uppercase(), editing: sel_mine.then(|| s.clone()) });
                st.error = None;
            }
            if ui.add_enabled(sel_mine, egui::Button::new(crate::tl!("Remove"))).clicked()
                && let Some(s) = &sel
            {
                st.user.remove(s);
            }
        });
        let mut save = false;
        let mut discard = false;
        if let Some(form) = st.form.as_mut() {
            ui.group(|ui| {
                egui::Grid::new("alias_form").num_columns(2).show(ui, |ui| {
                    ui.label(crate::tl!("Alias"));
                    ui.text_edit_singleline(&mut form.alias);
                    ui.end_row();
                    ui.label(crate::tl!("Command"));
                    ui.text_edit_singleline(&mut form.command);
                    ui.end_row();
                });
                // A few commands starting with what was typed, to pick from.
                let typed = form.command.trim().trim_start_matches('*').to_ascii_lowercase();
                if !typed.is_empty() && cadcraft_engine::find_command(&typed).is_none() {
                    ui.horizontal_wrapped(|ui| {
                        for c in cadcraft_engine::command_specs().iter().filter(|c| c.id.starts_with(&typed)).take(8) {
                            if ui.small_button(c.id.to_ascii_uppercase()).clicked() {
                                form.command = c.id.to_ascii_uppercase();
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    save = ui.button(crate::tl!("Save alias")).clicked();
                    discard = ui.button(crate::tl!("Discard")).clicked();
                });
            });
        }
        if save {
            save_form(st);
        }
        if discard {
            st.form = None;
            st.error = None;
        }
        if let Some(e) = &st.error {
            ui.colored_label(warn, e.as_str());
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button(crate::tl!("OK")).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
            apply_clicked = ui.button(crate::tl!("Apply")).clicked();
        });
    });
    if (ok && apply(app)) || cancel {
        *open = false;
    } else if apply_clicked {
        apply(app);
    }
    if !*open {
        app.alias_dlg = None;
    }
}
