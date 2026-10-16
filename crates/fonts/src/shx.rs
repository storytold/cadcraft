//! User-supplied compiled stroke fonts. No font files are bundled or searched in CAD bundles.
//!
//! Shape programs follow Autodesk's public Shape Descriptions / Special Codes Reference.
//! The binary record layouts are documented in `docs/shx-fonts.md`. All fixtures are synthetic.

mod program;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use cadcraft_geom::Vec2;

use crate::{Shaped, bidi};

const MAX_FILE_BYTES: usize = 8 << 20;
const MAX_PROGRAM_BYTES: usize = 8192;
const MAX_FONTS: usize = 32;
const MAX_LOADED_BYTES: usize = 64 << 20;
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILES: usize = 8192;
const MAX_CACHED_GLYPHS: usize = 4096;
const MAX_CACHED_POINTS: usize = 262_144;

/// Malformed, unsupported or excessively complex font data. Resolution keeps the fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Limit,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid SHX font",
            Self::Unsupported => "unsupported SHX font format or encoding",
            Self::Limit => "SHX font exceeds a complexity or memory limit",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
struct Glyph {
    strokes: Vec<Vec<Vec2>>,
    advance: f64,
}

#[derive(Debug, Default)]
struct GlyphCache {
    glyphs: HashMap<u16, Option<Arc<Glyph>>>,
    points: usize,
}

/// Parsed font programs, with bounded, lazy decoding shared by every placement.
#[derive(Debug)]
pub struct Font {
    cap: f64,
    unicode: bool,
    dual: bool,
    programs: HashMap<u16, Vec<u8>>,
    cache: RwLock<GlyphCache>,
    bytes: usize,
}

