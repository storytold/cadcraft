//! The Block Editor on the canvas (BEDIT): a tint over the drawing area so the editor can't be
//! mistaken for the drawing, the block's base point marked at the origin, the block's
//! parameters and actions (BPARAMETER, BACTION) drawn as the editor shows them, and a bar with
//! the block's name and Save Block, Save Block As and Close Block Editor. A block with visibility
//! states gets Visibility Mode (BVMODE), the state shown (picked from a list, as AutoCAD's
//! Visibility panel offers it), Visibility States, Make Visible and Make Invisible; with BVMODE 1
//! the objects the state hides are drawn dimmed.
//!
//! Parameters and actions are picked with a click ([`target`], [`click`]; Shift+click unpicks),
//! as in AutoCAD's editor: picked ones are drawn heavier with grips at their points, and clicking
//! a grip moves that point. MOVE, ERASE and Delete act on them (the engine's
//! `blocks.element.*`).

use std::sync::Arc;

use cadcraft_engine::doc::Handle;
use cadcraft_engine::doc::dynblock::{ElementPoint, Mark, ParameterKind, dynamic_block};
use cadcraft_engine::geom::Vec2;
use cadcraft_render::DisplayList;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Shape, Stroke, vec2};
use serde_json::{Value, json};

use super::Xf;
use crate::CadApp;
use crate::theme::Tokens;

fn id() -> egui::Id {
    egui::Id::unique("cadcraft-block-editor-bar")
}

/// The block the Block Editor is editing in the drawing shown, and whether it has unsaved changes.
fn editing(app: &CadApp) -> Option<(String, bool)> {
    let st = app.session.state().ok()?;
    st.block_editor().map(|n| (n.to_string(), st.block_unsaved()))
}

/// The edited block's visibility states: their names, the one shown, and what it hides.
#[derive(Clone, Default)]
struct States {
    names: Vec<String>,
    current: String,
    hidden: Vec<Handle>,
}

/// [`States`] when the block has a visibility parameter, worked out once per drawing revision
/// and state shown.
fn states(app: &CadApp, ctx: &egui::Context) -> Option<Arc<States>> {
    type Cache = Option<((u64, u64, Option<String>), Option<Arc<States>>)>;
    let st = app.session.state().ok()?;
    let be = st.block_edit.as_ref()?;
    let key = (st.uid, st.revision, be.visibility.clone());
    let cid = egui::Id::unique("cadcraft-block-editor-states");
    if let Some((k, v)) = ctx.data(|d| d.get_temp::<Cache>(cid)).flatten()
        && k == key
    {
        return v;
    }
    let names: Option<Vec<String>> = dynamic_block(&st.doc, &be.name).and_then(|m| {
        m.parameters.into_iter().find_map(|p| match p.kind {
            ParameterKind::Visibility { states, .. } => Some(states.into_iter().map(|s| s.name).collect()),
            _ => None,
        })
    });
    let v = names.zip(be.visibility_state(&st.doc)).map(|(names, (current, hidden))| Arc::new(States { names, current, hidden }));
    ctx.data_mut(|d| d.insert_temp::<Cache>(cid, Some((key, v.clone()))));
    v
}

/// The objects the visibility state hides, dimmed (BVMODE 1 leaves them on the canvas to pick).
fn paint_dimmed(app: &CadApp, ctx: &egui::Context, p: &Painter, xf: &Xf, hidden: &[Handle]) {
    type Cache = Option<((u64, u64, usize, i64), Arc<DisplayList>)>;
    let Ok(st) = app.session.state() else { return };
    let band = xf.scale.max(1e-300).log2().round() as i64;
    let key = (st.uid, st.revision, hidden.len(), band);
    let cid = egui::Id::unique("cadcraft-block-editor-dimmed");
    let list = match ctx.data(|d| d.get_temp::<Cache>(cid)).flatten() {
        Some((k, l)) if k == key => l,
        _ => {
            let opts = cadcraft_render::Options { tolerance: 0.5 / xf.scale.max(1e-12), ..Default::default() };
            let l = Arc::new(cadcraft_render::build_entities(&st.doc, hidden.iter().filter_map(|h| st.doc.entity(*h)).map(|e| e.as_ref()), &opts));
            ctx.data_mut(|d| d.insert_temp::<Cache>(cid, Some((key, l.clone()))));
            l
        }
    };
    // The canvas colour drawn over them, a little wider, fades them toward the background.
    let all: Vec<usize> = (0..list.prims.len()).collect();
    super::draw_highlight(p, xf, &list, &all, Tokens::get().canvas.gamma_multiply(0.7), super::Look { width: 2.0, dashed: false });
}

/// Tint the drawing area while the Block Editor is open (under the drawing).
pub(super) fn wash(app: &CadApp, p: &egui::Painter, rect: Rect) {
    if editing(app).is_some() {
        p.rect_filled(rect, 0.0, Tokens::get().accent.gamma_multiply(0.12));
    }
}

