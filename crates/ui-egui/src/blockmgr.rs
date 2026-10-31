//! The Block Manager: the drawing's blocks in a list beside a preview of the selected one, its
//! details and what can be done with it (insert, edit, create, rename, delete, write to a file).

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind};
use cadcraft_geom::{Bounds2, Vec2};
use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// The dialog's own state between frames.
#[derive(Clone, Default)]
struct State {
    selected: Option<String>,
    filter: String,
    /// The new name while renaming the selected block.
    rename: Option<String>,
    /// The last message (a refused delete or rename).
    note: Option<String>,
}

/// A block's preview, built once per block and drawing revision.
#[derive(Clone)]
struct Preview {
    key: (String, u64, u64),
    list: Arc<cadcraft_render::DisplayList>,
    bounds: Bounds2,
}

fn state_id() -> egui::Id {
    egui::Id::unique("cc_block_manager")
}

/// The named blocks a user works with: not the generated (`*D12`) ones nor layout blocks.
pub fn names(d: &Drawing) -> Vec<String> {
    let mut v: Vec<String> = d.blocks.keys().filter(|k| !k.starts_with('*')).cloned().collect();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

/// How many block references to `name` the drawing holds (model, layouts and other blocks).
pub fn reference_count(d: &Drawing, name: &str) -> usize {
    std::iter::once(&d.model)
        .chain(d.layouts.iter().map(|l| &l.entities))
        .chain(d.blocks.values().map(|b| &b.entities))
        .flat_map(|s| s.iter())
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)))
        .count()
}

/// Build the preview of block `name`: its objects as they draw, and their extents.
fn build(d: &Drawing, name: &str) -> Option<(cadcraft_render::DisplayList, Bounds2)> {
    let b = d.block(name)?;
    let mut bounds = Bounds2::EMPTY;
    for e in b.entities.iter().take(200_000) {
        let eb = cadcraft_doc::entity_bounds(d, e, 0);
        if !eb.is_empty() && eb.width().is_finite() && eb.height().is_finite() {
            bounds = bounds.union(&eb);
        }
    }
    if bounds.is_empty() {
        return Some((cadcraft_render::DisplayList::default(), bounds));
    }
    let size = bounds.width().max(bounds.height()).max(1e-9);
    let opts = cadcraft_render::Options { tolerance: size / 500.0, min_dash: size / 300.0, view_height: size, ..Default::default() };
    let list = cadcraft_render::build_entities(d, b.entities.iter().map(|e| &**e), &opts);
    Some((list, bounds))
}

