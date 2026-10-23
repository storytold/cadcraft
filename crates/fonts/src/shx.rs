//! SHX shape fonts: compiled shape files (`shapes 1.0` / `shapes 1.1`) and Unicode shape fonts
//! (`unifont 1.0`), read from the user's own files at run time (never bundled).
//!
//! A shape is a byte program for a pen: vectors in sixteen directions, pen up / down, scale,
//! a position stack, subshapes, displacements, octant, fractional and bulge arcs (the shape
//! description codes of the published shape definition format). A font is a shape file whose
//! shape 0 gives the height above and below the baseline; a character's shape number is its
//! character code, and the pen position where its program ends is its advance.
//!
//! Files are found by name ("romans.shx", "romans") in the folders of [`font_path`]
//! (`FONTPATH`, starting from the `CADCRAFT_FONTPATH` environment variable on native builds), or
//! among fonts added with [`register`] (web builds and tests). Big fonts (`bigfont 1.0`) are not
//! read yet.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

/// SHX files larger than this are never read.
const MAX_FILE_BYTES: usize = 32 << 20;
const MAX_SHAPES: usize = 65_536;
/// Nested subshapes (code 7); a shape naming itself stops here.
const MAX_SUBSHAPE_DEPTH: usize = 8;
const MAX_STACK: usize = 32;
/// Bounds of one glyph: commands run (subshapes included) and points drawn.
const MAX_OPS: usize = 100_000;
const MAX_GLYPH_POINTS: usize = 20_000;
const MAX_CACHED_GLYPHS: usize = 8192;
const MAX_CACHED_NAMES: usize = 1024;
const MAX_PATH_DIRS: usize = 16;
#[cfg(not(target_arch = "wasm32"))]
const MAX_DIR_ENTRIES: usize = 20_000;

/// The file layout, which also sets the width of a subshape number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `shapes 1.0` / `shapes 1.1`: shape numbers 1 to 255.
    Shapes,
    /// `unifont 1.0`: shape numbers are Unicode code points (16 bits).
    Unifont,
}

/// Why an SHX file wasn't loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShxError {
    /// Not an SHX file (no `AutoCAD-86` header).
    NotShx,
    /// A big font (`bigfont 1.0`), which CADCraft doesn't read yet.
    BigFont,
    /// The file ends before its tables do.
    Truncated,
    /// A shape file without shape 0: shapes, but no text font.
    NotAFont,
    TooLarge,
}

impl std::fmt::Display for ShxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ShxError::NotShx => "not an SHX file",
            ShxError::BigFont => "SHX big fonts are not available yet",
            ShxError::Truncated => "the SHX file is truncated",
            ShxError::NotAFont => "the SHX file holds shapes but no text font",
            ShxError::TooLarge => "the SHX file is too large",
        })
    }
}

impl std::error::Error for ShxError {}

/// One character of a font, in font units (baseline at y = 0, pen starting at x = 0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    pub strokes: Vec<Vec<(f64, f64)>>,
    /// Where the pen ends: the start of the next character.
    pub advance: f64,
}

/// A loaded SHX text font.
pub struct Font {
    kind: Kind,
    name: String,
    above: f64,
    below: f64,
    shapes: HashMap<u32, Vec<u8>>,
    cache: Mutex<HashMap<u32, Option<Arc<Glyph>>>>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font").field("kind", &self.kind).field("name", &self.name).field("shapes", &self.shapes.len()).finish()
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at.checked_add(1)?)?]))
}

/// The shape program of a definition: what follows its NUL-terminated name.
fn after_name(def: &[u8]) -> &[u8] {
    def.iter().position(|b| *b == 0).and_then(|n| def.get(n + 1..)).unwrap_or_default()
}

fn name_of(def: &[u8]) -> String {
    def.iter().take_while(|b| **b != 0).take(255).map(|b| char::from(*b)).collect()
}

