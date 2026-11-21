//! Editing a reference in place (REFEDIT) on the canvas: a bar over the drawing with the
//! reference's name (and, for an xref, the file it writes to), the working set's size, Add to /
//! Remove from Working Set, and Save Changes / Discard Changes (REFCLOSE). The rest of the
//! drawing is drawn faded by the renderer (XFADECTL) and locked by the engine.

use egui::{Rect, RichText, Stroke, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

fn id() -> egui::Id {
    egui::Id::unique("cadcraft-refedit-bar")
}

/// The edit shown: (block, xref file, working-set size).
fn editing(app: &CadApp) -> Option<(String, Option<String>, usize)> {
    let r = app.session.state().ok()?.doc.refedit.clone()?;
    Some((r.block, r.xref.map(|x| x.file), r.items.len()))
}

/// What a bar button asks for.
enum Act {
    Start(&'static str),
    Run(&'static str, serde_json::Value),
}

/// The bar (over the drawing, at its top).
pub(super) fn paint(app: &mut CadApp, ctx: &egui::Context, rect: Rect) {
    let Some((name, file, count)) = editing(app) else { return };
    let t = Tokens::get();
    let sel: Vec<String> = app.session.selection().iter().map(|h| h.hex()).collect();
    let mut act = None;
    egui::Area::new(id()).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(rect.center_top() + vec2(0.0, 8.0)).show(
        ctx,
        |ui| {
            egui::Frame::new().fill(t.cmd_bg).stroke(Stroke::new(1.0, t.accent)).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::tl!("Edit Reference")).color(t.accent).strong());
                    let shown = ui.label(RichText::new(&name).color(t.text));
                    if let Some(f) = &file {
                        shown.on_hover_text(crate::tf!("Saving writes {file}", file = f));
                    }
                    ui.label(RichText::new(crate::tf!("{n} in the working set", n = count)).color(t.text_dim));
                    ui.separator();
                    if ui.button(crate::tl!("Add to Working Set")).clicked() {
                        act = Some(if sel.is_empty() { Act::Start("refset") } else { Act::Run("refset", json!({ "add": sel })) });
                    }
                    if ui.button(crate::tl!("Remove from Working Set")).clicked() {
                        act = Some(if sel.is_empty() { Act::Start("refset") } else { Act::Run("refset", json!({ "remove": sel })) });
                    }
                    ui.separator();
                    if ui.button(RichText::new(crate::tl!("Save Changes")).color(t.accent)).clicked() {
                        act = Some(Act::Run("refclose", json!({ "option": "save" })));
                    }
                    if ui.button(crate::tl!("Discard Changes")).clicked() {
                        act = Some(Act::Run("refclose", json!({ "option": "discard" })));
                    }
                });
            });
        },
    );
    match act {
        Some(Act::Start(id)) => app.start(id),
        Some(Act::Run(id, p)) => {
            if let Err(e) = app.run(id, p) {
                app.set_status(e);
            }
        }
        None => {}
    }
}
