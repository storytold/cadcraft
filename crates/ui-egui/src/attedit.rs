//! The Edit Attributes dialog (Enhanced Attribute Editor): ATTEDIT / EATTEDIT typed or started
//! from a menu while one block reference with attributes is selected. It lists each attribute's
//! tag, prompt and value; Apply and OK run `attedit {handle, values}` with the values changed (one
//! undo step each). With nothing suitable selected ATTEDIT asks at the command line instead, and
//! JSON calls never open the dialog.

use cadcraft_engine::doc::{Drawing, EntityKind, Handle};
use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// One attribute row: what the drawing had when the dialog opened, and what is typed now.
#[derive(Clone, Debug)]
pub struct Row {
    pub tag: String,
    pub prompt: String,
    pub value: String,
    pub editable: bool,
    pub invisible: bool,
    pub multiline: bool,
    pub typed: String,
}

/// The dialog's state while it is open.
#[derive(Default)]
pub struct State {
    /// The block reference edited.
    pub handle: Option<Handle>,
    /// The drawing it is in (`DocState::uid`): handles repeat across drawings, and the dialog
    /// doesn't stop another drawing's tab from being chosen.
    pub doc: Option<u64>,
    /// Its block's name (the dynamic block's for a dynamic block reference).
    pub block: String,
    pub rows: Vec<Row>,
    /// The row that is current: shown in bold, and its value box takes the focus when the
    /// dialog opens (the attribute double-clicked).
    pub current: Option<usize>,
    /// The current row's value box still has to take the focus.
    pub focus: bool,
    /// Why the last Apply failed.
    pub error: Option<String>,
}

/// The block reference the dialog edits: the selection when it is exactly one block reference
/// with an attribute that can be changed.
fn selected_reference(app: &CadApp) -> Option<Handle> {
    let st = app.session.state().ok()?;
    let [h] = st.selection.as_slice() else { return None };
    matches!(st.doc.entity(*h).map(|e| &e.kind), Some(EntityKind::Insert(_))).then_some(*h)?;
    cadcraft_engine::attributes(&st.doc, *h).iter().any(|a| a.editable).then_some(*h)
}

/// The name shown for block reference `h`: its dynamic block's, not its anonymous copy's.
fn block_name(d: &Drawing, h: Handle) -> String {
    if let Some(r) = cadcraft_engine::doc::dynblock::reference(d, h) {
        return r.definition;
    }
    match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => i.block.clone(),
        _ => String::new(),
    }
}

/// Load the rows of block reference `h`.
fn load(app: &mut CadApp, h: Handle) {
    let Ok(st) = app.session.state() else { return };
    let rows = cadcraft_engine::attributes(&st.doc, h)
        .into_iter()
        .map(|a| Row {
            typed: a.value.clone(),
            tag: a.tag,
            prompt: a.prompt,
            value: a.value,
            editable: a.editable,
            invisible: a.invisible,
            multiline: a.multiline,
        })
        .collect();
    let current = app.attedit.current;
    app.attedit = State { handle: Some(h), doc: Some(st.uid), block: block_name(&st.doc, h), rows, current, focus: false, error: None };
}

/// Open the dialog on block reference `h` of the active drawing, with attribute `current` (an
/// index into its attributes, in `cadcraft_engine::attributes` order) current; without one, or
/// when that attribute can't be changed, the first one that can. False, and nothing opens, when
/// `h` is not a block reference with an attribute that can be changed.
pub fn open(app: &mut CadApp, h: Handle, current: Option<usize>) -> bool {
    let Ok(st) = app.session.state() else { return false };
    if !matches!(st.doc.entity(h).map(|e| &e.kind), Some(EntityKind::Insert(_))) {
        return false;
    }
    let attribs = cadcraft_engine::attributes(&st.doc, h);
    let Some(first) = attribs.iter().position(|a| a.editable) else { return false };
    let current = current.filter(|i| attribs.get(*i).is_some_and(|a| a.editable)).unwrap_or(first);
    app.attedit.current = Some(current);
    load(app, h);
    app.attedit.focus = true;
    app.ui.dialog = Some("attedit".into());
    true
}

/// The block reference the dialog edits, while the drawing it was opened for is the active one
/// and still has it.
fn edited(app: &CadApp) -> Option<Handle> {
    let h = app.attedit.handle?;
    let st = app.session.state().ok()?;
    (Some(st.uid) == app.attedit.doc && st.doc.entity(h).is_some()).then_some(h)
}

