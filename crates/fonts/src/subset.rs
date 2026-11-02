//! Whether a font may be embedded as a subset in exported files (PDF), from its licence bits
//! (OS/2 `fsType`): fonts marked "restricted licence", "no subsetting" or "bitmap only" are
//! never embedded; callers draw their text as outlines instead. Fonts without TrueType outlines
//! (CFF-flavoured OpenType) aren't embedded either.

use skrifa::FontRef;
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::types::Tag;

use crate::ttf::Face;

/// A glyph's horizontal advance in thousandths of an em (the units SVG measures text in), from
/// the face's own metrics: 0 when the face or the glyph has none.
pub fn advance(face: &Face, gid: u16) -> f64 {
    use skrifa::MetadataProvider as _;
    let Some(font) = face.font() else { return 0.0 };
    font.glyph_metrics(Size::new(1000.0), LocationRef::default()).advance_width(skrifa::GlyphId::new(u32::from(gid))).map_or(0.0, f64::from)
}

/// Whether a face may be embedded as a subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Embedding {
    /// Installable, editable or preview & print embedding (or no OS/2 table).
    Allowed,
    /// `fsType` restricted licence: no embedding.
    Restricted,
    /// `fsType` bit 8: the font may only be embedded whole.
    NoSubsetting,
    /// `fsType` bit 9: only bitmaps may be embedded.
    BitmapOnly,
    /// No TrueType outlines (`glyf` / `loca`), or the font doesn't parse.
    NotTrueType,
}

fn table<'a>(f: &FontRef<'a>, t: &[u8; 4]) -> Option<&'a [u8]> {
    f.table_data(Tag::new(t)).map(|d| d.as_bytes())
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

/// The embedding permission of a face.
pub fn embedding(face: &Face) -> Embedding {
    let Ok(f) = FontRef::from_index(&face.bytes, face.index) else { return Embedding::NotTrueType };
    if table(&f, b"glyf").is_none() || table(&f, b"loca").is_none() || table(&f, b"head").is_none() {
        return Embedding::NotTrueType;
    }
    let Some(fs_type) = table(&f, b"OS/2").and_then(|o| u16_at(o, 8)) else { return Embedding::Allowed };
    if fs_type & 0x0200 != 0 {
        Embedding::BitmapOnly
    } else if fs_type & 0x000F == 0x0002 {
        Embedding::Restricted
    } else if fs_type & 0x0100 != 0 {
        Embedding::NoSubsetting
    } else {
        Embedding::Allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_fonts_are_not_embedded() {
        let junk = Face { bytes: std::sync::Arc::new(b"not a font".to_vec()), index: 0 };
        assert_eq!(embedding(&junk), Embedding::NotTrueType);
        assert_eq!(embedding(&Face { bytes: std::sync::Arc::new(Vec::new()), index: 7 }), Embedding::NotTrueType);
    }
}
