//! The Visibility States dialog: BVSTATE typed, or Visibility States on the Block Editor's bar,
//! while the Block Editor is open on a block with a visibility parameter. It lists the block's
//! states with the one the editor shows checked, and makes (New…), renames, deletes, reorders
//! (Move Up, Move Down) and picks (Set current) states, as AutoCAD's dialog does. Changes are
//! kept in the dialog until OK, which runs them as one BVSTATE step (`bvstate {ops}`); Cancel
//! changes nothing. JSON calls and -BVSTATE skip it.

use cadcraft_engine::doc::dynblock::{ParameterKind, dynamic_block};
use egui::{RichText, Stroke, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// What a new state shows, as the New Visibility State dialog offers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewShows {
    Hide,
    Show,
    #[default]
    Unchanged,
}

impl NewShows {
    fn param(self) -> &'static str {
        match self {
            NewShows::Hide => "hide",
            NewShows::Show => "show",
            NewShows::Unchanged => "current",
        }
    }
}

/// The New Visibility State dialog's fields.
#[derive(Clone, Debug, Default)]
pub struct NewState {
    pub name: String,
    pub shows: NewShows,
    pub error: Option<String>,
}

/// The dialog's state while it is open: the states as they will be after OK.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// The states, in order.
    pub names: Vec<String>,
    /// The state the editor will show (checked).
    pub current: String,
    /// The row picked in the list.
    pub selected: usize,
    /// The changes made so far, as `bvstate` operations, run in turn on OK.
    pub ops: Vec<Value>,
    /// The row being renamed and the name typed.
    pub renaming: Option<(usize, String)>,
    /// The New Visibility State dialog, while open.
    pub new: Option<NewState>,
    /// Why the last change or OK was refused.
    pub error: Option<String>,
}

impl State {
    fn has(&self, name: &str, except: Option<usize>) -> bool {
        self.names.iter().enumerate().any(|(i, n)| Some(i) != except && n.eq_ignore_ascii_case(name.trim()))
    }

    /// A name no state has: "VisibilityState" and the lowest free number.
    fn free_name(&self) -> String {
        (0..).map(|i| format!("VisibilityState{i}")).find(|n| !self.has(n, None)).unwrap_or_default()
    }

    fn set_current(&mut self, i: usize) {
        if let Some(n) = self.names.get(i).cloned()
            && n != self.current
        {
            self.ops.push(json!({ "set": n }));
            self.current = n;
        }
    }

    /// Add state `new` (it becomes the one shown, as AutoCAD makes it); `Err` with why not.
    fn add(&mut self, new: &NewState) -> Result<(), String> {
        let name = new.name.trim();
        if name.is_empty() {
            return Err(crate::i18n::t("A visibility state needs a name.").to_string());
        }
        if self.has(name, None) {
            return Err(format!("{} \"{name}\"", crate::i18n::t("A visibility state already has the name")));
        }
        self.ops.push(json!({ "new": name, "visibility": new.shows.param() }));
        self.names.push(name.to_string());
        self.current = name.to_string();
        self.selected = self.names.len().saturating_sub(1);
        Ok(())
    }

    /// Rename row `i` to `to`; `Err` with why not.
    fn rename(&mut self, i: usize, to: &str) -> Result<(), String> {
        let to = to.trim();
        let Some(old) = self.names.get(i).cloned() else { return Ok(()) };
        if to == old {
            return Ok(());
        }
        if to.is_empty() {
            return Err(crate::i18n::t("A visibility state needs a name.").to_string());
        }
        if self.has(to, Some(i)) {
            return Err(format!("{} \"{to}\"", crate::i18n::t("A visibility state already has the name")));
        }
        self.ops.push(json!({ "rename": old, "to": to }));
        if self.current == old {
            self.current = to.to_string();
        }
        if let Some(n) = self.names.get_mut(i) {
            *n = to.to_string();
        }
        Ok(())
    }

    /// Whether row `i` can be deleted: not the state shown, and not the last one.
    fn deletable(&self, i: usize) -> bool {
        self.names.len() > 1 && self.names.get(i).is_some_and(|n| *n != self.current)
    }

    fn delete(&mut self, i: usize) {
        if !self.deletable(i) {
            return;
        }
        let n = self.names.remove(i);
        self.ops.push(json!({ "delete": n }));
        self.selected = self.selected.min(self.names.len().saturating_sub(1));
    }

