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

/// The commands the Cut/Copy/Paste events of this frame ask for. Pasted DXF text that isn't our
/// own last copy (another CADCraft's) becomes the clipboard first.
pub fn shortcut_commands(app: &mut CadApp, ctx: &egui::Context) -> Vec<&'static str> {
    let (events, shift, ctrl_v_key) = ctx.input(|i| {
        let ev: Vec<egui::Event> =
            i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect();
        let has_paste_ev = ev.iter().any(|e| matches!(e, egui::Event::Paste(_)));
        // egui-winit 0.36 swallows Key::V(pressed: true) on Ctrl+V when the OS clipboard holds an
        // image without text, emitting only Key::V(pressed: false) on key release.
        let ctrl_v = !has_paste_ev
            && i.events.iter().any(|e| match e {
                egui::Event::Key { key: egui::Key::V, modifiers, .. } => {
                    modifiers.command || modifiers.ctrl || i.modifiers.command || i.modifiers.ctrl
                }
                egui::Event::Key { key: egui::Key::Insert, modifiers, .. } => modifiers.shift || i.modifiers.shift,
                _ => false,
            });
        (ev, i.modifiers.shift, ctrl_v)
    });
    let mut out = Vec::new();
    if ctrl_v_key && try_load_os_clipboard_image(app) {
        out.push("pasteclip");
    }
    for e in events {
        match e {
            egui::Event::Copy => out.push(if shift { "copybase" } else { "copyclip" }),
            egui::Event::Cut => out.push("cutclip"),
            egui::Event::Paste(text) => {
                let h = hash(&text);
                let is_own = ctx.data(|d| d.get_temp::<u64>(text_id())) == Some(h);
                let is_dxf = !is_own && clipboard::load_system_text(&mut app.session, &text);
                if is_dxf {
                    ctx.data_mut(|d| d.insert_temp(text_id(), h));
                }
                let clean = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
                let t = clean.trim();
                let typing_in_cmd = !app.cmd.buffer.is_empty()
                    || app.session.prompting()
                    || t.starts_with('(')
                    || t.starts_with('!')
                    || cadcraft_engine::is_bare_lisp_prefix(t);
                let is_img = !is_own && !is_dxf && !typing_in_cmd && try_load_os_clipboard_image(app);
                let paste_to_cmd = !is_own
                    && !is_dxf
                    && !is_img
                    && !t.is_empty()
                    && (app.session.clipboard.is_empty() || typing_in_cmd);
                if paste_to_cmd {
                    app.cmd.buffer.push_str(&clean);
                    app.cmd.note_typing();
                } else {
                    out.push(if shift && !is_img { "pasteblock" } else { "pasteclip" });
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
        let has_os_text = arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok())
            .is_some_and(|t| !t.trim().is_empty());
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
    {
        let width = img.width as u32;
        let height = img.height as u32;
        if width > 0
            && height > 0
            && let Some(rgba) = image::RgbaImage::from_raw(width, height, img.bytes.into_owned())
            && store_rgba_in_session(app, rgba)
        {
            return true;
        }
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

    for fmt_name in ["PNG", "image/png", "JFIF", "image/jpeg", "GIF"] {
        if let Some(fmt) = clipboard_win::register_format(fmt_name) {
            let mut buf = Vec::new();
            if raw::get_vec(fmt.get(), &mut buf).is_ok()
                && !buf.is_empty()
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
        && let Ok(dyn_img) = image::load_from_memory(&bmp_bytes)
        && store_rgba_in_session(app, dyn_img.to_rgba8())
    {
        return true;
    }

    for cf_dib in [8u32, 17u32] {
        if raw::is_format_avail(cf_dib) {
            let mut dib = Vec::new();
            if raw::get_vec(cf_dib, &mut dib).is_ok() && dib.len() > 40 {
                let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]);
                let bpp = u16::from_le_bytes([dib[14], dib[15]]);
                let compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
                let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]);
                let palette_bytes = if clr_used > 0 {
                    clr_used * 4
                } else if bpp <= 8 {
                    (1u32 << bpp) * 4
                } else if compression == 3 && header_size == 40 {
                    12
                } else {
                    0
                };
                let offset = 14u32.saturating_add(header_size).saturating_add(palette_bytes);
                let file_size = 14u32.saturating_add(dib.len() as u32);
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
