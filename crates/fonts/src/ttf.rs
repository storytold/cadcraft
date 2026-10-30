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
//! Both start empty; the host sets its starting values ([`set_font_alt`], [`set_fallback_fonts`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};

use cadcraft_geom::Vec2;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::raw::types::Tag;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::{Run, Shaped};

/// One face of a font file (a TrueType collection holds several).
#[derive(Clone)]
pub struct Face {
    pub bytes: Arc<Vec<u8>>,
    /// Face index inside a collection (0 for a plain font file).
    pub index: u32,
}

/// The same face of the same loaded file.
impl PartialEq for Face {
    fn eq(&self, other: &Face) -> bool {
        Arc::ptr_eq(&self.bytes, &other.bytes) && self.index == other.index
    }
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
    pub(crate) fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.bytes, self.index).ok()
    }
    /// Whether two handles are the same face of the same loaded file.
    pub(crate) fn same(&self, other: &Face) -> bool {
        Arc::ptr_eq(&self.bytes, &other.bytes) && self.index == other.index
    }
    /// Whether the face has OpenType Arabic joining features (`init` / `medi` / `fina`). Faces
    /// without them get Unicode presentation forms instead ([`crate::bidi::presentation_forms`]).
    pub(crate) fn has_joining_features(&self) -> bool {
        use skrifa::raw::TableProvider;
        let tags = [Tag::new(b"init"), Tag::new(b"medi"), Tag::new(b"fina")];
        self.font()
            .and_then(|f| f.gsub().ok())
            .and_then(|g| g.feature_list().ok())
            .is_some_and(|l| l.feature_records().iter().any(|r| tags.contains(&r.feature_tag())))
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
/// Further files a family-name lookup loads: those starting with the name's first word.
const MAX_FIRST_WORD_FILES: usize = 24;
/// Total bytes of system font files loaded from disk: a drawing naming many styles can't pull
/// every installed font into memory.
const MAX_LOADED_BYTES: usize = 256 << 20;
const MAX_CACHED_NAMES: usize = 4096;
const MAX_CACHED_CHARS: usize = 50_000;
/// Cumulative outline points one shaped line may retain, so an oversized expanded glyph repeated
/// across a string cannot grow the result without bound (CWE-400).
pub(crate) const MAX_SHAPED_POINTS: usize = 8_000_000;

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
/// Fonts known to cover a right-to-left script, tried before the general search for its
/// characters (the general search loads a bounded number of files, and wide-coverage CJK fonts
/// come first in it). Matched as substrings of the normalised file stem.
const ARABIC_FALLBACK: &[&str] = &[
    "vazirmatn",
    "vazir",
    "notonaskharabic",
    "notosansarabic",
    "notokufiarabic",
    "amiri",
    "sahel",
    "samim",
    "geezapro",
    "segoeui",
    "tahoma",
    "arial",
    "dejavusans",
];
const HEBREW_FALLBACK: &[&str] = &["notosanshebrew", "notoserifhebrew", "arial", "tahoma", "segoeui", "dejavusans"];
/// Script-specific fallback files loaded per character, at most.
const MAX_SCRIPT_FILES: usize = 4;

/// Script-specific fallback font names for `c`, if any.
fn script_fallback(c: char) -> &'static [&'static str] {
    match c {
        '\u{0600}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}' => ARABIC_FALLBACK,
        '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}' => HEBREW_FALLBACK,
        _ => &[],
    }
}

const WEIGHT_WORDS: &[&str] = &["thin", "light", "medium", "semibold", "bold", "black", "heavy", "italic", "oblique", "mono", "condensed"];