/// Draw a display list fitted into `r` on `bg` (a framed square of the canvas colour).
pub fn paint_list(p: &egui::Painter, r: Rect, list: &cadcraft_render::DisplayList, bounds: &Bounds2) {
    let t = Tokens::get();
    p.rect_filled(r, 3.0, t.canvas);
    let inner = r.shrink(10.0);
    if !bounds.is_empty() {
        let (w, h) = (bounds.width().max(1e-12), bounds.height().max(1e-12));
        let k = (f64::from(inner.width()) / w).min(f64::from(inner.height()) / h);
        let c = bounds.center();
        let mid = inner.center();
        let at = |v: &Vec2| pos2(mid.x + ((v.x - c.x) * k) as f32, mid.y - ((v.y - c.y) * k) as f32);
        let bg = cadcraft_color::Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b());
        let clip = p.with_clip_rect(r.shrink(1.0));
        for prim in &list.prims {
            let rgb = prim.display_rgb(bg);
            let col = egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2);
            let pts = list.points(prim);
            match prim.kind {
                cadcraft_render::Kind::Polyline if pts.len() >= 2 => {
                    clip.add(egui::Shape::line(pts.iter().map(at).collect(), Stroke::new(1.0, col)));
                }
                cadcraft_render::Kind::Tris | cadcraft_render::Kind::Mask => {
                    let fill = if prim.kind == cadcraft_render::Kind::Mask { t.canvas } else { col };
                    let mut mesh = egui::Mesh::default();
                    for tri in pts.chunks_exact(3) {
                        let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
                        for v in tri {
                            mesh.colored_vertex(at(v), fill);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    clip.add(egui::Shape::mesh(mesh));
                }
                cadcraft_render::Kind::Point => {
                    if let Some(v) = pts.first() {
                        clip.circle_filled(at(v), 1.2, col);
                    }
                }
                _ => {}
            }
        }
    }
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

/// The preview of `name`, from the cache when the drawing hasn't changed.
fn preview(app: &CadApp, ctx: &egui::Context, name: &str) -> Option<Preview> {
    // The drawing too: two open drawings can have a block of that name at the same revision.
    let (doc, revision) = app.session.state().map(|s| (s.uid, s.revision)).unwrap_or((0, 0));
    let key = (name.to_string(), doc, revision);
    let id = egui::Id::unique("cc_block_manager_preview");
    if let Some(p) = ctx.data(|d| d.get_temp::<Preview>(id)).filter(|p| p.key == key) {
        return Some(p);
    }
    let d = app.session.doc().ok()?;
    let (list, bounds) = build(d, name)?;
    let p = Preview { key, list: Arc::new(list), bounds };
    ctx.data_mut(|d| d.insert_temp(id, p.clone()));
    Some(p)
}

/// [`reference_count`] of block `name`, counted once per block, drawing and revision (the
/// panel draws every frame; a count walks the whole drawing).
fn references(app: &CadApp, ctx: &egui::Context, name: &str) -> usize {
    let (doc, revision) = app.session.state().map(|s| (s.uid, s.revision)).unwrap_or((0, 0));
    let key = (name.to_string(), doc, revision);
    let id = egui::Id::unique("cc_block_manager_references");
    if let Some((k, n)) = ctx.data(|d| d.get_temp::<((String, u64, u64), usize)>(id))
        && k == key
    {
        return n;
    }
    let n = app.session.doc().map(|d| reference_count(d, name)).unwrap_or(0);
    ctx.data_mut(|d| d.insert_temp(id, (key, n)));
    n
}

/// What a button asked for, done once the window is drawn.
enum Act {
    Insert(String),
    Edit(String),
    Create,
    Rename(String, String),
    Delete(String),
    Write(String),
    /// Pin the window as a panel on the right; float the panel as a window.
    ToPanel,
    ToWindow,
}

/// The data the Block Manager shows this frame.
struct View {
    all: Vec<String>,
    selected: Option<String>,
    info: Option<(usize, usize, cadcraft_geom::Vec3, bool, String, Option<String>)>,
    pv: Option<Preview>,
    can_write: bool,
}

fn view(app: &CadApp, ctx: &egui::Context, st: &mut State) -> Option<View> {
    let d = app.session.doc().ok()?;
    let all = names(d);
    if st.selected.as_ref().is_some_and(|s| !all.iter().any(|n| n == s)) {
        st.selected = None;
        st.rename = None;
    }
    if st.selected.is_none() {
        st.selected = all.first().cloned();
    }
    let selected = st.selected.clone();
    let info = selected.as_deref().and_then(|n| {
        let b = d.block(n)?;
        Some((b.entities.len(), references(app, ctx, n), b.base, b.annotative, b.description.clone(), b.xref_path.clone()))
    });
    let pv = selected.as_deref().and_then(|n| preview(app, ctx, n));
    Some(View { all, selected, info, pv, can_write: app.services.pick_save.is_some() })
}

/// The manager's contents: in a window the list sits left of the preview, in a panel above it.
fn body(ui: &mut egui::Ui, v: &View, st: &mut State, as_panel: bool, act: &mut Option<Act>) {
    let View { all, selected, info, pv, can_write } = v;
    let can_write = *can_write;
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let w = if as_panel { (ui.available_width() - 70.0).max(80.0) } else { 200.0 };
        ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(crate::tl!("Search blocks")).desired_width(w));
        if !as_panel {
            ui.label(egui::RichText::new(format!("{} {}", all.len(), crate::tl!("blocks"))).color(t.text_dim));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Window ⇄ panel.
            let (icon, tip) = if as_panel {
                (crate::icons::Icon::Workspace, crate::tl!("Float as a window"))
            } else {
                (crate::icons::Icon::Properties, crate::tl!("Pin as a panel on the right"))
            };
            if crate::icons::button(ui, icon, 20.0, tip, false).clicked() {
                *act = Some(if as_panel { Act::ToWindow } else { Act::ToPanel });
            }
            if !as_panel && ui.button(crate::tl!("Create...")).on_hover_text(crate::tl!("Make a block from objects (BLOCK)")).clicked() {
                *act = Some(Act::Create);
            }
        });
    });
    ui.add_space(4.0);
    let avail = ui.available_size();
    let (list_w, height) =
        if as_panel { (avail.x, (avail.y * 0.38).clamp(120.0, 320.0)) } else { (230.0_f32.min(avail.x * 0.45), (avail.y - 4.0).max(200.0)) };
    let list = |ui: &mut egui::Ui, st: &mut State, act: &mut Option<Act>| {
        ui.allocate_ui_with_layout(vec2(list_w, height), egui::Layout::top_down(egui::Align::LEFT), |ui| {
            egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(2.0).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("cc_block_list").auto_shrink([false, false]).max_height(height - 6.0).show(ui, |ui| {
                    ui.set_width(list_w - 8.0);
                    let filter = st.filter.trim().to_lowercase();
                    let mut shown = 0;
                    for n in all.iter().filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter)) {
                        shown += 1;
                        let on = selected.as_deref() == Some(n.as_str());
                        let r = ui.selectable_label(on, n);
                        if r.clicked() {
                            st.selected = Some(n.clone());
                            st.rename = None;
                            st.note = None;
                        }
                        if r.double_clicked() {
                            *act = Some(Act::Insert(n.clone()));
                        }
                        r.on_hover_text(crate::tl!("Double-click to insert"));
                    }
                    if shown == 0 {
                        let msg =
                            if all.is_empty() { crate::tl!("No blocks defined in this drawing.") } else { crate::tl!("No blocks match the search.") };
                        ui.label(egui::RichText::new(msg).color(t.text_dim));
                    }
                });
            });
        });
    };
    let details = |ui: &mut egui::Ui, st: &mut State, act: &mut Option<Act>| {
        let side = if as_panel { ui.available_width().clamp(120.0, 360.0) } else { ui.available_width().min(height - 150.0).clamp(140.0, 320.0) };
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(side), side.min(if as_panel { 220.0 } else { side })), Sense::hover());
        match pv {
            Some(p) => paint_list(ui.painter(), r, &p.list, &p.bounds),
            None => {
                ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, crate::tl!("Preview"), crate::theme::small(), t.text_faint);
            }
        }
        if pv.as_ref().is_some_and(|p| p.bounds.is_empty()) {
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, crate::tl!("The block is empty"), crate::theme::small(), t.text_faint);
        }
        ui.add_space(4.0);
        let Some(name) = selected.clone() else { return };
        if let Some(mut new) = st.rename.take() {
            let mut keep = true;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut new).desired_width(160.0));
                if ui.button(crate::tl!("OK")).clicked() && !new.trim().is_empty() {
                    *act = Some(Act::Rename(name.clone(), new.trim().to_string()));
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    keep = false;
                }
            });
            if keep {
                st.rename = Some(new);
            }
        } else {
            ui.label(egui::RichText::new(&name).strong());
        }
        if let Some((objects, refs, base, annotative, description, xref)) = info {
            egui::Grid::new("cc_block_info").num_columns(2).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
                ui.label(egui::RichText::new(crate::tl!("Objects")).color(t.text_dim));
                ui.label(objects.to_string());
                ui.end_row();
                ui.label(egui::RichText::new(crate::tl!("References")).color(t.text_dim));
                ui.label(refs.to_string());
                ui.end_row();
                ui.label(egui::RichText::new(crate::tl!("Base point")).color(t.text_dim));
                ui.label(format!("{:.4}, {:.4}", base.x, base.y));
                ui.end_row();
                ui.label(egui::RichText::new(crate::tl!("Annotative")).color(t.text_dim));
                ui.label(if *annotative { crate::tl!("Yes") } else { crate::tl!("No") });
                ui.end_row();
                if let Some(path) = xref {
                    ui.label(egui::RichText::new(crate::tl!("External reference")).color(t.text_dim));
                    ui.label(path);
                    ui.end_row();
                }
                if !description.is_empty() {
                    ui.label(egui::RichText::new(crate::tl!("Description")).color(t.text_dim));
                    ui.label(description);
                    ui.end_row();
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::tl!("Insert")).clicked() {
                *act = Some(Act::Insert(name.clone()));
            }
            if ui.button(crate::tl!("Edit Block")).on_hover_text(crate::tl!("Open in the Block Editor")).clicked() {
                *act = Some(Act::Edit(name.clone()));
            }
            if ui.button(crate::tl!("Rename...")).clicked() {
                st.rename = Some(name.clone());
            }
            if ui.button(crate::tl!("Delete")).on_hover_text(crate::tl!("Delete a block that no object uses")).clicked() {
                *act = Some(Act::Delete(name.clone()));
            }
            if can_write && ui.button(crate::tl!("Save to File...")).on_hover_text(crate::tl!("Write the block as a drawing (WBLOCK)")).clicked() {
                *act = Some(Act::Write(name.clone()));
            }
            if as_panel && ui.button(crate::tl!("Create...")).on_hover_text(crate::tl!("Make a block from objects (BLOCK)")).clicked() {
                *act = Some(Act::Create);
            }
        });
        if let Some(n) = &st.note {
            ui.label(egui::RichText::new(n).color(t.text_dim));
        }
    };
    if as_panel {
        list(ui, st, act);
        ui.add_space(6.0);
        details(ui, st, act);
    } else {
        ui.horizontal_top(|ui| {
            list(ui, st, act);
            ui.vertical(|ui| details(ui, st, act));
        });
    }
}

