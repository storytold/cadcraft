//! Text and shapes embedded in complex linetypes (a gas line's `---- GAS ---- GAS ----`, a
//! fence's crosses).
//!
//! A dash element may carry a text (DXF `74` bit 2, `9` string) or a shape (`74` bit 4, `75`
//! shape number). As in a `.lin` definition, where the bracketed text or shape follows the dash
//! or gap that carries it, it is placed where that element ends, at every repeat of the pattern
//! (laid out from the start of the run like [`crate::linetype::apply`]):
//! - height: the element's scale (`46`), times the text style's fixed height when it has one,
//!   times the linetype scale;
//! - origin: the anchor shifted by the X/Y offsets (`44`/`45`, times the linetype scale) along
//!   and across the line;
//! - rotation (`50`): relative to the line direction, or absolute (`74` bit 1);
//! - font, width factor, oblique angle and mirroring from the text style (`340`).
//!
//! A shape comes from the SHX shape file its style (`340`) names, as the font table finds it
//! ([`cadcraft_fonts::shx::find`]): an unnamed shape-file style is read as its file name, a
//! named style by its font. It is drawn at the element's scale times the linetype scale per
//! text height of the file (its font record's `above`, else one vector unit), rotated and
//! offset like text. A shape whose file isn't found draws nothing and leaves its gap empty.

use cadcraft_doc::{Drawing, Linetype};
use cadcraft_fonts::{Shaped, TextParams};
use cadcraft_geom::Vec2;

/// Upper bound on the texts and shapes drawn along one run (hostile scales; `apply` caps the
/// dashes).
const MAX_TEXTS: usize = 10_000;

/// One text or shape element of the pattern, shaped once at the origin.
struct Element {
    /// Distance from the start of a pattern repeat to the anchor (where the element ends).
    at: f64,
    local: Shaped,
    rotation: f64,
    absolute: bool,
    offset: Vec2,
}

/// The texts and shapes of `lt` along the polyline `pts`, in the coordinates of `pts`. Empty
/// when the linetype has none or its scaled pattern is drawn continuous (shorter than
/// `min_pattern`, or repeating too often).
pub fn texts(d: &Drawing, pts: &[Vec2], lt: &Linetype, scale: f64, min_pattern: f64) -> Vec<Shaped> {
    let mut out = Vec::new();
    if pts.len() < 2 || !lt.pattern.iter().any(|e| e.text.as_deref().is_some_and(|t| !t.trim().is_empty()) || e.shape.is_some()) {
        return out;
    }
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let total = lt.pattern_length() * scale;
    let len: f64 = pts.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum();
    if !(total.is_finite() && total > 1e-12 && len.is_finite()) || total < min_pattern || len / total > 50_000.0 {
        return out;
    }
    let elements = elements(d, lt, scale);
    if elements.is_empty() {
        return out;
    }
    let mut walk = Walk { pts, i: 0, start: 0.0 };
    let mut base = 0.0;
    'repeats: while base <= len {
        for el in &elements {
            let s = base + el.at;
            if s > len + 1e-9 || out.len() >= MAX_TEXTS {
                break 'repeats;
            }
            let Some((p, dir)) = walk.at(s) else { break 'repeats };
            let origin = p + dir * el.offset.x + dir.perp() * el.offset.y;
            let rot = if el.absolute { el.rotation } else { dir.angle() + el.rotation };
            let mut sh = el.local.clone();
            sh.map(|q| origin + q.rotate(rot));
            out.push(sh);
        }
        base += total;
    }
    out
}

/// The shape element `el` at the origin (scaled by the element's and the linetype's scale), if
/// its shape file is found and has the shape.
fn shape(d: &Drawing, el: &cadcraft_doc::DashElement, scale: f64) -> Option<Shaped> {
    let number = el.shape.filter(|n| *n != 0)?;
    let style = el.style.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    let file = d.text_style(style).map_or_else(|| style.to_string(), |ts| ts.font.clone());
    let font = cadcraft_fonts::shx::find(&file)?;
    let glyph = font.glyph(number, false)?;
    let k = if el.scale.is_finite() && el.scale > 0.0 { el.scale } else { 1.0 } * scale / font.units_per_height();
    if !(k.is_finite() && k > 0.0) {
        return None;
    }
    let strokes = glyph.strokes.iter().map(|s| s.iter().map(|p| *p * k).collect()).collect();
    Some(Shaped { strokes, width: glyph.advance.x * k, ..Shaped::default() })
}