/// ATTEDIT / EATTEDIT typed or from a menu without parameters opens the dialog when one block
/// reference with attributes is selected; otherwise the command runs at the command line.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !matches!(id, "attedit" | "ate" | "eattedit") || !params.is_null() {
        return None;
    }
    let h = selected_reference(app)?;
    app.session.echo(format!("Command: {}", id.to_ascii_uppercase()));
    open(app, h, None).then_some(Ok(Value::Null))
}

/// The values typed that differ from the drawing's, as `attedit`'s `values`.
fn changed(rows: &[Row]) -> Map<String, Value> {
    rows.iter().filter(|r| r.editable && r.typed != r.value).map(|r| (r.tag.clone(), Value::String(r.typed.clone()))).collect()
}

/// Apply: run `attedit` with the changed values; true when it went through (or nothing changed).
pub fn apply(app: &mut CadApp) -> bool {
    let Some(h) = edited(app) else { return false };
    let values = changed(&app.attedit.rows);
    if values.is_empty() {
        return true;
    }
    match app.run("attedit", json!({ "handle": h.hex(), "values": values })) {
        Ok(_) => {
            load(app, h);
            true
        }
        Err(e) => {
            app.attedit.error = Some(e);
            false
        }
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    // The reference went away (undo, erase from the command line) or another drawing is active:
    // nothing to edit.
    if edited(app).is_none() {
        *open = false;
        return;
    }
    let t = Tokens::get();
    let mut choice: Option<&str> = None;
    let st = &mut app.attedit;
    egui::Window::new(crate::tl!("Enhanced Attribute Editor"))
        .id(egui::Id::unique("Enhanced Attribute Editor"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::tl!("Block:")).color(t.text_dim));
                ui.label(&st.block);
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("attedit-rows").num_columns(3).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                    ui.label(RichText::new(crate::tl!("Tag")).color(t.text_dim));
                    ui.label(RichText::new(crate::tl!("Prompt")).color(t.text_dim));
                    ui.label(RichText::new(crate::tl!("Value")).color(t.text_dim));
                    ui.end_row();
                    for (i, r) in st.rows.iter_mut().enumerate() {
                        let current = st.current == Some(i);
                        let tag = if r.invisible { format!("{} ({})", r.tag, crate::i18n::t("invisible")) } else { r.tag.clone() };
                        ui.label(if current { RichText::new(tag).strong() } else { RichText::new(tag) });
                        ui.label(&r.prompt);
                        let edit = if r.multiline {
                            egui::TextEdit::multiline(&mut r.typed).desired_rows(2)
                        } else {
                            egui::TextEdit::singleline(&mut r.typed)
                        };
                        let resp = ui.add_enabled(r.editable, edit.desired_width(220.0));
                        if resp.has_focus() {
                            st.current = Some(i);
                        }
                        if current && st.focus {
                            resp.request_focus();
                            resp.scroll_to_me(Some(egui::Align::Center));
                            st.focus = false;
                        }
                        if !r.editable {
                            let _ = resp.on_disabled_hover_text(crate::tl!("Constant"));
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(e) = &st.error {
                ui.add_space(4.0);
                ui.colored_label(t.warn, e);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let dirty = !changed(&st.rows).is_empty();
                if ui.button(crate::tl!("OK")).clicked() {
                    choice = Some("ok");
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    choice = Some("cancel");
                }
                if ui.add_enabled(dirty, egui::Button::new(crate::tl!("Apply"))).clicked() {
                    choice = Some("apply");
                }
            });
        });
    match choice {
        Some("ok") => {
            if apply(app) {
                *open = false;
            }
        }
        Some("apply") => {
            apply(app);
        }
        Some(_) => *open = false,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tag: &str, value: &str, typed: &str, editable: bool) -> Row {
        Row { tag: tag.into(), prompt: String::new(), value: value.into(), editable, invisible: false, multiline: false, typed: typed.into() }
    }

    #[test]
    fn only_changed_editable_values_are_sent() {
        let rows = [row("A", "1", "1", true), row("B", "2", "3", true), row("C", "x", "y", false)];
        let v = changed(&rows);
        assert_eq!(v.len(), 1);
        assert_eq!(v.get("B"), Some(&json!("3")));
    }
}
