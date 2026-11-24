//! The Edit Attributes dialog (Enhanced Attribute Editor): ATTEDIT / EATTEDIT typed or started
//! from a menu while one block reference with attributes is selected. It lists each attribute's
//! tag, prompt and value; Apply and OK run `attedit {handle, at}` with the values changed (one
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
    /// The whole attribute the row was loaded from (see `cadcraft_engine::AttributeRow`): a row
    /// index is only the attribute's place in the reference, and a reorder of the block's
    /// definitions moves a different attribute — the same tag, value and insertion point is not
    /// enough to tell them apart.
    pub record: cadcraft_engine::doc::Attrib,
    pub typed: String,
}

impl Row {
    /// Whether `a` is still the attribute this row was loaded from: the whole stored attribute must
    /// be equal, so a property the dialog doesn't show (text height, rotation, justification,
    /// style, the attribute's own overrides) tells a reordered one apart too.
    fn is(&self, a: &cadcraft_engine::AttributeRow) -> bool {
        self.record == a.record
    }
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

/// The block reference the dialog edits: the selection as the active viewport shows it when that
/// is exactly one block reference with an attribute that can be changed.
fn selected_reference(app: &CadApp) -> Option<Handle> {
    let selection = app.session.selection();
    let [h] = selection.as_slice() else { return None };
    let st = app.session.state().ok()?;
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
            record: a.record,
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

/// The values typed that differ from the drawing's, as `attedit`'s `at` (by row, so attributes
/// sharing a tag stay apart).
fn changed(rows: &[Row]) -> Map<String, Value> {
    rows.iter().enumerate().filter(|(_, r)| r.editable && r.typed != r.value).map(|(i, r)| (i.to_string(), Value::String(r.typed.clone()))).collect()
}

/// True while the reference's attributes are still the ones the dialog listed: every stored
/// property of every row's attribute, its place included (two attributes can share a tag and a
/// value, and a reorder of the block's definitions swaps those).
fn rows_unchanged(app: &CadApp, h: Handle) -> bool {
    let Ok(st) = app.session.state() else { return false };
    let now = cadcraft_engine::attributes(&st.doc, h);
    now.len() == app.attedit.rows.len() && now.iter().zip(&app.attedit.rows).all(|(a, r)| r.is(a))
}

/// Apply: run `attedit` with the changed values; true when it went through (or nothing changed).
pub fn apply(app: &mut CadApp) -> bool {
    let Some(h) = edited(app) else { return false };
    let values = changed(&app.attedit.rows);
    if values.is_empty() {
        return true;
    }
    if !rows_unchanged(app, h) {
        // Another command reordered, renamed or edited the attributes while the dialog was open:
        // a row index would address a different attribute now. Show what the drawing has, and say
        // in the dialog and on the command line why nothing was written.
        load(app, h);
        let msg = crate::tl!("The attributes changed while this dialog was open. Review them and apply again.").to_string();
        app.session.echo(msg.clone());
        app.attedit.error = Some(msg);
        return false;
    }
    match app.run("attedit", json!({ "handle": h.hex(), "at": values })) {
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
    use cadcraft_geom::Vec3;

    fn row(tag: &str, value: &str, typed: &str, editable: bool) -> Row {
        Row {
            tag: tag.into(),
            prompt: String::new(),
            value: value.into(),
            editable,
            invisible: false,
            multiline: false,
            record: cadcraft_engine::doc::Attrib {
                tag: tag.into(),
                text: cadcraft_engine::doc::Text {
                    insert: Vec3::ZERO,
                    align_pt: None,
                    height: 1.0,
                    value: value.into(),
                    rotation: 0.0,
                    width_factor: 1.0,
                    oblique: 0.0,
                    style: "Standard".into(),
                    halign: cadcraft_engine::doc::HAlign::Left,
                    valign: cadcraft_engine::doc::VAlign::Baseline,
                },
                invisible: false,
                constant: !editable,
                prompt: String::new(),
                props: cadcraft_engine::doc::AttribProps::default(),
                multiline: None,
                modes: cadcraft_engine::doc::AttribModes::default(),
            },
            typed: typed.into(),
        }
    }

    #[test]
    fn only_changed_editable_values_are_sent() {
        let rows = [row("A", "1", "1", true), row("B", "2", "3", true), row("C", "x", "y", false)];
        let v = changed(&rows);
        assert_eq!(v.len(), 1);
        assert_eq!(v.get("1"), Some(&json!("3")));
    }
}
