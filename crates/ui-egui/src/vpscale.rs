//! The status bar's viewport scale list and lock toggle (layouts only): they show the scale of
//! the active viewport (MSPACE) or of the one selected viewport, and change it through
//! `viewport.set`.

use cadcraft_doc::{EntityKind, Handle, Space, Viewport};
use egui::{Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// Standard ratio scales (paper units : model units).
pub const RATIOS: &[(&str, f64)] = &[
    ("1:1", 1.0),
    ("1:2", 0.5),
    ("1:4", 0.25),
    ("1:5", 0.2),
    ("1:8", 0.125),
    ("1:10", 0.1),
    ("1:16", 1.0 / 16.0),
    ("1:20", 0.05),
    ("1:30", 1.0 / 30.0),
    ("1:40", 0.025),
    ("1:50", 0.02),
    ("1:100", 0.01),
    ("1:200", 0.005),
    ("1:500", 0.002),
    ("1:1000", 0.001),
    ("2:1", 2.0),
    ("4:1", 4.0),
    ("8:1", 8.0),
    ("10:1", 10.0),
    ("100:1", 100.0),
];

/// Architectural scales for imperial drawings (paper inches per model inch).
pub const ARCHITECTURAL: &[(&str, f64)] = &[
    ("1/128\" = 1'-0\"", 1.0 / 1536.0),
    ("1/64\" = 1'-0\"", 1.0 / 768.0),
    ("1/32\" = 1'-0\"", 1.0 / 384.0),
    ("1/16\" = 1'-0\"", 1.0 / 192.0),
    ("3/32\" = 1'-0\"", 1.0 / 128.0),
    ("1/8\" = 1'-0\"", 1.0 / 96.0),
    ("3/16\" = 1'-0\"", 1.0 / 64.0),
    ("1/4\" = 1'-0\"", 1.0 / 48.0),
    ("3/8\" = 1'-0\"", 1.0 / 32.0),
    ("1/2\" = 1'-0\"", 1.0 / 24.0),
    ("3/4\" = 1'-0\"", 1.0 / 16.0),
    ("1\" = 1'-0\"", 1.0 / 12.0),
    ("1-1/2\" = 1'-0\"", 1.0 / 8.0),
    ("3\" = 1'-0\"", 0.25),
    ("6\" = 1'-0\"", 0.5),
    ("1'-0\" = 1'-0\"", 1.0),
];

/// Engineering (civil) scales for imperial drawings: one paper inch per tens of model feet.
pub const ENGINEERING: &[(&str, f64)] = &[
    ("1\" = 10'-0\"", 1.0 / 120.0),
    ("1\" = 20'-0\"", 1.0 / 240.0),
    ("1\" = 30'-0\"", 1.0 / 360.0),
    ("1\" = 40'-0\"", 1.0 / 480.0),
    ("1\" = 50'-0\"", 1.0 / 600.0),
    ("1\" = 60'-0\"", 1.0 / 720.0),
    ("1\" = 100'-0\"", 1.0 / 1200.0),
];

/// The scales offered for a drawing: ratios, plus architectural and engineering scales when it is imperial.
pub fn scales(metric: bool) -> Vec<(&'static str, f64)> {
    let mut v = RATIOS.to_vec();
    if !metric {
        v.extend_from_slice(ARCHITECTURAL);
        v.extend_from_slice(ENGINEERING);
    }
    v
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= b.abs() * 1e-6
}

/// How a scale reads: a standard name, else a ratio such as `1:37.5`.
pub fn label(scale: f64, metric: bool) -> String {
    if !(scale.is_finite() && scale > 0.0) {
        return "?".into();
    }
    if let Some((n, _)) = scales(metric).into_iter().find(|(_, v)| same(*v, scale)) {
        return n.to_string();
    }
    let num = |x: f64| {
        let r = (x * 1000.0).round() / 1000.0;
        if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r}") }
    };
    if scale >= 1.0 { format!("{}:1", num(scale)) } else { format!("1:{}", num(1.0 / scale)) }
}

/// The viewport the control acts on: the active one (MSPACE), else the single selected one.
pub fn target(app: &CadApp) -> Option<(Handle, Viewport)> {
    let st = app.session.state().ok()?;
    if !matches!(st.space, Space::Paper(_)) {
        return None;
    }
    if let Some(a) = st.active_viewport() {
        return Some(a);
    }
    let sel = app.session.selection();
    let [h] = sel.as_slice() else { return None };
    match &st.doc.space(&st.space)?.get(*h)?.kind {
        EntityKind::Viewport(v) if v.id != 1 => Some((*h, v.clone())),
        _ => None,
    }
}