#[derive(Default)]
struct Db {
    scanned: bool,
    /// Font files found by the scan and fonts compiled into the binary, by lower-case file stem.
    paths: HashMap<String, Source>,
    /// Loaded or registered font file bytes, by lower-case file stem.
    files: HashMap<String, Arc<Vec<u8>>>,
    /// Name lookups already answered, found or not.
    resolved: HashMap<String, Option<Face>>,
    fallback: Fallback,
    /// Bytes of font files loaded from disk so far (bounded by `MAX_LOADED_BYTES`).
    loaded_bytes: usize,
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

/// Where a font file's bytes come from.
enum Source {
    /// An installed font file, read on first use.
    File(PathBuf),
    /// A font compiled into the binary ([`crate::CRAFT_FONTS`]), copied on first use.
    Static(&'static [u8]),
}

fn lock() -> MutexGuard<'static, Db> {
    static DB: OnceLock<Mutex<Db>> = OnceLock::new();
    let mut g = DB.get_or_init(|| Mutex::new(Db::default())).lock().unwrap_or_else(PoisonError::into_inner);
    if !g.scanned {
        g.scanned = true;
        g.paths = scan().into_iter().map(|(stem, p)| (stem, Source::File(p))).collect();
        // Embedded craft-fonts fonts take the place of an installed file with the same stem.
        for f in crate::CRAFT_FONTS {
            g.paths.insert(f.stem(), Source::Static(f.bytes));
        }
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
        if self.loaded_bytes >= MAX_LOADED_BYTES {
            return None;
        }
        let bytes = Arc::new(match self.paths.get(stem)? {
            Source::File(path) => read_file(path)?,
            Source::Static(bytes) => bytes.to_vec(),
        });
        self.loaded_bytes = self.loaded_bytes.saturating_add(bytes.len());
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

    /// A face whose family or full name is `k`, among files with a similar stem, then among the
    /// files whose stem begins with the name's first word (`GOST.TTF` for "GOST type A").
    fn by_family(&mut self, k: &str) -> Option<Face> {
        let want = norm(k);
        if want.len() < 5 {
            return None;
        }
        let stems: Vec<String> = self.paths.keys().chain(self.files.keys()).cloned().collect();
        for stem in family_candidates(&stems, &want, &norm(k.split_whitespace().next().unwrap_or(""))) {
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

    /// Index in `fallback.faces` of a face of a font known to cover `c`'s script (loaded and
    /// added on demand), if one is installed and has `c`.
    fn search_script_fallback(&mut self, c: char) -> Option<usize> {
        let names = script_fallback(c);
        if names.is_empty() {
            return None;
        }
        let mut stems: Vec<(usize, usize, String)> = self
            .paths
            .keys()
            .chain(self.files.keys())
            .filter_map(|s| {
                let n = norm(s);
                names.iter().position(|p| n.contains(p)).map(|pref| (weight_penalty(s, ""), pref, s.clone()))
            })
            .collect();
        stems.sort();
        stems.dedup_by(|a, b| a.2 == b.2);
        for (_, _, stem) in stems.into_iter().take(MAX_SCRIPT_FILES) {
            let Some(bytes) = self.file(&stem) else { continue };
            let Some(face) = faces(&bytes).into_iter().find(|f| f.has_glyph(c)) else { continue };
            if let Some(i) = self.fallback.faces.iter().position(|f| f.same(&face)) {
                return Some(i);
            }
            if self.fallback.faces.len() >= MAX_FALLBACK_FACES {
                return None;
            }
            self.fallback.faces.push(face);
            return Some(self.fallback.faces.len() - 1);
        }
        None
    }

    /// Index in `fallback.faces` of the first face that has `c`.
    fn search_fallback(&mut self, c: char) -> Option<usize> {
        if let Some(r) = self.fallback.by_char.get(&c) {
            return *r;
        }
        let mut found = self.search_script_fallback(c);
        let mut i = 0;
        while found.is_none() && i < MAX_FALLBACK_FACES {
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

/// The font file stems to check for the family `want` (normalised), best first: those sharing a
/// long prefix with it ([`MAX_FAMILY_FILES`]), then those starting with its `first` word
/// ([`MAX_FIRST_WORD_FILES`]).
fn family_candidates(stems: &[String], want: &str, first: &str) -> Vec<String> {
    let mut by_prefix: Vec<(usize, usize, String)> =
        stems.iter().map(|s| (weight_penalty(s, want), common_prefix(&norm(s), want), s.clone())).filter(|(_, p, _)| *p >= 5).collect();
    by_prefix.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    by_prefix.dedup_by(|a, b| a.2 == b.2);
    let mut out: Vec<String> = by_prefix.into_iter().take(MAX_FAMILY_FILES).map(|(_, _, s)| s).collect();
    if first.chars().count() >= 4 {
        let mut by_word: Vec<(usize, usize, String)> =
            stems.iter().filter(|s| norm(s).starts_with(first) && !out.contains(s)).map(|s| (weight_penalty(s, want), s.len(), s.clone())).collect();
        by_word.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        by_word.dedup_by(|a, b| a.2 == b.2);
        out.extend(by_word.into_iter().take(MAX_FIRST_WORD_FILES).map(|(_, _, s)| s));
    }
    out
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
    let embedded_rank = PREFERRED_FALLBACK.iter().position(|p| *p == "notosans").unwrap_or(PREFERRED_FALLBACK.len());
    let mut v: Vec<(usize, usize, String)> = stems
        .iter()
        .map(|s| {
            let n = norm(s);
            let pref = PREFERRED_FALLBACK.iter().position(|p| n.contains(p)).unwrap_or(PREFERRED_FALLBACK.len());
            // Embedded craft-fonts fonts come right after the installed wide-coverage (CJK) fonts,
            // before the general-purpose ones, so the bounded search always reaches them.
            let pref = if crate::craft_fonts::is_embedded(s) { pref.min(embedded_rank) } else { pref };
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

/// Register font bytes that live for the whole process under a name; they are copied only when
/// the font is first used, the way the embedded craft-fonts fonts are listed.
#[cfg(test)]
pub(crate) fn register_static(name: &str, bytes: &'static [u8]) {
    let mut g = lock();
    let k = key(name);
    g.files.remove(&k);
    g.paths.insert(k, Source::Static(bytes));
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
    static P: Mutex<Prefs> = Mutex::new(Prefs { alt: String::new(), fallback: Vec::new() });
    P.lock().unwrap_or_else(PoisonError::into_inner)
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
    pub(crate) contours: Arc<[Vec<Vec2>]>,
    points: usize,
    pub(crate) advance: f64,
    pub(crate) cap: f64,
    /// The font maps the character (false: the missing-glyph shape).
    mapped: bool,
    /// Its glyph id and face, for output that keeps text as text ([`crate::TextGlyph`]).
    gid: u32,
    face: Option<Face>,
}

/// A glyph by character (through the face's character map) or by glyph id (shaper output).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum GlyphKey {
    Char(char),
    Id(u32),
}

/// Cached glyph outlines keyed by (font bytes address, length, face, glyph). Fonts live for the
/// whole process in the font table, so the address identifies them. A read-write lock: display
/// lists are built on all cores and nearly every lookup is a hit, which a mutex serialized.
#[derive(Default)]
struct GlyphCache {
    glyphs: HashMap<(usize, usize, u32, GlyphKey), Arc<GlyphOutline>>,
    points: usize,
}

/// Return the process-wide outline cache whose retained glyph and point counts are bounded during insertion.
fn glyph_cache() -> &'static RwLock<GlyphCache> {
    static C: OnceLock<RwLock<GlyphCache>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(GlyphCache::default()))
}

const MAX_CACHED_GLYPHS: usize = 40_000;
const UPEM: f32 = 1000.0;

#[derive(Default)]
struct Pen {
    contours: Vec<Vec<Vec2>>,
    cur: Vec<Vec2>,
    points: usize,
    truncated: bool,
}

const MAX_OUTLINE_POINTS: usize = 65_536;

impl Pen {
    /// Append one flattened outline point, discarding all contours if the per-glyph point limit is exceeded.
    fn push(&mut self, point: Vec2) {
        if self.truncated {
            return;
        }
        if self.points == MAX_OUTLINE_POINTS {
            // An incomplete contour is not safe to fill; retain only the advance for this glyph.
            self.truncated = true;
            self.cur.clear();
            self.contours.clear();
            return;
        }
        self.points += 1;
        self.cur.push(point);
    }

    fn pt(x: f32, y: f32) -> Vec2 {
        Vec2::new(f64::from(x), f64::from(y))
    }
    /// Close and retain a nondegenerate contour only if its closing point also fits the outline budget.
    fn flush(&mut self) {
        if self.cur.len() > 2 {
            if let (Some(f), Some(l)) = (self.cur.first().copied(), self.cur.last().copied())
                && f != l
            {
                self.push(f);
            }
            if !self.truncated {
                self.contours.push(std::mem::take(&mut self.cur));
            }
        } else {
            self.cur.clear();
        }
    }
}

impl OutlinePen for Pen {
    /// Finish the preceding contour and begin a new one through the budgeted point writer.
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.push(Self::pt(x, y));
    }
    /// Append a line endpoint through the budgeted point writer.
    fn line_to(&mut self, x: f32, y: f32) {
        self.push(Self::pt(x, y));
    }
    /// Flatten a quadratic segment into six samples, charging each retained point to the glyph budget.
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let a = self.cur.last().copied().unwrap_or_default();
        let c = Self::pt(cx0, cy0);
        let b = Self::pt(x, y);
        for i in 1..=6 {
            let t = f64::from(i) / 6.0;
            let u = 1.0 - t;
            self.push(a * (u * u) + c * (2.0 * u * t) + b * (t * t));
        }
    }
    /// Flatten a cubic segment into eight samples, charging each retained point to the glyph budget.
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let a = self.cur.last().copied().unwrap_or_default();
        let c0 = Self::pt(cx0, cy0);
        let c1 = Self::pt(cx1, cy1);
        let b = Self::pt(x, y);
        for i in 1..=8 {
            let t = f64::from(i) / 8.0;
            let u = 1.0 - t;
            self.push(a * (u * u * u) + c0 * (3.0 * u * u * t) + c1 * (3.0 * u * t * t) + b * (t * t * t));
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
    outline(face, GlyphKey::Char(c))
}

/// The outline of glyph `gid` of `face` (as a shaper returns it); its advance is the font's
/// nominal one.
pub(crate) fn glyph_by_id(face: &Face, gid: u32) -> Arc<GlyphOutline> {
    outline(face, GlyphKey::Id(gid))
}

/// Closed, flattened contours of a glyph at 1000 font units per em. Shared with the outline
/// cache so renderers can tessellate once before applying each occurrence's affine placement.
pub fn glyph_contours(face: &Face, gid: u16) -> Arc<[Vec<Vec2>]> {
    glyph_by_id(face, u32::from(gid)).contours.clone()
}

/// Load or build a glyph outline with bounded flattening, retaining its advance even when contours are discarded.
/// Cache insertion clears existing entries when the glyph-count or aggregate point allowance would be exceeded.
fn outline(face: &Face, k: GlyphKey) -> Arc<GlyphOutline> {
    let key = (face.bytes.as_ptr() as usize, face.bytes.len(), face.index, k);
    if let Some(g) = glyph_cache().read().unwrap_or_else(PoisonError::into_inner).glyphs.get(&key) {
        return g.clone();
    }
    let Some(f) = face.font() else { return Arc::new(GlyphOutline { cap: 1.0, ..Default::default() }) };
    let mapped = match k {
        GlyphKey::Char(c) => f.charmap().map(c),
        GlyphKey::Id(id) => Some(GlyphId::new(id)),
    };
    let gid = mapped.unwrap_or_default();
    let mut pen = Pen::default();
    if let Some(g) = f.outline_glyphs().get(gid) {
        let _ = g.draw(DrawSettings::unhinted(Size::new(UPEM), LocationRef::default()), &mut pen);
        pen.flush();
    }
    let advance = f64::from(f.glyph_metrics(Size::new(UPEM), LocationRef::default()).advance_width(gid).unwrap_or(UPEM * 0.5));
    let (cap, mapped, gid) = (cap_height(&f), mapped.is_some(), gid.to_u32());
    let points = pen.contours.iter().map(Vec::len).sum();
    let g = Arc::new(GlyphOutline { points, contours: pen.contours.into(), advance, cap, mapped, gid, face: Some(face.clone()) });
    let mut cache = glyph_cache().write().unwrap_or_else(PoisonError::into_inner);
    if cache.glyphs.len() >= MAX_CACHED_GLYPHS || cache.points.saturating_add(points) > MAX_SHAPED_POINTS {
        cache.glyphs.clear();
        cache.points = 0;
    }
    if let Some(old) = cache.glyphs.insert(key, g.clone()) {
        cache.points -= old.points;
    }
    cache.points += points;
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

/// The [`crate::TextGlyph`] of a glyph placed like [`place_glyph`] does (its outline index is
/// set when it is added). `None` for the missing-glyph shape.
pub(crate) fn text_glyph(g: &GlyphOutline, c: char, x: f64, h: f64, wf: f64, shear: f64) -> Option<crate::TextGlyph> {
    // Font units at 1000 per em scale by h / cap.
    text_glyph_at(g, c, Vec2::new(x, 0.0), h / g.cap, wf, shear)
}

/// [`text_glyph`] with the glyph's origin at `origin` and font units scaled by `scale`.
pub(crate) fn text_glyph_at(g: &GlyphOutline, c: char, origin: Vec2, scale: f64, wf: f64, shear: f64) -> Option<crate::TextGlyph> {
    let gid = u16::try_from(g.gid).ok().filter(|_| g.mapped)?;
    let face = g.face.clone()?;
    let em = 1000.0 * scale;
    (em.is_finite() && em > 0.0).then(|| crate::TextGlyph {
        face,
        gid,
        ch: c,
        outline: 0,
        origin,
        x: Vec2::new(em * wf, 0.0),
        y: Vec2::new(em * shear, em),
    })
}

/// Charge source outline points before copying and transforming a glyph.
/// Return `None` without spending fuel when it cannot fit; empty outlines remain valid at zero budget.
pub(crate) fn place_glyph_budgeted(g: &GlyphOutline, x: f64, h: f64, wf: f64, shear: f64, points_left: &mut usize) -> Option<Vec<Vec<Vec2>>> {
    *points_left = points_left.checked_sub(g.points)?;
    Some(place_glyph(g, x, h, wf, shear))
}

/// Contours of a glyph scaled so its font's cap height is `h`, at pen position `x`.
pub(crate) fn place_glyph(g: &GlyphOutline, x: f64, h: f64, wf: f64, shear: f64) -> Vec<Vec<Vec2>> {
    place_glyph_at(g, Vec2::new(x, 0.0), h / g.cap, wf, shear)
}

/// Contours of a glyph with its origin at `origin` and font units scaled by `scale`; the shear
/// slants about the baseline (y = 0).
pub(crate) fn place_glyph_at(g: &GlyphOutline, origin: Vec2, scale: f64, wf: f64, shear: f64) -> Vec<Vec<Vec2>> {
    g.contours
        .iter()
        .map(|ct| {
            ct.iter()
                .map(|p| {
                    let y = origin.y + p.y * scale;
                    Vec2::new(origin.x + p.x * scale * wf + y * shear, y)
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
    if crate::bidi::has_rtl(s)
        && let Some(sh) = crate::complex::shape(Some(face), s, height, width_factor, oblique)
    {
        return Some(sh);
    }
    let h = if height.is_finite() && height > 0.0 { height } else { 1.0 };
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    let shear = oblique.tan().clamp(-10.0, 10.0);
    let mut out = Shaped::default();
    let mut spans = Vec::new();
    let mut x = 0.0;
    // Cumulative retained-point budget: a compact CFF/OTF font can expand one glyph into a huge
    // outline, and a short repeated string copies it per occurrence, so cap the total geometry
    // kept in the Shaped result (CWE-400). Advances still accumulate so the width stays sensible.
    let mut points_left: usize = MAX_SHAPED_POINTS;
    for (c, under, over) in crate::decode_controls(s) {
        if crate::bidi::is_format_control(c) {
            continue;
        }
        let g = glyph_or_fallback(face, c);
        if !c.is_whitespace()
            && let Some(contours) = place_glyph_budgeted(&g, x, h, wf, shear, &mut points_left)
        {
            // Even an empty mapped glyph needs a valid outline slot for text export.
            out.push_glyph(contours, text_glyph(&g, c, x, h, wf, shear));
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
    if crate::bidi::has_rtl(s)
        && let Some(sh) = crate::complex::shape(Some(face), s, height, width_factor, 0.0)
    {
        return Some(sh.width);
    }
    let wf = if width_factor.is_finite() && width_factor.abs() > 1e-6 { width_factor } else { 1.0 };
    Some(
        crate::decode_controls(s)
            .iter()
            .filter(|(c, _, _)| !crate::bidi::is_format_control(*c))
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

#[cfg(test)]
mod family_tests {
    use super::*;

    #[test]
    fn a_family_is_looked_for_in_files_starting_with_its_first_word() {
        let stems: Vec<String> = ["gost", "gost2304a", "gost common", "arial", "isocpeur"].map(String::from).to_vec();
        let c = family_candidates(&stems, &norm("gost type a"), "gost");
        assert!(c.contains(&"gost".to_string()), "GOST.TTF holds \"GOST type A\": {c:?}");
        assert!(!c.contains(&"arial".to_string()));
        assert!(family_candidates(&stems, &norm("gost type a"), "go").is_empty(), "too short a word matches nothing");
        let many: Vec<String> = (0..100).map(|i| format!("gost{i:03}")).collect();
        assert!(family_candidates(&many, "gosttypea", "gost").len() <= MAX_FAMILY_FILES + MAX_FIRST_WORD_FILES);
    }
}

#[cfg(test)]
mod geometry_budget_tests {
    use super::*;
    /// Placement checks the source point cost before copying and accepts empty outlines without spending points.
    #[test]
    fn placement_checks_source_size_and_accepts_empty_glyphs() {
        let glyph = GlyphOutline { contours: vec![vec![Vec2::ZERO; 4]].into(), points: 4, cap: 1.0, ..Default::default() };
        let mut budget = 3;
        assert!(place_glyph_budgeted(&glyph, 0.0, 1.0, 1.0, 0.0, &mut budget).is_none());
        assert_eq!(budget, 3);
        let empty = GlyphOutline { cap: 1.0, ..Default::default() };
        assert_eq!(place_glyph_budgeted(&empty, 0.0, 1.0, 1.0, 0.0, &mut 0), Some(Vec::new()));
        let mut budget = 4;
        assert!(place_glyph_budgeted(&glyph, 0.0, 1.0, 1.0, 0.0, &mut budget).is_some());
        assert_eq!(budget, 0);
        assert!(place_glyph_budgeted(&glyph, 0.0, 1.0, 1.0, 0.0, &mut budget).is_none());
    }
    /// Exceeding the per-glyph flattening limit discards all contour geometry before it reaches the cache.
    #[test]
    fn flattened_outlines_are_bounded_before_caching() {
        let mut pen = Pen::default();
        for _ in 0..MAX_OUTLINE_POINTS {
            pen.line_to(0.0, 0.0);
        }
        pen.line_to(1.0, 1.0);
        pen.close();
        assert!(pen.truncated);
        assert!(pen.contours.is_empty() && pen.cur.is_empty());
        assert_eq!(pen.points, MAX_OUTLINE_POINTS);
    }
    /// A mapped empty glyph retains text metadata referring to an existing empty outline slot.
    #[test]
    fn mapped_empty_glyph_keeps_text_and_a_valid_outline_slot() {
        // Metadata placement does not parse the face; an empty face suffices to check the index contract.
        let glyph = GlyphOutline { cap: 1.0, mapped: true, gid: 1, face: Some(Face { bytes: Arc::new(Vec::new()), index: 0 }), ..Default::default() };
        let contours = place_glyph_budgeted(&glyph, 0.0, 1.0, 1.0, 0.0, &mut 0).unwrap();
        let mut shaped = Shaped::default();
        shaped.push_glyph(contours, text_glyph(&glyph, 'x', 0.0, 1.0, 1.0, 0.0));
        assert_eq!(shaped.text.len(), 1);
        assert_eq!(shaped.text[0].outline, 0);
        assert!(shaped.glyphs[shaped.text[0].outline].is_empty());
    }

    /// An empty glyph with no text mapping leaves the shaped result empty.
    #[test]
    fn unmapped_empty_glyph_does_not_make_the_shape_nonempty() {
        let glyph = GlyphOutline { cap: 1.0, ..Default::default() };
        let contours = place_glyph_budgeted(&glyph, 0.0, 1.0, 1.0, 0.0, &mut 0).unwrap();
        let mut shaped = Shaped::default();
        shaped.push_glyph(contours, text_glyph(&glyph, 'x', 0.0, 1.0, 1.0, 0.0));
        assert!(shaped.is_empty());
        assert!(shaped.text.is_empty());
    }

    /// Missing text metadata does not discard a glyph that has drawable contours.
    #[test]
    fn an_unmapped_glyph_with_contours_still_draws() {
        let mut shaped = Shaped::default();
        let contours = vec![vec![Vec2::ZERO, Vec2::new(1.0, 1.0)]];
        shaped.push_glyph(contours.clone(), None);
        assert!(!shaped.is_empty());
        assert_eq!(shaped.glyphs, vec![contours]);
        assert!(shaped.text.is_empty());
    }
}