impl Font {
    /// Read an SHX text font. A file cut short keeps the shapes read so far.
    pub fn parse(bytes: &[u8]) -> Result<Font, ShxError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(ShxError::TooLarge);
        }
        // "AutoCAD-86 <kind> <version>", CR LF, 0x1A.
        let head = bytes.get(..bytes.len().min(64)).unwrap_or_default();
        let end = head.iter().position(|b| *b == 0x1A).ok_or(ShxError::NotShx)?;
        let line = String::from_utf8_lossy(head.get(..end).unwrap_or_default()).to_ascii_lowercase();
        if !line.starts_with("autocad-86") {
            return Err(ShxError::NotShx);
        }
        let body = bytes.get(end + 1..).unwrap_or_default();
        if line.contains("bigfont") {
            Err(ShxError::BigFont)
        } else if line.contains("unifont") {
            Font::parse_unifont(body)
        } else if line.contains("shapes") {
            Font::parse_shapes(body)
        } else {
            Err(ShxError::NotShx)
        }
    }

    /// First and last shape number, shape count, a `(number, length)` index, the definitions.
    fn parse_shapes(b: &[u8]) -> Result<Font, ShxError> {
        let count = usize::from(u16_at(b, 4).ok_or(ShxError::Truncated)?);
        let mut at = 6usize.saturating_add(count.saturating_mul(4));
        let mut shapes = HashMap::new();
        let mut header: Option<Vec<u8>> = None;
        for i in 0..count {
            let (Some(num), Some(len)) = (u16_at(b, 6 + i * 4), u16_at(b, 8 + i * 4)) else { break };
            let Some(def) = b.get(at..at.saturating_add(usize::from(len))) else { break };
            at = at.saturating_add(usize::from(len));
            if num == 0 {
                header = Some(def.to_vec());
            } else {
                shapes.insert(u32::from(num), after_name(def).to_vec());
            }
        }
        let header = header.ok_or(if shapes.is_empty() && count > 0 { ShxError::Truncated } else { ShxError::NotAFont })?;
        Ok(Font::new(Kind::Shapes, &header, shapes))
    }

    /// Shape count, the length of the font description, the description, then
    /// `(number, length, definition)` records.
    fn parse_unifont(b: &[u8]) -> Result<Font, ShxError> {
        let info_len = usize::from(u16_at(b, 4).ok_or(ShxError::Truncated)?);
        let header = b.get(6..6usize.saturating_add(info_len)).ok_or(ShxError::Truncated)?;
        let mut at = 6usize.saturating_add(info_len);
        let mut shapes = HashMap::new();
        while shapes.len() < MAX_SHAPES {
            let (Some(num), Some(len)) = (u16_at(b, at), u16_at(b, at.saturating_add(2))) else { break };
            let start = at.saturating_add(4);
            let Some(def) = b.get(start..start.saturating_add(usize::from(len))) else { break };
            at = start.saturating_add(usize::from(len));
            if num != 0 {
                shapes.insert(u32::from(num), after_name(def).to_vec());
            }
        }
        Ok(Font::new(Kind::Unifont, header, shapes))
    }

    /// `header`: the font name, NUL, then the heights above and below the baseline.
    fn new(kind: Kind, header: &[u8], shapes: HashMap<u32, Vec<u8>>) -> Font {
        let metrics = after_name(header);
        let above = metrics.first().copied().filter(|a| *a > 0).map_or(1.0, f64::from);
        let below = metrics.get(1).copied().map_or(0.0, f64::from);
        Font { kind, name: name_of(header), above, below, shapes, cache: Mutex::new(HashMap::new()) }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }
    /// The font's own description (the name of shape 0).
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Height of a capital letter above the baseline in font units: text height maps to it.
    pub fn above(&self) -> f64 {
        self.above
    }
    /// Depth of descenders below the baseline, in font units.
    pub fn below(&self) -> f64 {
        self.below
    }
    /// Number of shapes (characters) in the font.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// The shape number of `c`. The `%%d`, `%%p` and `%%c` symbols sit at 127, 128 and 129 in
    /// fonts that don't have them at their Unicode code points.
    fn code(&self, c: char) -> Option<u32> {
        let u = u32::from(c);
        let limit = if self.kind == Kind::Shapes { 256 } else { 0x1_0000 };
        if u < limit && self.shapes.contains_key(&u) {
            return Some(u);
        }
        let alt = match c {
            '°' => 127,
            '±' => 128,
            '⌀' | 'Ø' => 129,
            _ => return None,
        };
        self.shapes.contains_key(&alt).then_some(alt)
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.code(c).is_some()
    }

    /// The strokes and advance of `c`, or `None` when the font has no such character.
    pub fn glyph(&self, c: char) -> Option<Arc<Glyph>> {
        let code = self.code(c)?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(g) = cache.get(&code) {
            return g.clone();
        }
        let g = self.shapes.get(&code).map(|program| {
            let mut pen = Pen::default();
            self.run(program, &mut pen, 0);
            pen.flush();
            Arc::new(Glyph { strokes: pen.strokes, advance: pen.pos.0 })
        });
        if cache.len() >= MAX_CACHED_GLYPHS {
            cache.clear();
        }
        cache.insert(code, g.clone());
        g
    }

    /// Run a shape program. Horizontal text only: a command behind code 14 is for vertical
    /// text, so its operands are read and it is skipped.
    fn run(&self, program: &[u8], pen: &mut Pen, depth: usize) {
        let mut it = program.iter().copied();
        let mut skip_next = false;
        while let Some(op) = it.next() {
            pen.ops += 1;
            if pen.ops > MAX_OPS {
                return;
            }
            let skip = std::mem::take(&mut skip_next);
            match op {
                0 => return,
                1 if !skip => pen.down = true,
                2 if !skip => {
                    pen.flush();
                    pen.down = false;
                }
                1 | 2 => {}
                3 | 4 => {
                    let Some(n) = it.next() else { return };
                    if !skip && n != 0 {
                        let k = f64::from(n);
                        pen.scale = (if op == 3 { pen.scale / k } else { pen.scale * k }).clamp(1e-9, 1e9);
                    }
                }
                5 if !skip && pen.stack.len() < MAX_STACK => pen.stack.push(pen.pos),
                6 if !skip => {
                    if let Some(p) = pen.stack.pop() {
                        pen.flush();
                        pen.pos = p;
                    }
                }
                5 | 6 => {}
                7 => {
                    let Some(first) = it.next() else { return };
                    let num = if self.kind == Kind::Unifont {
                        let Some(low) = it.next() else { return };
                        u32::from(first) << 8 | u32::from(low)
                    } else {
                        u32::from(first)
                    };
                    if !skip
                        && depth < MAX_SUBSHAPE_DEPTH
                        && let Some(sub) = self.shapes.get(&num)
                    {
                        self.run(sub, pen, depth + 1);
                    }
                }
                8 => {
                    let (Some(dx), Some(dy)) = (it.next(), it.next()) else { return };
                    if !skip {
                        pen.line(signed(dx), signed(dy));
                    }
                }
                9 => loop {
                    let (Some(dx), Some(dy)) = (it.next(), it.next()) else { return };
                    if dx == 0 && dy == 0 {
                        break;
                    }
                    if !skip {
                        pen.line(signed(dx), signed(dy));
                    }
                },
                10 => {
                    let (Some(r), Some(oct)) = (it.next(), it.next()) else { return };
                    if !skip {
                        pen.octant_arc(f64::from(r), 0.0, 0.0, oct);
                    }
                }
                11 => {
                    let (Some(so), Some(eo), Some(hi), Some(r), Some(oct)) = (it.next(), it.next(), it.next(), it.next(), it.next()) else { return };
                    if !skip {
                        pen.octant_arc(f64::from(hi) * 256.0 + f64::from(r), f64::from(so), f64::from(eo), oct);
                    }
                }
                12 => {
                    let (Some(dx), Some(dy), Some(b)) = (it.next(), it.next(), it.next()) else { return };
                    if !skip {
                        pen.bulge(signed(dx), signed(dy), signed(b));
                    }
                }
                13 => loop {
                    let (Some(dx), Some(dy)) = (it.next(), it.next()) else { return };
                    if dx == 0 && dy == 0 {
                        break;
                    }
                    let Some(b) = it.next() else { return };
                    if !skip {
                        pen.bulge(signed(dx), signed(dy), signed(b));
                    }
                },
                14 => skip_next = true,
                _ if skip => {}
                _ => {
                    let len = f64::from(op >> 4);
                    let (dx, dy) = DIRECTIONS.get(usize::from(op & 15)).copied().unwrap_or_default();
                    pen.line(dx * len, dy * len);
                }
            }
        }
    }
}

