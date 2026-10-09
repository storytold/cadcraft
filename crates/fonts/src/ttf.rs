//! TrueType/OpenType text: system fonts found at run time (never bundled), glyph outlines as
//! closed polylines in CAD text units (text height = cap height).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cadcraft_geom::Vec2;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

use crate::{Run, Shaped};

/// Font bytes by lower-case family/file stem.
type Db = HashMap<String, Arc<Vec<u8>>>;

fn db() -> &'static Mutex<Option<Db>> {
    static DB: OnceLock<Mutex<Option<Db>>> = OnceLock::new();
    DB.get_or_init(|| Mutex::new(None))
}

#[cfg(not(target_arch = "wasm32"))]
fn scan() -> Db {
    let mut out = Db::new();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(Into::into));
        if let Some(h) = std::env::var_os("HOME") {
            dirs.push(std::path::PathBuf::from(h).join("Library/Fonts"));
        }
    } else if cfg!(windows) {
        dirs.push("C:\\Windows\\Fonts".into());
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into));
        if let Some(h) = std::env::var_os("HOME") {
            dirs.push(std::path::PathBuf::from(&h).join(".fonts"));
            dirs.push(std::path::PathBuf::from(h).join(".local/share/fonts"));
        }
    }
    let mut stack = dirs;
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > 20_000 {
                return out;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext != "ttf" && ext != "otf" {
                continue;
            }
            if let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()) {
                out.entry(stem).or_insert_with(|| Arc::new(Vec::new()));
                // Load lazily: store the path in a side table by using an empty marker.
                PATHS
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default(), p.clone());
            }
        }
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
static PATHS: OnceLock<Mutex<HashMap<String, std::path::PathBuf>>> = OnceLock::new();

#[cfg(target_arch = "wasm32")]
fn scan() -> Db {
    Db::new()
}

/// Register font bytes under a name (web builds and tests).
pub fn register(name: &str, bytes: Vec<u8>) {
    let mut g = db().lock().unwrap_or_else(PoisonError::into_inner);
    g.get_or_insert_with(Db::new).insert(name.to_ascii_lowercase(), Arc::new(bytes));
}

/// Normalise a style font name ("arial.ttf", "Arial", "ARIALBD.TTF") to a lookup key.
fn key(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    n.trim_end_matches(".ttf").trim_end_matches(".otf").trim_end_matches(".ttc").to_string()
}

/// Bytes of a font by name, if installed. Stroke-font names (".shx", empty) return `None`.
pub fn find(name: &str) -> Option<Arc<Vec<u8>>> {
    let k = key(name);
    if k.is_empty() || k.ends_with(".shx") || k == crate::BUILTIN_FONT.to_ascii_lowercase() || k == "txt" || k == "simplex" || k == "romans" {
        return None;
    }
    let mut g = db().lock().unwrap_or_else(PoisonError::into_inner);
    let d = g.get_or_insert_with(scan);
    let candidates = [k.clone(), k.replace(' ', ""), format!("{k} regular"), format!("{}-regular", k.replace(' ', ""))];
    for c in &candidates {
        if let Some(b) = d.get(c) {
            if !b.is_empty() {
                return Some(b.clone());
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let path = PATHS.get().and_then(|m| m.lock().unwrap_or_else(PoisonError::into_inner).get(c).cloned())?;
                let bytes = std::fs::read(path).ok()?;
                let arc = Arc::new(bytes);
                d.insert(c.clone(), arc.clone());
                return Some(arc);
            }
        }
    }
    None
}

/// A glyph outline in font units (at 1000 units per em), flattened, plus its advance.
struct GlyphOutline {
    contours: Vec<Vec<Vec2>>,
    advance: f64,
}

/// Cached glyph outlines keyed by (font bytes address, length, char). Fonts live for the whole
/// process in the font table, so the address identifies them.
type GlyphCache = HashMap<(usize, usize, char), Arc<GlyphOutline>>;

fn glyph_cache() -> &'static Mutex<GlyphCache> {
    static C: OnceLock<Mutex<GlyphCache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

const MAX_CACHED_GLYPHS: usize = 40_000;
const UPEM: f32 = 1000.0;

struct Pen {
    contours: Vec<Vec<Vec2>>,
    cur: Vec<Vec2>,
}

