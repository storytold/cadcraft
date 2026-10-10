//! TrueType/OpenType text: system fonts found at run time (never bundled), glyph outlines as
//! closed polylines in CAD text units (text height = cap height).
//!
//! Fonts are found by file stem (`arial.ttf`, `msjh.ttc`) and, failing that, by family name
//! (`Noto Sans CJK TC`) among installed files with a similar name. TrueType collections
//! (`.ttc` / `.otc`) are scanned too; a stem lookup takes their first face.
//!
//! Two process-wide settings (like AutoCAD's profile variables) control substitution:
//! - [`font_alt`] (`FONTALT`): the font used for a text style font that can't be found (an SHX
//!   file we don't read, a missing TTF). Empty = the built-in stroke font.
//! - [`fallback_fonts`] (`FONTFALLBACK`): fonts tried first for characters the text's font
//!   lacks (CJK in an SHX big font, for example), before the automatic system font search.
//!
//! On native builds both start from the `CADCRAFT_FONTALT` / `CADCRAFT_FONTFALLBACK`
//! environment variables.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use cadcraft_geom::Vec2;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::{FontRef, MetadataProvider};

use crate::{Run, Shaped};

/// One face of a font file (a TrueType collection holds several).
#[derive(Clone)]
pub struct Face {
    pub bytes: Arc<Vec<u8>>,
    /// Face index inside a collection (0 for a plain font file).
    pub index: u32,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face").field("len", &self.bytes.len()).field("index", &self.index).finish()
    }
}

impl Face {
    /// Face `index` of font file `bytes`, if it parses.
    pub fn new(bytes: Arc<Vec<u8>>, index: u32) -> Option<Face> {
        FontRef::from_index(&bytes, index).ok()?;
        Some(Face { bytes, index })
    }
    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.bytes, self.index).ok()
    }
    /// Whether the face maps `c` to a glyph.
    pub fn has_glyph(&self, c: char) -> bool {
        self.font().is_some_and(|f| f.charmap().map(c).is_some())
    }
    fn names(&self) -> Vec<String> {
        let Some(f) = self.font() else { return Vec::new() };
        [skrifa::string::StringId::FAMILY_NAME, skrifa::string::StringId::TYPOGRAPHIC_FAMILY_NAME, skrifa::string::StringId::FULL_NAME]
            .into_iter()
            .flat_map(|id| f.localized_strings(id).map(|s| norm(&s.to_string())).collect::<Vec<_>>())
            .collect()
    }
}

/// Faces of a font file: every face of a collection (capped), or the single font.
pub fn faces(bytes: &Arc<Vec<u8>>) -> Vec<Face> {
    match FileRef::new(bytes) {
        Ok(FileRef::Collection(c)) => (0..c.len().min(MAX_FACES_PER_FILE)).filter_map(|i| Face::new(bytes.clone(), i)).collect(),
        Ok(FileRef::Font(_)) => vec![Face { bytes: bytes.clone(), index: 0 }],
        Err(_) => Vec::new(),
    }
}

const MAX_FACES_PER_FILE: u32 = 32;
/// Bounds of the missing-glyph search: files loaded, bytes loaded, faces checked per character.
const MAX_FALLBACK_FILES: usize = 24;
const MAX_FALLBACK_BYTES: usize = 160 << 20;
const MAX_FALLBACK_FACES: usize = 256;
/// Files loaded by one family-name lookup.
const MAX_FAMILY_FILES: usize = 6;
const MAX_CACHED_NAMES: usize = 4096;
const MAX_CACHED_CHARS: usize = 50_000;

/// Fonts known to cover many scripts (CJK first), tried first for missing glyphs. Matched as
/// substrings of the normalised file stem.
const PREFERRED_FALLBACK: &[&str] = &[
    "notosanscjk",
    "sourcehansans",
    "notoserifcjk",
    "sourcehanserif",
    "wqyzenhei",
    "wqymicrohei",
    "droidsansfallback",
    "pingfang",
    "hiraginosans",
    "stheiti",
    "msjh",
    "msyh",
    "yugoth",
    "msgothic",
    "malgun",
    "simsun",
    "mingliu",
    "arialuni",
    "notosans",
    "dejavusans",
    "segoeui",
    "arial",
];
const WEIGHT_WORDS: &[&str] = &["thin", "light", "medium", "semibold", "bold", "black", "heavy", "italic", "oblique", "mono", "condensed"];

