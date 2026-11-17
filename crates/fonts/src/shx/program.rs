//! Map the shared SHX pen interpreter's neutral strokes into CAD geometry.
use super::{ShxFont, ShxGlyph, ShxKind};
use cadcraft_geom::Vec2;

impl craft_shx::program::Font for ShxFont {
    fn kind(&self) -> ShxKind {
        self.kind
    }
    fn dual(&self) -> bool {
        self.dual
    }
    fn above(&self) -> f64 {
        self.above
    }
    fn char_width(&self) -> f64 {
        self.char_width
    }
    fn program(&self, number: u16) -> Option<&[u8]> {
        ShxFont::program(self, number)
    }
}

pub(super) fn outline(font: &ShxFont, number: u16, vertical: bool) -> Result<ShxGlyph, craft_shx::program::Broken> {
    let glyph = craft_shx::program::outline(font, number, vertical)?;
    Ok(ShxGlyph {
        strokes: glyph.strokes.into_iter().map(|stroke| stroke.into_iter().map(|p| Vec2::new(p.x, p.y)).collect()).collect(),
        advance: Vec2::new(glyph.advance.x, glyph.advance.y),
    })
}
