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
    let (events, shift, has_paste_ev, saw_v_pressed, saw_v_release, saw_shift_insert) = ctx.input(|i| {
        let ev: Vec<egui::Event> =
            i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect();
        let has_paste_ev = ev.iter().any(|e| matches!(e, egui::Event::Paste(_)));
        let saw_v_pressed = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. }
                    if modifiers.command || modifiers.ctrl || i.modifiers.command || i.modifiers.ctrl
            )
        });
        let saw_v_release = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. }
                    if modifiers.command || modifiers.ctrl || i.modifiers.command || i.modifiers.ctrl
            )
        });
        let saw_shift_insert = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { key: egui::Key::Insert, pressed: true, modifiers, .. }
                    if modifiers.shift || i.modifiers.shift
            )
        });
        (ev, i.modifiers.shift, has_paste_ev, saw_v_pressed, saw_v_release, saw_shift_insert)
    });
    // egui-winit 0.36 swallows Key::V(pressed: true) on Ctrl+V when the OS clipboard holds an
    // image without text, emitting only Key::V(pressed: false) on key release. Track whether
    // pressed: true arrived (outside ctx.input lock) so we never fire twice on press + release.
    let v_pressed_id = egui::Id::unique("cadcraft_ctrl_v_pressed_seen");
    let prev_v_pressed = ctx.data(|d| d.get_temp::<bool>(v_pressed_id)).unwrap_or(false);
    if saw_v_pressed {
        ctx.data_mut(|d| d.insert_temp(v_pressed_id, true));
    } else if saw_v_release {
        ctx.data_mut(|d| d.remove::<bool>(v_pressed_id));
    }
    let ctrl_v_key = !has_paste_ev && (saw_v_pressed || saw_shift_insert || (saw_v_release && !prev_v_pressed));
    let paste_cmd = || if shift { "pasteblock" } else { "pasteclip" };
    let mut out = Vec::new();
    if ctrl_v_key && try_load_os_clipboard_image(app) {
        app.paste.cancel();
        out.push(ShortcutAction::Command("pasteclip"));
    }
    for e in events {
        match e {
            egui::Event::Copy => out.push(ShortcutAction::Command(if shift { "copybase" } else { "copyclip" })),
            egui::Event::Cut => out.push(ShortcutAction::Command("cutclip")),
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
                let is_own = ctx.data(|d| d.get_temp::<u64>(text_id())) == Some(h);
                let is_dxf = if is_own {
                    true
                } else if clipboard::load_system_text(&mut app.session, &text) {
                    ctx.data_mut(|d| d.insert_temp(text_id(), h));
                    true
                } else {
                    false
                };
                if held.is_none() && answered.is_some() {
                    continue;
                }
                let clean = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
                let t = clean.trim();
                let typing_in_cmd = !app.cmd.buffer.is_empty()
                    || app.session.prompting()
                    || t.starts_with('(')
                    || t.starts_with('!')
                    || cadcraft_engine::is_bare_lisp_prefix(t);
                if !is_dxf
                    && !shift
                    && !typing_in_cmd
                    && cadcraft_engine::cmd::table::is_tsv_table(&text)
                    && (held.is_none() || held.as_ref().is_some_and(|p| p.id == "pasteclip"))
                {
                    out.push(ShortcutAction::PasteTable(text));
                    continue;
                }
                // A paste asked for from a menu or the command line is the command that was held.
                match held {
                    Some(p) if p.line.is_some() => run_now(app, p),
                    Some(p) => out.push(ShortcutAction::Command(p.id)),
                    None => {
                        let is_img = !is_own && !is_dxf && !typing_in_cmd && try_load_os_clipboard_image(app);
                        let paste_to_cmd = !is_own && !is_dxf && !is_img && !t.is_empty() && (app.session.clipboard.is_empty() || typing_in_cmd);
                        if paste_to_cmd {
                            app.cmd.buffer.push_str(&clean);
                            app.cmd.note_typing();
                        } else if is_img {
                            out.push(ShortcutAction::Command("pasteclip"));
                        } else {
                            out.push(ShortcutAction::Command(paste_cmd()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Synchronize the OS clipboard before executing `pasteclip` / `paste` from menus or the command line.
pub fn sync_os_clipboard_for_paste(app: &mut CadApp) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let has_os_text = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).is_some_and(|t| !t.trim().is_empty());
        if !has_os_text {
            let _ = try_load_os_clipboard_image(app);
            return;
        }
        if app.session.clipboard.is_empty() && app.session.clipboard_image.is_none() {
            let _ = try_load_os_clipboard_image(app);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn store_rgba_in_session(app: &mut CadApp, mut rgba: image::RgbaImage) -> bool {
    let (width, height) = (rgba.width(), rgba.height());
    if width == 0 || height == 0 {
        return false;
    }
    // Windows GDI 32-bit CF_DIB bitmaps (e.g. FastStone Capture) often have alpha = 0 across all pixels.
    if rgba.pixels().all(|p| p.0[3] == 0) {
        for p in rgba.pixels_mut() {
            p.0[3] = 255;
        }
    }
    let mut png_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    if rgba.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
        app.session.set_clipboard_image(png_bytes, width, height);
        return true;
    }
    false
}

/// Try to read a raster image from the OS clipboard and load it into `app.session`.
#[cfg(not(target_arch = "wasm32"))]
pub fn try_load_os_clipboard_image(app: &mut CadApp) -> bool {
    if let Ok(mut cb) = arboard::Clipboard::new()
        && let Ok(img) = cb.get_image()
        && let (Ok(width), Ok(height)) = (u32::try_from(img.width), u32::try_from(img.height))
        && width > 0
        && height > 0
        && let Some(rgba) = image::RgbaImage::from_raw(width, height, img.bytes.into_owned())
        && store_rgba_in_session(app, rgba)
    {
        return true;
    }
    #[cfg(target_os = "windows")]
    if try_load_windows_clipboard_image(app) {
        return true;
    }
    false
}

#[cfg(target_os = "windows")]
fn try_load_windows_clipboard_image(app: &mut CadApp) -> bool {
    use clipboard_win::{Clipboard, Getter, raw};
    let Ok(_clip) = Clipboard::new_attempts(10) else { return false };
    const MAX_DIB_BYTES: usize = 64 * 1024 * 1024;

    for fmt_name in ["PNG", "image/png", "JFIF", "image/jpeg", "GIF"] {
        if let Some(fmt) = clipboard_win::register_format(fmt_name) {
            let mut buf = Vec::new();
            if raw::get_vec(fmt.get(), &mut buf).is_ok()
                && !buf.is_empty()
                && buf.len() <= MAX_DIB_BYTES
                && let Ok(dyn_img) = image::load_from_memory(&buf)
                && store_rgba_in_session(app, dyn_img.to_rgba8())
            {
                return true;
            }
        }
    }

    let mut bmp_bytes = Vec::new();
    if clipboard_win::formats::Bitmap.read_clipboard(&mut bmp_bytes).is_ok()
        && !bmp_bytes.is_empty()
        && bmp_bytes.len() <= MAX_DIB_BYTES
        && let Ok(dyn_img) = image::load_from_memory(&bmp_bytes)
        && store_rgba_in_session(app, dyn_img.to_rgba8())
    {
        return true;
    }
    for cf_dib in [8u32, 17u32] {
        if raw::is_format_avail(cf_dib) {
            let mut dib = Vec::new();
            if raw::get_vec(cf_dib, &mut dib).is_ok() && dib.len() > 40 && dib.len() <= MAX_DIB_BYTES {
                let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]);
                if header_size < 40 || (header_size as usize) > dib.len() {
                    continue;
                }
                let bpp = u16::from_le_bytes([dib[14], dib[15]]);
                if bpp > 32 {
                    continue;
                }
                let compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
                let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]);
                let palette_bytes = if clr_used > 0 {
                    clr_used.checked_mul(4)
                } else if bpp <= 8 {
                    1u32.checked_shl(u32::from(bpp)).and_then(|n| n.checked_mul(4))
                } else if compression == 3 && header_size == 40 {
                    Some(12)
                } else {
                    Some(0)
                };
                let Some(palette_bytes) = palette_bytes else { continue };
                let Some(offset) = 14u32.checked_add(header_size).and_then(|n| n.checked_add(palette_bytes)) else {
                    continue;
                };
                let Ok(dib_len) = u32::try_from(dib.len()) else { continue };
                let Some(file_size) = 14u32.checked_add(dib_len) else { continue };
                if (offset as usize) > (file_size as usize) {
                    continue;
                }
                let mut bmp = Vec::with_capacity(file_size as usize);
                bmp.extend_from_slice(b"BM");
                bmp.extend_from_slice(&file_size.to_le_bytes());
                bmp.extend_from_slice(&[0u8; 4]);
                bmp.extend_from_slice(&offset.to_le_bytes());
                bmp.extend_from_slice(&dib);
                if let Ok(dyn_img) = image::load_from_memory(&bmp)
                    && store_rgba_in_session(app, dyn_img.to_rgba8())
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(target_arch = "wasm32")]
pub fn try_load_os_clipboard_image(_app: &mut CadApp) -> bool {
    false
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
    /// The drawing it was asked for in (its `uid`): it pastes there or not at all.
    doc: Option<u64>,
    /// The inputs the session had been given when it was asked for (`Session::inputs`).
    inputs: u64,
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

/// The drawing shown now, as a held paste remembers it.
fn active_doc(app: &CadApp) -> Option<u64> {
    app.session.state().ok().map(|s| s.uid)
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
    app.paste.request = Some(Pending { id, line: line.map(str::to_string), doc: active_doc(app), inputs: app.session.inputs });
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
    run_now(app, Pending { id, line: None, doc: active_doc(app), inputs: app.session.inputs });
}

/// Once per frame: ask the window for the system clipboard for a newly held paste, and give up on
/// one it never delivered. The text arrives as a `Paste` event ([`shortcut_actions`]).
pub fn pump(app: &mut CadApp, ctx: &egui::Context) {
    let (now, pasted) = ctx.input(|i| (i.time, i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))));
    if ctx.current_pass_index() == 0 {
        app.paste.frame += 1;
    }
    let (frame, late) = (app.paste.frame, app.paste.late);
    app.paste.sent.retain(|r| if late { r.until > now } else { r.frame + 1 >= frame });
    // A held paste belongs to the drawing it was asked for in. If another one is shown by now (a
    // file read in the background finished opening, or the tab was switched), it is dropped
    // rather than run there.
    let held = app.paste.request.as_ref().or(app.paste.wait.as_ref().map(|(p, _, _)| p));
    if held.is_some_and(|p| p.doc != active_doc(app)) {
        app.paste.cancel();
        app.session.echo("Paste cancelled: another drawing became active before the clipboard was read.");
    }
    // Input that reached the session since it was asked for answered the prompt of the command
    // still running (a coordinate for LINE), which the paste would now cancel: that input stands,
    // and the held paste is dropped like one a newly started command replaces.
    let held = app.paste.request.as_ref().or(app.paste.wait.as_ref().map(|(p, _, _)| p));
    if held.is_some_and(|p| p.inputs != app.session.inputs) {
        app.paste.cancel();
    }
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
    // in this frame completes the held command first ([`shortcut_actions`]).
    if pasted && ctx.egui_wants_keyboard_input() {
        app.paste.wait = None;
    }
    let gone = |app: &CadApp| app.paste.wait.as_ref().is_some_and(|(_, id, _)| !app.paste.sent.iter().any(|r| r.id == *id));
    if (gone(app) || (!pasted && app.paste.wait.as_ref().is_some_and(|(_, _, deadline)| now >= *deadline)))
        && let Some((p, _, _)) = app.paste.wait.take()
    {
        run_now(app, p);
    } else if app.paste.wait.is_some() && !late {
        ctx.request_repaint();
    }
}
