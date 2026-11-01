//! Edit Text: the window a double-click on a text object opens.
//!
//! TEXTEDIT already existed, but only as a command line: double-clicking a single-line text put
//! the caret in the command bar, one line, no context, and the text under the cursor was out of
//! sight. The window shows the same edit over the drawing: the object's own text, its type and
//! handle, and OK/Cancel. The engine call is unchanged (`textedit` with a handle and the new
//! text), so undo, locked layers and the DXF round trip behave exactly as before.

use egui::RichText;
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;
use cadcraft_engine::doc::EntityKind;

/// The text the window starts with, in the form the user edits: MText line breaks are `\P` in
/// the drawing and a newline here, and an empty dimension text means the measured value, shown
/// as `<>` the way the dimension itself does.
fn text_of(k: &EntityKind) -> Option<String> {
    match k {
        EntityKind::Text(t) => Some(t.value.clone()),
        EntityKind::AttDef(a) => Some(a.text.value.clone()),
        EntityKind::MText(t) => Some(t.contents.replace("\\P", "\n")),
        EntityKind::Dimension(d) => Some(if d.text.is_empty() { "<>".to_string() } else { d.text.clone() }),
        _ => None,
    }
}

#[derive(Default, Clone)]
struct State {
    handle: String,
    kind: String,
    text: String,
    /// What the drawing holds now, to tell an edit from a no-op.
    original: String,
    message: String,
}

/// Read the object the window is for. A handle that no longer exists, or an object that is not
/// text, leaves the window empty instead of failing: the drawing may have changed under it.
fn initial(app: &CadApp, hex: &str) -> State {
    let mut st = State { handle: hex.to_string(), ..Default::default() };
    let Some(h) = cadcraft_doc::Handle::parse_hex(hex) else { return st };
    let Ok(d) = app.session.doc() else { return st };
    let Some(e) = d.entity(h) else { return st };
    st.kind = e.kind.type_name().to_string();
    st.text = text_of(&e.kind).unwrap_or_default();
    st.original = st.text.clone();
    st
}

/// The Edit Text window for `spec`, which is `textedit:<hex handle>` so the handle survives the
/// dialog dispatch without a new field in the saved UI state.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, spec: &str, open: &mut bool) {
    let t = Tokens::get();
    let hex = spec.strip_prefix("textedit:").unwrap_or_default().to_string();
    let sid = egui::Id::unique("textedit_state").with(&hex);
    let cached: Option<State> = ctx.data_mut(|d| d.get_temp(sid));
    let mut st = match cached {
        Some(s) => s,
        None => initial(app, &hex),
    };
    // The window's own close button and the two buttons below all decide the same thing, so the
    // decision stays in a local until the window is gone: `open` is borrowed for the title bar.
    let mut keep = *open;
    let mut commit = false;
    let mut cancel = false;
    egui::Window::new(crate::tl!("Edit Text"))
        .id(egui::Id::unique("Edit Text"))
        .open(&mut keep)
        .collapsible(false)
        .resizable(true)
        .default_width(440.0)
        .show(ctx, |ui| {
            let head = format!("{} · {}", crate::i18n::t(&st.kind), st.handle);
            ui.label(RichText::new(head).color(t.text_faint).small());
            let r = ui.add(egui::TextEdit::multiline(&mut st.text).desired_width(f32::INFINITY).desired_rows(3).hint_text(crate::tl!("Text")));
            // Ctrl+Enter commits from inside the field, so a multiline text can keep plain Enter.
            if r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                commit = true;
            }
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("OK")).clicked() {
                    commit = true;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    cancel = true;
                }
                ui.label(RichText::new(crate::tl!("Ctrl+Enter to apply")).color(t.text_faint).small());
            });
            if !st.message.is_empty() {
                ui.label(RichText::new(st.message.as_str()).color(t.warn).small());
            }
        });
    if cancel {
        keep = false;
    }
    if commit {
        // Typing and undoing back to the same text is not an edit: closing without a command
        // keeps the undo stack honest.
        if st.text == st.original {
            keep = false;
        } else {
            let params = json!({ "handles": [st.handle.clone()], "text": st.text });
            match app.run("textedit", params) {
                Ok(_) => {
                    st.original = st.text.clone();
                    keep = false;
                }
                Err(e) => st.message = e,
            }
        }
    }
    *open = keep;
    ctx.data_mut(|d| d.insert_temp(sid, st));
}
