//! The text editors show text in the drawing's own fonts: each character is drawn with the
//! drawing's shaping (SHX strokes, TrueType outlines, the built-in stroke font and its fallbacks)
//! over an invisible egui text box whose advances are set to the font's, so the caret, the
//! selection and the glyphs line up.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use cadcraft_fonts::{Shaped, TextFont};
use egui::{Color32, Pos2, Stroke};

/// Most characters kept shaped.
const MAX_CACHED: usize = 4096;
/// The egui font's cap height as a share of its size: drawing fonts are scaled so their cap
/// height (the text height) matches it.
pub const CAP: f32 = 0.7;
/// Slant of italic text (radians), as the style's oblique angle adds to.
const ITALIC: f64 = 0.2618;

/// The font a run of the editor is set in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorFont {
    /// Style font file or inline family (`romans.shx`, `arial.ttf`, `Arial`).
    pub font: String,
    pub big_font: String,
    pub width_factor: f64,
    /// Radians.
    pub oblique: f64,
}

impl EditorFont {
    fn wf(&self) -> f64 {
        if self.width_factor.is_finite() && self.width_factor > 0.01 { self.width_factor.min(100.0) } else { 1.0 }
    }
}

type Key = (String, String, u64, u64, char);

struct Glyph {
    shaped: Shaped,
    /// Advance at cap height 1.
    advance: f64,
}

fn cache() -> &'static Mutex<HashMap<Key, std::sync::Arc<Glyph>>> {
    static C: OnceLock<Mutex<HashMap<Key, std::sync::Arc<Glyph>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fonts() -> &'static Mutex<HashMap<(String, String), TextFont>> {
    static F: OnceLock<Mutex<HashMap<(String, String), TextFont>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve(font: &str, big: &str) -> TextFont {
    let k = (font.to_string(), big.to_string());
    let mut f = fonts().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(t) = f.get(&k) {
        return t.clone();
    }
    let t = TextFont::resolve_style(font, big);
    if f.len() > 64 {
        f.clear();
    }
    f.insert(k, t.clone());
    t
}

/// Forget resolved fonts (the user's font folders changed).
pub fn clear() {
    fonts().lock().unwrap_or_else(PoisonError::into_inner).clear();
    cache().lock().unwrap_or_else(PoisonError::into_inner).clear();
}

fn glyph(f: &EditorFont, italic: bool, c: char) -> std::sync::Arc<Glyph> {
    let oblique = f.oblique + if italic { ITALIC } else { 0.0 };
    let key = (f.font.clone(), f.big_font.clone(), f.wf().to_bits(), oblique.to_bits(), c);
    if let Some(g) = cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return g.clone();
    }
    let font = resolve(&f.font, &f.big_font);
    let s = c.to_string();
    let shaped = if c.is_whitespace() { Shaped::default() } else { cadcraft_fonts::shape_line(&font, &s, 1.0, f.wf(), oblique) };
    // A line's width leaves out the gap after its last character: the advance is what a second
    // copy of the character adds.
    let two: String = [c, c].iter().collect();
    let mut advance = cadcraft_fonts::text_width(&font, &two, 1.0, f.wf()) - cadcraft_fonts::text_width(&font, &s, 1.0, f.wf());
    if !advance.is_finite() || advance <= 0.0 {
        advance = if c.is_whitespace() { cadcraft_fonts::space_width(&font, 1.0, f.wf()) } else { shaped.width };
    }
    let g = std::sync::Arc::new(Glyph { shaped, advance: if advance.is_finite() { advance.clamp(0.0, 20.0) } else { 0.6 } });
    let mut c2 = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if c2.len() >= MAX_CACHED {
        c2.clear();
    }
    c2.insert(key, g.clone());
    g
}

/// Advance of `c` in `f`, in points, at egui font size `size`.
pub fn advance(f: &EditorFont, italic: bool, c: char, size: f32) -> f32 {
    (glyph(f, italic, c).advance as f32) * size * CAP
}

/// Draw `c` in `f` with its baseline-left at `at` (screen), at egui font size `size`.
pub fn paint(painter: &egui::Painter, f: &EditorFont, italic: bool, bold: bool, c: char, at: Pos2, size: f32, color: Color32) {
    if c.is_whitespace() || c.is_control() {
        return;
    }
    let g = glyph(f, italic, c);
    let k = size * CAP;
    let map = |p: cadcraft_geom::Vec2| Pos2::new(at.x + p.x as f32 * k, at.y - p.y as f32 * k);
    let w = (k / if bold { 7.0 } else { 12.0 }).clamp(1.0, 4.0);
    for s in &g.shaped.strokes {
        let pts: Vec<Pos2> = s.iter().map(|p| map(*p)).collect();
        if pts.len() == 1
            && let Some(p) = pts.first()
        {
            painter.circle_filled(*p, w / 2.0, color);
        } else if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, Stroke::new(w, color)));
        }
    }
    for contours in &g.shaped.glyphs {
        let tris = cadcraft_render::triangulate_glyph(contours);
        if tris.len() < 3 {
            continue;
        }
        let mut mesh = egui::Mesh::default();
        for p in &tris {
            mesh.colored_vertex(map(*p), color);
        }
        mesh.indices = (0..u32::try_from(mesh.vertices.len()).unwrap_or(0)).collect();
        painter.add(egui::Shape::mesh(mesh));
        if bold {
            for ct in contours {
                let mut pts: Vec<Pos2> = ct.iter().map(|p| map(*p)).collect();
                if let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                painter.add(egui::Shape::line(pts, Stroke::new((k / 30.0).clamp(0.5, 2.0), color)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stroke_font_gives_advances_and_strokes() {
        let f = EditorFont { font: String::new(), big_font: String::new(), width_factor: 1.0, oblique: 0.0 };
        let a = advance(&f, false, 'A', 20.0);
        let i = advance(&f, false, 'i', 20.0);
        assert!(a > 5.0 && a < 30.0, "{a}");
        assert!(i < a, "an i is narrower than an A");
        let wide = EditorFont { width_factor: 2.0, ..f.clone() };
        assert!(advance(&wide, false, 'A', 20.0) > a * 1.5);
        assert!(advance(&f, false, ' ', 20.0) > 0.0);
        assert!(!glyph(&f, false, 'A').shaped.strokes.is_empty());
    }
}
