//! The Edit Block Definition dialog: BEDIT typed or started from a menu or the Tool Sets with no
//! block reference selected. It lists the drawing's blocks with a preview and description; a name
//! no block has yet starts a new block. OK runs BEDIT with that name; the dialog itself never
//! changes the drawing. JSON calls (`bedit {name}`) and a selected block reference skip it.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_engine::blockedit::CURRENT_DRAWING;
use cadcraft_engine::doc::{Drawing, EntityKind};
use cadcraft_geom::Vec2;
use cadcraft_render::DisplayList;
use egui::{RichText, Stroke, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// The dialog's state while it is open.
#[derive(Default)]
pub struct State {
    /// "Block to create or edit".
    pub name: String,
    /// Why the last OK failed, shown until the name changes.
    pub error: Option<String>,
    /// The preview of the block named, for (drawing uid, revision, block name).
    preview: Option<((u64, u64, String), Arc<DisplayList>)>,
}

/// The blocks the dialog offers: named blocks that aren't external references, sorted by name.
pub fn editable_blocks(d: &Drawing) -> Vec<String> {
    let mut names: Vec<String> =
        d.blocks.values().filter(|b| !b.anonymous && !b.name.starts_with('*') && b.xref_path.is_none()).map(|b| b.name.clone()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    // The drawing itself comes first, as in AutoCAD.
    names.insert(0, CURRENT_DRAWING.to_string());
    names
}

/// Opening a drawing with authoring elements (a dynamic block's own drawing) asks whether to
/// open it in the Block Editor, as AutoCAD does: Yes edits the drawing itself in the Block
/// Editor, No opens it as usual, Cancel closes it again.
pub fn authoring_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let name = app
        .session
        .state()
        .ok()
        .and_then(|st| st.path.clone())
        .and_then(|p| std::path::Path::new(&p).file_name().map(|f| f.to_string_lossy().into_owned()))
        .unwrap_or_else(|| crate::i18n::t("This drawing").to_string());
    let mut choice: Option<&str> = None;
    egui::Window::new(crate::tl!("Open in Block Editor"))
        .id(egui::Id::unique("Open in Block Editor"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(format!("{name} {}", crate::i18n::t("contains authoring elements. Open in Block Editor?")));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("Yes")).clicked() {
                    choice = Some("yes");
                }
                if ui.button(crate::tl!("No")).clicked() {
                    choice = Some("no");
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    choice = Some("cancel");
                }
            });
        });
    let Some(c) = choice else { return };
    *open = false;
    let r = match c {
        "yes" => app.run("bedit", json!({ "name": CURRENT_DRAWING })).map(|_| ()),
        "cancel" => app.run("close", Value::Null).map(|_| ()),
        _ => Ok(()),
    };
    if let Err(e) = r {
        app.session.echo(e);
    }
}

/// BEDIT typed or from a menu without parameters opens the dialog, unless exactly one block
/// reference is selected (it opens at once) or the Block Editor is open (BEDIT says why not).
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !matches!(id, "bedit" | "be") || !params.is_null() {
        return None;
    }
    let st = app.session.state().ok()?;
    if st.block_edit.is_some() {
        return None;
    }
    let refs = st.selection.iter().filter(|h| st.doc.entity(**h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_)))).count();
    if refs == 1 && st.selection.len() == 1 {
        return None;
    }
    // Start on the block last edited if it is still there, else the first one.
    let names = editable_blocks(&st.doc);
    let last = app.bedit.name.clone();
    let name = if names.iter().any(|n| n.eq_ignore_ascii_case(&last)) { last } else { names.first().cloned().unwrap_or_default() };
    app.bedit = State { name, ..State::default() };
    app.ui.dialog = Some("bedit".into());
    Some(Ok(Value::Null))
}

/// The preview of block `name` in `d`, built once per drawing revision.
fn preview(app: &mut CadApp, name: &str) -> Option<Arc<DisplayList>> {
    let st = app.session.state().ok()?;
    let key = (st.uid, st.revision, name.to_ascii_uppercase());
    if let Some((k, list)) = &app.bedit.preview
        && *k == key
    {
        return Some(list.clone());
    }
    // The drawing itself shows its model space.
    let entities = if name.eq_ignore_ascii_case(CURRENT_DRAWING) { &st.doc.model } else { &st.doc.block(name)?.entities };
    let rough = cadcraft_render::build_entities(&st.doc, entities.iter().map(|e| e.as_ref()), &cadcraft_render::Options::default());
    let size = rough.bounds.width().max(rough.bounds.height());
    // About half a pixel of the preview's 200 points.
    let opts = cadcraft_render::Options { tolerance: (size / 400.0).max(1e-9), ..Default::default() };
    let list =
        Arc::new(if rough.bounds.is_empty() { rough } else { cadcraft_render::build_entities(&st.doc, entities.iter().map(|e| e.as_ref()), &opts) });
    app.bedit.preview = Some((key, list.clone()));
    Some(list)
}