    /// Move row `i` one place up (`up`) or down.
    fn shift(&mut self, i: usize, up: bool) {
        let j = if up { i.checked_sub(1) } else { i.checked_add(1).filter(|j| *j < self.names.len()) };
        let Some(j) = j else { return };
        self.names.swap(i, j);
        self.selected = j;
        self.ops.push(json!({ "order": self.names }));
    }
}

/// The edited block's states and the one the editor shows, when the Block Editor is open on a
/// block with a visibility parameter.
fn current_states(app: &CadApp) -> Option<(Vec<String>, String)> {
    let st = app.session.state().ok()?;
    let be = st.block_edit.as_ref()?;
    let m = dynamic_block(&st.doc, &be.name)?;
    let names: Vec<String> = m.parameters.into_iter().find_map(|p| match p.kind {
        ParameterKind::Visibility { states, .. } => Some(states.into_iter().map(|s| s.name).collect()),
        _ => None,
    })?;
    let current = be.visibility_state(&st.doc).map(|(n, _)| n).or_else(|| names.first().cloned())?;
    Some((names, current))
}

/// BVSTATE typed or from the Block Editor's bar without parameters opens the dialog when the
/// edited block has visibility states; otherwise BVSTATE runs and says why it can't.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != "bvstate" || !params.is_null() {
        return None;
    }
    let (names, current) = current_states(app)?;
    let selected = names.iter().position(|n| *n == current).unwrap_or(0);
    app.bvstate = State { names, current, selected, ..State::default() };
    app.ui.dialog = Some("bvstate".into());
    Some(Ok(Value::Null))
}

/// The Visibility States window (and New Visibility State over it).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let (mut ok, mut cancel) = (false, false);
    let mut st = std::mem::take(&mut app.bvstate);
    let modal = st.new.is_some();
    egui::Window::new(crate::tl!("Visibility States"))
        .id(egui::Id::unique("Visibility States"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.add_enabled_ui(!modal, |ui| {
                ui.label(crate::tl!("Visibility states:"));
                ui.horizontal_top(|ui| {
                    egui::Frame::new().fill(t.field).stroke(Stroke::new(1.0, t.border)).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                        ui.set_min_size(vec2(300.0, 230.0));
                        egui::ScrollArea::vertical().id_salt("bvstate-list").max_height(230.0).min_scrolled_height(230.0).show(ui, |ui| {
                            ui.set_width(300.0);
                            list(ui, &mut st);
                        });
                    });
                    ui.vertical(|ui| {
                        let w = vec2(120.0, 0.0);
                        let i = st.selected;
                        let is_current = st.names.get(i).is_some_and(|n| *n == st.current);
                        if ui.add_enabled(!is_current, egui::Button::new(crate::tl!("Set current")).min_size(w)).clicked() {
                            st.set_current(i);
                        }
                        if ui.add(egui::Button::new(crate::tl!("New...")).min_size(w)).clicked() {
                            st.new = Some(NewState { name: st.free_name(), ..NewState::default() });
                        }
                        if ui.add_enabled(st.renaming.is_none(), egui::Button::new(crate::tl!("Rename")).min_size(w)).clicked() {
                            st.renaming = st.names.get(i).map(|n| (i, n.clone()));
                        }
                        if ui.add_enabled(st.deletable(i), egui::Button::new(crate::tl!("Delete")).min_size(w)).clicked() {
                            st.delete(i);
                        }
                        ui.add_space(18.0);
                        if ui.add_enabled(i > 0, egui::Button::new(crate::tl!("Move Up")).min_size(w)).clicked() {
                            st.shift(i, true);
                        }
                        if ui.add_enabled(i + 1 < st.names.len(), egui::Button::new(crate::tl!("Move Down")).min_size(w)).clicked() {
                            st.shift(i, false);
                        }
                    });
                });
                if let Some(e) = &st.error {
                    ui.colored_label(t.warn, e);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(crate::tl!("Cancel")).clicked() {
                            cancel = true;
                        }
                        if ui.button(crate::tl!("OK")).clicked() {
                            ok = true;
                        }
                    });
                });
            });
        });
    if st.new.is_some() {
        new_dialog(ctx, &mut st);
    }
    if ok {
        // A rename still being typed counts.
        if let Some((i, to)) = st.renaming.take()
            && let Err(e) = st.rename(i, &to)
        {
            st.error = Some(e);
            app.bvstate = st;
            return;
        }
        if !st.ops.is_empty()
            && let Err(e) = app.run("bvstate", json!({ "ops": st.ops }))
        {
            st.error = Some(e);
            app.bvstate = st;
            return;
        }
        *open = false;
    }
    if cancel {
        *open = false;
    }
    app.bvstate = st;
}