/// The base point marker and the Block Editor bar (over the drawing).
pub(super) fn paint(app: &mut CadApp, ctx: &egui::Context, p: &egui::Painter, xf: &Xf, rect: Rect) {
    let Some((name, unsaved)) = editing(app) else { return };
    let t = Tokens::get();
    // The base point: the origin of the editor's coordinates.
    let o = xf.to_screen(Vec2::ZERO);
    if rect.expand(16.0).contains(o) {
        let st = Stroke::new(1.5, t.accent);
        p.circle_stroke(o, 6.0, st);
        p.line_segment([o - vec2(12.0, 0.0), o + vec2(12.0, 0.0)], st);
        p.line_segment([o - vec2(0.0, 12.0), o + vec2(0.0, 12.0)], st);
    }
    paint_marks(app, ctx, p, xf);
    paint_grips(app, p, xf);
    let vis = states(app, ctx);
    if app.session.settings.bvmode
        && let Some(v) = &vis
        && !v.hidden.is_empty()
    {
        paint_dimmed(app, ctx, p, xf, &v.hidden);
    }
    let mut action: Option<(&str, bool)> = None;
    let mut pick: Option<String> = None;
    let bvmode = app.session.settings.bvmode;
    let mut toggle_mode = false;
    egui::Area::new(id()).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(rect.center_top() + vec2(0.0, 8.0)).show(
        ctx,
        |ui| {
            egui::Frame::new().fill(t.cmd_bg).stroke(Stroke::new(1.0, t.accent)).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::tl!("Block Editor")).color(t.accent).strong());
                    ui.label(RichText::new(&name).color(t.text));
                    if unsaved {
                        ui.label(RichText::new("●").color(t.warn)).on_hover_text(crate::tl!("Unsaved changes"));
                    }
                    ui.separator();
                    if ui.add_enabled(unsaved, egui::Button::new(crate::tl!("Save Block"))).clicked() {
                        action = Some(("bsave", false));
                    }
                    // The drawing itself has no block to save under another name.
                    let current = name == cadcraft_engine::blockedit::CURRENT_DRAWING;
                    if ui.add_enabled(!current, egui::Button::new(crate::tl!("Save Block As"))).clicked() {
                        action = Some(("bsaveas", true));
                    }
                    if ui.button(crate::tl!("Close Block Editor")).clicked() {
                        action = Some(("bclose", true));
                    }
                    if let Some(v) = &vis {
                        ui.separator();
                        // BVMODE: whether objects the state hides show dimmed (on) or not at all.
                        if ui
                            .selectable_label(bvmode, crate::tl!("Visibility Mode"))
                            .on_hover_text(crate::tl!("Show the objects this visibility state hides, dimmed (BVMODE)"))
                            .clicked()
                        {
                            toggle_mode = true;
                        }
                        craft_ui::ComboBox::from_id_salt("cadcraft-block-editor-state").selected_text(v.current.as_str()).show_ui(ui, |ui| {
                            for n in &v.names {
                                if ui.selectable_label(*n == v.current, n.as_str()).clicked() && *n != v.current {
                                    pick = Some(n.clone());
                                }
                            }
                        });
                        if ui.button(crate::tl!("Visibility States")).clicked() {
                            action = Some(("bvstate", true));
                        }
                        if ui.button(crate::tl!("Make Visible")).clicked() {
                            action = Some(("bvshow", true));
                        }
                        if ui.button(crate::tl!("Make Invisible")).clicked() {
                            action = Some(("bvhide", true));
                        }
                    }
                });
            });
        },
    );
    if toggle_mode {
        let _ = app.run("setvar", json!({ "name": "BVMODE", "value": i32::from(!bvmode) }));
    }
    if let Some(n) = pick {
        let _ = app.run("bvstate", json!({ "set": n }));
    }
    match action {
        // Prompting commands start like a menu item; BSAVE needs no answer.
        Some((id, true)) => app.start(id),
        Some((id, false)) => {
            let _ = app.run(id, json!({}));
        }
        None => {}
    }
}