/// Checked cursor shared by the record reader and program interpreter.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Limit)?;
        let result = self.bytes.get(self.pos..end).ok_or(Error::Invalid)?;
        self.pos = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        self.take(1)?.first().copied().ok_or(Error::Invalid)
    }
    fn word(&mut self) -> Result<u16, Error> {
        Ok(u16::from(self.byte()?) | (u16::from(self.byte()?) << 8))
    }
    fn name(&mut self) -> Result<(), Error> {
        let n = self.bytes.get(self.pos..).and_then(|s| s.iter().position(|b| *b == 0)).ok_or(Error::Invalid)?;
        self.take(n.checked_add(1).ok_or(Error::Limit)?)?;
        Ok(())
    }
    fn done(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

impl Font {
    /// Parse shapes 1.0/1.1 and Unicode unifont 1.0. Big fonts and packed encodings are rejected.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(Error::Limit);
        }
        let mut r = Cursor::new(bytes);
        let mut font = Self { cap: 0.0, unicode: false, dual: false, programs: HashMap::new(), cache: RwLock::default(), bytes: bytes.len() };
        if bytes.starts_with(b"AutoCAD-86 unifont 1.0\r\n\x1a") {
            r.take(25)?;
            let count = r.word()?;
            if count == 0 || r.word()? != 0 {
                return Err(Error::Invalid);
            }
            let size = usize::from(r.word()?);
            let mut definition = Cursor::new(r.take(size)?);
            definition.name()?;
            font.metrics(&mut definition)?;
            if definition.byte()? != 0 {
                return Err(Error::Unsupported);
            }
            let _embedding_permission = definition.byte()?;
            if definition.byte()? != 0 || !definition.done() {
                return Err(Error::Invalid);
            }
            font.unicode = true;
            for _ in 1..count {
                let id = r.word()?;
                let size = usize::from(r.word()?);
                font.record(id, r.take(size)?)?;
            }
            if !r.done() {
                return Err(Error::Invalid);
            }
        } else if bytes.starts_with(b"AutoCAD-86 shapes 1.0\r\n\x1a") || bytes.starts_with(b"AutoCAD-86 shapes 1.1\r\n\x1a") {
            r.take(24)?;
            let first = r.word()?;
            let last = r.word()?;
            let count = usize::from(r.word()?);
            if count == 0 {
                return Err(Error::Invalid);
            }
            // Check the entire index exists before allocating from its input-derived count.
            let mut index = Cursor::new(r.take(count.checked_mul(4).ok_or(Error::Limit)?)?);
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                entries.push((index.word()?, usize::from(index.word()?)));
            }
            if entries.first().map(|e| e.0) != Some(first) || entries.last().map(|e| e.0) != Some(last) {
                return Err(Error::Invalid);
            }
            for (id, size) in entries {
                let record = r.take(size)?;
                if id == 0 {
                    let mut definition = Cursor::new(record);
                    definition.name()?;
                    font.metrics(&mut definition)?;
                    if definition.byte()? != 0 || !definition.done() {
                        return Err(Error::Invalid);
                    }
                } else {
                    font.record(id, record)?;
                }
            }
            if r.take(3)? != b"EOF" || !r.done() {
                return Err(Error::Invalid);
            }
        } else {
            return Err(Error::Unsupported);
        }
        if font.cap <= 0.0 || font.programs.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(font)
    }

    fn metrics(&mut self, r: &mut Cursor<'_>) -> Result<(), Error> {
        self.cap = f64::from(r.byte()?);
        let _descent = r.byte()?;
        self.dual = match r.byte()? {
            0 => false,
            2 => true,
            _ => return Err(Error::Unsupported),
        };
        Ok(())
    }

    fn record(&mut self, id: u16, bytes: &[u8]) -> Result<(), Error> {
        let mut r = Cursor::new(bytes);
        r.name()?;
        let data = bytes.get(r.pos..).ok_or(Error::Invalid)?;
        if data.is_empty() || data.last() != Some(&0) || data.len() > MAX_PROGRAM_BYTES || self.programs.contains_key(&id) {
            return Err(Error::Invalid);
        }
        self.programs.insert(id, data.to_vec());
        Ok(())
    }

    fn id(&self, c: char) -> Option<u16> {
        if !self.unicode {
            match c {
                '°' if self.programs.contains_key(&256) => return Some(256),
                '±' if self.programs.contains_key(&257) => return Some(257),
                '⌀' | 'Ø' if self.programs.contains_key(&258) => return Some(258),
                _ => {}
            }
        }
        u16::try_from(u32::from(c)).ok()
    }

    fn glyph(&self, c: char) -> Option<Arc<Glyph>> {
        let id = self.id(c)?;
        if let Some(g) = self.cache.read().unwrap_or_else(PoisonError::into_inner).glyphs.get(&id) {
            return g.clone();
        }
        let result = program::decode(self, id).ok().map(Arc::new);
        let points = result.as_ref().map_or(0, |g| g.strokes.iter().map(Vec::len).sum());
        let mut cache = self.cache.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(g) = cache.glyphs.get(&id) {
            return g.clone();
        }
        if cache.glyphs.len() < MAX_CACHED_GLYPHS && cache.points.saturating_add(points) <= MAX_CACHED_POINTS {
            cache.points += points;
            cache.glyphs.insert(id, result.clone());
        }
        result
    }

    /// Horizontal layout uses the font's cap height and pen-end advances (no added stroke gap).
    pub(crate) fn shape(&self, text: &str, height: f64, width_factor: f64, oblique: f64) -> Shaped {
        let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
        let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
        let oblique = if oblique.is_finite() { oblique } else { 0.0 };
        let shear = oblique.tan().clamp(-10.0, 10.0);
        let scale = h / self.cap;
        let chars = crate::decode_controls(text);
        let logical: Vec<char> = chars.iter().map(|v| v.0).collect();
        let mut out = Shaped::default();
        let mut spans = Vec::new();
        let mut x = 0.0;
        for (run, rtl) in bidi::visual_runs(&logical, None) {
            let indices: Box<dyn Iterator<Item = usize>> = if rtl { Box::new(run.rev()) } else { Box::new(run) };
            for i in indices {
                let Some(&(c, under, over)) = chars.get(i) else { continue };
                if bidi::is_format_control(c) {
                    continue;
                }
                let c = if rtl { bidi::mirror(c) } else { c };
                let advance = if let Some(g) = self.glyph(c) {
                    for stroke in &g.strokes {
                        out.strokes.push(stroke.iter().map(|p| Vec2::new(x + p.x * scale * wf + p.y * scale * shear, p.y * scale)).collect());
                    }
                    g.advance * scale * wf
                } else {
                    let mut fallback = crate::shape_stroke(&c.to_string(), h, wf, oblique);
                    let advance = fallback.width + crate::stroke::GAP / crate::stroke::CAP * h * wf;
                    fallback.map(|p| p + Vec2::new(x, 0.0));
                    out.extend(fallback);
                    advance
                };
                spans.push((x, x + advance, under, over));
                x += advance;
            }
        }
        out.strokes.extend(crate::decorations(&spans, h));
        out.width = x.max(0.0);
        out
    }

    pub(crate) fn width(&self, text: &str, height: f64, width_factor: f64) -> f64 {
        if bidi::has_rtl(text) {
            // Mirrored punctuation can have a different advance in a supplied font.
            return self.shape(text, height, width_factor, 0.0).width;
        }
        let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
        let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
        crate::decode_controls(text)
            .into_iter()
            .filter(|(c, _, _)| !bidi::is_format_control(*c))
            .map(|(c, _, _)| {
                self.glyph(c).map_or_else(
                    || crate::line_width(&c.to_string(), h, wf) + crate::stroke::GAP / crate::stroke::CAP * h * wf,
                    |g| g.advance * h / self.cap * wf,
                )
            })
            .sum::<f64>()
            .max(0.0)
    }
}

