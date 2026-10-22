//! SVG export of a space (vector, for the web and documents).

use std::collections::HashMap;
use std::fmt::Write as _;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Space};
use cadcraft_fonts::TextGlyph;
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{DisplayList, Kind, PlotText};
use serde_json::Value;

/// How SVG export writes TEXT and MTEXT set in an installed outline font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextMode {
    /// As `<text>` elements: searchable and editable, with their font, size, position and
    /// rotation (the default). Stroke-font text is always geometry.
    #[default]
    Text,
    /// As glyph outlines: exactly as plotted, with no text elements.
    Geometry,
}

impl TextMode {
    /// The `text` option of an export or plot: `"geometry"` keeps glyph outlines; anything else
    /// (or no option) writes text.
    pub fn from_json(v: &Value) -> TextMode {
        if v.get("text").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("geometry")) { TextMode::Geometry } else { TextMode::Text }
    }
}

/// Export with a white background (colour 7 prints black), fitted to the extents.
pub fn export(d: &Drawing, space: &Space) -> String {
    export_window(d, space, None)
}

/// Export showing exactly `window` (the `viewBox`; geometry outside it is clipped by the viewer),
/// or the extents with a 2% margin when `window` is `None`. Objects are drawn with their
/// transparency, as on screen; outline-font text is written as text.
pub fn export_window(d: &Drawing, space: &Space, window: Option<Bounds2>) -> String {
    export_window_with(d, space, window, true)
}

/// [`export_window`], drawing transparency only when `transparency` is on (plots).
pub fn export_window_with(d: &Drawing, space: &Space, window: Option<Bounds2>, transparency: bool) -> String {
    export_window_with_text(d, space, window, transparency, TextMode::Text)
}

/// [`export_window_with`], writing text or its glyph outlines per `text`.
pub fn export_window_with_text(d: &Drawing, space: &Space, window: Option<Bounds2>, transparency: bool, text: TextMode) -> String {
    let opts = cadcraft_render::Options { transparency, text_glyphs: text == TextMode::Text, ..cadcraft_render::Options::default() };
    let list = cadcraft_render::build(d, space, &opts);
    export_list_with(&list, window, text)
}

