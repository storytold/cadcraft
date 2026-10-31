//! SHX shape fonts (AutoCAD's compiled `.shx` files): the drawing's own stroke fonts, read from
//! the user's font folders at run time. Nothing of any SHX font ships with CADCraft.
//!
//! Three file kinds are read (format as published in the AutoCAD Customization Guide, "Shape
//! Descriptions"):
//! - `shapes 1.0` / `1.1`: a table of (shape number, length) after three counts, then each shape
//!   as a NUL-terminated name and its bytes. Shape 0 holds the font's above / below heights.
//! - `unifont 1.0`: a shape count and the font information record, then each shape as number,
//!   length and bytes (shape numbers are Unicode code points).
//! - `bigfont 1.0`: escape-byte ranges, then a table of (number, length, offset).
//!
//! A shape is a little program for a pen: vector bytes (length in the high nibble, one of 16
//! directions in the low one) and the special codes 0–14 (pen up / down, scale, push / pop,
//! sub-shapes, displacements, octant, fractional and bulge arcs, vertical-only commands).
//! [`ShxFont::glyph`] runs it into polylines in font units; the end position is the advance.
//!
//! Everything read from the file is hostile: offsets and lengths are checked, programs are
//! bounded in steps, sub-shape nesting and points, and arcs in segments.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use cadcraft_geom::Vec2;

/// Largest SHX file read.
const MAX_FILE_BYTES: u64 = 16 << 20;
/// Most shapes taken from one file.
const MAX_SHAPES: usize = 70_000;
/// Most bytes one glyph program may run (sub-shapes included).
const MAX_STEPS: usize = 20_000;
/// Deepest sub-shape nesting.
const MAX_DEPTH: usize = 8;
/// Most points one glyph may produce.
const MAX_POINTS: usize = 50_000;
/// Deepest position stack (the format allows 4; some fonts push more).
const MAX_STACK: usize = 16;

/// What a shape file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Shapes,
    Unifont,
    Bigfont,
}

/// A parsed SHX font: its shapes' programs by shape number, and its metrics.
#[derive(Debug)]
pub struct ShxFont {
    pub name: String,
    pub kind: Kind,
    /// Height of capitals above the baseline, in font units (text height covers this).
    pub above: f64,
    /// Depth of descenders below the baseline, in font units.
    pub below: f64,
    shapes: HashMap<u32, Vec<u8>>,
    /// Hebrew letters at their Windows-1255 / DOS 862 places (worked out once, when read).
    hebrew: (bool, bool),
}

/// A glyph run into strokes: polylines in font units, baseline at y = 0, and the pen's final
/// x (the advance to the next character).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    pub strokes: Vec<Vec<Vec2>>,
    pub advance: f64,
}

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u32le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// The bytes after a shape's NUL-terminated name.
fn after_name(def: &[u8]) -> &[u8] {
    match def.iter().position(|b| *b == 0) {
        Some(n) => def.get(n + 1..).unwrap_or_default(),
        None => &[],
    }
}

