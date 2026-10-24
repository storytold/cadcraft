//! Small previews of recent drawings for the Start page's grid. A preview is made once on a
//! worker thread (the file is read and its model space drawn at [`W`] × [`H`] pixels) and kept as
//! a small PNG in the user's settings folder, named after the file's path, size and date, so a
//! changed file gets a new one. Whether a file is still there is checked every few seconds.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

/// Preview size in pixels: small files (a few tens of kilobytes), large enough to tell drawings
/// apart.
pub const W: u32 = 240;
pub const H: u32 = 160;
/// How often a file's presence is checked again.
const RECHECK_S: f64 = 5.0;

/// What the grid shows for a recent file.
#[derive(Clone)]
pub enum Thumb {
    /// Not made yet (being made, or waiting its turn).
    Pending,
    Ready(egui::TextureHandle),
    /// The file is not at its path.
    Missing,
    /// The file is there but could not be read or drawn.
    Failed,
}

#[derive(Default)]
struct Store {
    thumbs: HashMap<String, Thumb>,
    /// When each path was last checked, and whether it existed.
    seen: HashMap<String, (f64, bool)>,
    /// The path being made now, and the channel its PNG arrives on.
    #[cfg(not(target_arch = "wasm32"))]
    job: Option<(String, std::sync::mpsc::Receiver<Option<Vec<u8>>>)>,
}

fn store() -> &'static Mutex<Store> {
    static S: OnceLock<Mutex<Store>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Store::default()))
}

