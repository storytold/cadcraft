//! Lineweight Settings (LWEIGHT typed or chosen from Format ▸ Lineweight..., and Options ▸ User
//! Preferences ▸ Lineweight Settings...): the current lineweight of new objects (CELWEIGHT), the
//! units lineweights are listed in (LWUNITS), whether lineweights show in the drawing area
//! (LWDISPLAY) and the width the Default lineweight stands for (LWDEFAULT, saved in the drawing).
//!
//! A JSON call never opens the window: `lweight {"lineweight": 0.5}` sets the current lineweight,
//! and LWUNITS, LWDISPLAY and LWDEFAULT are set through SETVAR or OPTIONS. The window changes them
//! the same way (`app.run`), so every change is scriptable and the drawing's are undoable.

use cadcraft_doc::Lineweight;
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The dialog name in [`crate::UiState::dialog`].
pub const DIALOG: &str = "lweight";

/// Open the window for a typed or menu-invoked LWEIGHT (no parameters). `None` if `id` isn't
/// LWEIGHT or the call has parameters (the JSON form runs in the engine).
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() || !matches!(id, "lweight" | "lw" | "lineweight") {
        return None;
    }
    app.ui.dialog = Some(DIALOG.into());
    Some(Ok(Value::Null))
}

/// A lineweight as the interface lists it: `"0.25 mm"`, or `0.010"` with LWUNITS 0 (inches);
/// ByLayer, ByBlock and Default by name (untranslated: pass it through [`crate::i18n::t`]).
pub fn name(lw: Lineweight, mm: bool) -> String {
    match lw {
        Lineweight::Mm100(v) if !mm => format!("{:.3}\"", f64::from(v.min(211)) / 2540.0),
        other => other.name(),
    }
}

/// The Lineweight Settings dialog ([`DIALOG`]).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    window(app, ctx, open);
}

/// Show the Lineweight Settings window while `open`; its Close button and title-bar cross clear
/// `open`. Changes apply at once.
pub fn window(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut action: Option<(&'static str, Value)> = None;
    let mut close = false;
    egui::Window::new(crate::tl!("Lineweight Settings"))
        .id(egui::Id::unique("cc_lineweight_settings"))
        .open(open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| lineweights(app, ui, &mut action));
                ui.add_space(12.0);
                ui.vertical(|ui| settings(app, ui, &mut action));
            });
            ui.separator();
            ui.horizontal(|ui| {
                let current =
                    app.session.doc().map_or(Lineweight::ByLayer, |d| Lineweight::from_dxf(d.header.i64("CELWEIGHT", -1).clamp(-3, 211) as i16));
                ui.label(format!("{} {}", crate::tl!("Current Lineweight:"), crate::i18n::t(&name(current, app.session.settings.lwunits_mm))));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(crate::tl!("Close")).clicked();
                });
            });
        });
    if close {
        *open = false;
    }
    if let Some((cmd, p)) = action {
        // Errors are echoed on the command line by `run`.
        let _ = app.run(cmd, p);
    }
}

/// The lineweights a new object can take; a click makes one current (CELWEIGHT).
fn lineweights(app: &CadApp, ui: &mut egui::Ui, action: &mut Option<(&'static str, Value)>) {
    let t = Tokens::get();
    ui.label(RichText::new(crate::tl!("Lineweights")).strong());
    let Ok(d) = app.session.doc() else {
        ui.label(RichText::new(crate::tl!("Open a drawing to choose its current lineweight.")).small().color(t.text_dim));
        return;
    };
    let current = Lineweight::from_dxf(d.header.i64("CELWEIGHT", -1).clamp(-3, 211) as i16);
    let mm = app.session.settings.lwunits_mm;
    egui::Frame::NONE.fill(t.field).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        ui.set_width(150.0);
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            let all = [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default]
                .into_iter()
                .chain(Lineweight::STANDARD.iter().map(|v| Lineweight::Mm100(*v)));
            for lw in all {
                if ui.selectable_label(lw == current, crate::i18n::t(&name(lw, mm))).clicked() && lw != current {
                    let v = match lw {
                        Lineweight::Mm100(v) => json!(f64::from(v) / 100.0),
                        other => json!(other.name()),
                    };
                    *action = Some(("lweight", json!({ "lineweight": v })));
                }
            }
        });
    });
}

/// Units for listing, Display Lineweight and the drawing's Default lineweight.
fn settings(app: &CadApp, ui: &mut egui::Ui, action: &mut Option<(&'static str, Value)>) {
    let st = &app.session.settings;
    ui.label(RichText::new(crate::tl!("Units for Listing")).strong());
    for (mm, label) in [(true, crate::tl!("Millimeters (mm)")), (false, crate::tl!("Inches (in)"))] {
        if ui.radio(st.lwunits_mm == mm, label).clicked() && st.lwunits_mm != mm {
            *action = Some(("options", json!({ "LWUNITS": i32::from(mm) })));
        }
    }
    ui.add_space(8.0);
    let mut display = st.lwdisplay;
    if ui.checkbox(&mut display, crate::tl!("Display Lineweight")).changed() {
        *action = Some(("options", json!({ "LWDISPLAY": i32::from(display) })));
    }
    ui.add_space(8.0);
    let caption = ui.label(crate::tl!("Default lineweight:"));
    let default = app.session.doc().ok().map(|d| d.header.i64("LWDEFAULT", 25).clamp(0, 211) as u16);
    ui.add_enabled_ui(default.is_some(), |ui| {
        let shown = default.map_or_else(String::new, |v| name(Lineweight::Mm100(v), st.lwunits_mm));
        let combo = craft_ui::ComboBox::from_id_salt("cc_lwdefault").selected_text(shown).width(120.0).show_ui(ui, |ui| {
            for v in Lineweight::STANDARD {
                if ui.selectable_label(default == Some(v), name(Lineweight::Mm100(v), st.lwunits_mm)).clicked() && default != Some(v) {
                    *action = Some(("setvar", json!({ "name": "LWDEFAULT", "value": v })));
                }
            }
        });
        combo.response.labelled_by(caption.id);
    });
    ui.add_space(4.0);
    ui.label(RichText::new(crate::tl!("Objects whose lineweight is Default are drawn and plotted this wide.")).small().color(Tokens::get().text_dim));
    ui.allocate_space(vec2(200.0, 0.0));
}