/// The edited block's parameters and actions and, with BVMODE 1, the graph ids of the parameters
/// the visibility state shown leaves out (drawn dimmed; without BVMODE they aren't drawn), worked
/// out once per drawing revision, visibility state shown and BVMODE.
fn marks(app: &CadApp, ctx: &egui::Context) -> Arc<(Vec<Mark>, Vec<i32>)> {
    type Cache = Option<((u64, u64, Option<String>, bool), Arc<(Vec<Mark>, Vec<i32>)>)>;
    let Ok(st) = app.session.state() else { return Arc::default() };
    let Some(be) = &st.block_edit else { return Arc::default() };
    let bvmode = app.session.settings.bvmode;
    let key = (st.uid, st.revision, be.visibility.clone(), bvmode);
    let cid = egui::Id::unique("cadcraft-block-editor-marks");
    if let Some((k, m)) = ctx.data(|d| d.get_temp::<Cache>(cid)).flatten()
        && k == key
    {
        return m;
    }
    let m = Arc::new(if bvmode { (be.all_marks(&st.doc), be.inactive_parameters(&st.doc)) } else { (be.marks(&st.doc), Vec::new()) });
    ctx.data_mut(|d| d.insert_temp::<Cache>(cid, Some((key, m.clone()))));
    m
}

/// What a click in the Block Editor hits first: a picked parameter's or action's grip, or a
/// parameter or action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Target {
    Grip(i32, ElementPoint),
    Mark(i32),
}

/// How far from the cursor (screen points) a mark or grip still counts as hit.
const REACH: f32 = 6.0;

fn segment_distance(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    let d = b - a;
    let t = if d.length_sq() <= 1e-6 { 0.0 } else { ((p - a).dot(d) / d.length_sq()).clamp(0.0, 1.0) };
    (a + d * t).distance(p)
}

/// About where a label of `text` drawn at `at` with `align` lies (12-point text).
fn label_rect(at: Pos2, align: Align2, text: &str) -> Rect {
    let size = vec2(7.0 * text.chars().count() as f32, 14.0);
    align.anchor_size(at, size)
}

/// The graph id a mark stands for (a parameter's or an action's).
fn mark_node(m: &Mark) -> i32 {
    match m {
        Mark::Action { node, .. } => *node,
        other => other.parameter_node().unwrap_or(0),
    }
}

/// The grip or mark under `pos`, when the Block Editor is open with no command running.
pub(super) fn target(app: &CadApp, ctx: &egui::Context, xf: &Xf, pos: Option<Pos2>) -> Option<Target> {
    let pos = pos?;
    if app.session.prompting() || editing(app).is_none() {
        return None;
    }
    if let Some((n, w, _)) = app.session.element_grips().into_iter().find(|(_, _, at)| xf.to_screen(*at).distance(pos) <= REACH) {
        return Some(Target::Grip(n, w));
    }
    let to = |v: cadcraft_engine::geom::Vec3| xf.to_screen(v.xy());
    let marks = marks(app, ctx);
    marks
        .0
        .iter()
        .find(|m| match m {
            Mark::Linear { base, end, offset, label, .. } => {
                let (a, b) = (base.xy(), end.xy());
                let dir = (b - a).normalized();
                let shift = Vec2::new(-dir.y, dir.x) * *offset;
                let (da, db) = (xf.to_screen(a + shift), xf.to_screen(b + shift));
                let (sa, sb) = (to(*base), to(*end));
                [(sa, da), (sb, db), (da, db)].iter().any(|(p, q)| segment_distance(*p, *q, pos) <= REACH)
                    || label_rect(da + (db - da) * 0.5, Align2::CENTER_BOTTOM, label).contains(pos)
            }
            Mark::Point { at, label_at, label, .. } => {
                let (s, l) = (to(*at), to(*label_at));
                s.distance(pos) <= REACH || segment_distance(s, l, pos) <= REACH || label_rect(l, Align2::LEFT_BOTTOM, label).contains(pos)
            }
            Mark::List { at, label, .. } => {
                let s = to(*at);
                s.distance(pos) <= REACH + 2.0 || label_rect(s + vec2(8.0, 0.0), Align2::LEFT_CENTER, label).contains(pos)
            }
            Mark::Parameter { at, label, .. } => label_rect(to(*at), Align2::LEFT_BOTTOM, label).contains(pos),
            Mark::Action { at, name, .. } => label_rect(to(*at), Align2::CENTER_CENTER, name).expand2(vec2(4.0, 2.0)).contains(pos),
        })
        .map(|m| Target::Mark(mark_node(m)))
}

/// A click on `t`: a grip starts moving its point; a parameter or action is picked (Shift+click
/// unpicks it).
pub(super) fn click(app: &mut CadApp, t: Target, shift: bool) {
    let r = match t {
        Target::Grip(n, w) => app.session.start_element_grip(n, w).map(|_| Value::Null).map_err(|e| e.to_string()),
        Target::Mark(n) => {
            let picked = app.session.picked_elements();
            if shift && picked.contains(&n) {
                let rest: Vec<i32> = picked.into_iter().filter(|p| *p != n).collect();
                app.run("blocks.element.select", json!({ "nodes": rest }))
            } else {
                app.run("blocks.element.select", json!({ "nodes": [n], "add": true }))
            }
        }
    };
    if let Err(e) = r {
        app.session.echo(e);
    }
}

