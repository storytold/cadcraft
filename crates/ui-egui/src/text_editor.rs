//! In-place editing of single-line text: TEXTEDIT with a TEXT or an attribute definition picked
//! (double-clicking one does that) shows a field over the text instead of only asking "Enter new
//! text" at the command line, as AutoCAD and BricsCAD do. Enter commits the field, Escape keeps the
//! text as it was; the command line's prompt still takes typed text as before.

use cadcraft_engine::Input;
use cadcraft_engine::doc::{HAlign, VAlign};
use egui::{FontId, Key, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

const MIN_PX: f32 = 13.0;
const MAX_PX: f32 = 48.0;

fn state_id() -> egui::Id {
    egui::Id::unique("text_editor_state")
}

/// The field's current contents while the editor is open.
pub fn editing(ctx: &egui::Context) -> Option<String> {
    ctx.data(|d| d.get_temp::<String>(state_id()))
}

/// Show the field while TEXTEDIT asks for a picked single-line text's new value; clears its state
/// otherwise.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let asking = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point);
    let (Some(t), true) = (app.session.editing_text(), asking) else {
        ctx.data_mut(|d| d.remove::<String>(state_id()));
        return;
    };
    let Some(xf) = app.canvas.xf else { return };
    let tk = Tokens::get();
    let first = editing(ctx).is_none();
    let mut buf = editing(ctx).unwrap_or_else(|| t.value.clone());
    let px = ((t.height * xf.scale) as f32).clamp(MIN_PX, MAX_PX);
    let font = FontId::proportional(px);
    let text_w = ctx.fonts_mut(|f| f.layout_no_wrap(buf.clone(), font.clone(), tk.text).size().x);
    let width = (text_w + px * 2.0).clamp(120.0, (xf.rect.width() * 0.9).max(120.0));
    // The field over the text: anchored by its justification (rotation is ignored while editing).
    let at = xf.to_screen(t.justify_point());
    let x = match t.halign {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => at.x,
        HAlign::Center | HAlign::Middle => at.x - width / 2.0,
        HAlign::Right => at.x - width,
    };
    let y = match (t.halign, t.valign) {
        (HAlign::Middle, _) | (_, VAlign::Middle) => at.y - px / 2.0,
        (_, VAlign::Top) => at.y,
        _ => at.y - px,
    };
    let pos = pos2(
        x.clamp(xf.rect.left(), (xf.rect.right() - width).max(xf.rect.left())),
        y.clamp(xf.rect.top(), (xf.rect.bottom() - px - 8.0).max(xf.rect.top())),
    );
    let te = state_id().with("field");
    let mut done = None;
    egui::Area::new(state_id().with("area")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::NONE.fill(tk.panel).stroke(Stroke::new(1.0, tk.accent)).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
            let r = ui.add_sized(vec2(width, px + 6.0), egui::TextEdit::singleline(&mut buf).id(te).font(font));
            if first {
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                done = Some(true);
            }
        });
    });
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        done = Some(false);
    }
    match done {
        None => {
            ctx.data_mut(|d| d.insert_temp(state_id(), buf));
        }
        Some(commit) => {
            ctx.data_mut(|d| d.remove::<String>(state_id()));
            // Enter commits the field; Escape answers the prompt with Enter, which keeps the text.
            let _ = app.session.input(if commit { Input::Text(buf) } else { Input::Enter });
        }
    }
}