/// Whether `path` is a file that exists (checked at most every [`RECHECK_S`] seconds).
pub fn exists(path: &str, now: f64) -> bool {
    let mut s = store().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((at, ok)) = s.seen.get(path)
        && now - at < RECHECK_S
    {
        return *ok;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let ok = std::path::Path::new(path).is_file();
    #[cfg(target_arch = "wasm32")]
    let ok = true;
    if s.seen.len() > 256 {
        s.seen.clear();
    }
    if s.seen.get(path).is_some_and(|(_, was)| *was != ok) {
        // Back (or gone): make or drop its preview again.
        s.thumbs.remove(path);
    }
    s.seen.insert(path.to_string(), (now, ok));
    ok
}

/// The preview of `path`, starting to make it when there is none.
pub fn get(ctx: &egui::Context, path: &str) -> Thumb {
    let now = ctx.input(|i| i.time);
    if !exists(path, now) {
        return Thumb::Missing;
    }
    let mut s = store().lock().unwrap_or_else(PoisonError::into_inner);
    poll(&mut s, ctx);
    if let Some(t) = s.thumbs.get(path) {
        return t.clone();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(png) = cache_file(path).and_then(|f| std::fs::read(f).ok()) {
            let t = texture(ctx, path, &png).map_or(Thumb::Failed, Thumb::Ready);
            s.thumbs.insert(path.to_string(), t.clone());
            return t;
        }
        if s.job.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let p = path.to_string();
            let c = ctx.clone();
            let spawned = std::thread::Builder::new().name("cadcraft-thumb".into()).spawn(move || {
                let png = std::panic::catch_unwind(|| make(&p)).ok().flatten();
                let _ = tx.send(png);
                c.request_repaint();
            });
            if spawned.is_ok() {
                s.job = Some((path.to_string(), rx));
            } else {
                s.thumbs.insert(path.to_string(), Thumb::Failed);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
    Thumb::Pending
}

/// Take a finished preview.
#[cfg(not(target_arch = "wasm32"))]
fn poll(s: &mut Store, ctx: &egui::Context) {
    let Some((path, rx)) = &s.job else { return };
    let got = match rx.try_recv() {
        Ok(png) => Some(png),
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
    };
    let path = path.clone();
    s.job = None;
    let t = match got.flatten() {
        Some(png) => {
            if let Some(f) = cache_file(&path) {
                if let Some(dir) = f.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(f, &png);
            }
            texture(ctx, &path, &png).map_or(Thumb::Failed, Thumb::Ready)
        }
        None => Thumb::Failed,
    };
    if s.thumbs.len() > 64 {
        s.thumbs.clear();
    }
    s.thumbs.insert(path, t);
}

#[cfg(target_arch = "wasm32")]
fn poll(_s: &mut Store, _ctx: &egui::Context) {}

fn texture(ctx: &egui::Context, path: &str, png: &[u8]) -> Option<egui::TextureHandle> {
    let (w, h, rgba) = cadcraft_render::raster::decode_png(png)?;
    let img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &rgba);
    Some(ctx.load_texture(format!("cc-thumb:{path}"), img, egui::TextureOptions::LINEAR))
}

/// The preview of `path` as a small PNG: the one the file carries (a DWG's preview image, a
/// DXF's THUMBNAILIMAGE), else its model space drawn.
#[cfg(not(target_arch = "wasm32"))]
fn make(path: &str) -> Option<Vec<u8>> {
    if let Some(png) = embedded(path) {
        return Some(png);
    }
    let f = cadcraft_engine::cmd::file::read_file(path).ok()?;
    let d = f.drawing();
    let space = cadcraft_engine::doc::Space::Model;
    let opts = cadcraft_render::Options { min_dash: 0.0, ..cadcraft_render::Options::default() };
    let list = cadcraft_render::build(d, &space, &opts);
    let view = cadcraft_render::raster::View::fit(&list.bounds, W, H, 0.06);
    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default())
}

/// Most bytes read looking for a file's own preview.
#[cfg(not(target_arch = "wasm32"))]
const MAX_PREVIEW_BYTES: u64 = 8 << 20;

/// The preview image a drawing file carries, as a PNG; `None` when it has none (or only a blank
/// one).
#[cfg(not(target_arch = "wasm32"))]
fn embedded(path: &str) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let lower = path.to_ascii_lowercase();
    let mut f = std::fs::File::open(path).ok()?;
    if lower.ends_with(".dwg") {
        // The preview's address is at 0x0D (Open Design Alliance DWG specification, "Image data").
        let mut head = [0u8; 0x11];
        f.read_exact(&mut head).ok()?;
        let at = u64::from(u32::from_le_bytes(head.get(0x0D..0x11)?.try_into().ok()?));
        f.seek(SeekFrom::Start(at)).ok()?;
        let mut sec = Vec::new();
        f.take(MAX_PREVIEW_BYTES).read_to_end(&mut sec).ok()?;
        return dwg_preview(&sec, at).and_then(finish);
    }
    if lower.ends_with(".dxf") {
        let mut text = String::new();
        // The THUMBNAILIMAGE section comes after the header, classes and tables: read a bounded
        // amount from the start.
        f.take(MAX_PREVIEW_BYTES * 4).read_to_string(&mut text).ok()?;
        return dxf_preview(&text).and_then(|bmp| dib(&bmp)).and_then(finish);
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
/// An image as (width, height, straight RGBA).
type Rgba = (u32, u32, Vec<u8>);

/// A DWG preview section starting at file offset `at`: a PNG kept as is, a bitmap decoded.
#[cfg(not(target_arch = "wasm32"))]
fn dwg_preview(sec: &[u8], at: u64) -> Option<Rgba> {
    const SENTINEL: [u8; 16] = [0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B];
    if sec.get(..16)? != SENTINEL {
        return None;
    }
    let count = *sec.get(20)?;
    let mut i = 21usize;
    let mut bmp = None;
    let mut png = None;
    for _ in 0..count.min(8) {
        let code = *sec.get(i)?;
        let start = u64::from(u32::from_le_bytes(sec.get(i + 1..i + 5)?.try_into().ok()?));
        let size = u32::from_le_bytes(sec.get(i + 5..i + 9)?.try_into().ok()?) as usize;
        i += 9;
        let off = usize::try_from(start.checked_sub(at)?).ok()?;
        let data = sec.get(off..off.checked_add(size)?);
        match code {
            2 => bmp = data,
            6 => png = data,
            _ => {}
        }
    }
    if let Some(p) = png
        && let Some((w, h, rgba)) = cadcraft_render::raster::decode_png(p)
    {
        return Some((w, h, unpremultiply(rgba)));
    }
    dib(bmp?)
}

/// The THUMBNAILIMAGE section's bitmap (hex in group 310 lines).
#[cfg(not(target_arch = "wasm32"))]
fn dxf_preview(text: &str) -> Option<Vec<u8>> {
    let at = text.find("THUMBNAILIMAGE")?;
    let mut lines = text.get(at..)?.lines().map(str::trim);
    let mut out = Vec::new();
    while let Some(code) = lines.next() {
        let value = lines.next()?;
        match code {
            "310" => {
                let b = value.as_bytes();
                for pair in b.chunks(2) {
                    let s = std::str::from_utf8(pair).ok()?;
                    out.push(u8::from_str_radix(s, 16).ok()?);
                }
            }
            "0" if value == "ENDSEC" => break,
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(not(target_arch = "wasm32"))]
/// A device-independent bitmap (BITMAPINFOHEADER, then a palette, then rows bottom up): 8, 24
/// or 32 bits a pixel, uncompressed.
fn dib(b: &[u8]) -> Option<Rgba> {
    let u32_at = |i: usize| b.get(i..i + 4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes);
    let u16_at = |i: usize| b.get(i..i + 2).and_then(|s| s.try_into().ok()).map(u16::from_le_bytes);
    let header = u32_at(0)? as usize;
    let w = i32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
    let h = i32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let bits = u16_at(14)?;
    if u32_at(16)? != 0 || w <= 0 || h == 0 || w > 2048 || h.unsigned_abs() > 2048 {
        return None;
    }
    let (w, top_down, h) = (w as usize, h < 0, h.unsigned_abs() as usize);
    let colors = match (bits, u32_at(32)?) {
        (8, 0) => 256,
        (8, n) => (n as usize).min(256),
        _ => 0,
    };
    let palette = header.checked_add(colors * 4)?;
    let stride = (w * usize::from(bits)).div_ceil(32) * 4;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_row = if top_down { y } else { h - 1 - y };
        let row = palette.checked_add(src_row.checked_mul(stride)?)?;
        for x in 0..w {
            let (r, g, bl) = match bits {
                8 => {
                    let i = usize::from(*b.get(row + x)?);
                    let p = header + i * 4;
                    (*b.get(p + 2)?, *b.get(p + 1)?, *b.get(p)?)
                }
                24 => {
                    let p = row + x * 3;
                    (*b.get(p + 2)?, *b.get(p + 1)?, *b.get(p)?)
                }
                32 => {
                    let p = row + x * 4;
                    (*b.get(p + 2)?, *b.get(p + 1)?, *b.get(p)?)
                }
                _ => return None,
            };
            let o = (y * w + x) * 4;
            out.get_mut(o..o + 4)?.copy_from_slice(&[r, g, bl, 255]);
        }
    }
    Some((w as u32, h as u32, out))
}

#[cfg(not(target_arch = "wasm32"))]
fn unpremultiply(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.chunks_mut(4) {
        if let [r, g, b, a] = px
            && *a > 0
            && *a < 255
        {
            let k = |v: u8| ((u32::from(v) * 255 + u32::from(*a) / 2) / u32::from(*a)).min(255) as u8;
            (*r, *g, *b) = (k(*r), k(*g), k(*b));
        }
    }
    rgba
}

#[cfg(not(target_arch = "wasm32"))]
/// A carried preview as a PNG, or `None` when it is blank (one colour).
fn finish((w, h, rgba): Rgba) -> Option<Vec<u8>> {
    let first = rgba.get(..4)?.to_vec();
    if rgba.chunks(4).all(|p| p == first.as_slice()) {
        return None;
    }
    // Opaque pixels: straight and premultiplied are the same.
    let opaque: Vec<u8> =
        rgba.chunks(4).flat_map(|p| [p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0), p.get(2).copied().unwrap_or(0), 255]).collect();
    cadcraft_render::raster::encode_png(w, h, &opaque)
}

/// The settings folder (as the app keeps its settings): `CADCRAFT_CONFIG_DIR`, `%APPDATA%\CADCraft`,
/// `$XDG_CONFIG_HOME/cadcraft` or `~/.config/cadcraft`.
#[cfg(not(target_arch = "wasm32"))]
fn config_dir() -> Option<std::path::PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if let Some(d) = var("CADCRAFT_CONFIG_DIR") {
        return Some(d);
    }
    if cfg!(windows) {
        return var("APPDATA").map(|a| a.join("CADCraft"));
    }
    if cfg!(target_os = "macos") {
        return var("HOME").map(|h| h.join("Library/Application Support/CADCraft"));
    }
    var("XDG_CONFIG_HOME").map(|x| x.join("cadcraft")).or_else(|| var("HOME").map(|h| h.join(".config/cadcraft")))
}

/// The cached preview file of `path`: named after the path, the file's size and its date.
#[cfg(not(target_arch = "wasm32"))]
fn cache_file(path: &str) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    // FNV-1a: stable across builds (the std hasher is not promised to be).
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    path.hash(&mut h);
    m.len().hash(&mut h);
    mtime.hash(&mut h);
    Some(config_dir()?.join("thumbnails").join(format!("{:016x}.png", h.finish())))
}

