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
    verify: bool,
    preset: bool,
    lock: bool,
    multiline: bool,
    height: f64,
    layer: String,
}

fn temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(egui::Id::unique(id)))
}

fn set_temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(id), v));
}

/// A text field committing on Enter / focus loss when changed (an empty value too); a
/// `multiline` one takes Enter as a line break and commits on focus loss. Returns the committed
/// value, and the changed value still being typed.
///
/// `label` is the visible row label's widget id, so the field has an accessible name.
fn text_field(ui: &mut egui::Ui, id: egui::Id, value: &str, label: egui::Id, multiline: bool) -> (Option<String>, Option<String>) {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    let edit = if multiline { egui::TextEdit::multiline(&mut buf).desired_rows(3) } else { egui::TextEdit::singleline(&mut buf) };
    let resp = ui.add(edit.id(id.with("te")).desired_width(180.0)).labelled_by(label);
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
        return (None, (buf != value).then_some(buf));
    }
    ui.data_mut(|d| d.remove::<String>(id));
    ((resp.lost_focus() && buf != value).then_some(buf), None)
}

/// A changed field value not committed yet: the `battman` parameters it commits with, and the
/// pass it was typed in.
#[derive(Clone)]
struct Pending {
    field: egui::Id,
    params: Value,
    pass: u64,
}

const PENDING: &str = "battman_pending";

/// The text fields of one pass. A field commits when it loses focus, but selecting another
/// attribute or block (or closing the dialog) can make it go before it sees that, so the value
/// being typed is kept and committed then instead of being lost.
struct Edits {
    pass: u64,
    /// The value being typed in the previous pass, and whether its field is still shown.
    last: Option<Pending>,
    last_shown: bool,
    typing: Option<Pending>,
    /// This pass's commits, in order.
    commits: Vec<Value>,
}

impl Edits {
    fn new(ctx: &egui::Context) -> Self {
        let pass = ctx.cumulative_pass_nr();
        // Only from the pass just before: a dialog closed by something else never commits later.
        let last = temp::<Pending>(ctx, PENDING).filter(|p| pass.saturating_sub(p.pass) <= 1);
        Self { pass, last, last_shown: false, typing: None, commits: Vec::new() }
    }

    /// Show a text field (see [`text_field`]); `params` gives the command's parameters for a
    /// value, `None` if it isn't one. Returns the value committed this pass.
    fn field(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        value: &str,
        label: egui::Id,
        multiline: bool,
        params: impl Fn(&str) -> Option<Value>,
    ) -> Option<String> {
        if self.last.as_ref().is_some_and(|p| p.field == id) {
            self.last_shown = true;
        }
        let (commit, typing) = text_field(ui, id, value, label, multiline);
        if let Some(p) = typing.and_then(|v| params(&v)) {
            self.typing = Some(Pending { field: id, params: p, pass: self.pass });
        }
        let v = commit?;
        self.commits.push(params(&v)?);
        Some(v)
    }

