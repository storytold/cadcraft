//! Block Attribute Manager (BATTMAN): a block's attribute definitions in a list, with buttons to
//! synchronize, move and remove them and fields to edit the selected one. Every change runs the
//! `battman` command (`app.run`), so it is undoable and scriptable.

use cadcraft_doc::EntityKind;
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// One attribute definition as listed.
struct Row {
    tag: String,
    prompt: String,
    default: String,
    invisible: bool,
    constant: bool,
    height: f64,
    layer: String,
}

fn temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(egui::Id::unique(id)))
}

fn set_temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(id), v));
}

/// A text field committing on Enter / focus loss when changed (an empty value too).
///
/// `label` is the visible row label's widget id, so the field has an accessible name.
fn text_field(ui: &mut egui::Ui, id: egui::Id, value: &str, label: egui::Id) -> Option<String> {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).id(id.with("te")).desired_width(180.0)).labelled_by(label);
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    (resp.lost_focus() && buf != value).then_some(buf)
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// Show the dialog; returns the command to run.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Option<(&'static str, Value)> {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        egui::Window::new("Block Attribute Manager").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("Open a drawing first.");
        });
        return None;
    };
    let mut blocks: Vec<String> = d
        .blocks
        .values()
        .filter(|b| !b.anonymous && b.entities.iter().any(|e| matches!(e.kind, EntityKind::AttDef(_))))
        .map(|b| b.name.clone())
        .collect();
    blocks.sort_by_key(|n| n.to_ascii_uppercase());
    let layers: Vec<String> = d.layers.iter().map(|l| l.name.clone()).collect();
    let mut block = temp::<String>(ctx, "battman_block").filter(|b| blocks.contains(b)).or_else(|| blocks.first().cloned()).unwrap_or_default();
    let rows: Vec<Row> = d
        .block(&block)
        .map(|b| {
            b.entities
                .iter()
                .filter_map(|e| match &e.kind {
                    EntityKind::AttDef(a) => Some(Row {
                        tag: a.tag.clone(),
                        prompt: a.prompt.clone(),
                        default: a.text.value.clone(),
                        invisible: a.invisible,
                        constant: a.constant,
                        height: a.text.height,
                        layer: e.common.layer.clone(),
                    }),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let refs = count_references(d, &block);
    let mut sel = temp::<String>(ctx, "battman_sel").filter(|s| rows.iter().any(|r| r.tag == *s)).or_else(|| rows.first().map(|r| r.tag.clone()));
    let mut apply = temp::<bool>(ctx, "battman_apply").unwrap_or(true);
    let mut action: Option<(&'static str, Value)> = None;
    let act = |p: Value, apply: bool| {
        let mut p = p;
        if let Some(o) = p.as_object_mut() {
            o.insert("sync".into(), json!(apply));
        }
        Some(("battman", p))
    };
    egui::Window::new("Block Attribute Manager").open(open).default_width(460.0).resizable(false).show(ctx, |ui| {
        if blocks.is_empty() {
            ui.label("This drawing has no blocks with attributes.");
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Block:");
            craft_ui::ComboBox::from_id_salt("battman_blocks").selected_text(block.as_str()).width(180.0).show_ui(ui, |ui| {
                for b in &blocks {
                    if ui.selectable_label(*b == block, b).clicked() {
                        block = b.clone();
                        sel = None;
                    }
                }
            });
            ui.label(RichText::new(format!("Found in drawing: {refs}")).color(t.text_dim));
        });
        ui.separator();
        {
            egui::ScrollArea::vertical().max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("battman_grid").striped(true).num_columns(4).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                    for h in ["Tag", "Prompt", "Default", "Modes"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &rows {
                        if ui.selectable_label(sel.as_deref() == Some(r.tag.as_str()), &r.tag).clicked() {
                            sel = Some(r.tag.clone());
                        }
                        ui.label(&r.prompt);
                        ui.label(&r.default);
                        let modes = [(r.invisible, "I"), (r.constant, "C")].iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect::<String>();
                        ui.label(RichText::new(modes).color(t.text_dim));
                        ui.end_row();
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button("Sync").on_hover_text("Bring every reference in line with these definitions").clicked() {
                    action = act(json!({ "name": block, "action": "sync" }), true);
                }
                let picked = sel.clone().unwrap_or_default();
                let i = rows.iter().position(|r| r.tag == picked);
                if ui.add_enabled(i.is_some_and(|i| i > 0), egui::Button::new("Move Up")).clicked() {
                    action = act(json!({ "name": block, "action": "up", "tag": picked }), apply);
                }
                if ui.add_enabled(i.is_some_and(|i| i + 1 < rows.len()), egui::Button::new("Move Down")).clicked() {
                    action = act(json!({ "name": block, "action": "down", "tag": picked }), apply);
                }
                if ui.add_enabled(i.is_some(), egui::Button::new("Remove")).clicked() {
                    action = act(json!({ "name": block, "action": "remove", "tag": picked }), apply);
                }
            });
        }
        ui.separator();
        if let Some(r) = sel.as_ref().and_then(|s| rows.iter().find(|r| r.tag == *s)) {
            ui.label(RichText::new(format!("Edit attribute {}", r.tag)).strong());
            let edit = |key: &str, v: Value| json!({ "name": block, "action": "edit", "tag": r.tag, key: v });
            egui::Grid::new("battman_edit").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                let label = ui.label("Tag");
                if let Some(v) = text_field(ui, egui::Id::unique(("battman_tag", &block, &r.tag)), &r.tag, label.id) {
                    action = act(edit("newTag", json!(v)), apply);
                    sel = Some(v.trim().to_ascii_uppercase());
                }
                ui.end_row();
                let label = ui.label("Prompt");
                if let Some(v) = text_field(ui, egui::Id::unique(("battman_prompt", &block, &r.tag)), &r.prompt, label.id) {
                    action = act(edit("prompt", json!(v)), apply);
                }
                ui.end_row();
                let label = ui.label("Default");
                if let Some(v) = text_field(ui, egui::Id::unique(("battman_default", &block, &r.tag)), &r.default, label.id) {
                    action = act(edit("default", json!(v)), apply);
                }
                ui.end_row();
                ui.label("Mode");
                ui.horizontal(|ui| {
                    let (mut inv, mut con) = (r.invisible, r.constant);
                    if ui.checkbox(&mut inv, "Invisible").changed() {
                        action = act(edit("invisible", json!(inv)), apply);
                    }
                    if ui.checkbox(&mut con, "Constant").changed() {
                        action = act(edit("constant", json!(con)), apply);
                    }
                });
                ui.end_row();
                let label = ui.label("Height");
                let height = text_field(ui, egui::Id::unique(("battman_height", &block, &r.tag)), &fmt_num(r.height), label.id);
                if let Some(h) = height.and_then(|v| v.trim().parse::<f64>().ok()).filter(|h| h.is_finite() && *h > 0.0) {
                    action = act(edit("height", json!(h)), apply);
                }
                ui.end_row();
                ui.label("Layer");
                craft_ui::ComboBox::from_id_salt("battman_layer").selected_text(r.layer.as_str()).show_ui(ui, |ui| {
                    for l in &layers {
                        if ui.selectable_label(*l == r.layer, l).clicked() && *l != r.layer {
                            action = act(edit("layer", json!(l)), apply);
                        }
                    }
                });
                ui.end_row();
            });
        }
        ui.separator();
        ui.checkbox(&mut apply, "Apply changes to existing references");
    });
    set_temp(ctx, "battman_block", block);
    if let Some(s) = sel {
        set_temp(ctx, "battman_sel", s);
    }
    set_temp(ctx, "battman_apply", apply);
    action
}

/// References of block `name` in model space, the layouts and other blocks.
fn count_references(d: &cadcraft_doc::Drawing, name: &str) -> usize {
    let is_ref = |e: &std::sync::Arc<cadcraft_doc::Entity>| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name));
    let spaces = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
    let blocks = d.blocks.iter().filter(|(k, _)| !k.eq_ignore_ascii_case(name)).map(|(_, b)| &b.entities);
    spaces.chain(blocks).map(|st| st.iter().filter(|e| is_ref(e)).count()).sum()
}