/// The sixteen vector directions, counter-clockwise from +X; the longer axis is one unit.
const DIRECTIONS: [(f64, f64); 16] = [
    (1.0, 0.0),
    (1.0, 0.5),
    (1.0, 1.0),
    (0.5, 1.0),
    (0.0, 1.0),
    (-0.5, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.5),
    (-1.0, 0.0),
    (-1.0, -0.5),
    (-1.0, -1.0),
    (-0.5, -1.0),
    (0.0, -1.0),
    (0.5, -1.0),
    (1.0, -1.0),
    (1.0, -0.5),
];

/// A program byte as a signed displacement.
fn signed(b: u8) -> f64 {
    f64::from(i8::from_le_bytes([b]))
}

struct Pen {
    pos: (f64, f64),
    scale: f64,
    down: bool,
    stack: Vec<(f64, f64)>,
    strokes: Vec<Vec<(f64, f64)>>,
    current: Vec<(f64, f64)>,
    points: usize,
    ops: usize,
}

impl Default for Pen {
    fn default() -> Self {
        Pen { pos: (0.0, 0.0), scale: 1.0, down: true, stack: Vec::new(), strokes: Vec::new(), current: Vec::new(), points: 0, ops: 0 }
    }
}

impl Pen {
    /// End the stroke being drawn.
    fn flush(&mut self) {
        if self.current.len() > 1 {
            self.strokes.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    /// Move to `p`, drawing when the pen is down.
    fn to(&mut self, p: (f64, f64)) {
        if !(p.0.is_finite() && p.1.is_finite()) {
            return;
        }
        if self.down && self.points < MAX_GLYPH_POINTS {
            if self.current.is_empty() {
                self.current.push(self.pos);
            }
            self.current.push(p);
            self.points += 1;
        } else {
            self.flush();
        }
        self.pos = p;
    }

    fn line(&mut self, dx: f64, dy: f64) {
        self.to((self.pos.0 + dx * self.scale, self.pos.1 + dy * self.scale));
    }

    /// An arc of `sweep` radians about `center`, starting at angle `a0`, as short chords.
    fn arc(&mut self, center: (f64, f64), r: f64, a0: f64, sweep: f64) {
        let n = (sweep.abs() / (std::f64::consts::PI / 32.0)).ceil().clamp(1.0, 64.0);
        let mut k = 1.0;
        while k <= n {
            let a = a0 + sweep * k / n;
            self.to((center.0 + r * a.cos(), center.1 + r * a.sin()));
            k += 1.0;
        }
    }

    /// Codes 10 and 11: an arc spanning octants (45 degree steps) from the pen position.
    /// `oct` is a signed byte: negative = clockwise, then the starting octant (0-7) and the
    /// number of octants (0 = 8, a full circle). `start` and `end` are the offsets of a
    /// fractional arc into its first and last octant, in 1/256 of an octant.
    fn octant_arc(&mut self, radius: f64, start: f64, end: f64, oct: u8) {
        let clockwise = oct & 0x80 != 0;
        let oct = if clockwise { oct.wrapping_neg() } else { oct };
        let first = f64::from((oct >> 4) & 7);
        let span = match oct & 7 {
            0 => 8.0,
            n => f64::from(n),
        };
        let dir = if clockwise { -1.0 } else { 1.0 };
        let octant = std::f64::consts::FRAC_PI_4;
        let a0 = (first + dir * start / 256.0) * octant;
        // A fractional end stops inside the last octant instead of at its far edge.
        let last = if end > 0.0 { span - 1.0 + end / 256.0 } else { span };
        let sweep = (first + dir * last) * octant - a0;
        let r = radius * self.scale;
        let center = (self.pos.0 - r * a0.cos(), self.pos.1 - r * a0.sin());
        self.arc(center, r, a0, sweep);
    }

    /// Codes 12 and 13: an arc to a displacement. `bulge` / 127 is twice the arc's height over
    /// its chord: 0 a straight line, 127 a semicircle; positive is counter-clockwise.
    fn bulge(&mut self, dx: f64, dy: f64, bulge: f64) {
        let (dx, dy) = (dx * self.scale, dy * self.scale);
        let chord = dx.hypot(dy);
        let b = (bulge / 127.0).clamp(-1.0, 1.0);
        if b.abs() < 1e-9 || chord < 1e-12 {
            self.to((self.pos.0 + dx, self.pos.1 + dy));
            return;
        }
        let angle = 4.0 * b.atan();
        // The centre lies on the chord's bisector, to the left of the chord for a
        // counter-clockwise arc.
        let offset = chord / 2.0 / (angle / 2.0).tan();
        let mid = (self.pos.0 + dx / 2.0, self.pos.1 + dy / 2.0);
        let center = (mid.0 - dy / chord * offset, mid.1 + dx / chord * offset);
        let r = (self.pos.0 - center.0).hypot(self.pos.1 - center.1);
        let a0 = (self.pos.1 - center.1).atan2(self.pos.0 - center.0);
        let end = (self.pos.0 + dx, self.pos.1 + dy);
        self.arc(center, r, a0, angle);
        // Land exactly on the end point: advances add up over a line of text.
        self.pos = end;
        if let Some(p) = self.current.last_mut() {
            *p = end;
        }
    }
}

#[derive(Default)]
struct Db {
    /// Fonts added with [`register`], by key.
    registered: HashMap<String, Arc<Font>>,
    /// Name lookups already answered, found or not.
    resolved: HashMap<String, Option<Arc<Font>>>,
    /// Folders searched for SHX files (`None`: not read from the environment yet).
    dirs: Option<Vec<PathBuf>>,
}

fn lock() -> MutexGuard<'static, Db> {
    static DB: OnceLock<Mutex<Db>> = OnceLock::new();
    let mut g = DB.get_or_init(|| Mutex::new(Db::default())).lock().unwrap_or_else(PoisonError::into_inner);
    if g.dirs.is_none() {
        #[cfg(not(target_arch = "wasm32"))]
        let env = std::env::var("CADCRAFT_FONTPATH").unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let env = String::new();
        g.dirs = Some(split_dirs(&env));
    }
    g
}

/// Folders of a path list: `;` separated (also `:` where that can't be a drive letter).
fn split_dirs(list: &str) -> Vec<PathBuf> {
    let seps: &[char] = if cfg!(windows) { &[';'] } else { &[';', ':'] };
    list.split(seps).map(str::trim).filter(|d| !d.is_empty() && *d != ".").take(MAX_PATH_DIRS).map(PathBuf::from).collect()
}

/// Lookup key of a style font name: its file name in lower case with ".shx". Names with
/// another extension (a TrueType file) have no key.
fn key(name: &str) -> Option<String> {
    let n = name.trim().to_ascii_lowercase();
    let file = n.rsplit(['/', '\\']).next().unwrap_or_default();
    if file.is_empty() || file.len() > 255 {
        return None;
    }
    match file.rsplit_once('.') {
        Some((_, "shx")) => Some(file.to_string()),
        Some((_, "ttf" | "otf" | "ttc" | "otc")) => None,
        _ => Some(format!("{file}.shx")),
    }
}

/// FONTPATH: the folders searched for SHX files, `;` separated.
pub fn font_path() -> String {
    let g = lock();
    g.dirs.iter().flatten().map(|d| d.display().to_string()).collect::<Vec<_>>().join(";")
}

/// Set FONTPATH from a `;` separated list of folders ("" or "." clears it).
pub fn set_font_path(list: &str) {
    let mut g = lock();
    g.dirs = Some(split_dirs(list));
    g.resolved.clear();
}

/// Add an SHX font under a file name (web builds and tests).
pub fn register(name: &str, bytes: &[u8]) -> Result<(), ShxError> {
    let font = Arc::new(Font::parse(bytes)?);
    let mut g = lock();
    if let Some(k) = key(name) {
        g.registered.insert(k, font);
    }
    g.resolved.clear();
    Ok(())
}

/// The SHX text font with this file name ("romans.shx", "romans"), when one is registered or in
/// a [`font_path`] folder and can be read.
pub fn find(name: &str) -> Option<Arc<Font>> {
    let k = key(name)?;
    let mut g = lock();
    if let Some(f) = g.registered.get(&k) {
        return Some(f.clone());
    }
    if let Some(r) = g.resolved.get(&k) {
        return r.clone();
    }
    let found = g.dirs.iter().flatten().find_map(|d| load(d, &k));
    if g.resolved.len() >= MAX_CACHED_NAMES {
        g.resolved.clear();
    }
    g.resolved.insert(k, found.clone());
    found
}

/// The font in file `name` of folder `dir`, matching the name without regard to case.
#[cfg(not(target_arch = "wasm32"))]
fn load(dir: &std::path::Path, name: &str) -> Option<Arc<Font>> {
    let path =
        std::fs::read_dir(dir).ok()?.flatten().take(MAX_DIR_ENTRIES).find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))?.path();
    if std::fs::metadata(&path).ok()?.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    Font::parse(&std::fs::read(&path).ok()?).ok().map(Arc::new)
}