    /// The parameters to run, in order: a value whose field went away without committing it,
    /// this pass's commits, and the value being typed if the dialog is `closing`.
    fn finish(self, ctx: &egui::Context, closing: bool) -> Vec<Value> {
        let mut out = Vec::new();
        if let Some(p) = self.last.filter(|_| !self.last_shown) {
            ctx.data_mut(|d| d.remove::<String>(p.field));
            out.push(p.params);
        }
        out.extend(self.commits);
        match self.typing {
            Some(p) if !closing => set_temp(ctx, PENDING, p),
            typing => {
                ctx.data_mut(|d| d.remove::<Pending>(egui::Id::unique(PENDING)));
                if let Some(p) = typing {
                    ctx.data_mut(|d| d.remove::<String>(p.field));
                    out.push(p.params);
                }
            }
        }
        out
    }
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// Show the dialog; runs the text fields' commits and returns the command to run after them.
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
                        verify: a.modes.verify,
                        preset: a.modes.preset,
                        lock: a.modes.lock_position,
                        multiline: a.multiline.is_some(),
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
    let with_sync = |p: Value, apply: bool| {
        let mut p = p;
        if let Some(o) = p.as_object_mut() {
            o.insert("sync".into(), json!(apply));
        }
        p
    };
    let act = |p: Value, apply: bool| Some(("battman", with_sync(p, apply)));
    let mut edits = Edits::new(ctx);
    egui::Window::new("Block Attribute Manager").open(open).default_width(460.0).resizable(false).show(ctx, |ui| {
        if blocks.is_empty() {
            ui.label("This drawing has no blocks with attributes.");
            return;
        }
        ui.horizontal(|ui| {
            let label = ui.label("Block:");
            let combo = craft_ui::ComboBox::from_id_salt("battman_blocks").selected_text(block.as_str()).width(180.0).show_ui(ui, |ui| {
                for b in &blocks {
                    if ui.selectable_label(*b == block, b).clicked() {
                        block = b.clone();
                        sel = None;
                    }
                }
            });
            combo.response.labelled_by(label.id);
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
                        let modes = [(r.invisible, "I"), (r.constant, "C"), (r.verify, "V"), (r.preset, "P"), (r.lock, "L"), (r.multiline, "M")]
                            .iter()
                            .filter(|(on, _)| *on)
                            .map(|(_, m)| *m)
                            .collect::<String>();
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
                let text = |key: &'static str| move |v: &str| Some(with_sync(edit(key, json!(v)), apply));
                let label = ui.label("Tag");
                let id = egui::Id::unique(("battman_tag", &block, &r.tag));
                if let Some(v) = edits.field(ui, id, &r.tag, label.id, false, text("newTag")) {
                    sel = Some(v.trim().to_ascii_uppercase());
                }
                ui.end_row();
                let label = ui.label("Prompt");
                edits.field(ui, egui::Id::unique(("battman_prompt", &block, &r.tag)), &r.prompt, label.id, false, text("prompt"));
                ui.end_row();
                let label = ui.label("Default");
                // A multiline definition's default can hold several lines.
                let id = egui::Id::unique(("battman_default", &block, &r.tag));
                edits.field(ui, id, &r.default, label.id, r.multiline, text("default"));
                ui.end_row();
                ui.label("Mode");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        for (key, on, label) in
                            [("invisible", r.invisible, "Invisible"), ("constant", r.constant, "Constant"), ("verify", r.verify, "Verify")]
                        {
                            let mut v = on;
                            if ui.checkbox(&mut v, label).changed() {
                                action = act(edit(key, json!(v)), apply);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for (key, on, label) in
                            [("preset", r.preset, "Preset"), ("lockPosition", r.lock, "Lock position"), ("multiline", r.multiline, "Multiple lines")]
                        {
                            let mut v = on;
                            if ui.checkbox(&mut v, label).changed() {
                                action = act(edit(key, json!(v)), apply);
                            }
                        }
                    });
                });
                ui.end_row();
                let label = ui.label("Height");
                let height = |v: &str| {
                    let h = v.trim().parse::<f64>().ok().filter(|h| h.is_finite() && *h > 0.0)?;
                    Some(with_sync(edit("height", json!(h)), apply))
                };
                edits.field(ui, egui::Id::unique(("battman_height", &block, &r.tag)), &fmt_num(r.height), label.id, false, height);
                ui.end_row();
                let label = ui.label("Layer");
                let combo = craft_ui::ComboBox::from_id_salt("battman_layer").selected_text(r.layer.as_str()).show_ui(ui, |ui| {
                    for l in &layers {
                        if ui.selectable_label(*l == r.layer, l).clicked() && *l != r.layer {
                            action = act(edit("layer", json!(l)), apply);
                        }
                    }
                });
                combo.response.labelled_by(label.id);
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
    // The fields' commits first: an edit typed before a button was clicked happened before it.
    for p in edits.finish(ctx, !*open) {
        // Errors are echoed on the command line by `run`.
        let _ = app.run("battman", p);
    }
    action
}

/// References of block `name` in model space, the layouts and other blocks.
fn count_references(d: &cadcraft_doc::Drawing, name: &str) -> usize {
    let is_ref = |e: &std::sync::Arc<cadcraft_doc::Entity>| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name));
    let spaces = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
    let blocks = d.blocks.iter().filter(|(k, _)| !k.eq_ignore_ascii_case(name)).map(|(_, b)| &b.entities);
    spaces.chain(blocks).map(|st| st.iter().filter(|e| is_ref(e)).count()).sum()
}
