//! The one-line in-place text editor (#842): double-clicking TEXT, an attribute definition or a
//! dimension (TEXTEDIT with the object picked) opens a text box over the text, at its position
//! and height. Enter or a click outside keeps what was typed, Esc cancels; an emptied box keeps a
//! text's old value and gives a dimension back its measured value (`<>`). What the box edits comes
//! from the engine ([`cadcraft_engine::Session::text_editor`]); the new text goes back through
//! the running command, so undo, scripts and the command line work as before.

use cadcraft_engine::{EditorKind, Input, TextEditor};
use egui::text::{CCursor, CCursorRange};
use egui::{Color32, FontId, Key, LayerId, RichText, Stroke, pos2};

use crate::CadApp;
use crate::theme::Tokens;

/// Smallest and largest size of the text in the box, in points: readable at any zoom.
const MIN_PX: f32 = 13.0;
const MAX_PX: f32 = 36.0;

#[derive(Clone)]
struct State {
    /// The object being edited ([`TextEditor::key`]): another one starts afresh.
    key: String,
    text: String,
    /// The box has taken the keyboard once (typing goes into it, not the command line).
    focused: bool,
}

fn state_id() -> egui::Id {
    egui::Id::unique("line_editor_state")
}

fn edit_id() -> egui::Id {
    egui::Id::unique("line_editor_text")
}

/// The one-line text the running command asks for, when it is asking for it now.
fn current(app: &CadApp) -> Option<TextEditor> {
    let ed = app.session.text_editor()?;
    let asking = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point);
    (asking && matches!(ed.kind, EditorKind::Line | EditorKind::Dimension { .. })).then_some(ed)
}

enum Done {
    No,
    Ok,
    Cancel,
}

/// Show the editor while TEXTEDIT asks for one line of text; clears its state otherwise.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let Some(ed) = current(app) else {
        ctx.data_mut(|d| d.remove::<State>(state_id()));
        return;
    };
    let t = Tokens::get();
    let mt = &ed.mtext;
    let kept = ctx.data_mut(|d| d.get_temp::<State>(state_id())).filter(|st| st.key == ed.key);
    // A click outside counts from the frame after the box opened (not the double-click itself).
    let armed = kept.is_some();
    let mut st = kept.unwrap_or_else(|| State { key: ed.key.clone(), text: mt.contents.clone(), focused: false });

    let (rect, scale) = app.canvas.xf.map_or((ctx.content_rect(), 20.0), |x| (x.rect, x.scale.max(1e-6)));
    let height = if mt.height > 0.0 && mt.height.is_finite() { mt.height } else { 0.2 };
    let text_px = (height * scale) as f32;
    let text_px = if text_px.is_finite() { text_px.clamp(0.0, 10_000.0) } else { 0.0 };
    let px = text_px.clamp(MIN_PX, MAX_PX);
    let font = FontId::proportional(px);
    let typed_w = ctx.layer_painter(LayerId::background()).layout_no_wrap(st.text.clone(), font.clone(), Color32::WHITE).size().x;
    let width = (typed_w + 2.0 * px).clamp(120.0, (rect.width() * 0.9).max(120.0));
    // The box sits over the text, by its justification (rotation is ignored while editing, as in
    // the MTEXT editor): left, centre or right, and top, middle or baseline.
    let code = mt.attach.clamp(1, 9) - 1;
    let (col, row) = (f32::from(code % 3), code / 3);
    let anchor = app.canvas.xf.map_or(rect.center(), |x| x.to_screen(mt.insert.xy()));
    let middle = match row {
        0 => anchor.y + text_px / 2.0,
        1 => anchor.y,
        _ => anchor.y - text_px / 2.0,
    };
    let box_h = px + 8.0;
    let max_x = (rect.right() - width - 8.0).max(rect.left());
    let max_y = (rect.bottom() - box_h - 24.0).max(rect.top());
    let x = (anchor.x - col * width / 2.0).clamp(rect.left(), max_x);
    let y = (middle - box_h / 2.0).clamp(rect.top(), max_y);

    let mut done = Done::No;
    let area = egui::Area::new(egui::Id::unique("line_editor_area")).order(egui::Order::Foreground).fixed_pos(pos2(x, y)).show(ctx, |ui| {
        egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.accent)).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
            let out = egui::TextEdit::singleline(&mut st.text)
                .id(edit_id())
                .font(font)
                .text_color(t.text)
                .frame(egui::Frame::NONE)
                .desired_width(width)
                .show(ui);
            // It opens with the whole text selected, as AutoCAD's does: typing replaces it.
            if !st.focused {
                out.response.request_focus();
                let mut s = out.state;
                s.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(st.text.chars().count()))));
                s.store(ui.ctx(), edit_id());
                st.focused = true;
            }
            if out.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                done = Done::Ok;
            }
            if let EditorKind::Dimension { measured } = &ed.kind {
                ui.label(RichText::new(format!("<> = {measured}")).small().color(t.text_dim));
            }
        });
    });
    let outside = ctx.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        done = Done::Cancel;
    } else if armed && outside {
        done = Done::Ok;
    }
    match done {
        Done::No => {
            ctx.data_mut(|d| d.insert_temp(state_id(), st));
        }
        Done::Ok => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            let new = st.text.replace(['\r', '\n'], " ");
            let dimension = matches!(ed.kind, EditorKind::Dimension { .. });
            // Emptied: a text keeps its value, a dimension shows its measurement again.
            let answer = match new.as_str() {
                "" if dimension => (mt.contents != "<>").then(|| "<>".to_string()),
                "" => None,
                s if s == mt.contents => None,
                _ => Some(new),
            };
            // Unchanged text is kept with Enter: no undo step.
            let r = app.session.input(answer.map_or(Input::Enter, Input::Text));
            if let Err(e) = r {
                app.session.echo(e.to_string());
            }
        }
        Done::Cancel => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            app.session.cancel();
        }
    }
}