#[cfg(target_arch = "wasm32")]
fn load(_dir: &std::path::Path, _name: &str) -> Option<Arc<Font>> {
    None
}

/// Synthetic SHX files for tests: our own shapes, laid out as the file kinds are.
#[cfg(test)]
pub(crate) mod testfont {
    /// A `shapes 1.0` font: `above` / `below`, then `(character code, shape program)`.
    pub(crate) fn shapes(above: u8, below: u8, glyphs: &[(u8, &[u8])]) -> Vec<u8> {
        let mut defs: Vec<(u16, Vec<u8>)> = vec![(0, [b"Test font\0".as_slice(), &[above, below, 0, 0]].concat())];
        defs.extend(glyphs.iter().map(|(c, p)| (u16::from(*c), [&[0u8], *p].concat())));
        let mut out = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        let last = defs.iter().map(|d| d.0).max().unwrap_or(0);
        for v in [0, last, defs.len() as u16] {
            out.extend(v.to_le_bytes());
        }
        for (n, d) in &defs {
            out.extend(n.to_le_bytes());
            out.extend((d.len() as u16).to_le_bytes());
        }
        for (_, d) in &defs {
            out.extend(d);
        }
        out.extend(b"EOF");
        out
    }

    /// A `unifont 1.0` font.
    pub(crate) fn unifont(above: u8, below: u8, glyphs: &[(u16, &[u8])]) -> Vec<u8> {
        let info = [b"Test unifont\0".as_slice(), &[above, below, 0, 0, 0, 0]].concat();
        let mut out = b"AutoCAD-86 unifont 1.0\r\n\x1a".to_vec();
        out.extend((glyphs.len() as u32 + 1).to_le_bytes());
        out.extend((info.len() as u16).to_le_bytes());
        out.extend(info);
        for (c, p) in glyphs {
            out.extend(c.to_le_bytes());
            out.extend((p.len() as u16 + 1).to_le_bytes());
            out.push(0);
            out.extend(*p);
        }
        out
    }

