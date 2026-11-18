//! The COMPARE overlay: what both drawings share is dimmed, the differences are drawn over it in
//! the comparison colours (only in this drawing / only in the other one), their areas are
//! outlined, and a bar shows the counts with previous / next area and close.

use std::sync::Arc;

use cadcraft_engine::cmd::file::compare::Comparison;
use cadcraft_engine::doc::{Drawing, Handle};
use cadcraft_render::DisplayList;
use egui::{Rect, RichText, Stroke, vec2};
use serde_json::json;

use super::{Xf, draw_highlight};
use crate::CadApp;
use crate::theme::Tokens;

/// Objects drawn per side; a larger difference shows its areas only.
const MAX_DRAWN: usize = 20_000;

/// The display lists of the differences, rebuilt when the drawing, the comparison or the zoom
/// band changes.
#[derive(Clone, Default)]
struct Cache {
    key: (u64, u64, usize, usize, i64),
    ours: Option<Arc<DisplayList>>,
    theirs: Option<Arc<DisplayList>>,
}

fn id() -> egui::Id {
    egui::Id::unique("cadcraft-compare-overlay")
}

/// The display lists of the differences for this frame: the stored ones when they still match
/// `key`, otherwise rebuilt at zoom `scale` and stored.
fn cache_for(ctx: &egui::Context, key: (u64, u64, usize, usize, i64), doc: &Drawing, c: &Comparison, scale: f64) -> Cache {
    let mut cache: Cache = ctx.data_mut(|d| d.get_temp(id())).unwrap_or_default();
    if cache.key != key || cache.ours.is_none() {
        let opts = cadcraft_render::Options { tolerance: 0.5 / scale.max(1e-12), ..Default::default() };
        let ours: Vec<Handle> = c.added.iter().copied().chain(c.changed.iter().map(|(a, _)| *a)).take(MAX_DRAWN).collect();
        let theirs: Vec<Handle> = c.removed.iter().copied().chain(c.changed.iter().map(|(_, b)| *b)).take(MAX_DRAWN).collect();
        let list = |d: &Drawing, hs: &[Handle]| {
            Arc::new(cadcraft_render::build_entities(d, hs.iter().filter_map(|h| d.entity(*h)).map(|e| e.as_ref()), &opts))
        };
        cache = Cache { key, ours: Some(list(doc, &ours)), theirs: Some(list(&c.other, &theirs)) };
        ctx.data_mut(|d| d.insert_temp(id(), cache.clone()));
    }
    cache
}

/// Draw the comparison over the canvas when it was made for the drawing and tab shown.
pub(super) fn paint(app: &mut CadApp, ctx: &egui::Context, p: &egui::Painter, xf: &Xf, rect: Rect) {
    let t = Tokens::get();
    let Ok(st) = app.session.state() else { return };
    let Some(c) = app.session.compare.as_ref().filter(|c| c.uid == st.uid && c.space == st.space && st.mspace.is_none()) else { return };
    // Dim everything; the differences are drawn on top.
    p.rect_filled(rect, 0.0, t.canvas.gamma_multiply(0.75));
    let band = xf.scale.max(1e-300).log2().round() as i64;
    let key = (st.uid, st.revision, Arc::as_ptr(&c.other) as usize, c.differences(), band);
    let cache = cache_for(ctx, key, &st.doc, c, xf.scale);
    for (list, color) in [(&cache.theirs, t.compare_other), (&cache.ours, t.compare_this)] {
        if let Some(l) = list {
            let all: Vec<usize> = (0..l.prims.len()).collect();
            draw_highlight(p, xf, l, &all, color, super::Look { width: 2.0, dashed: false });
        }
    }
    // The areas holding differences; the one shown last by next/previous stands out.
    for (i, b) in c.regions.iter().enumerate() {
        let r = Rect::from_two_pos(xf.to_screen(b.min), xf.to_screen(b.max)).expand(6.0);
        if r.intersects(rect) {
            let w = if c.at == Some(i) { 2.5 } else { 1.0 };
            p.rect_stroke(r, 4.0, Stroke::new(w, t.compare_area), egui::StrokeKind::Middle);
        }
    }
    // The comparison bar.
    let (added, removed, changed, name, areas, at) = (c.added.len(), c.removed.len(), c.changed.len(), c.other_name.clone(), c.regions.len(), c.at);
    let mut action = None;
    egui::Area::new(id().with("bar")).order(egui::Order::Foreground).fixed_pos(rect.center_top() + vec2(-200.0, 8.0)).show(ctx, |ui| {
        egui::Frame::new().fill(t.cmd_bg).stroke(Stroke::new(1.0, t.cmd_border)).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("■").color(t.compare_this));
                ui.label(RichText::new(crate::tf!("Only in this drawing: {n}", n = added)).color(t.text));
                ui.label(RichText::new("■").color(t.compare_other));
                ui.label(RichText::new(crate::tf!("Only in {name}: {n}", name = name, n = removed)).color(t.text));
                ui.label(RichText::new(crate::tf!("Changed: {n}", n = changed)).color(t.text));
                ui.separator();
                if areas > 0 {
                    let shown = at.map_or_else(|| "–".to_string(), |i| (i + 1).to_string());
                    ui.label(RichText::new(format!("{shown}/{areas}")).color(t.text_dim));
                }
                if ui.button("◀").on_hover_text(crate::tl!("Previous Change")).clicked() {
                    action = Some("compare.previous");
                }
                if ui.button("▶").on_hover_text(crate::tl!("Next Change")).clicked() {
                    action = Some("compare.next");
                }
                if ui.button(crate::tl!("Export")).on_hover_text(crate::tl!("Export Comparison")).clicked() {
                    action = Some("compareexport");
                }
                if ui.button("×").on_hover_text(crate::tl!("Close Comparison")).clicked() {
                    action = Some("compareclose");
                }
            });
        });
    });
    if let Some(a) = action {
        let _ = app.run(a, json!({}));
    }
}
