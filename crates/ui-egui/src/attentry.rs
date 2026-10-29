//! Edit Attributes: INSERT asks for its attribute values in one dialog when ATTDIA is 1
//! ([`cadcraft_engine::Session::attribute_entry`]). OK answers INSERT's prompts in turn, Cancel
//! cancels the command; values typed at the command line still work, and the dialog goes away
//! after the first one.

use egui::{RichText, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// The values being edited, for the block they were made for.
#[derive(Clone, Default)]
struct State {
    block: String,
    values: Vec<String>,
    /// The first field has had the keyboard focus once.
    focused: bool,
}

fn state_id() -> egui::Id {
    egui::Id::unique("attentry_state")
}

enum Done {
    No,
    Ok,
    Cancel,
}

/// Show the dialog while INSERT waits for attribute values with ATTDIA 1.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let Some(entry) = app.session.attribute_entry() else {
        ctx.data_mut(|d| d.remove::<State>(state_id()));
        return;
    };
    let t = Tokens::get();
    let mut st = ctx
        .data_mut(|d| d.get_temp::<State>(state_id()))
        .filter(|s| s.block == entry.block && s.values.len() == entry.rows.len())
        .unwrap_or_else(|| State { block: entry.block.clone(), values: entry.rows.iter().map(|r| r.default.clone()).collect(), focused: false });
    let mut done = Done::No;
    let st_focused = st.focused;
    st.focused = true;
    egui::Window::new(crate::tl!("Edit Attributes"))
        .id(egui::Id::unique("Edit Attributes"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Block name:"));
                ui.label(RichText::new(&entry.block).strong());
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("attentry_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    for (i, (row, value)) in entry.rows.iter().zip(st.values.iter_mut()).enumerate() {
                        let shown = if row.prompt.is_empty() { row.tag.as_str() } else { row.prompt.as_str() };
                        let label = ui.label(shown).on_hover_text(crate::tf!("Tag: {tag}", tag = row.tag));
                        let field = egui::TextEdit::singleline(value).id(egui::Id::unique(("attentry_value", i))).desired_width(220.0);
                        let resp = ui.add(field).labelled_by(label.id);
                        if i == 0 && !st_focused {
                            resp.request_focus();
                        }
                        // Enter in a field is OK, the dialog's default button.
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            done = Done::Ok;
                        }
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("OK")).clicked() {
                    done = Done::Ok;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    done = Done::Cancel;
                }
                ui.label(RichText::new(crate::tl!("Values typed at the command line work too")).small().color(t.text_dim));
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        done = Done::Cancel;
    }
    match done {
        Done::No => {
            ctx.data_mut(|d| d.insert_temp(state_id(), st));
        }
        Done::Ok => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            for a in entry.answers(&st.values) {
                if app.session.input(cadcraft_engine::Input::Text(a)).is_err() {
                    break;
                }
            }
        }
        Done::Cancel => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            app.session.cancel();
        }
    }
}