impl ShxFont {
    /// Parse SHX bytes; `None` when they are not a shape font this reader knows.
    pub fn parse(name: &str, b: &[u8]) -> Option<ShxFont> {
        let head = b.get(..b.len().min(40))?;
        let text: String = head.iter().take_while(|c| **c != 0x1a).map(|c| *c as char).collect();
        let start = text.len() + 1;
        let lower = text.to_ascii_lowercase();
        if !lower.starts_with("autocad-86") {
            return None;
        }
        let mut shapes: HashMap<u32, Vec<u8>> = HashMap::new();
        let (kind, mut above, mut below);
        if lower.contains("unifont") {
            kind = Kind::Unifont;
            let count = usize::try_from(u32le(b, start)?).ok()?.min(MAX_SHAPES);
            let info_len = usize::from(u16le(b, start + 4)?);
            let info = b.get(start + 6..start + 6 + info_len)?;
            let rest = after_name(info);
            above = f64::from(*rest.first().unwrap_or(&0));
            below = f64::from(*rest.get(1).unwrap_or(&0));
            let mut i = start + 6 + info_len;
            for _ in 0..count {
                let (Some(num), Some(len)) = (u16le(b, i), u16le(b, i + 2)) else { break };
                let len = usize::from(len);
                let Some(def) = b.get(i + 4..i + 4 + len) else { break };
                shapes.insert(u32::from(num), after_name(def).to_vec());
                i += 4 + len;
            }
        } else if lower.contains("bigfont") {
            kind = Kind::Bigfont;
            let count = usize::from(u16le(b, start)?).min(MAX_SHAPES);
            let ranges = usize::from(u16le(b, start + 2)?);
            let mut i = start + 4 + ranges * 4;
            above = 0.0;
            below = 0.0;
            // The table's records may point at the same bytes: what is copied is bounded in
            // total, not only per record.
            let mut copied = 0usize;
            for _ in 0..count {
                let (Some(num), Some(len), Some(off)) = (u16le(b, i), u16le(b, i + 2), u32le(b, i + 4)) else { break };
                i += 8;
                let (len, off) = (usize::from(len), usize::try_from(off).ok()?);
                let Some(def) = b.get(off..off.saturating_add(len)) else { continue };
                copied = copied.saturating_add(len);
                if copied > MAX_FILE_BYTES as usize {
                    break;
                }
                let body = after_name(def);
                if num == 0 {
                    above = f64::from(*body.first().unwrap_or(&0));
                } else {
                    shapes.insert(u32::from(num), body.to_vec());
                }
            }
        } else if lower.contains("shapes") {
            kind = Kind::Shapes;
            let count = usize::from(u16le(b, start + 4)?).min(MAX_SHAPES);
            let mut index = Vec::with_capacity(count);
            let mut i = start + 6;
            for _ in 0..count {
                let (Some(num), Some(len)) = (u16le(b, i), u16le(b, i + 2)) else { break };
                index.push((num, usize::from(len)));
                i += 4;
            }
            above = 0.0;
            below = 0.0;
            for (num, len) in index {
                let Some(def) = b.get(i..i + len) else { break };
                i += len;
                let body = after_name(def);
                if num == 0 {
                    above = f64::from(*body.first().unwrap_or(&0));
                    below = f64::from(*body.get(1).unwrap_or(&0));
                } else {
                    shapes.insert(u32::from(num), body.to_vec());
                }
            }
        } else {
            return None;
        }
        if above <= 0.0 {
            // A font without its information shape: take the capital letter's height.
            above = 10.0;
        }
        let mut f = ShxFont { name: name.to_string(), kind, above, below, shapes, hebrew: (false, false) };
        f.hebrew = (f.find_hebrew_1255(), f.find_hebrew_862());
        Some(f)
    }

    /// Whether the font draws shape `code`.
    pub fn has(&self, code: u32) -> bool {
        self.shapes.contains_key(&code)
    }

    /// How many of `codes` the font draws.
    fn count(&self, codes: std::ops::RangeInclusive<u32>) -> usize {
        codes.filter(|c| self.shapes.contains_key(c)).count()
    }

    /// A one-byte font with Hebrew letters at their Windows-1255 places (0xE0..0xFA) and not a
    /// Cyrillic Windows-1251 font (which fills 0xC0..0xDF with capitals as well).
    fn find_hebrew_1255(&self) -> bool {
        self.kind != Kind::Unifont && self.count(0xE0..=0xFA) >= 20 && self.count(0xC0..=0xDF) < 16
    }

    /// A one-byte font with Hebrew letters at their DOS 862 places (0x80..0x9A) and not a
    /// Cyrillic DOS 866 font (which also has small letters at 0xA0..0xAF).
    fn find_hebrew_862(&self) -> bool {
        self.kind != Kind::Unifont && self.count(0x80..=0x9A) >= 20 && self.count(0xA0..=0xAF) < 8
    }

    fn hebrew_1255(&self) -> bool {
        self.hebrew.0
    }
    fn hebrew_862(&self) -> bool {
        self.hebrew.1
    }

