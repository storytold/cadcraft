//! Real text in PDF plots: the outline-font glyphs a plot draws ([`DisplayList::texts`]) are
//! written as text in their font, which the PDF writer embeds as a subset with a `ToUnicode`
//! map, so the text can be selected, searched and copied.
//!
//! Fonts whose licence bits forbid embedding a subset, fonts without TrueType outlines, and
//! anything past the caps on fonts and font data stay as filled outlines.

use std::collections::HashMap;

use cadcraft_fonts::subset::{self, Embedding};
use cadcraft_fonts::ttf::Face;
use cadcraft_render::{DisplayList, PlotText};
use craft_pdf::write::backend::text::Font;

/// Most embedded fonts in one file.
const MAX_FONTS: usize = 64;
/// Most font file data embedded from in one file (bytes of the font files the subsets are
/// taken from).
const MAX_FONT_BYTES: usize = 256 << 20;

/// A face's identity: its loaded file and face index.
type FaceKey = (usize, usize, u32);

fn key(f: &Face) -> FaceKey {
    (f.bytes.as_ptr() as usize, f.bytes.len(), f.index)
}

/// The fonts of a document, decided face by face as glyphs are written.
pub(super) struct Fonts {
    /// Off: every glyph stays an outline.
    enabled: bool,
    /// Each face seen, with its font when it is embedded. The face is kept so its key isn't
    /// reused by another file loaded at the same address.
    by_face: HashMap<FaceKey, (Face, Option<Font>)>,
    count: usize,
    bytes: usize,
}

impl Fonts {
    pub(super) fn new(enabled: bool) -> Fonts {
        Fonts { enabled, by_face: HashMap::new(), count: 0, bytes: 0 }
    }

    /// The font a glyph of `face` is written in; `None`: draw its outline.
    pub(super) fn font(&mut self, face: &Face) -> Option<Font> {
        if !self.enabled {
            return None;
        }
        let k = key(face);
        if let Some((_, f)) = self.by_face.get(&k) {
            return f.clone();
        }
        let len = face.bytes.len();
        let font = (self.count < MAX_FONTS && self.bytes.saturating_add(len) <= MAX_FONT_BYTES && subset::embedding(face) == Embedding::Allowed)
            .then(|| Font::new(face.bytes.clone().into(), face.index))
            .flatten();
        if font.is_some() {
            self.count += 1;
            self.bytes = self.bytes.saturating_add(len);
        }
        self.by_face.insert(k, (face.clone(), font.clone()));
        font
    }
}

/// The text glyphs of a list that stand for a primitive of it, by primitive.
pub(super) fn texts(list: &DisplayList) -> HashMap<u32, &PlotText> {
    list.texts.iter().filter(|t| (t.prim as usize) < list.prims.len() && t.glyph.gid != 0).map(|t| (t.prim, t)).collect()
}