impl Pen {
    fn pt(x: f32, y: f32) -> Vec2 {
        Vec2::new(f64::from(x), f64::from(y))
    }
    fn flush(&mut self) {
        if self.cur.len() > 2 {
            if let (Some(f), Some(l)) = (self.cur.first().copied(), self.cur.last().copied())
                && f != l
            {
                self.cur.push(f);
            }
            self.contours.push(std::mem::take(&mut self.cur));
        } else {
            self.cur.clear();
        }
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.cur.push(Self::pt(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cur.push(Self::pt(x, y));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let a = self.cur.last().copied().unwrap_or_default();
        let c = Self::pt(cx0, cy0);
        let b = Self::pt(x, y);
        for i in 1..=6 {
            let t = f64::from(i) / 6.0;
            let u = 1.0 - t;
            self.cur.push(a * (u * u) + c * (2.0 * u * t) + b * (t * t));
        }
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let a = self.cur.last().copied().unwrap_or_default();
        let c0 = Self::pt(cx0, cy0);
        let c1 = Self::pt(cx1, cy1);
        let b = Self::pt(x, y);
        for i in 1..=8 {
            let t = f64::from(i) / 8.0;
            let u = 1.0 - t;
            self.cur.push(a * (u * u * u) + c0 * (3.0 * u * u * t) + c1 * (3.0 * u * t * t) + b * (t * t * t));
        }
    }
    fn close(&mut self) {
        self.flush();
    }
}

/// Cap height of a font in font units at 1000 units per em.
fn cap_height(f: &FontRef) -> f64 {
    let metrics = f.metrics(Size::new(UPEM), LocationRef::default());
    if let Some(c) = metrics.cap_height.filter(|c| *c > 0.0) {
        return f64::from(c);
    }
    // Fall back to glyph 'H' if the font doesn't store cap_height in OS/2 (e.g. OS/2 v1).
    let gid = f.charmap().map('H').unwrap_or_default();
    if gid.to_u32() != 0 {
        let mut pen = Pen { contours: Vec::new(), cur: Vec::new() };
        if let Some(g) = f.outline_glyphs().get(gid) {
            let _ = g.draw(DrawSettings::unhinted(Size::new(UPEM), LocationRef::default()), &mut pen);
            pen.flush();
            let max_y = pen.contours.iter().flatten().map(|p| p.y).fold(0.0, f64::max);
            if max_y > 10.0 {
                return max_y;
            }
        }
    }
    f64::from((metrics.ascent * 0.72).max(1.0))
}

fn glyph(font: &[u8], f: &FontRef, c: char) -> Arc<GlyphOutline> {
    let key = (font.as_ptr() as usize, font.len(), c);
    if let Some(g) = glyph_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return g.clone();
    }
    let gid = f.charmap().map(c).unwrap_or_default();
    let mut pen = Pen { contours: Vec::new(), cur: Vec::new() };
    if let Some(g) = f.outline_glyphs().get(gid) {
        let _ = g.draw(DrawSettings::unhinted(Size::new(UPEM), LocationRef::default()), &mut pen);
        pen.flush();
    }
    let advance = f64::from(f.glyph_metrics(Size::new(UPEM), LocationRef::default()).advance_width(gid).unwrap_or(UPEM * 0.5));
    let g = Arc::new(GlyphOutline { contours: pen.contours, advance });
    let mut cache = glyph_cache().lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() > MAX_CACHED_GLYPHS {
        cache.clear();
    }
    cache.insert(key, g.clone());
    g
}

/// Shape one line with a TrueType font: closed glyph contours grouped per glyph (baseline at
/// y = 0, x from 0) plus `%%u`/`%%o` decorations as strokes. Text height = cap height.
pub fn shape(font: &[u8], s: &str, height: f64, width_factor: f64, oblique: f64) -> Option<Shaped> {
    let f = FontRef::new(font).ok()?;
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let scale = h / cap_height(&f);
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let mut out = Shaped::default();
    let mut spans = Vec::new();
    let mut x = 0.0;
    for (c, under, over) in crate::decode_controls(s) {
        let g = glyph(font, &f, c);
        if !c.is_whitespace() {
            let contours: Vec<Vec<Vec2>> = g
                .contours
                .iter()
                .map(|ct| {
                    ct.iter()
                        .map(|p| {
                            let y = p.y * scale;
                            Vec2::new(x + p.x * scale * wf + y * shear, y)
                        })
                        .collect()
                })
                .collect();
            if !contours.is_empty() {
                out.glyphs.push(contours);
            }
        }
        let adv = g.advance * scale * wf;
        spans.push((x, x + adv, under, over));
        x += adv;
    }
    out.strokes.extend(crate::decorations(&spans, h));
    out.width = x;
    Some(out)
}

/// Width of one line set in a TrueType font.
pub fn width(font: &[u8], s: &str, height: f64, width_factor: f64) -> Option<f64> {
    let f = FontRef::new(font).ok()?;
    let scale = height / cap_height(&f);
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    Some(crate::decode_controls(s).iter().map(|(c, _, _)| glyph(font, &f, *c).advance * scale * wf).sum())
}

/// Lay out one line with a TrueType font: closed glyph contours (baseline at y = 0).
pub fn layout_line(font: &[u8], s: &str, height: f64, width_factor: f64, oblique: f64) -> Option<Run> {
    let sh = shape(font, s, height, width_factor, oblique)?;
    let mut strokes: Vec<Vec<Vec2>> = sh.glyphs.into_iter().flatten().collect();
    strokes.extend(sh.strokes);
    Some(Run { strokes, width: sh.width })
}