    /// The shape that draws `c`: its code point, or for a one-byte font a Hebrew letter at
    /// its Windows-1255 or DOS (862) position, and the old AutoCAD positions of °, ± and ⌀.
    pub fn code_for(&self, c: char) -> Option<u32> {
        let u = u32::from(c);
        // A big font numbers its shapes by its code page's double-byte codes, not by Unicode:
        // a character beyond one byte is not looked for there by its code point (it would draw
        // an unrelated shape); it comes from the main or the built-in font instead.
        if self.kind == Kind::Bigfont && u > 0xFF {
            return None;
        }
        let mut tries: Vec<u32> = vec![u];
        // Only a font that holds Hebrew there: a Cyrillic font has its letters at the same
        // places (Windows-1251 0xC0..0xFF, DOS 866 0x80..0xAF), and Hebrew text must not turn
        // into Russian.
        if (0x05D0..=0x05EA).contains(&u) {
            if self.hebrew_1255() {
                tries.push(u - 0x05D0 + 0xE0);
            }
            if self.hebrew_862() {
                tries.push(u - 0x05D0 + 0x80);
            }
        }
        // °, ± and ⌀: their Unicode places in a Unicode font; in a one-byte font AutoCAD's own
        // places (127, 128, 129) unless those hold Hebrew letters (DOS 862), and never a
        // Windows code page place, which other code pages use for letters (Ш, ר…). A font
        // without them draws them from the built-in font.
        let unicode = self.kind == Kind::Unifont;
        let legacy = !unicode && !self.hebrew_862();
        match c {
            '°' => {
                if unicode {
                    tries.push(0xB0);
                }
                if legacy {
                    tries.push(0x7F);
                }
            }
            '±' => {
                if unicode {
                    tries.push(0xB1);
                }
                if legacy {
                    tries.push(0x80);
                }
            }
            '⌀' | '∅' | 'Ø' => {
                tries.extend([0x2205, 0x2300]);
                if unicode {
                    tries.push(0xD8);
                }
                if legacy {
                    tries.push(0x81);
                }
            }
            _ => {}
        }
        tries.into_iter().find(|t| self.has(*t))
    }

    /// Run shape `code` into strokes (font units).
    pub fn glyph(&self, code: u32) -> Option<Glyph> {
        let prog = self.shapes.get(&code)?;
        let mut pen = Pen { pos: Vec2::ZERO, down: true, scale: 1.0, stack: Vec::new(), strokes: Vec::new(), cur: Vec::new(), steps: 0, points: 0 };
        self.run(prog, &mut pen, 0);
        pen.lift();
        Some(Glyph { strokes: pen.strokes, advance: pen.pos.x })
    }