/// ` opacity="…"` for a transparent primitive (none for an opaque one or a wipeout's mask).
fn opacity(p: &cadcraft_render::DPrim) -> String {
    if p.kind == Kind::Mask || p.transparency == 0 {
        return String::new();
    }
    format!(r#" opacity="{:.2}""#, f64::from(p.alpha()) / 255.0)
}

/// Write a display list (as [`export_window`] does for a space's own), writing text.
pub fn export_list(list: &DisplayList, window: Option<Bounds2>) -> String {
    export_list_with(list, window, TextMode::Text)
}

/// [`export_list`], writing outline-font text as `<text>` (`text` = [`TextMode::Text`]) or as
/// the glyph outlines the list holds.
pub fn export_list_with(list: &DisplayList, window: Option<Bounds2>, text: TextMode) -> String {
    let window = window.filter(|w| !w.is_empty());
    let b = window.unwrap_or(list.bounds);
    // A single point (or a zero-size window) has no extent: show a unit box centred on it.
    let point = !b.is_empty() && b.width().max(b.height()) <= 1e-9;
    let (w, h) = if b.is_empty() {
        (1.0, 1.0)
    } else if point {
        (0.0, 0.0)
    } else {
        (b.width().max(1e-9), b.height().max(1e-9))
    };
    let m = if point {
        0.5
    } else if window.is_some() {
        0.0
    } else {
        (w.max(h)) * 0.02
    };
    let (x0, y1) = if b.is_empty() { (0.0, 1.0) } else { (b.min.x - m, b.max.y + m) };
    let vw = w + 2.0 * m;
    let vh = h + 2.0 * m;
    let stroke = vw.max(vh) / 1500.0;
    // Decimals: at least 6, more when the stroke would otherwise lose its leading digits.
    let dp = (3.0 - stroke.log10().floor()).clamp(6.0, 15.0) as usize;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {vw:.dp$} {vh:.dp$}" width="{:.0}" height="{:.0}"><rect width="100%" height="100%" fill="white"/>"#,
        1600.0,
        1600.0 * vh / vw
    );
    let white = Rgb(255, 255, 255);
    let tx = |x: f64| x - x0;
    let ty = |y: f64| y1 - y;
    // Text runs (and the glyph outlines they replace), when text is written as text.
    let runs = if text == TextMode::Text { text_runs(list) } else { Vec::new() };
    let mut run_at: HashMap<u32, usize> = HashMap::new();
    let mut replaced = vec![false; list.prims.len()];
    for (i, run) in runs.iter().enumerate() {
        run_at.insert(run.first, i);
        for p in run.prims.iter().filter_map(|p| usize::try_from(*p).ok()) {
            if let Some(slot) = replaced.get_mut(p) {
                *slot = true;
            }
        }
    }
    for (i, p) in list.prims.iter().enumerate() {
        if let Some(&ri) = u32::try_from(i).ok().and_then(|k| run_at.get(&k))
            && let Some(run) = runs.get(ri)
        {
            write_run(&mut s, list, run, white, dp, &tx, &ty);
        }
        if replaced.get(i).copied().unwrap_or(false) {
            continue;
        }
        let c = p.display_rgb(white).hex();
        let op = opacity(p);
        let pts = list.points(p);
        match p.kind {
            Kind::Polyline => {
                let _ = write!(
                    s,
                    r#"<polyline fill="none" stroke="{c}"{op} stroke-width="{stroke:.dp$}" stroke-linecap="round" stroke-linejoin="round" points=""#
                );
                for q in pts {
                    let _ = write!(s, "{:.dp$},{:.dp$} ", tx(q.x), ty(q.y));
                }
                s.push_str(r#""/>"#);
            }
            Kind::Tris | Kind::Mask => {
                // A wipeout's mask hides what lies under it with the white background.
                let c = if p.kind == Kind::Mask { white.hex() } else { c };
                let _ = write!(s, r#"<path fill="{c}"{op} d=""#);
                for t in pts.chunks(3) {
                    if let [a, b2, cc] = t {
                        let _ =
                            write!(s, "M{:.dp$} {:.dp$}L{:.dp$} {:.dp$}L{:.dp$} {:.dp$}Z", tx(a.x), ty(a.y), tx(b2.x), ty(b2.y), tx(cc.x), ty(cc.y));
                    }
                }
                s.push_str(r#""/>"#);
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    let _ = write!(s, r#"<circle cx="{:.dp$}" cy="{:.dp$}" r="{:.dp$}" fill="{c}"{op}/>"#, tx(q.x), ty(q.y), stroke);
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(bp), Some(dir)) = (pts.first(), pts.get(1)) {
                    // A window can lie far from the base point: reach past it.
                    let far = (vw + vh) * 2.0 + if window.is_some() { bp.dist(b.center()) } else { 0.0 };
                    let a = if ray { *bp } else { *bp - *dir * far };
                    let e = *bp + *dir * far;
                    let _ = write!(
                        s,
                        r#"<line x1="{:.dp$}" y1="{:.dp$}" x2="{:.dp$}" y2="{:.dp$}" stroke="{c}"{op} stroke-width="{stroke:.dp$}"/>"#,
                        tx(a.x),
                        ty(a.y),
                        tx(e.x),
                        ty(e.y)
                    );
                }
            }
        }
    }
    s.push_str("</svg>");
    s
}

/// One line of text: the glyphs of a text run and where its baseline starts.
struct TextRun {
    /// The primitive of the first glyph (the run is written before it and the glyphs' own
    /// outline primitives are left out).
    first: u32,
    prims: Vec<u32>,
    text: String,
    origin: Vec2,
    /// One em along the baseline and one em up, in world units (font-unit axes in em).
    x: Vec2,
    y: Vec2,
    face: cadcraft_fonts::ttf::Face,
    em: f64,
}

/// Most characters written in one `<text>` run; a longer run continues in another element.
const MAX_RUN_CHARS: usize = 100_000;
/// Largest text em (world units) written as text; a hostile height stays outlines.
const MAX_TEXT_EM: f64 = 1.0e12;

/// The outline-font glyphs of `list` grouped into lines of text. Consecutive glyphs of one
/// object, font, colour and orientation form a run; a line break (a jump across the baseline)
/// starts a new one. Glyphs whose placement is degenerate stay outlines.
fn text_runs(list: &DisplayList) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut i = 0;
    while let Some(t) = list.texts.get(i) {
        let Some((em, prim)) = glyph_prim(list, t) else {
            i += 1;
            continue;
        };
        let glyph = &t.glyph;
        let Some(first_char) = text_char(glyph.ch) else {
            i += 1;
            continue;
        };
        let mut run = TextRun {
            first: t.prim,
            prims: vec![t.prim],
            text: String::from(first_char),
            origin: glyph.origin,
            x: glyph.x,
            y: glyph.y,
            face: glyph.face.clone(),
            em,
        };
        let mut prev = t;
        let mut j = i + 1;
        while let Some(n) = list.texts.get(j) {
            let Some((_, nprim)) = glyph_prim(list, n) else { break };
            if nprim.color != prim.color
                || nprim.aci7 != prim.aci7
                || nprim.transparency != prim.transparency
                || nprim.handle != prim.handle
                || n.glyph.face != glyph.face
                || !same_vec(glyph.x, n.glyph.x)
                || !same_vec(glyph.y, n.glyph.y)
                || !on_line(prev, n, em)
            {
                break;
            }
            if run.text.chars().count() >= MAX_RUN_CHARS {
                break;
            }
            let Some(c) = text_char(n.glyph.ch) else { break };
            if space_between(prev, n, em) {
                run.text.push(' ');
            }
            run.text.push(c);
            run.prims.push(n.prim);
            prev = n;
            j += 1;
        }
        if !run.text.is_empty() {
            runs.push(run);
        }
        i = j.max(i + 1);
    }
    runs
}

/// The em size and outline primitive of a plot glyph, when it can be written as text.
fn glyph_prim<'a>(list: &'a DisplayList, t: &PlotText) -> Option<(f64, &'a cadcraft_render::DPrim)> {
    let prim = usize::try_from(t.prim).ok().and_then(|i| list.prims.get(i))?;
    if prim.kind != Kind::Tris || t.glyph.gid == 0 {
        return None;
    }
    let em = glyph_em(&t.glyph)?;
    Some((em, prim))
}

/// One em of a glyph in world units, from its two axes (the determinant gives the area, the
/// baseline axis its width): finite, positive and not hostile.
fn glyph_em(g: &TextGlyph) -> Option<f64> {
    let (xx, xy, yx, yy) = (g.x.x, g.x.y, g.y.x, g.y.y);
    let (ox, oy) = (g.origin.x, g.origin.y);
    if ![xx, xy, yx, yy, ox, oy].iter().all(|v| v.is_finite()) {
        return None;
    }
    let width = xx.hypot(xy);
    if !(width.is_finite() && width > 0.0) {
        return None;
    }
    let em = (xx * yy - xy * yx).abs() / width;
    (em.is_finite() && em > 0.0 && em <= MAX_TEXT_EM).then_some(em)
}

fn same_vec(a: Vec2, b: Vec2) -> bool {
    let scale = 1.0 + a.len().max(b.len());
    a.dist(b) <= 1e-9 * scale
}

/// Whether `next` sits on the same baseline as `prev` (across it by at most a fifth of an em).
fn on_line(prev: &PlotText, next: &PlotText, em: f64) -> bool {
    let x = prev.glyph.x;
    let width = x.len();
    if !(width.is_finite() && width > 0.0) {
        return false;
    }
    let d = next.glyph.origin - prev.glyph.origin;
    let perp = (d.x * x.y - d.y * x.x).abs() / width;
    let along = d.dot(x) / width;
    perp <= 0.2 * em && along >= -0.5 * em
}

/// Whether the gap between two glyphs is a space (a gap wider than the previous glyph's own
/// advance). Whitespace has no outline glyph, so the gap is where the space was drawn.
fn space_between(prev: &PlotText, next: &PlotText, em: f64) -> bool {
    let x = prev.glyph.x;
    let width = x.len();
    if !(width.is_finite() && width > 0.0) {
        return false;
    }
    let advance = cadcraft_fonts::subset::advance(&prev.glyph.face, prev.glyph.gid);
    if !advance.is_finite() {
        return false;
    }
    let expected = advance / 1000.0 * width;
    let along = (next.glyph.origin - prev.glyph.origin).dot(x) / width;
    let extra = along - expected;
    extra > 0.15 * em && extra <= MAX_TEXT_EM
}

/// A character SVG text may hold: not a control character or an XML noncharacter.
fn text_char(c: char) -> Option<char> {
    let u = u32::from(c);
    if c.is_control() || (u & 0xFFFE) == 0xFFFE {
        return None;
    }
    Some(c)
}

/// Write one text run: a `<text>` with the glyphs' font, size and a matrix mapping em units
/// (font size) to world coordinates. SVG's y axis points down, so the matrix includes the flip.
fn write_run(s: &mut String, list: &DisplayList, run: &TextRun, white: Rgb, dp: usize, tx: &impl Fn(f64) -> f64, ty: &impl Fn(f64) -> f64) {
    let Some(p) = usize::try_from(run.first).ok().and_then(|i| list.prims.get(i)) else { return };
    let color = p.display_rgb(white).hex();
    let op = opacity(p);
    let family = run.face.family_name();
    let family = if family.is_empty() { "sans-serif".to_string() } else { escape(&family) };
    let em = run.em;
    let a = run.x.x / em;
    let b = -run.x.y / em;
    let c = run.y.x / em;
    let d = -run.y.y / em;
    let e = tx(run.origin.x);
    let f = ty(run.origin.y);
    if ![a, b, c, d, e, f, em].iter().all(|v| v.is_finite()) {
        return;
    }
    let _ = write!(
        s,
        r#"<text x="0" y="0" transform="matrix({a:.dp$} {b:.dp$} {c:.dp$} {d:.dp$} {e:.dp$} {f:.dp$})" font-family="{family}" font-size="{em:.dp$}" text-anchor="start" fill="{color}"{op}>"#
    );
    escape_into(s, &run.text);
    s.push_str("</text>");
}

/// `s` with `&`, `<`, `>`, `"` and `'` escaped for XML, and characters XML can't hold left out.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(&mut out, s);
    out
}

fn escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => {
                if let Some(c) = text_char(c) {
                    out.push(c);
                }
            }
        }
    }
}