#[derive(Default)]
struct Db {
    scanned: bool,
    /// Font files found by the scan, by lower-case file stem.
    paths: HashMap<String, PathBuf>,
    /// Loaded or registered font file bytes, by lower-case file stem.
    files: HashMap<String, Arc<Vec<u8>>>,
    /// Name lookups already answered, found or not.
    resolved: HashMap<String, Option<Face>>,
    fallback: Fallback,
}

/// State of the missing-glyph search.
#[derive(Default)]
struct Fallback {
    /// Candidate file stems, best first (built on first use).
    order: Option<Vec<String>>,
    /// Next candidate in `order` to load.
    next: usize,
    files: usize,
    bytes: usize,
    /// Faces of the loaded candidates, in candidate order.
    faces: Vec<Face>,
    /// Character → index in `faces` (None: nothing covers it).
    by_char: HashMap<char, Option<usize>>,
}

fn lock() -> MutexGuard<'static, Db> {
    static DB: OnceLock<Mutex<Db>> = OnceLock::new();
    let mut g = DB.get_or_init(|| Mutex::new(Db::default())).lock().unwrap_or_else(PoisonError::into_inner);
    if !g.scanned {
        g.scanned = true;
        g.paths = scan();
    }
    g
}

#[cfg(not(target_arch = "wasm32"))]
fn scan() -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(Into::into));
        if let Some(h) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(h).join("Library/Fonts"));
        }
    } else if cfg!(windows) {
        dirs.push("C:\\Windows\\Fonts".into());
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
        }
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into));
        if let Some(h) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(&h).join(".fonts"));
            dirs.push(PathBuf::from(h).join(".local/share/fonts"));
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
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                continue;
            }
            if let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()) {
                out.entry(stem).or_insert(p);
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
fn scan() -> HashMap<String, PathBuf> {
    HashMap::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &std::path::Path) -> Option<Vec<u8>> {
    /// Font files larger than this are never read.
    const MAX_FILE_BYTES: u64 = 96 << 20;
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

#[cfg(target_arch = "wasm32")]
fn read_file(_path: &std::path::Path) -> Option<Vec<u8>> {
    None
}

impl Db {
    /// Bytes of the font file with this stem, loading it on first use.
    fn file(&mut self, stem: &str) -> Option<Arc<Vec<u8>>> {
        if let Some(b) = self.files.get(stem) {
            return Some(b.clone());
        }
        let bytes = Arc::new(read_file(self.paths.get(stem)?)?);
        self.files.insert(stem.to_string(), bytes.clone());
        Some(bytes)
    }

    fn lookup(&mut self, k: &str) -> Option<Face> {
        let candidates = [k.to_string(), k.replace(' ', ""), format!("{k} regular"), format!("{}-regular", k.replace(' ', ""))];
        for c in &candidates {
            if let Some(face) = self.file(c).and_then(|b| faces(&b).into_iter().next()) {
                return Some(face);
            }
        }
        self.by_family(k)
    }

    /// A face whose family or full name is `k`, among files with a similar stem.
    fn by_family(&mut self, k: &str) -> Option<Face> {
        let want = norm(k);
        if want.len() < 5 {
            return None;
        }
        let mut stems: Vec<(usize, usize, String)> = self
            .paths
            .keys()
            .chain(self.files.keys())
            .map(|s| (weight_penalty(s, &want), common_prefix(&norm(s), &want), s.clone()))
            .filter(|(_, p, _)| *p >= 5)
            .collect();
        stems.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        stems.dedup_by(|a, b| a.2 == b.2);
        for (_, _, stem) in stems.into_iter().take(MAX_FAMILY_FILES) {
            let Some(bytes) = self.file(&stem) else { continue };
            if let Some(face) = faces(&bytes).into_iter().find(|f| f.names().iter().any(|n| *n == want || format!("{n}regular") == want)) {
                return Some(face);
            }
        }
        None
    }

    /// Load the next fallback candidate file. False when none is left or a bound is reached.
    fn load_next_fallback(&mut self) -> bool {
        if self.fallback.order.is_none() {
            let stems: Vec<String> = self.paths.keys().chain(self.files.keys()).cloned().collect();
            self.fallback.order = Some(rank_fallback(&stems));
        }
        loop {
            if self.fallback.files >= MAX_FALLBACK_FILES || self.fallback.bytes >= MAX_FALLBACK_BYTES {
                return false;
            }
            let Some(stem) = self.fallback.order.as_ref().and_then(|o| o.get(self.fallback.next)).cloned() else { return false };
            self.fallback.next += 1;
            let Some(bytes) = self.file(&stem) else { continue };
            self.fallback.files += 1;
            self.fallback.bytes = self.fallback.bytes.saturating_add(bytes.len());
            let fs = faces(&bytes);
            if !fs.is_empty() {
                self.fallback.faces.extend(fs);
                return true;
            }
        }
    }

    /// Index in `fallback.faces` of the first face that has `c`.
    fn search_fallback(&mut self, c: char) -> Option<usize> {
        if let Some(r) = self.fallback.by_char.get(&c) {
            return *r;
        }
        let mut found = None;
        let mut i = 0;
        while i < MAX_FALLBACK_FACES {
            if i >= self.fallback.faces.len() && !self.load_next_fallback() {
                break;
            }
            if self.fallback.faces.get(i).is_some_and(|f| f.has_glyph(c)) {
                found = Some(i);
                break;
            }
            i += 1;
        }
        if self.fallback.by_char.len() >= MAX_CACHED_CHARS {
            self.fallback.by_char.clear();
        }
        self.fallback.by_char.insert(c, found);
        found
    }
}

/// Lower-case, without spaces, hyphens and underscores.
fn norm(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).flat_map(char::to_lowercase).collect()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// Weight / style words in a file name that the wanted name doesn't ask for.
fn weight_penalty(stem: &str, want: &str) -> usize {
    let n = norm(stem);
    WEIGHT_WORDS.iter().filter(|w| n.contains(*w) && !want.contains(*w)).count()
}

/// Order font file stems for the missing-glyph search: known wide-coverage (CJK) fonts first,
/// regular weights before bold, light and the like, then by name.
pub(crate) fn rank_fallback(stems: &[String]) -> Vec<String> {
    let mut v: Vec<(usize, usize, String)> = stems
        .iter()
        .map(|s| {
            let n = norm(s);
            let pref = PREFERRED_FALLBACK.iter().position(|p| n.contains(p)).unwrap_or(PREFERRED_FALLBACK.len());
            (pref, weight_penalty(s, ""), s.clone())
        })
        .collect();
    v.sort();
    v.dedup_by(|a, b| a.2 == b.2);
    v.into_iter().map(|(_, _, s)| s).collect()
}

/// Register font bytes under a name (web builds and tests).
pub fn register(name: &str, bytes: Vec<u8>) {
    let mut g = lock();
    g.files.insert(key(name), Arc::new(bytes));
    g.resolved.clear();
    g.fallback = Fallback::default();
}

/// Normalise a style font name ("arial.ttf", "Arial", "ARIALBD.TTF") to a lookup key.
fn key(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    n.trim_end_matches(".ttf").trim_end_matches(".otf").trim_end_matches(".ttc").trim_end_matches(".otc").to_string()
}

/// Names that mean a stroke font: never looked up as TrueType.
fn is_stroke_name(k: &str) -> bool {
    k.is_empty() || k.ends_with(".shx") || k == crate::BUILTIN_FONT.to_ascii_lowercase() || k == "txt" || k == "simplex" || k == "romans"
}

/// A face of an installed (or registered) font by file name or family name. Stroke-font names
/// (".shx", empty) return `None`.
pub fn find(name: &str) -> Option<Face> {
    let k = key(name);
    if is_stroke_name(&k) {
        return None;
    }
    let mut g = lock();
    if let Some(r) = g.resolved.get(&k) {
        return r.clone();
    }
    let r = g.lookup(&k);
    if g.resolved.len() >= MAX_CACHED_NAMES {
        g.resolved.clear();
    }
    g.resolved.insert(k, r.clone());
    r
}

struct Prefs {
    alt: String,
    fallback: Vec<String>,
}

fn prefs() -> MutexGuard<'static, Prefs> {
    static P: OnceLock<Mutex<Prefs>> = OnceLock::new();
    P.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let env = |_: &str| String::new();
        Mutex::new(Prefs { alt: clean_name(&env("CADCRAFT_FONTALT")), fallback: split_list(&env("CADCRAFT_FONTFALLBACK")) })
    })
    .lock()
    .unwrap_or_else(PoisonError::into_inner)
}