    fn run(&self, p: &[u8], pen: &mut Pen, depth: usize) {
        let mut i = 0usize;
        let byte = |i: usize| p.get(i).copied();
        let sbyte = |i: usize| p.get(i).map(|b| f64::from(*b as i8));
        // Code 14: the next command is for vertical text only; it is read and not drawn.
        let mut skip = false;
        while i < p.len() {
            pen.steps += 1;
            if pen.steps > MAX_STEPS || pen.points > MAX_POINTS {
                return;
            }
            let Some(code) = byte(i) else { return };
            i += 1;
            let draw = !skip;
            skip = false;
            if code >= 0x10 {
                let len = f64::from(code >> 4);
                let d = DIRS.get(usize::from(code & 0x0f)).copied().unwrap_or((1.0, 0.0));
                if draw {
                    pen.move_by(Vec2::new(d.0, d.1) * (len * pen.scale));
                }
                continue;
            }
            match code {
                0 => {
                    if draw {
                        return;
                    }
                }
                1 => {
                    if draw {
                        pen.down = true;
                    }
                }
                2 => {
                    if draw {
                        pen.lift();
                        pen.down = false;
                    }
                }
                3 | 4 => {
                    let f = f64::from(byte(i).unwrap_or(1).max(1));
                    i += 1;
                    if draw {
                        pen.scale = if code == 3 { pen.scale / f } else { pen.scale * f };
                    }
                }
                5 => {
                    if draw && pen.stack.len() < MAX_STACK {
                        pen.stack.push(pen.pos);
                    }
                }
                6 => {
                    if draw && let Some(q) = pen.stack.pop() {
                        pen.lift();
                        pen.pos = q;
                    }
                }
                7 => {
                    let (num, extra) = match self.kind {
                        Kind::Shapes => (u32::from(byte(i).unwrap_or(0)), 1),
                        // Little-endian, like every number in a unifont file; a font that wrote
                        // it high byte first still finds its shape.
                        Kind::Unifont => {
                            let (lo, hi) = (byte(i).unwrap_or(0), byte(i + 1).unwrap_or(0));
                            let le = u32::from(u16::from_le_bytes([lo, hi]));
                            let be = u32::from(u16::from_be_bytes([lo, hi]));
                            (if self.shapes.contains_key(&le) || !self.shapes.contains_key(&be) { le } else { be }, 2)
                        }
                        Kind::Bigfont => match byte(i) {
                            Some(0) => (u32::from(u16::from_be_bytes([byte(i + 1).unwrap_or(0), byte(i + 2).unwrap_or(0)])), 7),
                            _ => (u32::from(byte(i).unwrap_or(0)), 1),
                        },
                    };
                    i += extra;
                    if draw
                        && depth < MAX_DEPTH
                        && let Some(sub) = self.shapes.get(&num)
                    {
                        let saved = pen.scale;
                        self.run(sub, pen, depth + 1);
                        pen.scale = saved;
                    }
                }
                8 => {
                    let (dx, dy) = (sbyte(i).unwrap_or(0.0), sbyte(i + 1).unwrap_or(0.0));
                    i += 2;
                    if draw {
                        pen.move_by(Vec2::new(dx, dy) * pen.scale);
                    }
                }
                9 => loop {
                    let (dx, dy) = (sbyte(i).unwrap_or(0.0), sbyte(i + 1).unwrap_or(0.0));
                    i += 2;
                    if (dx == 0.0 && dy == 0.0) || i > p.len() {
                        break;
                    }
                    if draw {
                        pen.move_by(Vec2::new(dx, dy) * pen.scale);
                    }
                },
                10 => {
                    let r = f64::from(byte(i).unwrap_or(0)) * pen.scale;
                    let oct = byte(i + 1).unwrap_or(0);
                    i += 2;
                    if draw {
                        let (cw, start, n) = octants(oct);
                        let a0 = start * 45.0;
                        let sweep = n * 45.0;
                        pen.arc(r, a0, if cw { a0 - sweep } else { a0 + sweep });
                    }
                }
                11 => {
                    let (so, eo) = (f64::from(byte(i).unwrap_or(0)), f64::from(byte(i + 1).unwrap_or(0)));
                    let r = f64::from(u16::from_be_bytes([byte(i + 2).unwrap_or(0), byte(i + 3).unwrap_or(0)])) * pen.scale;
                    let oct = byte(i + 4).unwrap_or(0);
                    i += 5;
                    if draw {
                        let (cw, start, n) = octants(oct);
                        let (so, eo) = (so * 45.0 / 256.0, eo * 45.0 / 256.0);
                        let (a0, a1) = if cw {
                            let a0 = start * 45.0 - so;
                            let last = start - (n - 1.0);
                            (a0, if eo > 0.0 { last * 45.0 - eo } else { (last - 1.0) * 45.0 })
                        } else {
                            let a0 = start * 45.0 + so;
                            let last = start + (n - 1.0);
                            (a0, if eo > 0.0 { last * 45.0 + eo } else { (last + 1.0) * 45.0 })
                        };
                        pen.arc(r, a0, a1);
                    }
                }
                12 => {
                    let (dx, dy, bu) = (sbyte(i).unwrap_or(0.0), sbyte(i + 1).unwrap_or(0.0), sbyte(i + 2).unwrap_or(0.0));
                    i += 3;
                    if draw {
                        pen.bulge(Vec2::new(dx, dy) * pen.scale, bu / 127.0);
                    }
                }
                13 => loop {
                    let (dx, dy) = (sbyte(i).unwrap_or(0.0), sbyte(i + 1).unwrap_or(0.0));
                    i += 2;
                    if (dx == 0.0 && dy == 0.0) || i > p.len() {
                        break;
                    }
                    let bu = sbyte(i).unwrap_or(0.0);
                    i += 1;
                    if draw {
                        pen.bulge(Vec2::new(dx, dy) * pen.scale, bu / 127.0);
                    }
                },
                14 => skip = true,
                _ => {}
            }
        }
    }
}

