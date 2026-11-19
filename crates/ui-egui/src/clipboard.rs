//! The system clipboard: Cut/Copy/Paste key presses, and the drawing clipboard mirrored as DXF
//! text so it can be pasted into another CADCraft window.
//!
//! egui turns Cmd/Ctrl+C, X and V into `Copy`, `Cut` and `Paste(text)` events instead of key
//! presses, so the clipboard shortcuts are read from those. A `Paste` arrives only when the system
//! clipboard holds text; every copy therefore puts its DXF text there.

use std::hash::{DefaultHasher, Hash, Hasher};

use cadcraft_engine::cmd::clipboard;

use crate::CadApp;

/// egui memory: the clipboard copy last mirrored, and the hash of the text it became.
fn serial_id() -> egui::Id {
    egui::Id::unique("cc_clipboard_serial")
}
fn text_id() -> egui::Id {
    egui::Id::unique("cc_clipboard_text")
}

/// Line endings normalised (pasted text arrives with `\n`), so our own text is recognised.
fn hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.replace("\r\n", "\n").hash(&mut h);
    h.finish()
}

/// Put a new copy (COPYCLIP, CUTCLIP, COPYBASE from anywhere) on the system clipboard, once.
pub fn mirror(app: &CadApp, ctx: &egui::Context) {
    let Some(src) = &app.session.clipboard_source else { return };
    if src.uid.is_none() || ctx.data(|d| d.get_temp::<u64>(serial_id())) == Some(src.serial) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(serial_id(), src.serial));
    if let Some(text) = clipboard::system_text(&app.session) {
        ctx.data_mut(|d| d.insert_temp(text_id(), hash(&text)));
        ctx.copy_text(text);
    }
}

/// What a Cut/Copy/Paste clipboard event on the canvas asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    Command(&'static str),
    PasteTable(String),
}

/// The actions the Cut/Copy/Paste events of this frame ask for. Pasted DXF text that isn't our
/// own last copy (another CADCraft's) becomes the clipboard first; non-DXF tab-separated table text
/// starts `pastetable`.
pub fn shortcut_actions(app: &mut CadApp, ctx: &egui::Context) -> Vec<ShortcutAction> {
    let (events, shift) = ctx.input(|i| {
        let ev: Vec<egui::Event> =
            i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect();
        (ev, i.modifiers.shift)
    });
    let paste_cmd = || if shift { "pasteblock" } else { "pasteclip" };
    let mut out = Vec::new();
    for e in events {
        match e {
            egui::Event::Copy => out.push(ShortcutAction::Command(if shift { "copybase" } else { "copyclip" })),
            egui::Event::Cut => out.push(ShortcutAction::Command("cutclip")),
            egui::Event::Paste(text) => {
                let h = hash(&text);
                if ctx.data(|d| d.get_temp::<u64>(text_id())) == Some(h) {
                    out.push(ShortcutAction::Command(paste_cmd()));
                } else if clipboard::load_system_text(&mut app.session, &text) {
                    ctx.data_mut(|d| d.insert_temp(text_id(), h));
                    out.push(ShortcutAction::Command(paste_cmd()));
                } else if !shift && cadcraft_engine::cmd::table::is_tsv_table(&text) {
                    out.push(ShortcutAction::PasteTable(text));
                } else {
                    out.push(ShortcutAction::Command(paste_cmd()));
                }
            }
            _ => {}
        }
    }
    out
}

/// The command-id view of [`shortcut_actions`].
pub fn shortcut_commands(app: &mut CadApp, ctx: &egui::Context) -> Vec<&'static str> {
    shortcut_actions(app, ctx)
        .into_iter()
        .filter_map(|a| match a {
            ShortcutAction::Command(id) => Some(id),
            ShortcutAction::PasteTable(_) => None,
        })
        .collect()
}