#[cfg(not(target_arch = "wasm32"))]
struct Fnv(u64);

#[cfg(not(target_arch = "wasm32"))]
impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// The "file not found" picture: a page with a folded corner drawn in dashes (it is not there),
/// a red circle with a question mark on it.
pub fn paint_missing(p: &egui::Painter, r: egui::Rect, ink: egui::Color32) {
    use egui::{Stroke, pos2, vec2};
    let h = r.height().min(r.width() * 1.3) * 0.82;
    let w = h * 0.76;
    let page = egui::Rect::from_center_size(r.center(), vec2(w, h));
    let fold = w * 0.3;
    let pts = [
        page.left_top(),
        pos2(page.right() - fold, page.top()),
        pos2(page.right(), page.top() + fold),
        page.right_bottom(),
        page.left_bottom(),
        page.left_top(),
    ];
    let st = Stroke::new((h / 40.0).clamp(1.5, 3.0), ink);
    for seg in pts.windows(2) {
        if let [a, b] = seg {
            p.extend(egui::Shape::dashed_line(&[*a, *b], st, h / 14.0, h / 22.0));
        }
    }
    p.line_segment([pos2(page.right() - fold, page.top()), pos2(page.right() - fold, page.top() + fold)], st);
    p.line_segment([pos2(page.right() - fold, page.top() + fold), pos2(page.right(), page.top() + fold)], st);
    let red = egui::Color32::from_rgb(0xd9, 0x3f, 0x3f);
    let c = pos2(page.right() - w * 0.12, page.bottom() - w * 0.12);
    let rad = w * 0.3;
    p.circle_filled(c, rad, red);
    p.circle_stroke(c, rad, Stroke::new(2.0, egui::Color32::WHITE));
    p.text(c, egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(rad * 1.5), egui::Color32::WHITE);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bitmap_decodes_bottom_up_with_its_palette() {
        // 2×2, 8 bits, two palette colours: bottom row red/green, top row green/red.
        let mut b = Vec::new();
        b.extend(40u32.to_le_bytes());
        b.extend(2i32.to_le_bytes());
        b.extend(2i32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(8u16.to_le_bytes());
        b.extend([0u8; 16]);
        b.extend(2u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend([0, 0, 255, 0, 0, 255, 0, 0]); // BGRA: red, green
        b.extend([0, 1, 0, 0, 1, 0, 0, 0]);
        let (w, h, px) = super::dib(&b).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&px[0..4], &[0, 255, 0, 255], "top-left is the last row in the file");
        assert_eq!(&px[8..12], &[255, 0, 0, 255]);
        assert!(super::dib(&b[..20]).is_none());
        assert!(super::finish((1, 1, vec![9, 9, 9, 255])).is_none(), "a blank preview is no preview");
    }
}