/// The 16 vector directions, counter-clockwise from east, as unit-square steps.
const DIRS: [(f64, f64); 16] = [
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

/// An octant byte: clockwise (sign bit), the starting octant (bits 4–6) and the number of
/// octants (bits 0–3, 0 meaning all 8).
fn octants(b: u8) -> (bool, f64, f64) {
    let cw = b & 0x80 != 0;
    let start = f64::from((b >> 4) & 0x07);
    let n = match b & 0x0f {
        0 => 8.0,
        n => f64::from(n.min(8)),
    };
    (cw, start, n)
}

struct Pen {
    pos: Vec2,
    down: bool,
    scale: f64,
    stack: Vec<Vec2>,
    strokes: Vec<Vec<Vec2>>,
    cur: Vec<Vec2>,
    steps: usize,
    points: usize,
}

impl Pen {
    fn lift(&mut self) {
        let cur = std::mem::take(&mut self.cur);
        if cur.len() >= 2 {
            self.strokes.push(cur);
        } else if let Some(p) = cur.first()
            && self.down
        {
            // A dot: drawn as a tiny stroke.
            self.strokes.push(vec![*p, *p + Vec2::new(0.25, 0.0)]);
        }
    }
    fn line_to(&mut self, q: Vec2) {
        if !q.is_finite() {
            return;
        }
        if self.down {
            if self.cur.is_empty() {
                self.cur.push(self.pos);
            }
            self.cur.push(q);
            self.points += 1;
        }
        self.pos = q;
    }
    fn move_by(&mut self, d: Vec2) {
        let q = self.pos + d;
        if !q.is_finite() {
            return;
        }
        if self.down {
            self.line_to(q);
        } else {
            self.pos = q;
        }
    }
    /// An arc of radius `r` from angle `a0` to `a1` (degrees; the pen is on the circle at `a0`).
    fn arc(&mut self, r: f64, a0: f64, a1: f64) {
        if !(r.is_finite() && r > 0.0) {
            return;
        }
        let (s, e) = (a0.to_radians(), a1.to_radians());
        let center = self.pos - Vec2::from_angle(s) * r;
        let n = (((e - s).abs() / (std::f64::consts::PI / 16.0)).ceil() as usize).clamp(1, 64);
        for k in 1..=n {
            let a = s + (e - s) * k as f64 / n as f64;
            let q = center + Vec2::from_angle(a) * r;
            if self.down {
                self.line_to(q);
            } else if q.is_finite() {
                self.pos = q;
            }
        }
    }
    /// An arc over chord `d` whose height is `bulge` × half the chord (a bulge of ±1 is a half
    /// circle, positive counter-clockwise); 0 is a straight line.
    fn bulge(&mut self, d: Vec2, bulge: f64) {
        let start = self.pos;
        let end = start + d;
        if bulge.abs() < 1e-9 || d.len() < 1e-12 {
            self.move_by(d);
            return;
        }
        let theta = 4.0 * bulge.atan();
        let chord = d.len();
        let r = chord / (2.0 * (theta / 2.0).sin()).abs();
        let mid = start.mid(end);
        let h = r * (theta / 2.0).cos();
        let n = d.perp() / chord;
        let center = mid + n * (h * theta.signum());
        let (a0, a1) = ((start - center).angle(), (start - center).angle() + theta);
        let segs = ((theta.abs() / (std::f64::consts::PI / 16.0)).ceil() as usize).clamp(1, 64);
        for k in 1..=segs {
            let q = if k == segs { end } else { center + Vec2::from_angle(a0 + (a1 - a0) * k as f64 / segs as f64) * r.abs() };
            if self.down {
                self.line_to(q);
            } else if q.is_finite() {
                self.pos = q;
            }
        }
    }
}

// --- Finding fonts -------------------------------------------------------------------------

#[derive(Default)]
struct Registry {
    generation: Option<u64>,
    /// `.shx` files in the user's font folders, by lower-case stem.
    files: HashMap<String, PathBuf>,
    /// Fonts registered by name (tests, web builds).
    bytes: HashMap<String, Arc<Vec<u8>>>,
    loaded: HashMap<String, Option<Arc<ShxFont>>>,
}

fn registry() -> MutexGuard<'static, Registry> {
    static R: OnceLock<Mutex<Registry>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Registry::default())).lock().unwrap_or_else(PoisonError::into_inner)
}

/// The lower-case file stem of a style font name: `C:\13\FONTS\HEBTXT.SHX` → `hebtxt`.
pub fn stem(name: &str) -> String {
    let n = name.trim();
    let file = n.rsplit(['\\', '/']).next().unwrap_or(n);
    let lower = file.to_ascii_lowercase();
    lower.strip_suffix(".shx").unwrap_or(&lower).trim().to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn scan(dirs: &[String]) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    // A folder listed earlier wins.
    let mut stack: Vec<PathBuf> = dirs.iter().rev().map(PathBuf::from).collect();
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut sub = Vec::new();
        for e in rd.flatten() {
            seen += 1;
            if seen > 50_000 {
                return out;
            }
            let p = e.path();
            // Real folders only: a link to a folder could lead in a circle.
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                sub.push(p);
                continue;
            }
            if ft.is_symlink() && p.is_dir() {
                continue;
            }
            let is_shx = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("shx"));
            if is_shx && let Some(s) = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()) {
                out.entry(s).or_insert(p);
            }
        }
        stack.extend(sub.into_iter().rev());
    }
    out
}