/// The picked parameters' and actions' grips: squares at their points, as entity grips.
fn paint_grips(app: &CadApp, p: &Painter, xf: &Xf) {
    let t = Tokens::get();
    for (_, _, at) in app.session.element_grips() {
        let r = Rect::from_center_size(xf.to_screen(at), vec2(9.0, 9.0));
        p.rect_filled(r, 0.0, t.grip);
        p.rect_stroke(r, 0.0, Stroke::new(1.0, t.canvas), egui::StrokeKind::Middle);
    }
}

/// A filled arrow at `at` pointing along screen direction `u`, as a linear parameter's grip.
fn arrow(p: &Painter, at: Pos2, u: egui::Vec2, color: Color32) {
    let n = vec2(-u.y, u.x);
    let (len, half) = (9.0, 5.0);
    p.add(Shape::convex_polygon(vec![at - u * (len * 0.4) + n * half, at + u * (len * 0.6), at - u * (len * 0.4) - n * half], color, Stroke::NONE));
}

/// Draw each parameter and action of the edited block: a linear parameter as a dimension with
/// its label and arrow grips, a point parameter as a marker with its label, other parameters by
/// name, and actions as badges.
fn paint_marks(app: &CadApp, ctx: &egui::Context, p: &Painter, xf: &Xf) {
    let t = Tokens::get();
    let font = FontId::proportional(12.0);
    let to = |v: cadcraft_engine::geom::Vec3| xf.to_screen(v.xy());
    let marks = marks(app, ctx);
    let (all, inactive) = &*marks;
    let picked = app.session.picked_elements();
    for m in all {
        // A parameter the visibility state leaves out shows dimmed (BVMODE 1).
        let dim = m.parameter_node().is_some_and(|n| inactive.contains(&n));
        let fade = |c: Color32| if dim { c.gamma_multiply(0.35) } else { c };
        let (accent, grip_dyn) = (fade(t.accent), fade(t.grip_dyn));
        // Picked ones are drawn heavier.
        let width = if picked.contains(&mark_node(m)) { 2.5 } else { 1.0 };
        let line = Stroke::new(width, accent);
        match m {
            Mark::Linear { base, end, offset, label, base_grip, end_grip, .. } => {
                let (a, b) = (base.xy(), end.xy());
                let dir = (b - a).normalized();
                let shift = Vec2::new(-dir.y, dir.x) * *offset;
                let (da, db) = (xf.to_screen(a + shift), xf.to_screen(b + shift));
                let (sa, sb) = (to(*base), to(*end));
                p.line_segment([sa, da], line);
                p.line_segment([sb, db], line);
                p.line_segment([da, db], line);
                let along = sb - sa;
                let u = if along.length() > 1e-6 { along / along.length() } else { vec2(1.0, 0.0) };
                if *base_grip {
                    arrow(p, sa, -u, grip_dyn);
                }
                if *end_grip {
                    arrow(p, sb, u, grip_dyn);
                }
                p.text(da + (db - da) * 0.5, Align2::CENTER_BOTTOM, label, font.clone(), accent);
            }
            Mark::Point { at, label_at, label, grip, .. } => {
                let (s, l) = (to(*at), to(*label_at));
                p.line_segment([s, l], line);
                if *grip {
                    p.rect_filled(Rect::from_center_size(s, vec2(8.0, 8.0)), 0.0, grip_dyn);
                } else {
                    p.rect_stroke(Rect::from_center_size(s, vec2(8.0, 8.0)), 0.0, line, egui::StrokeKind::Middle);
                }
                p.text(l, Align2::LEFT_BOTTOM, label, font.clone(), accent);
            }
            Mark::List { at, label, grip, .. } => {
                // A list grip: a triangle pointing down, the label to its right.
                let s = to(*at);
                if *grip {
                    let pts = vec![s + vec2(-6.0, -5.0), s + vec2(6.0, -5.0), s + vec2(0.0, 6.0)];
                    p.add(Shape::convex_polygon(pts, grip_dyn, Stroke::new(1.0, fade(t.canvas))));
                }
                p.text(s + vec2(8.0, 0.0), Align2::LEFT_CENTER, label, font.clone(), accent);
            }
            Mark::Parameter { at, label, .. } => {
                p.text(to(*at), Align2::LEFT_BOTTOM, label, font.clone(), accent);
            }
            Mark::Action { at, name, .. } => {
                let c = to(*at);
                let g = p.layout_no_wrap(name.clone(), font.clone(), t.text);
                let r = Rect::from_center_size(c, g.size() + vec2(8.0, 4.0));
                p.rect_filled(r, 3.0, t.cmd_bg);
                p.rect_stroke(r, 3.0, Stroke::new(width, t.warn), egui::StrokeKind::Middle);
                p.galley(r.min + vec2(4.0, 2.0), g, t.text);
            }
        }
    }
}
