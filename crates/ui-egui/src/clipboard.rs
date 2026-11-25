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
                // The oldest request sent in an earlier frame and still unanswered gets this
                // `Paste`. It completes the held command only if the command asked for that
                // request; otherwise it answers one whose command is gone and pastes nothing. With
                // no such request it is an ordinary Ctrl+V.
                let answered = app.paste.sent.first().filter(|r| r.frame < app.paste.frame).map(|r| r.id);
                if answered.is_some() {
                    app.paste.sent.remove(0);
                }
                let mine = app.paste.wait.as_ref().is_some_and(|(_, id, _)| answered == Some(*id));
                let held = if mine { app.paste.wait.take().map(|(p, _, _)| p) } else { None };
                // The text is the system clipboard's either way, so a stale answer still counts.
                let h = hash(&text);
                if ctx.data(|d| d.get_temp::<u64>(text_id())) != Some(h) && clipboard::load_system_text(&mut app.session, &text) {
                    ctx.data_mut(|d| d.insert_temp(text_id(), h));
                }
                if held.is_none() && answered.is_some() {
                    continue;
                }
                // A paste asked for from a menu or the command line is the command that was held.
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

/// How long the Wayland backend may take to answer a `RequestPaste`. Its read starts after the
/// frame that queued the request has been painted and is given 3 s from then (`DELIVERY_BUDGET`,
/// egui-winit `clipboard_bridge.rs`, counted from `begin_regular`); an answer later than that
/// never comes. The 2 s of margin over the budget cover that painting and the frame that
/// delivers the answer, which are not bounded.
const ANSWER_SECS: f64 = 5.0;

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
pub struct Paste {
    /// Asked for this frame; the window has not been asked for the clipboard yet.
    request: Option<Pending>,
    /// Asked for the clipboard; its `Paste` event, or the time to give up.
    /// The held command, the request it asked for ([`Sent::id`]), and the time to give up.
    wait: Option<(Pending, u64, f64)>,
    /// A held command is being run now, and must not be held again.
    pub(crate) running: bool,
    /// The `RequestPaste`s that may still be answered, oldest first. A `Paste` answers the oldest,
    /// so one that arrives after its command was escaped or gave up is not taken for a paste of
    /// its own, nor for the answer to a newer request.
    sent: Vec<Sent>,
    next_id: u64,
    /// Frames pumped so far.
    frame: u64,
    /// Whether the window may answer later than the next frame (native Wayland), see
    /// [`Self::set_late_answers`].
    late: bool,
}

/// A `RequestPaste` that may still be answered.
struct Sent {
    id: u64,
    /// The frame it was sent in; its answer is in a later frame's input.
    frame: u64,
    /// When it stops being answerable on a window that answers late.
    until: f64,
}

impl Default for Paste {
    fn default() -> Self {
        Paste { request: None, wait: None, running: false, sent: Vec::new(), next_id: 0, frame: 0, late: true }
    }
}

impl Paste {
    /// Say whether the window may answer a request later than the next frame. egui-winit reads
    /// the clipboard when it handles the command, right after the frame that sent it is painted,
    /// and the text is in the next frame's input (`request_paste`, `begin_regular_clipboard_paste`
    /// and `take_egui_input`), or nothing comes when there is no text; only its Wayland backend
    /// reads asynchronously. A request is then answerable for one frame, not for seconds, so a
    /// Ctrl+V after it is never taken for its answer. On Wayland that Ctrl+V within
    /// [`ANSWER_SECS`] of a request that got no answer is, once, taken for it.
    pub fn set_late_answers(&mut self, late: bool) {
        self.late = late;
    }

    /// Drop a held paste (Escape, or another command started). A request already sent to the
    /// window stays in `sent`, so its answer, if it comes, pastes nothing.
    pub fn cancel(&mut self) {
        self.request = None;
        self.wait = None;
    }
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
    app.paste.cancel();
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
    let (now, pasted) = ctx.input(|i| (i.time, i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))));
    if ctx.current_pass_index() == 0 {
        app.paste.frame += 1;
    }
    let (frame, late) = (app.paste.frame, app.paste.late);
    app.paste.sent.retain(|r| if late { r.until > now } else { r.frame + 1 >= frame });
    if let Some(p) = app.paste.request.take() {
        // A focused text field would take the `Paste` itself.
        if ctx.egui_wants_keyboard_input() {
            run_now(app, p);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            let id = app.paste.next_id;
            app.paste.next_id += 1;
            app.paste.sent.push(Sent { id, frame, until: now + ANSWER_SECS });
            ctx.request_repaint_after_secs(WAIT_SECS as f32);
            app.paste.wait = Some((p, id, now + WAIT_SECS));
        }
    }
    // A request that can no longer be answered, or the time to give up, ends the wait; a `Paste`
    // in this frame completes the held command first ([`shortcut_commands`]).
    let gone = |app: &CadApp| app.paste.wait.as_ref().is_some_and(|(_, id, _)| !app.paste.sent.iter().any(|r| r.id == *id));
    if (gone(app) || (!pasted && app.paste.wait.as_ref().is_some_and(|(_, _, deadline)| now >= *deadline)))
        && let Some((p, _, _)) = app.paste.wait.take()
    {
        run_now(app, p);
    } else if app.paste.wait.is_some() && !late {
        ctx.request_repaint();
    }
}