#[cfg(target_arch = "wasm32")]
fn scan(_dirs: &[String]) -> HashMap<String, PathBuf> {
    HashMap::new()
}

/// Register SHX bytes under a font name (tests, web builds).
pub fn register(name: &str, bytes: Vec<u8>) {
    let mut r = registry();
    let k = stem(name);
    r.loaded.remove(&k);
    r.bytes.insert(k, Arc::new(bytes));
}

/// The SHX font a style names (`romans_s.shx`, `miryl`, a full path), from the user's font
/// folders ([`crate::ttf::set_font_dirs`]); `None` when there is none.
pub fn find(name: &str) -> Option<Arc<ShxFont>> {
    let k = stem(name);
    if k.is_empty() {
        return None;
    }
    let mut r = registry();
    let generation = crate::ttf::generation();
    if r.generation != Some(generation) {
        r.files = scan(&crate::ttf::font_dirs());
        r.loaded.clear();
        r.generation = Some(generation);
    }
    if let Some(f) = r.loaded.get(&k) {
        return f.clone();
    }
    let bytes: Option<Arc<Vec<u8>>> = match r.bytes.get(&k) {
        Some(b) => Some(b.clone()),
        None => r.files.get(&k).and_then(|p| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                (std::fs::metadata(p).ok()?.len() <= MAX_FILE_BYTES).then(|| std::fs::read(p).ok()).flatten().map(Arc::new)
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = p;
                None
            }
        }),
    };
    let font = bytes.and_then(|b| ShxFont::parse(&k, &b)).map(Arc::new);
    if r.loaded.len() > 256 {
        r.loaded.clear();
    }
    r.loaded.insert(k, font.clone());
    font
}

