//! Named Viewports (View > Viewports > Named Viewports...): the drawing's saved model space
//! viewport configurations, with the selected one's tiles drawn as a preview. Restore runs
//! `vports.named` in the Model tab; in a layout it starts MVIEW Restore with the name, so the
//! viewports' corners are picked next. Delete runs `vports.named` with `delete`.

use cadcraft_doc::{Space, VportConfig, VportTile};
use cadcraft_engine::Input;
use egui::{RichText, Sense, Stroke, vec2};
use serde_json::{Value, json};

use super::{set_temp, temp};
use crate::CadApp;
use crate::theme::Tokens;

/// Draws the tiles of a configuration in a 180 x 120 box; tile coordinates (0, 0) are the bottom left.
fn preview(ui: &mut egui::Ui, tiles: &[VportTile], t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(180.0, 120.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.panel);
    let at = |x: f64, y: f64| egui::pos2(rect.min.x + x.clamp(0.0, 1.0) as f32 * rect.width(), rect.max.y - y.clamp(0.0, 1.0) as f32 * rect.height());
    for tile in tiles.iter().filter(|t| t.min.is_finite() && t.max.is_finite()) {
        let r = egui::Rect::from_two_pos(at(tile.min.x, tile.min.y), at(tile.max.x, tile.max.y));
        p.rect_stroke(r, 0.0, Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
    }
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

/// Show the dialog; returns the command to run.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Option<(&'static str, Value)> {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        egui::Window::new("Named Viewports").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("Open a drawing first.");
        });
        return None;
    };
    let configs: Vec<(String, Vec<VportTile>)> = d.vports.iter().filter(|c| !c.is_active()).map(|c| (c.name.clone(), c.tiles.clone())).collect();
    let model = app.session.layout_space() == Space::Model;
    let current = if model { app.session.viewport_configuration() } else { None };
    let mut sel = temp::<String>(ctx, "namedvp_sel")
        .filter(|s| configs.iter().any(|c| c.0 == *s))
        .or_else(|| current.clone())
        .or_else(|| configs.first().map(|c| c.0.clone()));
    let mut action: Option<(&'static str, Value)> = None;
    let mut restore = false;
    let mut close = false;
    egui::Window::new("Named Viewports").open(open).default_width(400.0).resizable(false).show(ctx, |ui| {
        if model {
            ui.label(format!("Current name: {}", current.as_deref().unwrap_or(VportConfig::ACTIVE)));
        } else {
            ui.label(RichText::new("Saved model space configurations to place in this layout").color(t.text_dim));
        }
        ui.separator();
        if configs.is_empty() {
            ui.label("This drawing has no named viewport configurations.");
        }
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("namedvp_list").max_height(120.0).min_scrolled_width(180.0).show(ui, |ui| {
                ui.set_min_width(180.0);
                for (name, _) in &configs {
                    let r = ui.selectable_label(sel.as_deref() == Some(name.as_str()), name);
                    if r.clicked() {
                        sel = Some(name.clone());
                    }
                    if r.double_clicked() {
                        sel = Some(name.clone());
                        restore = true;
                    }
                }
            });
            let tiles = sel.as_ref().and_then(|s| configs.iter().find(|c| c.0 == *s)).map(|c| c.1.as_slice()).unwrap_or_default();
            preview(ui, tiles, &t);
        });
        ui.separator();
        ui.horizontal(|ui| {
            let hint = if model { "Show this configuration's viewports" } else { "Place this configuration's viewports in the layout" };
            if ui.add_enabled(sel.is_some(), egui::Button::new("Restore")).on_hover_text(hint).clicked() {
                restore = true;
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new("Delete")).on_hover_text("Delete the saved configuration").clicked()
                && let Some(n) = &sel
            {
                action = Some(("vports.named", json!({ "name": n, "delete": true })));
            }
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    if let Some(n) = sel.clone() {
        set_temp(ctx, "namedvp_sel", n);
    }
    if close {
        *open = false;
    }
    let Some(name) = sel.filter(|_| restore) else { return action };
    *open = false;
    if model {
        return Some(("vports.named", json!({ "name": name })));
    }
    // The viewports' corners are picked in the layout, as for MVIEW Restore.
    app.start("mview");
    if app.session.running.as_ref().is_some_and(|r| r.id == "mview") {
        for i in [Input::Keyword("Restore".into()), Input::Text(name)] {
            if let Err(e) = app.session.input(i) {
                app.session.echo(e.to_string());
                break;
            }
        }
    }
    None
}
