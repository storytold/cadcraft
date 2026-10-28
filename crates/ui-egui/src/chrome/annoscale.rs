//! The status bar's annotation scale list (model space, and inside a layout viewport): it shows
//! the current annotation scale (CANNOSCALE, or the active viewport's) and sets it from the
//! drawing's scale list through `setvar`; its last entry starts SCALELISTEDIT. Inside a viewport
//! whose annotation scale differs from its viewport scale, a button beside it synchronizes them
//! (`viewport.syncscale`).

use egui::{Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// Draw the annotation scale list ending at `right` (model space, or a layout with an active
/// viewport); returns its left edge, or `right` when nothing is drawn.
pub fn status(app: &mut CadApp, ui: &mut egui::Ui, right: f32, cy: f32) -> f32 {
    let viewport = app.session.state().ok().and_then(|st| st.active_viewport());
    if !matches!(app.session.layout_space(), cadcraft_doc::Space::Model) && viewport.is_none() {
        return right;
    }
    let Ok(d) = app.session.doc() else { return right };
    let Ok(current) = app.session.anno_scale().map(|s| s.name) else { return right };
    let differ = viewport.as_ref().is_some_and(|(_, vp)| d.viewport_scales_differ(vp));
    let names: Vec<String> = d.scales.iter().map(|s| s.name.clone()).collect();
    let t = Tokens::get();
    let p = ui.painter().clone();
    // The synchronize button, when the active viewport's two scales differ.
    let right = if differ {
        let br = Rect::from_min_size(pos2(right - 20.0, cy - 9.0), vec2(18.0, 18.0));
        let bresp = ui.interact(br, ui.scope_id().with("anno_sync"), Sense::click());
        if bresp.hovered() {
            p.rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.5));
        }
        icons::paint(&p, br.shrink(2.0), Icon::SyncScale, false);
        icons::describe_control(ui, &bresp, "Synchronize Viewport Scale", None, false);
        if bresp.on_hover_text(crate::tl!("Annotation scale is not equal to viewport scale: click to synchronize")).clicked() {
            let res = app.run("viewport.syncscale", json!({}));
            app.canvas.list = None;
            if let Err(e) = res {
                app.status = Some((e, crate::now_ms()));
            }
        }
        br.left() - 4.0
    } else {
        right
    };
    let g = p.layout_no_wrap(current.clone(), crate::theme::small(), t.text);
    let w = g.size().x.min(160.0) + 22.0;
    let r = Rect::from_min_size(pos2(right - w, cy - 9.0), vec2(w, 18.0));
    let resp = ui.interact(r, ui.scope_id().with("anno_scale"), Sense::click());
    p.rect_filled(r, 3.0, if resp.hovered() { t.control_hover } else { t.control.gamma_multiply(0.5) });
    p.with_clip_rect(r.shrink2(vec2(4.0, 0.0))).galley(pos2(r.left() + 6.0, r.center().y - g.size().y / 2.0), g, t.text);
    let (ax, ay) = (r.right() - 9.0, r.center().y);
    p.line_segment([pos2(ax - 3.0, ay - 1.5), pos2(ax, ay + 1.5)], egui::Stroke::new(1.2, t.text_dim));
    p.line_segment([pos2(ax, ay + 1.5), pos2(ax + 3.0, ay - 1.5)], egui::Stroke::new(1.2, t.text_dim));
    crate::icons::describe_control(ui, &resp, "Annotation scale", None, false);
    let resp = resp.on_hover_text(crate::tl!("Annotation scale"));
    let mut pick: Option<String> = None;
    let mut edit = false;
    egui::Popup::from_toggle_button_response(&resp).width(170.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for n in &names {
                if ui.selectable_label(n.eq_ignore_ascii_case(&current), n).clicked() {
                    pick = Some(n.clone());
                    ui.close();
                }
            }
            ui.separator();
            if ui.selectable_label(false, crate::i18n::t("Scale List...")).clicked() {
                edit = true;
                ui.close();
            }
        });
    });
    if let Some(n) = pick {
        let res = app.run("setvar", json!({ "name": "CANNOSCALE", "value": n }));
        app.canvas.list = None;
        if let Err(e) = res {
            app.status = Some((e, crate::now_ms()));
        }
    }
    if edit {
        app.start("scalelistedit");
    }
    r.left() - 6.0
}