/// SHX fonts in the user's font folders, by file stem (for font lists).
pub fn installed() -> Vec<String> {
    let mut r = registry();
    let generation = crate::ttf::generation();
    if r.generation != Some(generation) {
        r.files = scan(&crate::ttf::font_dirs());
        r.loaded.clear();
        r.generation = Some(generation);
    }
    let mut v: Vec<String> = r.files.keys().chain(r.bytes.keys()).cloned().collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny `shapes 1.0` font of our own: shape 0 (above 4, below 1), `A` an up-and-over
    /// path, `B` with an octant arc, `C` using a sub-shape.
    pub(crate) fn sample() -> Vec<u8> {
        let mut b = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        let shapes: Vec<(u16, Vec<u8>)> = vec![
            (0, [b"T\0".to_vec(), vec![4, 1, 0, 0]].concat()),
            // A: up 4 (0x44), right 2 (0x20), down 4 (0x4C), pen up, right 1 (0x10), end.
            (u16::from(b'A'), [b"A\0".to_vec(), vec![0x44, 0x20, 0x4C, 2, 0x10, 0]].concat()),
            // B: pen up, right 4, pen down, half circle of radius 2 from 0° CCW (4 octants) back to the
            // origin, pen up, then 5 to the right.
            (u16::from(b'B'), [b"B\0".to_vec(), vec![2, 0x40, 1, 10, 2, 0x04, 2, 8, 5, 0, 0]].concat()),
            // C: sub-shape A, then 8,(1,0).
            (u16::from(b'C'), [b"C\0".to_vec(), vec![7, b'A', 0]].concat()),
        ];
        let first = 0u16;
        let last = u16::from(b'C');
        b.extend(first.to_le_bytes());
        b.extend(last.to_le_bytes());
        b.extend(u16::try_from(shapes.len()).unwrap_or(0).to_le_bytes());
        for (n, d) in &shapes {
            b.extend(n.to_le_bytes());
            b.extend(u16::try_from(d.len()).unwrap_or(0).to_le_bytes());
        }
        for (_, d) in &shapes {
            b.extend(d);
        }
        b
    }

    /// A tiny `unifont 1.0` font of our own: `A` up 4, `C` sub-shape `A` (code 7 then the shape
    /// number as two bytes, low byte first).
    fn unifont_sample() -> Vec<u8> {
        let mut b = b"AutoCAD-86 unifont 1.0\r\n\x1a".to_vec();
        let shapes: Vec<(u16, Vec<u8>)> = vec![
            (u16::from(b'A'), [b"A\0".to_vec(), vec![0x44, 0x20, 0]].concat()),
            (u16::from(b'C'), [b"C\0".to_vec(), vec![7, b'A', 0, 0]].concat()),
        ];
        let info = [b"U\0".to_vec(), vec![4, 1, 0, 0, 0, 0]].concat();
        b.extend(u32::try_from(shapes.len() + 1).unwrap_or(0).to_le_bytes());
        b.extend(u16::try_from(info.len()).unwrap_or(0).to_le_bytes());
        b.extend(&info);
        for (n, d) in &shapes {
            b.extend(n.to_le_bytes());
            b.extend(u16::try_from(d.len()).unwrap_or(0).to_le_bytes());
            b.extend(d);
        }
        b
    }

    #[test]
    fn a_unifont_subshape_number_is_little_endian() {
        let f = ShxFont::parse("u", &unifont_sample()).expect("parses");
        assert_eq!(f.kind, Kind::Unifont);
        let c = f.glyph(u32::from(b'C')).expect("C");
        assert_eq!(c.strokes, f.glyph(u32::from(b'A')).expect("A").strokes, "C draws its sub-shape A");
        assert_eq!(c.advance, 2.0);
    }

    #[test]
    fn reads_a_shapes_font_and_runs_its_programs() {
        let f = ShxFont::parse("t", &sample()).expect("parses");
        assert_eq!(f.kind, Kind::Shapes);
        assert_eq!((f.above, f.below), (4.0, 1.0));
        let a = f.glyph(u32::from(b'A')).expect("A");
        assert_eq!(a.strokes, vec![vec![Vec2::new(0.0, 0.0), Vec2::new(0.0, 4.0), Vec2::new(2.0, 4.0), Vec2::new(2.0, 0.0)]]);
        assert_eq!(a.advance, 3.0);
        let b = f.glyph(u32::from(b'B')).expect("B");
        let arc = b.strokes.first().expect("an arc");
        let top = arc.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((top - 2.0).abs() < 1e-9, "a half circle of radius 2 reaches y = 2: {top}");
        assert!(arc.last().is_some_and(|p| (p.x - 0.0).abs() < 1e-9 && p.y.abs() < 1e-9), "{arc:?}");
        assert_eq!(f.glyph(u32::from(b'C')).map(|g| g.advance), Some(3.0));
    }

    #[test]
    fn text_in_an_shx_style_is_drawn_with_the_font() {
        register("tiny_test.shx", sample());
        let font = crate::TextFont::resolve("C:\\FONTS\\TINY_TEST.SHX");
        assert!(matches!(font, crate::TextFont::Shx { .. }), "{font:?}");
        // Text height 8 is twice the font's capital height (4): A is 3 units wide, B 5.
        let sh = crate::shape_line(&font, "AB", 8.0, 1.0, 0.0);
        assert!((sh.width - 16.0).abs() < 1e-9, "{}", sh.width);
        let top = sh.strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((top - 8.0).abs() < 1e-9, "{top}");
        // A character the font lacks comes from the stroke font.
        let with_x = crate::shape_line(&font, "AX", 8.0, 1.0, 0.0);
        assert!(with_x.width > 6.0 && with_x.strokes.len() > 1);
        assert!(crate::ttf::available("tiny_test.shx"));
    }

    /// A shapes font with a dot glyph at each of `codes`.
    fn font_with(codes: impl IntoIterator<Item = u16>) -> ShxFont {
        let mut shapes: Vec<(u16, Vec<u8>)> = vec![(0, b"H\0\x04\x01\x00\x00".to_vec())];
        shapes.extend(codes.into_iter().map(|c| (c, b"\0\x14\x00".to_vec())));
        let mut b = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        b.extend(0u16.to_le_bytes());
        b.extend(shapes.last().map_or(0, |s| s.0).to_le_bytes());
        b.extend(u16::try_from(shapes.len()).unwrap_or(0).to_le_bytes());
        for (n, d) in &shapes {
            b.extend(n.to_le_bytes());
            b.extend(u16::try_from(d.len()).unwrap_or(0).to_le_bytes());
        }
        for (_, d) in &shapes {
            b.extend(d);
        }
        ShxFont::parse("h", &b).expect("parses")
    }

    #[test]
    fn hebrew_letters_find_their_one_byte_positions() {
        // Code page 1255: the 27 letters at 0xE0..0xFA.
        let f = font_with((0xE0..=0xFA).filter(|c| *c != 0xE1));
        assert_eq!(f.code_for('א'), Some(0xE0));
        assert_eq!(f.code_for('ב'), None);
        // Code page 862: the letters at 0x80..0x9A.
        let f = font_with(0x80..=0x9A);
        assert_eq!(f.code_for('א'), Some(0x80));
        // A Cyrillic-style font (0xC0..0xFF all drawn) is not taken for Hebrew.
        let f = font_with(0xC0..=0xFF);
        assert_eq!(f.code_for('א'), None);
    }

    /// A big font whose records all point at the same large body is read with a bounded copy,
    /// and a character beyond one byte is not looked up in it by its Unicode code point (#2316
    /// review).
    #[test]
    fn big_fonts_are_bounded_and_not_read_by_unicode() {
        let mut b = b"AutoCAD-86 bigfont 1.0\r\n\x1a".to_vec();
        let count: u16 = 60_000;
        b.extend(count.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        let records = b.len() + usize::from(count) * 8;
        let body_len: u16 = 60_000;
        for n in 0..count {
            b.extend(n.wrapping_add(1).to_le_bytes());
            b.extend(body_len.to_le_bytes());
            b.extend(u32::try_from(records).unwrap_or(0).to_le_bytes());
        }
        // One shared body: a name, then a short program.
        b.extend(b"X\0\x14\0");
        b.resize(records + usize::from(body_len), 0);
        let f = ShxFont::parse("big", &b).expect("a big font");
        let held: usize = f.shapes.values().map(Vec::len).sum();
        assert!(held <= (MAX_FILE_BYTES as usize) + usize::from(body_len), "{held} bytes held");
        // 中 (U+4E2D) is not a double-byte code of the font's code page: not looked up.
        assert!(f.has(1));
        assert_eq!(f.code_for('中'), None);
    }

    #[test]
    fn hostile_bytes_never_panic() {
        let good = sample();
        for cut in 0..good.len() {
            let _ = ShxFont::parse("x", good.get(..cut).unwrap_or_default());
        }
        let mut bad = good.clone();
        for i in 24..bad.len() {
            if let Some(b) = bad.get_mut(i) {
                *b = b.wrapping_mul(37).wrapping_add(11);
            }
            if let Some(f) = ShxFont::parse("x", &bad) {
                for c in 0..300 {
                    let _ = f.glyph(c);
                }
            }
        }
        // A shape that calls itself stops.
        let mut b = b"AutoCAD-86 shapes 1.0\r\n\x1a".to_vec();
        let shapes: Vec<(u16, Vec<u8>)> = vec![(0, b"T\0\x04\x01\x00\x00".to_vec()), (65, b"\0\x07\x41\x10\x00".to_vec())];
        b.extend(0u16.to_le_bytes());
        b.extend(65u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        for (n, d) in &shapes {
            b.extend(n.to_le_bytes());
            b.extend(u16::try_from(d.len()).unwrap_or(0).to_le_bytes());
        }
        for (_, d) in &shapes {
            b.extend(d);
        }
        let f = ShxFont::parse("loop", &b).expect("parses");
        assert!(f.glyph(65).is_some());
    }
}

#[cfg(test)]
mod survey {
    /// `CADCRAFT_SHX_DIR=… cargo test -p cadcraft-fonts survey -- --ignored --nocapture`: how each
    /// font of a folder is read (a look at real fonts; not run normally).
    #[test]
    #[ignore = "needs a folder of fonts"]
    fn survey_fonts() {
        let Ok(dir) = std::env::var("CADCRAFT_SHX_DIR") else { return };
        let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        v.sort();
        for p in v {
            let b = std::fs::read(&p).unwrap();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            match super::ShxFont::parse(&name, &b) {
                Some(f) => println!(
                    "{name}: {:?} 1255={} 862={} E0..FA={} C0..DF={} 80..9A={} A0..AF={} U+05D0={} U+0430={} E0~430={}",
                    f.kind,
                    f.hebrew_1255(),
                    f.hebrew_862(),
                    f.count(0xE0..=0xFA),
                    f.count(0xC0..=0xDF),
                    f.count(0x80..=0x9A),
                    f.count(0xA0..=0xAF),
                    f.has(0x05D0),
                    f.has(0x0430),
                    f.glyph(0xE0).map(|g| g.strokes) == f.glyph(0x430).map(|g| g.strokes)
                ),
                None => println!("{name}: unreadable"),
            }
        }
    }
}
