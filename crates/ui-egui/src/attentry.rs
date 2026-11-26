//! Edit Attributes: INSERT asks for its attribute values in one dialog when ATTDIA is 1
//! ([`cadcraft_engine::Session::attribute_entry`]). OK answers INSERT's prompts in turn, Cancel
//! cancels the command; values typed at the command line still work, and the dialog goes away
//! after the first one. The draft, the window and its fields belong to the drawing and command
//! that asked for them, so an INSERT started after a cancel starts again from its own defaults
//! and takes no confirmation meant for the dialog it replaced.

use egui::{RichText, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// The values being edited, for the drawing and command that asked for them.
#[derive(Clone)]
struct State {
    /// The drawing and the command instance ([`cadcraft_engine::AttribEntry`]): a draft made for
    /// one command is never used by another, even for the same block.
    drawing: u64,
    command: u64,
    block: String,
    values: Vec<String>,
    /// The first field has had the keyboard focus once.
    focused: bool,
    /// When this owner's dialog appeared ([`egui::InputState::time`]): it takes no answer before
    /// it has been drawn once, so a click or key meant for the dialog it replaced cannot confirm
    /// values nobody has seen.
    shown_at: f64,
}

impl State {
    /// Whether this draft is the one `entry` asks for.
    fn owns(&self, entry: &cadcraft_engine::AttribEntry) -> bool {
        self.drawing == entry.drawing && self.command == entry.command && self.block == entry.block && self.values.len() == entry.rows.len()
    }
}

fn state_id() -> egui::Id {
    egui::Id::unique("attentry_state")
}

/// The dialog's window id, with the drawing and command it belongs to: the window and every
/// widget in it are new for a replacement command, so a press on the old dialog's OK never ends
/// on the new one, and no focus, keyboard or scroll state carries over.
fn window_id(entry: &cadcraft_engine::AttribEntry) -> egui::Id {
    egui::Id::unique(("Edit Attributes", entry.drawing, entry.command))
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
    let now = ctx.input(|i| i.time);
    let mut st = match ctx.data_mut(|d| d.get_temp::<State>(state_id())).filter(|s| s.owns(&entry)) {
        Some(st) => st,
        None => {
            // A command (or drawing) replaced the dialog between frames. Nothing queued for the old
            // one — a click or key waiting in the app's synthetic input — is the replacement's: its
            // dialog has not been drawn yet, and its own defaults are what OK submits.
            app.synthetic.clear();
            State {
                drawing: entry.drawing,
                command: entry.command,
                block: entry.block.clone(),
                values: entry.rows.iter().map(|r| r.default.clone()).collect(),
                focused: false,
                shown_at: now,
            }
        }
    };
    // No answer before the dialog has been drawn for this owner: whatever was confirmed was
    // confirmed at the dialog this one replaced.
    let fresh = st.shown_at == now;
    let mut done = Done::No;
    let st_focused = st.focused;
    st.focused = true;
    egui::Window::new(crate::tl!("Edit Attributes"))
        .id(window_id(&entry))
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
                        let field = egui::TextEdit::singleline(value)
                            .id(egui::Id::unique(("attentry_value", entry.drawing, entry.command, i)))
                            .desired_width(220.0);
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
    if crate::framework::escape_pressed(ctx) {
        done = Done::Cancel;
    }
    if fresh {
        done = Done::No;
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