/// Draw the scale list and lock toggle ending at `right`; returns their left edge.
pub fn status(app: &mut CadApp, ui: &mut egui::Ui, right: f32, cy: f32) -> f32 {
    let Some((h, vp)) = target(app) else { return right };
    let t = Tokens::get();
    let metric = app.session.doc().is_ok_and(|d| cadcraft_render::paper::paper_unit_mm(d) == 1.0);
    let scale = if vp.view_height > 0.0 { vp.height / vp.view_height } else { 0.0 };
    let p = ui.painter().clone();
    // Lock toggle.
    let lr = Rect::from_min_size(pos2(right - 20.0, cy - 9.0), vec2(18.0, 18.0));
    let lresp = ui.interact(lr, ui.scope_id().with("vp_lock"), Sense::click());
    if lresp.hovered() {
        p.rect_filled(lr, 3.0, t.control_hover.gamma_multiply(0.5));
    }
    icons::paint(&p, lr.shrink(2.0), if vp.locked { Icon::Lock } else { Icon::Unlock }, false);
    let tip = if vp.locked { crate::tl!("Unlock viewport") } else { crate::tl!("Lock viewport") };
    if lresp.on_hover_text(tip).clicked() {
        let r = app.run("viewport.set", json!({ "handle": h.hex(), "locked": !vp.locked }));
        app.canvas.list = None;
        if let Err(e) = r {
            app.status = Some((e, crate::now_ms()));
        }
    }
    // Scale list.
    let text = label(scale, metric);
    let g = p.layout_no_wrap(text, crate::theme::small(), if vp.locked { t.text_dim } else { t.text });
    let w = g.size().x + 22.0;
    let sr = Rect::from_min_max(pos2(lr.left() - 4.0 - w, cy - 9.0), pos2(lr.left() - 4.0, cy + 9.0));
    let sresp = ui.interact(sr, ui.scope_id().with("vp_scale"), Sense::click());
    p.rect_filled(sr, 3.0, if sresp.hovered() { t.control_hover } else { t.control.gamma_multiply(0.5) });
    p.galley(pos2(sr.left() + 6.0, sr.center().y - g.size().y / 2.0), g, t.text);
    let (ax, ay) = (sr.right() - 9.0, sr.center().y);
    p.line_segment([pos2(ax - 3.0, ay - 1.5), pos2(ax, ay + 1.5)], egui::Stroke::new(1.2, t.text_dim));
    p.line_segment([pos2(ax, ay + 1.5), pos2(ax + 3.0, ay - 1.5)], egui::Stroke::new(1.2, t.text_dim));
    let sresp = sresp.on_hover_text(if vp.locked { crate::tl!("Viewport is locked") } else { crate::tl!("Viewport scale") });
    let mut pick = None;
    if !vp.locked {
        egui::Popup::from_toggle_button_response(&sresp).width(150.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (name, v) in scales(metric) {
                    if ui.selectable_label(same(v, scale), name).clicked() {
                        pick = Some(v);
                        ui.close();
                    }
                }
            });
        });
    }
    if let Some(v) = pick {
        let r = app.run("viewport.set", json!({ "handle": h.hex(), "scale": v }));
        app.canvas.list = None;
        if let Err(e) = r {
            app.status = Some((e, crate::now_ms()));
        }
    }
    sr.left()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label(0.02, true), "1:50");
        assert_eq!(label(1.0 / 48.0, false), "1/4\" = 1'-0\"");
        assert_eq!(label(1.0 / 48.0, true), "1:48");
        assert_eq!(label(1.0 / 37.5, true), "1:37.5");
        assert_eq!(label(3.0, true), "3:1");
        assert_eq!(label(f64::NAN, true), "?");
        assert_eq!(label(0.0, true), "?");
    }

    #[test]
    fn engineering_scales_for_imperial_drawings() {
        assert_eq!(label(1.0 / 120.0, false), "1\" = 10'-0\"");
        assert_eq!(label(1.0 / 240.0, false), "1\" = 20'-0\"");
        assert_eq!(label(1.0 / 1200.0, false), "1\" = 100'-0\"");
        assert_eq!(label(1.0 / 240.0, true), "1:240");
        let names: Vec<_> = scales(false).into_iter().map(|(n, _)| n).collect();
        for n in ["1\" = 10'-0\"", "1\" = 20'-0\"", "1\" = 30'-0\"", "1\" = 40'-0\"", "1\" = 50'-0\"", "1\" = 60'-0\"", "1\" = 100'-0\""] {
            assert!(names.contains(&n), "{n} missing");
        }
    }
}