    /// A small font: `I` a vertical bar (advance 2), `L` (advance 6), `-` drawn with the pen
    /// lifted to mid height (advance 4), space (advance 4), `O` a full circle (advance 6), and
    /// a degree sign at 127.
    pub(crate) fn sample() -> Vec<u8> {
        shapes(
            6,
            2,
            &[
                (b' ', &[2, 0x40, 0]),
                (b'I', &[0x64, 2, 0x6C, 0x20, 0]),
                (b'L', &[2, 0x64, 1, 0x6C, 0x40, 2, 0x20, 0]),
                (b'-', &[2, 0x34, 1, 0x40, 2, 0x3C, 0]),
                (b'O', &[2, 0x34, 1, 10, 3, 0x40, 2, 0x3C, 0x60, 0]),
                (127, &[2, 0x54, 1, 10, 1, 0x40, 2, 0x5C, 0x20, 0]),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn reads_a_shapes_font() {
        let f = Font::parse(&testfont::sample()).unwrap();
        assert_eq!((f.kind(), f.name(), f.above(), f.below(), f.len()), (Kind::Shapes, "Test font", 6.0, 2.0, 6));
        let i = f.glyph('I').unwrap();
        assert_eq!(i.strokes, vec![vec![(0.0, 0.0), (0.0, 6.0)]]);
        assert_eq!(i.advance, 2.0);
        // Pen up, move, pen down: only the foot and the stem are drawn.
        let l = f.glyph('L').unwrap();
        assert_eq!(l.strokes, vec![vec![(0.0, 6.0), (0.0, 0.0), (4.0, 0.0)]]);
        assert_eq!(l.advance, 6.0);
        assert!(f.glyph('Z').is_none() && !f.has_glyph('é'));
        // %%d is shape 127 in a font without U+00B0.
        assert!(f.glyph('°').is_some());
    }

    #[test]
    fn reads_a_unifont_with_subshapes() {
        // U+00C9 is E (U+0045) with an accent stroke; the subshape number takes two bytes.
        let bytes = testfont::unifont(
            6,
            2,
            &[(0x45, &[0x64, 0x40, 2, 0x6C, 0x20, 0]), (0xC9, &[7, 0x00, 0x45, 2, 8, (-5i8) as u8, 7, 1, 0x21, 2, 8, 3, (-8i8) as u8, 0])],
        );
        let f = Font::parse(&bytes).unwrap();
        assert_eq!((f.kind(), f.name(), f.len()), (Kind::Unifont, "Test unifont", 2));
        let e = f.glyph('E').unwrap();
        let acute = f.glyph('É').unwrap();
        assert_eq!(acute.strokes.first(), e.strokes.first());
        assert_eq!(acute.strokes.len(), e.strokes.len() + 1);
        assert_eq!(acute.advance, e.advance);
    }

    #[test]
    fn arcs_displacements_scale_and_stack() {
        let f = Font::parse(&testfont::shapes(
            10,
            0,
            &[
                // A full circle of radius 3 starting at its rightmost point.
                (b'a', &[10, 3, 0x00, 0]),
                // A clockwise quarter of radius 2 from octant 2 (top) to octant 0 (right).
                (b'b', &[10, 2, (-0x22i8) as u8, 0]),
                // A semicircle bulge over a chord of 4.
                (b'c', &[12, 4, 0, 127, 0]),
                // Scale by 2, push, draw up, pop, signed displacement, divide back.
                (b'd', &[4, 2, 5, 0x24, 6, 8, 3, (-1i8) as u8, 3, 2, 0x10, 0]),
                // A fractional arc from 55 to 95 degrees of radius 3.
                (b'e', &[11, 56, 28, 0, 3, 0x12, 0]),
                // Polyline displacements ended by (0,0); the vertical-only move is skipped.
                (b'f', &[9, 1, 1, 1, (-1i8) as u8, 0, 0, 14, 8, 50, 50, 0]),
            ],
        ))
        .unwrap();
        let a = f.glyph('a').unwrap();
        let pts = a.strokes.first().unwrap();
        assert!(pts.iter().all(|p| ((p.0 + 3.0).hypot(p.1) - 3.0).abs() < 1e-9), "all on the circle about (-3, 0)");
        assert!(close(*pts.last().unwrap(), (0.0, 0.0)) && a.advance.abs() < 1e-9);

        let b = f.glyph('b').unwrap();
        assert!(close(*b.strokes.first().unwrap().last().unwrap(), (2.0, -2.0)));

        let c = f.glyph('c').unwrap();
        let pts = c.strokes.first().unwrap();
        assert!(close(*pts.last().unwrap(), (4.0, 0.0)));
        // Counter-clockwise from (0,0) to (4,0): the arc bows below the chord.
        assert!(pts.iter().any(|p| (p.1 + 2.0).abs() < 1e-9) && pts.iter().all(|p| p.1 < 1e-9));

        let d = f.glyph('d').unwrap();
        assert_eq!(d.strokes, vec![vec![(0.0, 0.0), (0.0, 4.0)], vec![(0.0, 0.0), (6.0, -2.0), (7.0, -2.0)]]);

        let e = f.glyph('e').unwrap();
        let pts = e.strokes.first().unwrap();
        let (first, last) = (*pts.first().unwrap(), *pts.last().unwrap());
        let start = (45.0 + 56.0 * 45.0 / 256.0_f64).to_radians();
        let end = (90.0 + 28.0 * 45.0 / 256.0_f64).to_radians();
        let center = (first.0 - 3.0 * start.cos(), first.1 - 3.0 * start.sin());
        assert!(close(last, (center.0 + 3.0 * end.cos(), center.1 + 3.0 * end.sin())));

        let g = f.glyph('f').unwrap();
        assert_eq!(g.strokes, vec![vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]]);
        assert_eq!(g.advance, 2.0);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert_eq!(Font::parse(b"").unwrap_err(), ShxError::NotShx);
        assert_eq!(Font::parse(b"not a font at all").unwrap_err(), ShxError::NotShx);
        assert_eq!(Font::parse(b"AutoCAD-86 bigfont 1.0\r\n\x1a\0\0\0\0").unwrap_err(), ShxError::BigFont);
        assert_eq!(Font::parse(b"AutoCAD-86 shapes 1.0\r\n\x1a").unwrap_err(), ShxError::Truncated);
        // Shapes without shape 0 are a shape file, not a text font.
        let mut shapes_only = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        shapes_only.extend([1, 0, 1, 0, 1, 0, 1, 0, 3, 0, 0, 0x14, 0]);
        assert_eq!(Font::parse(&shapes_only).unwrap_err(), ShxError::NotAFont);
    }

    #[test]
    fn hostile_shx_does_not_panic() {
        let good = testfont::sample();
        // Every truncation and every single-byte corruption parses or fails cleanly, and
        // every glyph of what parses can be drawn.
        let draw = |bytes: &[u8]| {
            if let Ok(f) = Font::parse(bytes) {
                for c in (0u8..=255).map(char::from) {
                    let _ = f.glyph(c);
                }
            }
        };
        for n in 0..good.len() {
            draw(good.get(..n).unwrap());
            for v in [0u8, 7, 14, 0x7F, 0xFF] {
                let mut bad = good.clone();
                bad[n] = v;
                draw(&bad);
            }
        }
        // A shape that calls itself, an unbalanced stack, runaway scale and endless arcs.
        let f = Font::parse(&testfont::shapes(
            1,
            0,
            &[
                (b'a', &[7, b'a', 0x10, 0]),
                (b'b', &[6, 6, 6, 5, 5, 5, 0]),
                (b'c', &[4, 255, 4, 255, 4, 255, 4, 255, 4, 255, 0xF0, 3, 0, 0]),
                (b'd', &[13, 127, 127, 127, 127, 127, 127]),
                (b'e', &[11, 255, 255, 255, 255, 0xFF]),
                (b'f', &[14, 14, 14, 9]),
            ],
        ))
        .unwrap();
        for c in 'a'..='f' {
            let g = f.glyph(c).unwrap();
            assert!(g.advance.is_finite());
            assert!(g.strokes.iter().flatten().all(|p| p.0.is_finite() && p.1.is_finite()), "{c}");
        }
        // The self-reference stops at the depth limit.
        assert_eq!(f.glyph('a').unwrap().strokes.len(), 1);
    }

    #[test]
    fn finds_registered_fonts_by_file_name() {
        register("Fonts\\ShxTest-A.SHX", &testfont::sample()).unwrap();
        assert!(find("shxtest-a.shx").is_some() && find("SHXTEST-A").is_some());
        assert!(find("shxtest-missing.shx").is_none() && find("shxtest-a.ttf").is_none() && find("").is_none());
        assert_eq!(register("shxtest-bad.shx", b"junk").unwrap_err(), ShxError::NotShx);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn finds_fonts_in_font_path_folders() {
        let dir = std::env::temp_dir().join(format!("cadcraft-shx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ShxPath-Test.SHX"), testfont::sample()).unwrap();
        let before = font_path();
        set_font_path(&format!("{};{}", dir.join("missing").display(), dir.display()));
        let found = find("shxpath-test.shx");
        set_font_path(&before);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.map(|f| f.len()), Some(6));
    }
}