/// Draw `list` fitted into `rect`, or `note` when there is nothing to draw.
fn draw_preview(ui: &mut egui::Ui, list: Option<&DisplayList>, note: &str) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(220.0, 170.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, t.canvas);
    p.rect_stroke(rect, 4.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    match list.filter(|l| !l.bounds.is_empty() && !l.prims.is_empty()) {
        Some(l) => {
            let b = l.bounds;
            let inner = rect.shrink(12.0);
            let (w, h) = (b.width().max(1e-9), b.height().max(1e-9));
            let scale = (f64::from(inner.width()) / w).min(f64::from(inner.height()) / h);
            let xf = Xf { rect, center: Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0), scale };
            crate::canvas::draw_list(&p, &xf, l, Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()), false);
        }
        None => {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, note, crate::theme::small(), t.text_dim);
        }
    }
}

/// The Edit Block Definition window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let Ok(d) = app.session.state().map(|st| st.doc.clone()) else {
        *open = false;
        return;
    };
    let names = editable_blocks(&d);
    let typed = app.bedit.name.trim().to_string();
    let existing = names.iter().find(|n| n.eq_ignore_ascii_case(&typed)).cloned();
    let list = existing.as_deref().and_then(|n| preview(app, n));
    let description = existing.as_deref().and_then(|n| d.block(n)).map(|b| b.description.clone()).unwrap_or_default();
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Edit Block Definition"))
        .id(egui::Id::unique("Edit Block Definition"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            let name_label = ui.label(crate::tl!("Block to create or edit"));
            let edit = ui.add(egui::TextEdit::singleline(&mut app.bedit.name).desired_width(460.0)).labelled_by(name_label.id);
            if edit.changed() {
                app.bedit.error = None;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                egui::Frame::new().fill(t.field).stroke(Stroke::new(1.0, t.border)).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                    ui.set_min_size(vec2(222.0, 250.0));
                    egui::ScrollArea::vertical().id_salt("bedit-blocks").max_height(250.0).min_scrolled_height(250.0).show(ui, |ui| {
                        ui.set_width(222.0);
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                            for n in &names {
                                let row = ui.selectable_label(existing.as_deref() == Some(n.as_str()), n);
                                if row.clicked() {
                                    app.bedit.name = n.clone();
                                    app.bedit.error = None;
                                }
                                if row.double_clicked() {
                                    ok = true;
                                }
                            }
                        });
                    });
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::tl!("Preview")).color(t.text_dim));
                    let note = if typed.is_empty() {
                        String::new()
                    } else if existing.is_none() {
                        crate::tl!("New block").to_string()
                    } else {
                        String::new()
                    };
                    draw_preview(ui, list.as_deref(), &note);
                    ui.add_space(4.0);
                    ui.label(RichText::new(crate::tl!("Description")).color(t.text_dim));
                    let text = if description.is_empty() { "—".to_string() } else { description.clone() };
                    ui.add(egui::Label::new(text).wrap());
                });
            });
            if let Some(e) = &app.bedit.error {
                ui.colored_label(t.warn, e);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.add_enabled(!typed.is_empty(), egui::Button::new(crate::tl!("OK"))).clicked() {
                        ok = true;
                    }
                });
            });
        });
    if cancel || (ok && confirm(app)) {
        *open = false;
    }
}

/// OK: open the Block Editor on the name given (a new name starts a new block). Returns whether
/// it opened; if not, the dialog shows why and stays open.
pub fn confirm(app: &mut CadApp) -> bool {
    let name = app.bedit.name.trim().to_string();
    if name.is_empty() {
        return false;
    }
    match app.run("bedit", json!({ "name": name, "create": true })) {
        Ok(_) => true,
        Err(e) => {
            app.bedit.error = Some(e);
            false
        }
    }
}