/// A font name setting: trimmed, capped, "." = none.
fn clean_name(s: &str) -> String {
    let s = s.trim();
    if s == "." { String::new() } else { s.chars().take(255).collect() }
}

fn split_list(s: &str) -> Vec<String> {
    s.split([',', ';']).map(clean_name).filter(|n| !n.is_empty()).take(16).collect()
}

/// FONTALT: the font used for a text style font that can't be found. Empty = the built-in
/// stroke font.
pub fn font_alt() -> String {
    prefs().alt.clone()
}

/// Set FONTALT ("" or "." clears it).
pub fn set_font_alt(name: &str) {
    prefs().alt = clean_name(name);
}

/// FONTFALLBACK: fonts tried first for characters a text's font lacks, comma-separated.
pub fn fallback_fonts() -> String {
    prefs().fallback.join(", ")
}

/// Set FONTFALLBACK from a comma- or semicolon-separated list ("" or "." clears it).
pub fn set_fallback_fonts(list: &str) {
    prefs().fallback = split_list(list);
}

/// Characters worth a fallback search (not spaces, controls or private-use code points).
fn wants_fallback(c: char) -> bool {
    let u = u32::from(c);
    !(c.is_whitespace() || c.is_control() || (0xE000..=0xF8FF).contains(&u) || u >= 0xF_0000 || (u & 0xFFFE) == 0xFFFE)
}