/// Do what a button asked; false when the manager should close (a command takes over).
fn apply(app: &mut CadApp, st: &mut State, act: Option<Act>) -> bool {
    match act {
        Some(Act::Insert(n)) => {
            crate::dialogs::insert_block(app, &n);
            return false;
        }
        Some(Act::Edit(n)) => {
            if let Err(e) = app.run("bedit", json!({ "name": n })) {
                app.session.echo(e);
            }
            return false;
        }
        Some(Act::Create) => {
            app.start("block");
            return false;
        }
        Some(Act::Rename(from, to)) => match app.run("rename", json!({ "table": "block", "from": from, "to": to })) {
            Ok(_) => {
                st.selected = Some(to);
                st.rename = None;
                st.note = None;
            }
            Err(e) => st.note = Some(e),
        },
        Some(Act::Delete(n)) => {
            let refs = app.session.doc().map(|d| reference_count(d, &n)).unwrap_or(0);
            if refs > 0 {
                st.note = Some(format!("{} ({refs})", crate::tl!("The block is in use and can't be deleted")));
            } else {
                match app.run("purge", json!({ "type": "blocks", "names": n })) {
                    Ok(_) => {
                        st.selected = None;
                        st.note = None;
                    }
                    Err(e) => st.note = Some(e),
                }
            }
        }
        Some(Act::Write(n)) => {
            if let Some(pick) = app.services.pick_save.as_ref()
                && let Some(path) = pick(&format!("{n}.dwg"))
                && let Err(e) = app.run("wblock", json!({ "path": path, "name": n }))
            {
                st.note = Some(e);
            }
        }
        Some(Act::ToPanel) => {
            app.ui.blocks_window = false;
            let _ = open_panel(app);
            return false;
        }
        Some(Act::ToWindow) => {
            app.ui.blocks_window = true;
            let _ = crate::docking::command(app, false, &json!({ "operation": "close", "panel": crate::docking::Panel::Blocks.id() }));
            app.ui.dialog = Some("blocks".into());
        }
        None => {}
    }
    true
}

