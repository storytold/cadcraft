//! The Block Editor on the canvas (BEDIT): a tint over the drawing area so the editor can't be
//! mistaken for the drawing, the block's base point marked at the origin, and a bar with the
//! block's name and Save Block, Save Block As and Close Block Editor.

use cadcraft_engine::geom::Vec2;
use egui::{Rect, RichText, Stroke, vec2};
use serde_json::json;

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