/// A face that has `c`, for text whose own font lacks it: FONTFALLBACK fonts, then FONTALT,
/// then installed fonts (wide-coverage CJK fonts first). Bounded and cached.
pub fn fallback_for(c: char) -> Option<Face> {
    if !wants_fallback(c) {
        return None;
    }
    let (list, alt) = {
        let p = prefs();
        (p.fallback.clone(), p.alt.clone())
    };
    for n in list.iter().chain(std::iter::once(&alt)) {
        if let Some(f) = find(n).filter(|f| f.has_glyph(c)) {
            return Some(f);
        }
    }
    let mut g = lock();
    let i = g.search_fallback(c)?;
    g.fallback.faces.get(i).cloned()
}

/// A glyph outline in font units (at 1000 units per em), flattened, plus its advance and the
/// font's cap height.
#[derive(Default)]
pub(crate) struct GlyphOutline {
    pub(crate) contours: Vec<Vec<Vec2>>,
    pub(crate) advance: f64,
    pub(crate) cap: f64,
    /// The font maps the character (false: the missing-glyph shape).
    mapped: bool,
}

/// Cached glyph outlines keyed by (font bytes address, length, face, char). Fonts live for the
/// whole process in the font table, so the address identifies them.
type GlyphCache = HashMap<(usize, usize, u32, char), Arc<GlyphOutline>>;

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

/// Cap height of a font in font units at 1000 units per em: the OS/2 value, else the height
/// of its "H", else an estimate from the ascent.
fn cap_height(f: &FontRef) -> f64 {
    let metrics = f.metrics(Size::new(UPEM), LocationRef::default());
    let measured = || {
        let gid = f.charmap().map('H')?;
        let b = f.glyph_metrics(Size::new(UPEM), LocationRef::default()).bounds(gid)?;
        Some(b.y_max).filter(|y| y.is_finite() && *y > 0.0)
    };
    let cap = metrics.cap_height.filter(|c| c.is_finite() && *c > 0.0).or_else(measured).unwrap_or(metrics.ascent * 0.72);
    f64::from(cap).max(1.0)
}

