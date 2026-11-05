//! SHX shape fonts: text fonts, big fonts (East Asian double-byte) and Unicode fonts that a
//! drawing's text styles name, read at run time from files the user has. None is bundled.
//!
//! - [`ShxFont::parse`] reads the compiled containers (`shapes 1.0/1.1`, `bigfont 1.0`,
//!   `unifont 1.0`) and [`ShxFont::glyph`] runs a shape's pen program (the vector length and
//!   direction codes and special codes 0–14 of the public shape description language) into
//!   polylines in font units. Every count, offset and loop in a file is bounded: a hostile or
//!   truncated file never panics, it reads as far as it is valid or not at all.
//! - [`compile`] turns shape source (`.shp`) into a compiled file, as COMPILE does. The tests
//!   build every font they use with it: no font file is in this repository.
//! - Fonts are found by name through the font table (`table`): registered bytes
//!   ([`register`], the web build's files opened alongside a drawing), an app-set
//!   [`FontProvider`], the folders of opened drawings ([`add_drawing_dir`]) and the support
//!   search path ([`set_search_path`], `CADCRAFT_SHXPATH`), after the font map
//!   ([`set_font_map`]). CADCraft never looks inside another CAD program's installation.
//! - [`ShxText`] is a text style's pair of fonts (main + big font) and orientation, which the
//!   text layout draws ([`crate::TextFont::Shx`]): advances are the pen's end positions, so
//!   widths and extents follow the font's own metrics.
//!
//! Binary container layouts (little-endian): a signature line ending in CR LF 0x1A, then
//! - shapes: first and last shape number, count, an index of (number, record length), the
//!   records (a NUL-terminated name, then the program) and `EOF`;
//! - big font: index size, count, escape-range count, the ranges (first, last lead byte),
//!   an index of (number, record length, file offset) and the records;
//! - unifont: shape count (32 bits), the length of the font record, the font record (name,
//!   above, below, modes, encoding, embedding, 0) and then each shape as (number, length,
//!   record).

mod compile;
mod parse;
mod program;
mod table;
#[cfg(test)]
mod tests;
mod text;

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use cadcraft_geom::Vec2;

pub use compile::compile;
pub use table::{
    FontProvider, add_drawing_dir, clear_registered, drawing_dirs, find, font_map, generation, register, search_path, set_font_map, set_provider,
    set_search_path,
};
pub use text::ShxText;
pub(crate) use text::{shape, width};

/// Why a font file can't be read or a shape source can't be compiled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShxError {
    /// Not an SHX file (unknown signature).
    NotShx,
    /// The file ends inside its header or index.
    Truncated,
    /// The file is larger, or has more of something, than CADCraft reads.
    TooLarge,
    /// The file is well-formed but holds no shape.
    Empty,
    /// Shape source error: line and message.
    Source(usize, String),
}

impl std::fmt::Display for ShxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShxError::NotShx => f.write_str("not an SHX shape or font file"),
            ShxError::Truncated => f.write_str("the SHX file is truncated"),
            ShxError::TooLarge => f.write_str("the SHX file is too large"),
            ShxError::Empty => f.write_str("the SHX file has no shapes"),
            ShxError::Source(line, msg) => write!(f, "line {line}: {msg}"),
        }
    }
}

impl std::error::Error for ShxError {}

/// The kind of SHX file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShxKind {
    /// A shape file or a text font with single-byte character codes (`shapes 1.0/1.1`).
    Shapes,
    /// A big font: double-byte character codes after an escape (lead) byte.
    BigFont,
    /// A Unicode font: shape numbers are Unicode code points.
    Unifont,
}

/// A shape's program run into polylines, in font units (pen starts at the origin).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShxGlyph {
    /// Pen-down runs (two points or more).
    pub strokes: Vec<Vec<Vec2>>,
    /// Where the pen ends: the advance to the next character.
    pub advance: Vec2,
}

/// One shape record: where its name and program are in [`ShxFont::data`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Record {
    name: (usize, usize),
    program: (usize, usize),
}

/// Glyphs cached per font, keyed by (shape, vertical).
type GlyphCache = HashMap<(u16, bool), Option<Arc<ShxGlyph>>>;

/// Most glyphs cached per font (a big font has thousands; text uses a few hundred).
const MAX_CACHED_GLYPHS: usize = 16_384;

