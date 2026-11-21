//! The system clipboard: Cut/Copy/Paste key presses, and the drawing clipboard mirrored as DXF
//! text so it can be pasted into another CADCraft window.
//!
//! egui turns Cmd/Ctrl+C, X and V into `Copy`, `Cut` and `Paste(text)` events instead of key
//! presses, so the clipboard shortcuts are read from those. A `Paste` arrives only when the system
//! clipboard holds text; every copy therefore puts its DXF text there.

use std::hash::{DefaultHasher, Hash, Hasher};

use cadcraft_engine::cmd::clipboard;
use cadcraft_engine::{CommandSpec, Session};

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

/// The commands the Cut/Copy/Paste events of this frame ask for. Pasted DXF text that isn't our
/// own last copy (another CADCraft's) becomes the clipboard first.
pub fn shortcut_commands(app: &mut CadApp, ctx: &egui::Context) -> Vec<&'static str> {
    let (events, shift) = ctx.input(|i| {
        let ev: Vec<egui::Event> =
            i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect();
        (ev, i.modifiers.shift)
    });
    let mut out = Vec::new();
    for e in events {
        match e {
            egui::Event::Copy => out.push(if shift { "copybase" } else { "copyclip" }),
            egui::Event::Cut => out.push("cutclip"),
            egui::Event::Paste(text) => {
                let h = hash(&text);
                if ctx.data(|d| d.get_temp::<u64>(text_id())) != Some(h) && clipboard::load_system_text(&mut app.session, &text) {
                    ctx.data_mut(|d| d.insert_temp(text_id(), h));
                }
                // A paste asked for from a menu or the command line is the command that was held.
                let held = app.paste.wait.take().map(|(p, _)| p);
                match held {
                    Some(p) if p.line.is_some() => run_now(app, p),
                    Some(p) => out.push(p.id),
                    None => out.push(if shift { "pasteblock" } else { "pasteclip" }),
                }
            }
            _ => {}
        }
    }
    out
}

/// How long a menu, context menu or typed paste waits for the window to deliver the system
/// clipboard before it pastes what this window already holds (the clipboard holds no text, or an
/// image, and then no `Paste` ever arrives).
const WAIT_SECS: f64 = 0.5;

/// Whether this build can ask the window for the system clipboard on demand. The web build gets
/// a `Paste` only from the browser's own paste gesture, so it keeps pasting what the window holds.
const ON_DEMAND: bool = cfg!(not(target_arch = "wasm32"));

/// A paste command asked for outside the keyboard, held until the system clipboard has been read.
struct Pending {
    id: &'static str,
    /// The command line text it was typed as, when it was.
    line: Option<String>,
}

/// Paste commands waiting for the system clipboard ([`defer`], [`pump`]).
#[derive(Default)]
pub struct Paste {
    /// Asked for this frame; the window has not been asked for the clipboard yet.
    request: Option<Pending>,
    /// Asked for the clipboard; its `Paste` event, or the time to give up.
    wait: Option<(Pending, f64)>,
    /// A held command is being run now, and must not be held again.
    running: bool,
}

pub(crate) fn paste_id(id: &str) -> Option<&'static str> {
    ["pasteclip", "pasteblock", "pasteorig"].into_iter().find(|p| *p == id)
}

/// Whether the menus offer `spec`. The paste commands count as available while a drawing is open
/// and nothing was copied in this window, since the system clipboard may hold DXF text
/// copied in another one; they report an empty clipboard when they run and it does not.
pub fn offered(session: &Session, spec: &CommandSpec) -> bool {
    (spec.enabled)(session).is_ok() || (ON_DEMAND && paste_id(spec.id).is_some() && session.clipboard.is_empty() && session.doc().is_ok())
}

/// Hold the paste command `id` (typed as `line`, if it was) until the system clipboard has been
/// read, so the newest of this window's clipboard and the system's is pasted. `false` when the
/// command runs right away: not a paste, a build that cannot ask, or already read.
pub fn defer(app: &mut CadApp, id: &str, line: Option<&str>) -> bool {
    let Some(id) = paste_id(id).filter(|_| ON_DEMAND && !app.paste.running) else { return false };
    app.paste.request = Some(Pending { id, line: line.map(str::to_string) });
    true
}

/// Run a held command, or `id` from a `Paste` event, without holding it again.
fn run_now(app: &mut CadApp, p: Pending) {
    app.paste.running = true;
    match p.line {
        Some(line) => {
            app.cmdline(&line);
        }
        None => {
            app.cmd.buffer.clear();
            app.start(p.id);
        }
    }
    app.paste.running = false;
}

/// Run `id`, a paste command whose `Paste` event has been read.
pub fn run_read(app: &mut CadApp, id: &'static str) {
    run_now(app, Pending { id, line: None });
}

/// Once per frame: ask the window for the system clipboard for a newly held paste, and give up on
/// one it never delivered. The text arrives as a `Paste` event ([`shortcut_commands`]).
pub fn pump(app: &mut CadApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    if let Some(p) = app.paste.request.take() {
        // A focused text field would take the `Paste` itself.
        if ctx.egui_wants_keyboard_input() {
            run_now(app, p);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ctx.request_repaint_after_secs(WAIT_SECS as f32);
            app.paste.wait = Some((p, now + WAIT_SECS));
        }
    }
    if app.paste.wait.as_ref().is_some_and(|(_, deadline)| now >= *deadline)
        && let Some((p, _)) = app.paste.wait.take()
    {
        run_now(app, p);
    }
}
