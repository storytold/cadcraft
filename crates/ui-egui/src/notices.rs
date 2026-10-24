//! Warnings commands show in a window ([`cadcraft_engine::Session::notices`]): a hatch boundary
//! that isn't closed, selected objects that don't enclose an area. The first one shows until OK
//! closes it; the keys stay with the command line.

use egui::vec2;

use crate::CadApp;
use crate::theme::Tokens;

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let Some(n) = app.session.notices.first().cloned() else { return };
    let t = Tokens::get();
    let mut close = false;
    egui::Window::new(crate::i18n::t(&n.title))
        .id(egui::Id::new("cc_notice"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -60.0))
        .show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                // A warning triangle with an exclamation mark.
                let (r, _) = ui.allocate_exact_size(vec2(30.0, 28.0), egui::Sense::hover());
                let p = ui.painter();
                let pts = vec![
                    egui::pos2(r.center().x, r.top() + 1.0),
                    egui::pos2(r.right() - 1.0, r.bottom() - 1.0),
                    egui::pos2(r.left() + 1.0, r.bottom() - 1.0),
                ];
                p.add(egui::Shape::convex_polygon(pts, t.warning, egui::Stroke::NONE));
                p.text(
                    egui::pos2(r.center().x, r.center().y + 3.0),
                    egui::Align2::CENTER_CENTER,
                    "!",
                    egui::FontId::proportional(17.0),
                    egui::Color32::BLACK,
                );
                ui.add_space(6.0);
                ui.add(egui::Label::new(crate::i18n::t(&n.message)).wrap());
            });
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 0.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(crate::tl!("OK")).clicked() {
                    close = true;
                }
            });
        });
    if close {
        app.session.notices.remove(0);
    }
}
