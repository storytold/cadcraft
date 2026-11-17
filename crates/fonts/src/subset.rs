//! Whether a font may be embedded as a subset in exported files (PDF), from its licence bits
//! (OS/2 `fsType`): fonts marked "restricted licence", "no subsetting" or "bitmap only" are
//! never embedded; callers draw their text as outlines instead. Fonts without TrueType outlines
//! (CFF-flavoured OpenType) aren't embedded either.

use crate::ttf::Face;

/// Whether a face may be embedded as a subset.
pub use craft_font_catalog::SubsetEmbedding as Embedding;

/// The embedding permission of a face.
pub fn embedding(face: &Face) -> Embedding {
    craft_font_catalog::subset_embedding(&face.bytes, face.index)
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