/// The text and shape elements of `lt`, scaled by `scale`, in pattern order.
fn elements(d: &Drawing, lt: &Linetype, scale: f64) -> Vec<Element> {
    let mut out = Vec::new();
    let mut at = 0.0;
    let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
    for el in &lt.pattern {
        at += el.length.abs() * scale;
        if el.text.is_none()
            && let Some(local) = shape(d, el, scale)
        {
            out.push(Element {
                at,
                local,
                rotation: finite(el.rotation),
                absolute: el.absolute,
                offset: Vec2::new(finite(el.offset.x), finite(el.offset.y)) * scale,
            });
            continue;
        }
        let Some(text) = el.text.as_deref().filter(|t| !t.trim().is_empty()) else { continue };
        let ts = d.text_style(el.style.as_deref().unwrap_or("Standard")).cloned().unwrap_or_default();
        let k = if el.scale.is_finite() && el.scale > 0.0 { el.scale } else { 1.0 };
        let fixed = if ts.height.is_finite() && ts.height > 0.0 { ts.height } else { 1.0 };
        let height = k * fixed * scale;
        if !(height.is_finite() && height > 0.0) {
            continue;
        }
        let font = crate::style_font(&ts);
        let params = TextParams {
            width_factor: if ts.width_factor.is_finite() && ts.width_factor > 0.0 { ts.width_factor } else { 1.0 },
            oblique: if ts.oblique.is_finite() { ts.oblique } else { 0.0 },
            backwards: ts.backwards,
            upside_down: ts.upside_down,
            ..TextParams::new(Vec2::ZERO, height)
        };
        let (local, _) = cadcraft_fonts::place(&font, text, &params);
        out.push(Element {
            at,
            local,
            rotation: finite(el.rotation),
            absolute: el.absolute,
            offset: Vec2::new(finite(el.offset.x), finite(el.offset.y)) * scale,
        });
    }
    out
}

/// Points along a polyline at increasing distances.
struct Walk<'a> {
    pts: &'a [Vec2],
    /// Current segment `pts[i]..pts[i + 1]`.
    i: usize,
    /// Distance from the start of the polyline to `pts[i]`.
    start: f64,
}