/// The outline of `c` in `face` (its missing-glyph shape when unmapped; empty when the face
/// doesn't parse).
pub(crate) fn glyph(face: &Face, c: char) -> Arc<GlyphOutline> {
    let key = (face.bytes.as_ptr() as usize, face.bytes.len(), face.index, c);
    if let Some(g) = glyph_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return g.clone();
    }
    let Some(f) = face.font() else { return Arc::new(GlyphOutline { cap: 1.0, ..Default::default() }) };
    let mapped = f.charmap().map(c);
    let gid = mapped.unwrap_or_default();
    let mut pen = Pen { contours: Vec::new(), cur: Vec::new() };
    if let Some(g) = f.outline_glyphs().get(gid) {
        let _ = g.draw(DrawSettings::unhinted(Size::new(UPEM), LocationRef::default()), &mut pen);
        pen.flush();
    }
    let advance = f64::from(f.glyph_metrics(Size::new(UPEM), LocationRef::default()).advance_width(gid).unwrap_or(UPEM * 0.5));
    let g = Arc::new(GlyphOutline { contours: pen.contours, advance, cap: cap_height(&f), mapped: mapped.is_some() });
    let mut cache = glyph_cache().lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() > MAX_CACHED_GLYPHS {
        cache.clear();
    }
    cache.insert(key, g.clone());
    g
}

/// The outline of `c` from a fallback font, if any installed font has it.
pub(crate) fn fallback_glyph(c: char) -> Option<Arc<GlyphOutline>> {
    let f = fallback_for(c)?;
    Some(glyph(&f, c)).filter(|g| g.mapped)
}

/// The glyph of `c` in `face`, or from a fallback font when `face` lacks it.
fn glyph_or_fallback(face: &Face, c: char) -> Arc<GlyphOutline> {
    let g = glyph(face, c);
    if g.mapped || c.is_whitespace() {
        return g;
    }
    fallback_glyph(c).unwrap_or(g)
}

/// Contours of a glyph scaled so its font's cap height is `h`, at pen position `x`.
pub(crate) fn place_glyph(g: &GlyphOutline, x: f64, h: f64, wf: f64, shear: f64) -> Vec<Vec<Vec2>> {
    let scale = h / g.cap;
    g.contours
        .iter()
        .map(|ct| {
            ct.iter()
                .map(|p| {
                    let y = p.y * scale;
                    Vec2::new(x + p.x * scale * wf + y * shear, y)
                })
                .collect()
        })
        .collect()
}

/// Shape one line with a TrueType face: closed glyph contours grouped per glyph (baseline at
/// y = 0, x from 0) plus `%%u`/`%%o` decorations as strokes. Text height = cap height.
/// Characters the face lacks come from a fallback font, scaled to its own cap height.
pub fn shape(face: &Face, s: &str, height: f64, width_factor: f64, oblique: f64) -> Option<Shaped> {
    face.font()?;
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let mut out = Shaped::default();
    let mut spans = Vec::new();
    let mut x = 0.0;
    for (c, under, over) in crate::decode_controls(s) {
        let g = glyph_or_fallback(face, c);
        if !c.is_whitespace() {
            let contours = place_glyph(&g, x, h, wf, shear);
            if !contours.is_empty() {
                out.glyphs.push(contours);
            }
        }
        let adv = g.advance * h / g.cap * wf;
        spans.push((x, x + adv, under, over));
        x += adv;
    }
    out.strokes.extend(crate::decorations(&spans, h));
    out.width = x;
    Some(out)
}

/// Width of one line set in a TrueType face.
pub fn width(face: &Face, s: &str, height: f64, width_factor: f64) -> Option<f64> {
    face.font()?;
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    Some(
        crate::decode_controls(s)
            .iter()
            .map(|(c, _, _)| {
                let g = glyph_or_fallback(face, *c);
                g.advance * height / g.cap * wf
            })
            .sum(),
    )
}

/// Lay out one line with a TrueType face: closed glyph contours (baseline at y = 0).
pub fn layout_line(face: &Face, s: &str, height: f64, width_factor: f64, oblique: f64) -> Option<Run> {
    let sh = shape(face, s, height, width_factor, oblique)?;
    let mut strokes: Vec<Vec<Vec2>> = sh.glyphs.into_iter().flatten().collect();
    strokes.extend(sh.strokes);
    Some(Run { strokes, width: sh.width })
}