/// Open the Block Manager the way it was last used: a panel pinned on the right (the first
/// time) or a window.
pub fn open(app: &mut CadApp) -> Result<serde_json::Value, String> {
    if app.ui.blocks_window {
        app.ui.dialog = Some("blocks".into());
        return Ok(serde_json::Value::Null);
    }
    open_panel(app)
}

/// Show the Blocks panel, docked on the right of the drawing.
fn open_panel(app: &mut CadApp) -> Result<serde_json::Value, String> {
    if app.ui.dialog.as_deref() == Some("blocks") {
        app.ui.dialog = None;
    }
    let panel = crate::docking::Panel::Blocks;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    // The first time: pinned at the right edge, beside the Layers and Properties palettes.
    if first && app.ui.docking.layout.contains(&crate::docking::Panel::Properties) {
        return crate::docking::command(app, false, &json!({ "operation": "move", "panel": panel.id(), "target": "properties", "zone": "right" }));
    }
    Ok(r)
}

/// The Blocks panel.
pub fn panel(app: &mut CadApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let Some(v) = view(app, &ctx, &mut st) else { return };
    let mut act = None;
    egui::ScrollArea::vertical().id_salt("dock-blocks").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::NONE.inner_margin(6.0).show(ui, |ui| body(ui, &v, &mut st, true, &mut act));
    });
    // The panel stays open whatever the button did.
    apply(app, &mut st, act);
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
}