#[derive(Default)]
struct Registry {
    paths: HashMap<String, PathBuf>,
    fonts: HashMap<String, Option<Arc<Font>>>,
    bytes: usize,
}

fn key(name: &str) -> Option<String> {
    let name = name.trim().rsplit(['/', '\\']).next()?;
    if name.is_empty() || name.len() > 255 {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix(".shx") {
        (!stem.is_empty() && !stem.contains(':')).then(|| stem.to_owned())
    } else if lower.contains('.') || lower.contains(':') {
        None
    } else {
        Some(lower)
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(scan()))
}

fn scan() -> Registry {
    #[cfg(target_arch = "wasm32")]
    return Registry::default();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut registry = Registry::default();
        if let Some(paths) = std::env::var_os("CADCRAFT_SHXPATH") {
            let mut files = 0usize;
            for dir in std::env::split_paths(&paths).take(32) {
                let Ok(entries) = std::fs::read_dir(dir) else { continue };
                for entry in entries.take(MAX_FILES.saturating_sub(files)) {
                    files += 1;
                    let Ok(entry) = entry else { continue };
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("shx"))
                        && let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(key)
                    {
                        registry.paths.entry(name).or_insert(path);
                    }
                }
            }
        }
        registry
    }
}

/// Register user-provided bytes (also works on WASM); never writes or embeds a font file.
pub fn register(name: &str, bytes: &[u8]) -> Result<(), Error> {
    let name = key(name).ok_or(Error::Invalid)?;
    let font = Arc::new(Font::parse(bytes)?);
    let mut registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
    let old = registry.fonts.get(&name).and_then(Option::as_ref).map_or(0, |f| f.bytes);
    let total = registry.bytes.saturating_sub(old).saturating_add(font.bytes);
    if total > MAX_LOADED_BYTES || (!registry.fonts.contains_key(&name) && registry.fonts.len() >= MAX_FONTS) {
        return Err(Error::Limit);
    }
    registry.bytes = total;
    registry.fonts.insert(name, Some(font));
    Ok(())
}

/// Resolve by file name or stem from the configured directories or registered font bytes.
pub fn find(name: &str) -> Option<Arc<Font>> {
    let name = key(name)?;
    let mut registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(font) = registry.fonts.get(&name) {
        return font.clone();
    }
    let path = registry.paths.get(&name)?;
    if registry.fonts.len() >= MAX_FONTS {
        return None;
    }
    let mut bytes = Vec::new();
    let result = std::fs::File::open(path).ok().and_then(|file| {
        file.take((MAX_FILE_BYTES + 1) as u64).read_to_end(&mut bytes).ok()?;
        if registry.bytes.saturating_add(bytes.len()) > MAX_LOADED_BYTES {
            return None;
        }
        Font::parse(&bytes).ok().map(Arc::new)
    });
    if let Some(font) = &result {
        registry.bytes += font.bytes;
    }
    registry.fonts.insert(name, result.clone());
    result
}