/// The states, the one shown checked; the row being renamed is a text box.
fn list(ui: &mut egui::Ui, st: &mut State) {
    let t = Tokens::get();
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        for i in 0..st.names.len() {
            let name = st.names.get(i).cloned().unwrap_or_default();
            if let Some((r, typed)) = &mut st.renaming
                && *r == i
            {
                let edit = ui.add(egui::TextEdit::singleline(typed).desired_width(280.0));
                edit.request_focus();
                if ui.input(|k| k.key_pressed(egui::Key::Escape)) {
                    st.renaming = None;
                } else if edit.lost_focus() {
                    let to = typed.clone();
                    st.renaming = None;
                    st.error = st.rename(i, &to).err();
                }
                continue;
            }
            let check = if name == st.current { "✔ " } else { "    " };
            let row = ui.selectable_label(st.selected == i, RichText::new(format!("{check}{name}")).color(t.text));
            if row.clicked() {
                st.selected = i;
                st.error = None;
            }
            // Double-click shows that state, as Set current does.
            if row.double_clicked() {
                st.set_current(i);
            }
        }
    });
}

/// The New Visibility State dialog: a name and what the new state shows.
fn new_dialog(ctx: &egui::Context, st: &mut State) {
    let Some(mut new) = st.new.take() else { return };
    let t = Tokens::get();
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new(crate::tl!("New Visibility State"))
        .id(egui::Id::unique("New Visibility State"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(crate::tl!("Visibility state name:"));
            let edit = ui.add(egui::TextEdit::singleline(&mut new.name).desired_width(460.0));
            if edit.changed() {
                new.error = None;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            ui.add_space(6.0);
            ui.group(|ui| {
                ui.label(crate::tl!("Visibility options for new states"));
                ui.radio_value(&mut new.shows, NewShows::Hide, crate::tl!("Hide all existing objects in new state"));
                ui.radio_value(&mut new.shows, NewShows::Show, crate::tl!("Show all existing objects in new state"));
                ui.radio_value(&mut new.shows, NewShows::Unchanged, crate::tl!("Leave visibility of existing objects unchanged in new state"));
            });
            if let Some(e) = &new.error {
                ui.colored_label(t.warn, e);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(crate::tl!("OK")).clicked() {
                        ok = true;
                    }
                });
            });
        });
    if ok {
        match st.add(&new) {
            Ok(()) => return,
            Err(e) => new.error = Some(e),
        }
    }
    if cancel || !open {
        return;
    }
    st.new = Some(new);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State { names: vec!["A".into(), "B".into(), "C".into()], current: "A".into(), ..State::default() }
    }

    #[test]
    fn changes_become_bvstate_operations_in_order() {
        let mut st = state();
        st.add(&NewState { name: "D".into(), shows: NewShows::Hide, error: None }).unwrap();
        assert_eq!((st.current.as_str(), st.selected), ("D", 3), "a new state is the one shown");
        st.rename(1, "Bee").unwrap();
        st.shift(1, true);
        st.delete(2);
        st.set_current(0);
        assert_eq!(st.names, ["Bee", "A", "D"]);
        assert_eq!(st.current, "Bee");
        assert_eq!(
            st.ops,
            vec![
                json!({ "new": "D", "visibility": "hide" }),
                json!({ "rename": "B", "to": "Bee" }),
                json!({ "order": ["Bee", "A", "C", "D"] }),
                json!({ "delete": "C" }),
                json!({ "set": "Bee" }),
            ]
        );
    }

    /// An app in the Block Editor on block "B" (a rectangle and a circle); with `visibility`, the
    /// block has a visibility parameter. Returns (app, rectangle, circle).
    fn editing(visibility: bool) -> (CadApp, String, String) {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        let s = &mut app.session;
        s.execute("new", &Value::Null).unwrap();
        let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
        let r = made(s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 5] })).unwrap());
        let c = made(s.execute("circle", &json!({ "center": [5, 2.5], "radius": 1 })).unwrap());
        s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [r, c], "keep": "delete" })).unwrap();
        s.execute("bedit", &json!({ "name": "B" })).unwrap();
        if visibility {
            s.execute("bparameter", &json!({ "type": "visibility", "at": [0, 6] })).unwrap();
        }
        // The editor's objects (the block's, with the block's handles).
        let d = app.session.doc().unwrap();
        let of =
            |rect: bool| d.model.iter().find(|e| matches!(e.kind, cadcraft_engine::doc::EntityKind::LwPolyline(_)) == rect).unwrap().handle.hex();
        let (r, c) = (of(true), of(false));
        (app, r, c)
    }

    /// Object Visibility ▸ in the right-click menu's entries, if there.
    fn object_visibility(app: &CadApp) -> Option<crate::menus::Entry> {
        crate::context_menu::entries(app)
            .into_iter()
            .flatten()
            .find(|e| matches!(e, crate::menus::Entry::Sub { label, .. } if label == "Object Visibility"))
    }

    #[test]
    fn the_shortcut_menu_hides_and_shows_selected_objects() {
        let (mut app, _, c) = editing(false);
        let h = cadcraft_engine::cmd::curves::handle_value(&json!(c)).unwrap();
        assert!(object_visibility(&app).is_none(), "only with a selection");
        app.session.set_selection(vec![h]);
        assert!(!object_visibility(&app).unwrap().enabled(), "greyed out without a visibility parameter");
        app.session.execute("bparameter", &json!({ "type": "visibility", "at": [0, 6] })).unwrap();
        app.session.set_selection(vec![h]);
        assert!(object_visibility(&app).unwrap().enabled());
        crate::context_menu::choose(&mut app, "visibility:hide");
        assert!(app.session.doc().unwrap().hidden_objects.contains(&h));
        app.session.execute("setvar", &json!({ "name": "BVMODE", "value": 1 })).unwrap();
        app.session.set_selection(vec![h]);
        crate::context_menu::choose(&mut app, "visibility:show-all");
        app.session.execute("setvar", &json!({ "name": "BVMODE", "value": 0 })).unwrap();
        assert!(app.session.doc().unwrap().hidden_objects.is_empty());
    }

    #[test]
    fn bvstate_opens_the_dialog_and_ok_applies_its_changes_as_one_step() {
        let (mut app, _, _) = editing(false);
        assert!(route(&mut app, "bvstate", &Value::Null).is_none(), "no visibility parameter: BVSTATE says why");
        let (mut app, _, _) = editing(true);
        assert!(route(&mut app, "bvstate", &json!({ "set": "X" })).is_none(), "JSON calls skip the dialog");
        assert!(route(&mut app, "bvstate", &Value::Null).is_some());
        assert_eq!(app.ui.dialog.as_deref(), Some("bvstate"));
        assert_eq!((app.bvstate.names.clone(), app.bvstate.current.clone()), (vec!["VisibilityState0".to_string()], "VisibilityState0".to_string()));
        let mut st = app.bvstate.clone();
        st.add(&NewState { name: "Empty".into(), shows: NewShows::Hide, error: None }).unwrap();
        st.rename(0, "Full").unwrap();
        let before = app.session.state().unwrap().undo.len();
        app.run("bvstate", json!({ "ops": st.ops })).unwrap();
        assert_eq!(app.session.state().unwrap().undo.len(), before + 1, "one undo step");
        let out = app.session.execute("bvstate", &json!({})).unwrap();
        assert_eq!(out, json!({ "current": "Empty", "states": ["Full", "Empty"] }));
        // A refused operation undoes the ones before it.
        let bad = json!({ "ops": [{ "new": "Third" }, { "delete": "Nope" }] });
        assert!(app.run("bvstate", bad).is_err());
        assert_eq!(app.session.execute("bvstate", &json!({})).unwrap()["states"], json!(["Full", "Empty"]));
    }

    #[test]
    fn refused_changes_say_why_and_change_nothing() {
        let mut st = state();
        assert!(st.add(&NewState { name: "b".into(), ..NewState::default() }).is_err(), "names are case-insensitive");
        assert!(st.add(&NewState { name: "  ".into(), ..NewState::default() }).is_err());
        assert!(st.rename(0, "c").is_err());
        assert!(!st.deletable(0), "the state shown can't be deleted");
        st.delete(0);
        st.shift(0, true);
        st.shift(2, false);
        st.rename(9, "X").unwrap();
        assert_eq!(st.names, ["A", "B", "C"]);
        assert!(st.ops.is_empty());
        assert_eq!(st.free_name(), "VisibilityState0");
    }
}