/// The Block Manager as a window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    // No drawing open: the window closes (it doesn't wait to appear with the next drawing).
    let Some(v) = view(app, ctx, &mut st) else {
        *open = false;
        return;
    };
    let mut act = None;
    egui::Window::new(crate::tl!("Block Manager"))
        .id(egui::Id::unique("Blocks"))
        .open(open)
        .default_size(vec2(640.0, 440.0))
        .min_size(vec2(480.0, 320.0))
        .resizable(true)
        .show(ctx, |ui| body(ui, &v, &mut st, false, &mut act));
    let to_panel = matches!(act, Some(Act::ToPanel));
    if !apply(app, &mut st, act) && !to_panel {
        *open = false;
    }
    if to_panel {
        *open = false;
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
}

#[cfg(test)]
mod tests {
    use cadcraft_engine::Session;
    use serde_json::json;

    #[test]
    fn blocks_list_their_references_and_build_a_preview() {
        let mut s = Session::new();
        let c = s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        s.execute("block", &json!({"name": "Bolt", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
        s.execute("insert", &json!({"name": "Bolt", "at": [5, 5]})).unwrap();
        s.execute("insert", &json!({"name": "Bolt", "at": [9, 5]})).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(super::names(d), ["Bolt"]);
        assert_eq!(super::reference_count(d, "bolt"), 2);
        let (list, bounds) = super::build(d, "Bolt").unwrap();
        assert!(!list.prims.is_empty());
        assert!((bounds.width() - 4.0).abs() < 1e-6);
    }

    /// The window closes when no drawing is open, instead of waiting to appear with the next
    /// one (#2327 review).
    #[test]
    fn the_window_closes_without_a_drawing() {
        let mut app = crate::CadApp::new(Session::empty(), crate::Services::default());
        let ctx = egui::Context::default();
        let mut open = true;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| super::dialog(&mut app, ui.ctx(), &mut open));
        out.textures_delta.clear();
        assert!(!open);
    }

    /// The reference count is kept per revision: it follows a new reference.
    #[test]
    fn the_reference_count_follows_the_drawing() {
        let mut app = crate::CadApp::new(Session::new(), crate::Services::default());
        let c = app.run("circle", json!({"center": [0, 0], "radius": 2})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        app.run("block", json!({"name": "Bolt", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
        app.run("insert", json!({"name": "Bolt", "at": [5, 5]})).unwrap();
        let ctx = egui::Context::default();
        assert_eq!(super::references(&app, &ctx, "Bolt"), 1);
        assert_eq!(super::references(&app, &ctx, "Bolt"), 1);
        app.run("insert", json!({"name": "Bolt", "at": [9, 5]})).unwrap();
        assert_eq!(super::references(&app, &ctx, "Bolt"), 2);
    }
}
