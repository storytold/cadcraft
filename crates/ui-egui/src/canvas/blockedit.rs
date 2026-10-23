//! The Block Editor on the canvas (BEDIT): a tint over the drawing area so the editor can't be
//! mistaken for the drawing, the block's base point marked at the origin, the block's
//! parameters and actions (BPARAMETER, BACTION) drawn as the editor shows them, and a bar with
//! the block's name and Save Block, Save Block As and Close Block Editor.

use std::sync::Arc;

use cadcraft_engine::doc::dynblock::Mark;
use cadcraft_engine::geom::Vec2;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Shape, Stroke, vec2};
use serde_json::json;

use super::Xf;
use crate::CadApp;
use crate::theme::Tokens;

fn id() -> egui::Id {
    egui::Id::new("cadcraft-block-editor-bar")
}

/// The block the Block Editor is editing in the drawing shown, and whether it has unsaved changes.
fn editing(app: &CadApp) -> Option<(String, bool)> {
    let st = app.session.state().ok()?;
    st.block_editor().map(|n| (n.to_string(), st.block_unsaved()))
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
    let mut action: Option<(&str, bool)> = None;
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
                });
            });
        },
    );
    match action {
        // Prompting commands start like a menu item; BSAVE needs no answer.
        Some((id, true)) => app.start(id),
        Some((id, false)) => {
            let _ = app.run(id, json!({}));
        }
        None => {}
    }
}

/// The edited block's parameters and actions, worked out once per drawing revision.
fn marks(app: &CadApp, ctx: &egui::Context) -> Arc<Vec<Mark>> {
    type Cache = Option<((u64, u64), Arc<Vec<Mark>>)>;
    let Ok(st) = app.session.state() else { return Arc::default() };
    let Some(be) = &st.block_edit else { return Arc::default() };
    let key = (st.uid, st.revision);
    let cid = egui::Id::new("cadcraft-block-editor-marks");
    if let Some((k, m)) = ctx.data(|d| d.get_temp::<Cache>(cid)).flatten()
        && k == key
    {
        return m;
    }
    let m = Arc::new(be.marks(&st.doc));
    ctx.data_mut(|d| d.insert_temp::<Cache>(cid, Some((key, m.clone()))));
    m
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
    let line = Stroke::new(1.0, t.accent);
    let font = FontId::proportional(12.0);
    let to = |v: cadcraft_engine::geom::Vec3| xf.to_screen(v.xy());
    for m in marks(app, ctx).iter() {
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
                    arrow(p, sa, -u, t.grip_dyn);
                }
                if *end_grip {
                    arrow(p, sb, u, t.grip_dyn);
                }
                p.text(da + (db - da) * 0.5, Align2::CENTER_BOTTOM, label, font.clone(), t.accent);
            }
            Mark::Point { at, label_at, label, grip, .. } => {
                let (s, l) = (to(*at), to(*label_at));
                p.line_segment([s, l], line);
                if *grip {
                    p.rect_filled(Rect::from_center_size(s, vec2(8.0, 8.0)), 0.0, t.grip_dyn);
                } else {
                    p.rect_stroke(Rect::from_center_size(s, vec2(8.0, 8.0)), 0.0, line, egui::StrokeKind::Middle);
                }
                p.text(l, Align2::LEFT_BOTTOM, label, font.clone(), t.accent);
            }
            Mark::Parameter { at, label, .. } => {
                p.text(to(*at), Align2::LEFT_BOTTOM, label, font.clone(), t.accent);
            }
            Mark::Action { at, name, .. } => {
                let c = to(*at);
                let g = p.layout_no_wrap(name.clone(), font.clone(), t.text);
                let r = Rect::from_center_size(c, g.size() + vec2(8.0, 4.0));
                p.rect_filled(r, 3.0, t.cmd_bg);
                p.rect_stroke(r, 3.0, Stroke::new(1.0, t.warn), egui::StrokeKind::Middle);
                p.galley(r.min + vec2(4.0, 2.0), g, t.text);
            }
        }
    }
}