impl Walk<'_> {
    /// The point at distance `s` (not less than the previous call's) and the unit direction of
    /// its segment; the last segment is extended for `s` slightly past the end.
    fn at(&mut self, s: f64) -> Option<(Vec2, Vec2)> {
        loop {
            let (a, b) = (*self.pts.get(self.i)?, *self.pts.get(self.i + 1)?);
            let seg = a.dist(b);
            let last = self.i + 2 >= self.pts.len();
            if seg > 1e-15 && (s <= self.start + seg || last) {
                let dir = (b - a) / seg;
                return Some((a + dir * (s - self.start), dir));
            }
            if last {
                return None;
            }
            self.start += seg;
            self.i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::DashElement;

    fn gas() -> Linetype {
        Linetype {
            name: "GAS".into(),
            description: String::new(),
            pattern: vec![
                DashElement::dash(0.5),
                DashElement { text: Some("G".into()), scale: 0.1, ..DashElement::dash(-0.2) },
                DashElement::dash(-0.25),
            ],
        }
    }

    /// The leftmost x of each text.
    fn starts(sh: &[Shaped]) -> Vec<f64> {
        sh.iter().map(|s| s.strokes.iter().flatten().map(|p| p.x).fold(f64::INFINITY, f64::min)).collect()
    }

    #[test]
    fn one_text_per_repeat_at_the_end_of_its_element() {
        let d = Drawing::new_imperial();
        let t = texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &gas(), 1.0, 0.0);
        // Pattern 0.95 long: anchors at 0.7 and 1.65.
        assert_eq!(t.len(), 2);
        let x = starts(&t);
        assert!(x[0] >= 0.69 && x[0] < 0.75, "{x:?}");
        assert!(x[1] >= 1.64 && x[1] < 1.70, "{x:?}");
    }

    #[test]
    fn no_text_when_drawn_continuous() {
        let d = Drawing::new_imperial();
        assert!(texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &gas(), 1.0, 5.0).is_empty());
        let plain = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        assert!(texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &plain, 1.0, 0.0).is_empty());
    }

    /// A shape file written for this test: shape 130 a 2 x 2 box with its pen ending at x = 2.
    fn register_shapes() {
        let src = "*130,7,BOX\n1,020,024,028,02C,2,020,0\n";
        cadcraft_fonts::shx::register("cadcraft-render-lt.shx", &cadcraft_fonts::shx::compile(src).unwrap()).unwrap();
    }

    fn fence(style: &str) -> Linetype {
        Linetype {
            name: "FENCE".into(),
            description: String::new(),
            pattern: vec![
                DashElement::dash(0.5),
                DashElement { shape: Some(130), style: Some(style.into()), scale: 0.1, ..DashElement::dash(-0.2) },
                DashElement::dash(-0.25),
            ],
        }
    }

    #[test]
    fn shape_elements_draw_their_shape_at_each_repeat() {
        register_shapes();
        let d = Drawing::new_imperial();
        // An unnamed shape-file style reads as its file name (DXF); scale 0.1 per vector unit.
        let t = texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &fence("cadcraft-render-lt.shx"), 1.0, 0.0);
        assert_eq!(t.len(), 2);
        let x = starts(&t);
        assert!((x[0] - 0.7).abs() < 1e-9 && (x[1] - 1.65).abs() < 1e-9, "{x:?}");
        let ys = t[0].strokes.iter().flatten().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
        assert!((ys.0).abs() < 1e-9 && (ys.1 - 0.2).abs() < 1e-9, "{ys:?}");
        // Along a vertical line the shape turns with it; absolute rotation keeps it level.
        let t = texts(&d, &[Vec2::ZERO, Vec2::new(0.0, 2.0)], &fence("cadcraft-render-lt.shx"), 2.0, 0.0);
        assert_eq!(t.len(), 1);
        let xs = t[0].strokes.iter().flatten().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
        assert!((xs.0 + 0.4).abs() < 1e-9 && xs.1.abs() < 1e-9, "{xs:?}");
        // A named style whose font is the shape file.
        let mut d = Drawing::new_imperial();
        d.text_styles.push(cadcraft_doc::TextStyle { name: "SHAPES".into(), font: "cadcraft-render-lt.shx".into(), ..Default::default() });
        assert_eq!(texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &fence("SHAPES"), 1.0, 0.0).len(), 2);
        // A shape file that isn't found, or a shape it lacks, draws nothing.
        assert!(texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &fence("cadcraft-no-such-file.shx"), 1.0, 0.0).is_empty());
        let mut lt = fence("cadcraft-render-lt.shx");
        lt.pattern[1].shape = Some(131);
        assert!(texts(&d, &[Vec2::ZERO, Vec2::new(2.0, 0.0)], &lt, 1.0, 0.0).is_empty());
    }

    #[test]
    fn text_in_an_shx_style_uses_the_font_metrics() {
        // A text font written for this test: A is 4 wide with a gap of 2, cap height 6.
        let src = "*0,4,render test\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";
        cadcraft_fonts::shx::register("cadcraft-render-text.shx", &cadcraft_fonts::shx::compile(src).unwrap()).unwrap();
        let mut d = Drawing::new_imperial();
        d.text_styles.push(cadcraft_doc::TextStyle { name: "SHX".into(), font: "cadcraft-render-text.shx".into(), ..Default::default() });
        let t = cadcraft_doc::Text {
            insert: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0),
            align_pt: None,
            height: 3.0,
            value: "AA".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "SHX".into(),
            halign: Default::default(),
            valign: Default::default(),
        };
        let (sh, bb) = crate::place_text_entity(&d, &t, "AA");
        assert!((sh.width - 6.0).abs() < 1e-9 && (bb.width() - 6.0).abs() < 1e-9);
        let maxy = sh.strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((maxy - 3.0).abs() < 1e-9);
    }

    #[test]
    fn hostile_values_never_panic() {
        let d = Drawing::new_imperial();
        let mut lt = gas();
        for el in &mut lt.pattern {
            el.scale = f64::NAN;
            el.offset = Vec2::new(f64::INFINITY, f64::NAN);
            el.rotation = f64::NEG_INFINITY;
        }
        let pts = [Vec2::ZERO, Vec2::ZERO, Vec2::new(1e6, 0.0), Vec2::new(1e6, 0.0)];
        for scale in [f64::NAN, 0.0, -1.0, 1e-9, 1e300] {
            let t = texts(&d, &pts, &lt, scale, 0.0);
            assert!(t.len() <= MAX_TEXTS);
        }
    }
}