/// A parsed SHX file.
pub struct ShxFont {
    kind: ShxKind,
    name: String,
    /// Shape 0 (the font record) is present: the file is a text font, not only shapes.
    header: bool,
    above: f64,
    below: f64,
    /// Modes 2: horizontal and vertical (code 14 is honoured).
    dual: bool,
    /// Unifont encoding: 0 Unicode, 1 packed multibyte, 2 shape file.
    encoding: u8,
    /// Extended big font: the font record gives a character width (subshape cell).
    char_width: f64,
    /// Big font escape ranges (lead bytes).
    ranges: Vec<(u16, u16)>,
    data: Vec<u8>,
    records: HashMap<u16, Record>,
    cache: RwLock<GlyphCache>,
}

impl std::fmt::Debug for ShxFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShxFont")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("above", &self.above)
            .field("below", &self.below)
            .field("dual", &self.dual)
            .field("shapes", &self.records.len())
            .finish()
    }
}

impl ShxFont {
    /// Parse a compiled SHX file.
    pub fn parse(bytes: &[u8]) -> Result<ShxFont, ShxError> {
        parse::parse(bytes)
    }
    pub fn kind(&self) -> ShxKind {
        self.kind
    }
    /// The font name in its font record (shape 0), or empty.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Whether the file has a font record (shape 0): a text font rather than a shape file.
    pub fn is_font(&self) -> bool {
        self.header
    }
    /// Vector units uppercase letters rise above the baseline (big font: the character height).
    /// Text height `h` draws the font at `h / above` per unit. 0 for a shape file.
    pub fn above(&self) -> f64 {
        self.above
    }
    /// Vector units lowercase descenders drop below the baseline.
    pub fn below(&self) -> f64 {
        self.below
    }
    /// The font supports vertical text (modes 2).
    pub fn is_dual(&self) -> bool {
        self.dual
    }
    /// The units per text height: [`Self::above`], or 1 for a file without a font record.
    pub fn units_per_height(&self) -> f64 {
        if self.above > 0.0 { self.above } else { 1.0 }
    }
    /// Big font escape ranges: `(first, last)` lead bytes.
    pub fn escape_ranges(&self) -> &[(u16, u16)] {
        &self.ranges
    }
    /// Whether `b` starts a double-byte character in this big font.
    pub fn is_lead_byte(&self, b: u8) -> bool {
        self.ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&u16::from(b)))
    }
    /// Unifont encoding (0 Unicode, 1 packed multibyte, 2 shape file); 0 for other kinds.
    pub fn encoding(&self) -> u8 {
        self.encoding
    }
    pub fn shape_count(&self) -> usize {
        self.records.len()
    }
    pub fn has_shape(&self, number: u16) -> bool {
        number != 0 && self.records.contains_key(&number)
    }
    /// The shape numbers in the file, sorted (not shape 0).
    pub fn shape_numbers(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.records.keys().copied().filter(|n| *n != 0).collect();
        v.sort_unstable();
        v
    }
    /// The name of shape `number` as the file stores it.
    pub fn shape_name(&self, number: u16) -> Option<String> {
        let r = self.records.get(&number)?;
        self.data.get(r.name.0..r.name.1).map(|b| b.iter().map(|c| char::from(*c)).collect())
    }
    /// The number of the shape called `name` (case-insensitive), as a linetype's
    /// `[SHAPE,file.shx]` element names it.
    pub fn shape_number(&self, name: &str) -> Option<u16> {
        let want = name.trim();
        if want.is_empty() {
            return None;
        }
        let mut found: Vec<u16> = self
            .records
            .iter()
            .filter(|(n, r)| **n != 0 && self.data.get(r.name.0..r.name.1).is_some_and(|b| b.eq_ignore_ascii_case(want.as_bytes())))
            .map(|(n, _)| *n)
            .collect();
        found.sort_unstable();
        found.first().copied()
    }
    /// The program bytes of shape `number`.
    pub(crate) fn program(&self, number: u16) -> Option<&[u8]> {
        let r = self.records.get(&number)?;
        self.data.get(r.program.0..r.program.1)
    }
    /// Shape `number` drawn horizontally or vertically, in font units; `None` when the font
    /// has no such shape or its program is broken. Cached.
    pub fn glyph(&self, number: u16, vertical: bool) -> Option<Arc<ShxGlyph>> {
        if !self.has_shape(number) {
            return None;
        }
        let vertical = vertical && self.dual;
        let key = (number, vertical);
        if let Some(g) = self.cache.read().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return g.clone();
        }
        let g = program::outline(self, number, vertical).ok().map(Arc::new);
        let mut c = self.cache.write().unwrap_or_else(PoisonError::into_inner);
        if c.len() >= MAX_CACHED_GLYPHS {
            c.clear();
        }
        c.insert(key, g.clone());
        g
    }
}
