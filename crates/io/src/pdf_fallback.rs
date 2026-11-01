//! Conservative PDF graphics interpreter for unlayered PDFs with clipping.
//!
//! Device-space paths are flattened with a bounded tolerance and intersected with actual clip
//! polygons. The adapter never mutates the source PDF and never invents source CAD layer names.
use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, Hatch, HatchLoop, Line, LwPolyline, Space, Text, VAlign};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use craft_pdf::interpret::{
    BlendMode, ClipPath, Context, Device, FillRule, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, RectExt,
    SoftMask, TransformExt, font::Glyph,
};
use geo::{Area, BooleanOps, BoundingRect, Contains, Coord, LineString, MultiLineString, MultiPolygon, Point as GPoint, Polygon, Rect as GRect};
use kurbo::{Affine, BezPath, PathEl, Rect, Shape};

use crate::{IoError, Result};
const PT_TO_MM: f64 = 25.4 / 72.0;
const FLATNESS: f64 = 0.04; // PDF points (0.014 mm at paper scale).
const MAX_POINTS: usize = 2_000_000;
const MAX_ENTITIES: usize = 300_000;
const MAX_CLIPS: usize = 64;
fn err(s: impl Into<String>) -> IoError {
    IoError::Format(format!("PDF import: {}", s.into()))
}
fn cad(p: Coord<f64>, height: f64) -> Vec3 {
    Vec3::new(p.x * PT_TO_MM, (height - p.y) * PT_TO_MM, 0.0)
}
fn common(layer: &str, paint: &Paint<'_>) -> Common {
    let color = match paint {
        Paint::Color(c) => {
            let [r, g, b, _] = c.to_rgba().to_rgba8();
            if r < 5 && g < 5 && b < 5 { Color::Index(7) } else { Color::True(Rgb(r, g, b)) }
        }
        _ => Color::ByLayer,
    };
    Common { layer: layer.into(), color, ..Default::default() }
}
fn paths(path: &BezPath, transform: Affine) -> std::result::Result<Vec<LineString<f64>>, String> {
    // Bound work *before* flattening: a short PDF can request enormous subdivision from
    // extreme coordinates, and applying the global budget only afterward is too late.
    if path.elements().len() > 20_000 {
        return Err("PDF path has too many segments".into());
    }
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut curves = 0usize;
    for e in path.elements() {
        let pts: Vec<kurbo::Point> = match e {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![*p],
            PathEl::QuadTo(a, b) => {
                curves += 1;
                vec![*a, *b]
            }
            PathEl::CurveTo(a, b, c) => {
                curves += 1;
                vec![*a, *b, *c]
            }
            PathEl::ClosePath => vec![],
        };
        for p in pts {
            let p = transform * p;
            if !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 1e7 || p.y.abs() > 1e7 {
                return Err("PDF path has non-finite or excessive coordinates".into());
            }
            lo[0] = lo[0].min(p.x);
            lo[1] = lo[1].min(p.y);
            hi[0] = hi[0].max(p.x);
            hi[1] = hi[1].max(p.y);
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(0.0);
    let estimate = (span / FLATNESS).sqrt().ceil() as usize;
    if curves.saturating_mul(estimate).saturating_mul(8).saturating_add(path.elements().len()) > MAX_POINTS {
        return Err("PDF path exceeds the curve subdivision limit".into());
    }
    let mut lines: Vec<LineString<f64>> = Vec::new();
    let mut cur: Vec<Coord<f64>> = Vec::new();
    let mut start = None;
    let mut count = 0usize;
    let mut overflow = false;
    kurbo::flatten(path.elements().iter().copied().map(|e| transform * e), FLATNESS, |e| {
        count = count.saturating_add(1);
        if count > MAX_POINTS {
            overflow = true;
            return;
        }
        match e {
            PathEl::MoveTo(p) => {
                if !cur.is_empty() {
                    lines.push(LineString(std::mem::take(&mut cur)));
                }
                let c = Coord { x: p.x, y: p.y };
                cur.push(c);
                start = Some(c);
            }
            PathEl::LineTo(p) => cur.push(Coord { x: p.x, y: p.y }),
            PathEl::ClosePath => {
                if let Some(s) = start
                    && cur.last() != Some(&s)
                {
                    cur.push(s);
                }
            }
            _ => {}
        }
    });
    if overflow {
        return Err("PDF path exceeded the flattened-point limit".into());
    }
    if !cur.is_empty() {
        lines.push(LineString(cur));
    }
    Ok(lines)
}

fn polygon(lines: Vec<LineString<f64>>, rule: FillRule) -> std::result::Result<MultiPolygon<f64>, String> {
    let mut result = MultiPolygon::<f64>(vec![]);
    for mut ring in lines {
        if ring.0.len() < 3 {
            continue;
        }
        if ring.0.first() != ring.0.last() {
            let Some(first) = ring.0.first().copied() else { continue };
            ring.0.push(first);
        }
        if ring.0.iter().any(|p| !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 1e8 || p.y.abs() > 1e8) {
            return Err("non-finite or excessive coordinate".into());
        }
        let p = Polygon::new(ring, vec![]);
        // Multiple overlapping non-zero contours require true winding semantics. Refuse rather
        // than guessing; even-odd contours can be combined by symmetric difference.
        if !result.0.is_empty() && matches!(rule, FillRule::NonZero) {
            return Err("multi-contour non-zero fills or clips are not yet supported".into());
        }
        result = if result.0.is_empty() { MultiPolygon(vec![p]) } else { result.xor(&p) };
    }
    Ok(result)
}
struct Collector {
    drawing: Drawing,
    height: f64,
    width: f64,
    paint_count: usize,
    paper_background_skipped: bool,
    other_white_fills: usize,
    clips: Vec<MultiPolygon<f64>>,
    failure: Option<String>,
    points: usize,
    entities: usize,
    images: usize,
    text_missing: usize,
    lines_approximated: usize,
    fills: usize,
    glyphs: usize,
    patterns: usize,
    trans: bool,
}
impl Collector {
    fn fail(&mut self, s: impl Into<String>) {
        if self.failure.is_none() {
            self.failure = Some(s.into());
        }
    }
    fn add(&mut self, layer: &str, paint: &Paint<'_>, kind: EntityKind) {
        if self.failure.is_some() {
            return;
        }
        self.entities += 1;
        if self.entities > MAX_ENTITIES {
            self.fail("too many CAD entities");
            return;
        }
        if let Err(e) = self.drawing.add(&Space::Model, common(layer, paint), kind) {
            self.fail(e.to_string());
        }
    }
    fn budget(&mut self, paths: &[LineString<f64>]) -> bool {
        self.points = self.points.saturating_add(paths.iter().map(|p| p.0.len()).sum::<usize>());
        if self.points > MAX_POINTS {
            self.fail("too many flattened points");
            false
        } else {
            true
        }
    }
    fn stroke(&mut self, path: &BezPath, t: Affine, paint: &Paint<'_>) {
        let ls = match paths(path, t) {
            Ok(v) => v,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        if !self.budget(&ls) {
            return;
        }
        let mut out = MultiLineString(ls);
        for c in &self.clips {
            out = c.clip(&out, false);
            if out.0.is_empty() {
                return;
            }
        }
        if path.elements().iter().any(|e| matches!(e, PathEl::CurveTo(..) | PathEl::QuadTo(..))) {
            self.lines_approximated += 1;
        }
        for l in out.0 {
            let v: Vec<_> = l.0.into_iter().map(|p| PolyVertex::new(cad(p, self.height).xy())).collect();
            if v.len() == 2 {
                self.add(
                    "PDF_LINEWORK",
                    paint,
                    EntityKind::Line(Line { a: Vec3::new(v[0].p.x, v[0].p.y, 0.0), b: Vec3::new(v[1].p.x, v[1].p.y, 0.0) }),
                );
            } else if v.len() > 2 {
                self.add(
                    "PDF_LINEWORK",
                    paint,
                    EntityKind::LwPolyline(LwPolyline { vertices: v, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false }),
                );
            }
        }
    }
    fn fill(&mut self, path: &BezPath, t: Affine, paint: &Paint<'_>, rule: FillRule) {
        let ls = match paths(path, t) {
            Ok(v) => v,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        if !self.budget(&ls) {
            return;
        }
        let mut shape = match polygon(ls, rule) {
            Ok(p) => p,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        for c in &self.clips {
            shape = shape.intersection(c);
            if shape.0.is_empty() {
                return;
            }
        }
        let opaque_white = matches!(paint, Paint::Color(c) if c.to_rgba().to_rgba8() == [255, 255, 255, 255]);
        // PDFs often paint white paper before anything else. In a CAD model the canvas
        // already supplies that background; retaining it as a hatch can cover the drawing.
        // Skip only the first painted object when it is a single nearly page-sized rectangle.
        if self.paint_count == 1
            && opaque_white
            && shape.0.len() == 1
            && shape.0[0].interiors().is_empty()
            && let Some(rect) = shape.bounding_rect()
        {
            let area = rect.width() * rect.height();
            if rect.width() >= self.width * 0.90
                && rect.height() >= self.height * 0.90
                && rect.min().x <= self.width * 0.05
                && rect.min().y <= self.height * 0.05
                && rect.max().x >= self.width * 0.95
                && rect.max().y >= self.height * 0.95
                && area > 0.0
                && shape.unsigned_area() >= area * 0.999
            {
                self.paper_background_skipped = true;
                return;
            }
        }
        if opaque_white {
            self.other_white_fills += 1;
        }
        for p in shape.0 {
            let mut loops = Vec::new();
            for (ring, outer) in std::iter::once((p.exterior(), true)).chain(p.interiors().iter().map(|r| (r, false))) {
                let mut verts: Vec<_> = ring.0.iter().map(|p| PolyVertex::new(cad(*p, self.height).xy())).collect();
                if verts.len() > 1 && verts.first() == verts.last() {
                    verts.pop();
                }
                if verts.len() >= 3 {
                    loops.push(HatchLoop { vertices: verts, outer, sources: vec![] });
                }
            }
            if loops.is_empty() {
                continue;
            }
            let h = Hatch {
                pattern: "SOLID".into(),
                solid: true,
                loops,
                scale: 1.0,
                angle: 0.0,
                associative: false,
                style: 0,
                elevation: 0.0,
                gradient: None,
                origin: Vec2::ZERO,
                background: None,
                pattern_lines: vec![],
            };
            self.add(if opaque_white { "PDF_WHITE_MASKS" } else { "PDF_FILLS" }, paint, EntityKind::Hatch(h));
            self.fills += 1;
        }
    }
}
impl<'a> Device<'a> for Collector {
    fn set_soft_mask(&mut self, mask: Option<SoftMask<'a>>) {
        if mask.is_some() {
            self.trans = true;
        }
    }
    fn set_blend_mode(&mut self, b: BlendMode) {
        if !matches!(b, BlendMode::Normal) {
            self.trans = true;
        }
    }
    fn draw_path(&mut self, path: &BezPath, t: Affine, paint: &Paint<'a>, mode: &PathDrawMode) {
        self.paint_count = self.paint_count.saturating_add(1);
        if matches!(paint, Paint::Pattern(_)) {
            self.patterns += 1;
            return;
        }
        if let Paint::Color(c) = paint
            && c.to_rgba().to_rgba8()[3] < 255
        {
            self.trans = true;
            return;
        }
        match mode {
            PathDrawMode::Stroke(_) => self.stroke(path, t, paint),
            PathDrawMode::Fill(r) => self.fill(path, t, paint, *r),
        }
    }
    fn push_clip_path(&mut self, c: &ClipPath) {
        if self.clips.len() >= MAX_CLIPS {
            self.fail("clip nesting limit exceeded");
            return;
        }
        let ls = match paths(&c.path, Affine::IDENTITY) {
            Ok(v) => v,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        if !self.budget(&ls) {
            return;
        }
        match polygon(ls, c.fill) {
            Ok(p) => self.clips.push(p),
            Err(e) => self.fail(e),
        }
    }
    fn pop_clip_path(&mut self) {
        if self.clips.pop().is_none() {
            self.fail("unbalanced PDF clips");
        }
    }
    fn push_transparency_group(&mut self, opacity: f32, mask: Option<SoftMask<'a>>, blend: BlendMode) {
        if opacity < 0.999 || mask.is_some() || !matches!(blend, BlendMode::Normal) {
            self.trans = true;
        }
    }
    fn pop_transparency_group(&mut self) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: Affine) {
        self.paint_count = self.paint_count.saturating_add(1);
        self.images += 1;
    }
    fn draw_glyph(&mut self, glyph: &Glyph<'a>, t: Affine, gt: Affine, paint: &Paint<'a>, mode: &GlyphDrawMode) {
        if !matches!(mode, GlyphDrawMode::Fill | GlyphDrawMode::Stroke(_)) {
            return;
        }
        self.paint_count = self.paint_count.saturating_add(1);
        if let Paint::Color(c) = paint
            && c.to_rgba().to_rgba8()[3] < 255
        {
            self.trans = true;
            return;
        }
        let m = t * gt;
        match glyph {
            Glyph::Outline(outline) => {
                let path = outline.outline();
                let contours = match paths(&path, m) {
                    Ok(v) => v,
                    Err(e) => {
                        self.fail(e);
                        return;
                    }
                };
                if !self.budget(&contours) {
                    return;
                }
                // Preserve original font outlines visibly. For clipped glyphs do not emit misleading
                // partial characters: nontrivial text clipping requires a separate path Boolean conversion.
                let pts: Vec<_> = contours.iter().flat_map(|r| r.0.iter()).collect();
                if !pts.is_empty() {
                    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                    for p in pts {
                        x0 = x0.min(p.x);
                        y0 = y0.min(p.y);
                        x1 = x1.max(p.x);
                        y1 = y1.max(p.y);
                    }
                    let bbox = GRect::new(Coord { x: x0, y: y0 }, Coord { x: x1, y: y1 });
                    if self.clips.iter().any(|c| !c.contains(&bbox)) {
                        self.fail("a text glyph crosses a clipping boundary");
                        return;
                    }
                }
                if matches!(mode, GlyphDrawMode::Stroke(_)) {
                    self.stroke(&path, m, paint);
                } else {
                    let rings: Vec<_> = contours.into_iter().filter(|r| r.0.len() >= 3).collect();
                    let mut loops = Vec::new();
                    for (i, ring) in rings.iter().enumerate() {
                        let Some(first) = ring.0.first() else { continue };
                        let nested = rings
                            .iter()
                            .enumerate()
                            .filter(|(j, other)| {
                                *j != i && other.0.len() >= 3 && Polygon::new((*other).clone(), vec![]).contains(&GPoint::new(first.x, first.y))
                            })
                            .count();
                        let mut verts: Vec<_> = ring.0.iter().map(|p| PolyVertex::new(cad(*p, self.height).xy())).collect();
                        if verts.len() > 1 && verts.first() == verts.last() {
                            verts.pop();
                        }
                        if verts.len() >= 3 {
                            loops.push(HatchLoop { vertices: verts, outer: nested % 2 == 0, sources: vec![] });
                        }
                    }
                    if !loops.is_empty() {
                        let h = Hatch {
                            pattern: "SOLID".into(),
                            solid: true,
                            loops,
                            scale: 1.0,
                            angle: 0.0,
                            associative: false,
                            style: 0,
                            elevation: 0.0,
                            gradient: None,
                            origin: Vec2::ZERO,
                            background: None,
                            pattern_lines: vec![],
                        };
                        self.add("PDF_TEXT_OUTLINES", paint, EntityKind::Hatch(h));
                    }
                }
            }
            Glyph::Type3(_) => {
                self.fail("Type 3 PDF text cannot yet be converted safely");
                return;
            }
        }
        let Some(value) = glyph.as_unicode() else {
            self.text_missing += 1;
            return;
        };
        let value = match value {
            craft_pdf::interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
            craft_pdf::interpret::hayro_cmap::BfString::String(s) => s,
        };
        if value.trim().is_empty() {
            return;
        }
        let p = m * kurbo::Point::ORIGIN;
        let x = m * kurbo::Point::new(1000.0, 0.0);
        let y = m * kurbo::Point::new(0.0, 1000.0);
        let dx = x.x - p.x;
        let dy = -(x.y - p.y);
        let h = ((y.x - p.x).hypot(y.y - p.y)) * PT_TO_MM * 0.72;
        if !h.is_finite() || h <= 1e-8 {
            self.text_missing += 1;
            return;
        }
        let pos = cad(Coord { x: p.x, y: p.y }, self.height);
        let text = Text {
            insert: pos,
            align_pt: None,
            height: h,
            value,
            rotation: dy.atan2(dx),
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        };
        self.add("PDF_TEXT_EDIT", paint, EntityKind::Text(text));
        self.glyphs += 1;
    }
}

pub fn import_pdf(bytes: &[u8], page_number: usize) -> Result<Drawing> {
    let pdf = craft_pdf::open_slice(bytes, None).map_err(|e| err(e.to_string()))?;
    // The interpreter does not expose optional-content names to a Device. Never flatten named
    // layers; leave those PDFs with the strict layer-aware importer.
    let root = pdf.xref().get::<craft_pdf::syntax::object::Dict<'_>>(pdf.xref().root_id());
    if root.as_ref().and_then(|d| d.get::<craft_pdf::syntax::object::Dict<'_>>(b"OCProperties")).is_some() {
        return Err(err("this layered PDF needs clipping support not yet available"));
    }
    let page = pdf
        .pages()
        .get(page_number.checked_sub(1).ok_or_else(|| err("page numbers start at 1"))?)
        .ok_or_else(|| err("page number does not exist"))?;
    let (w, h) = page.render_dimensions();
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(err("invalid page dimensions"));
    }
    let initial = page.initial_transform(true).to_kurbo();
    let cache = InterpreterCache::new();
    let warned = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = warned.clone();
    let settings = InterpreterSettings {
        warning_sink: std::sync::Arc::new(move |_| {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }),
        ..InterpreterSettings::default()
    };
    let mut ctx = Context::new(initial, Rect::new(0.0, 0.0, w as f64, h as f64), &cache, page.xref(), settings);
    let mut d = Collector {
        drawing: Drawing::new_metric(),
        height: h as f64,
        width: w as f64,
        paint_count: 0,
        paper_background_skipped: false,
        other_white_fills: 0,
        clips: vec![],
        failure: None,
        points: 0,
        entities: 0,
        images: 0,
        text_missing: 0,
        lines_approximated: 0,
        fills: 0,
        glyphs: 0,
        patterns: 0,
        trans: false,
    };
    let mut pageclip = page.intersected_crop_box().to_kurbo().to_path(0.1);
    pageclip.apply_affine(initial);
    d.push_clip_path(&ClipPath { path: pageclip, fill: FillRule::NonZero });
    craft_pdf::interpret_page(page, &mut ctx, &mut d).map_err(|e| err(e.to_string()))?;
    if warned.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(err("PDF interpretation reported an unsupported font, image, or truncated content"));
    }
    if let Some(e) = d.failure {
        return Err(err(e));
    }
    for layer in &mut d.drawing.layers {
        if layer.name == "PDF_TEXT_EDIT" || layer.name == "PDF_WHITE_MASKS" {
            layer.on = false;
        }
    }
    if d.trans {
        return Err(err("transparency, blending or masks cannot be converted safely"));
    }
    if d.patterns > 0 {
        return Err(err("pattern paints cannot be converted safely"));
    }
    if d.drawing.model.is_empty() {
        return Err(err("no supported vector geometry was found"));
    }
    d.drawing
        .read_notes
        .push("PDF was imported at paper size in millimetres. Calibrate against a known dimension and compare the drawing with the source.".into());
    d.drawing.read_notes.push(format!("PDF has no retained CAD layer names: geometry was placed on PDF_LINEWORK and PDF_FILLS; original text outlines are on PDF_TEXT_OUTLINES. {} curves were approximated with polylines; {} filled regions and {} editable text characters were imported. Editable characters are on the initially hidden PDF_TEXT_EDIT layer and use a substitute font. Dashed lines and lineweights require review.",d.lines_approximated,d.fills,d.glyphs));
    if d.paper_background_skipped {
        d.drawing.read_notes.push("Omitted the initial page-sized white paper background so linework remains visible on the CAD canvas.".into());
    }
    if d.other_white_fills > 0 {
        d.drawing.read_notes.push(format!(
            "Preserved {} white mask(s) on the initially hidden PDF_WHITE_MASKS layer. Lines hidden behind masks in the PDF may now be visible; enabling the layer can obscure text. Compare with the PDF.",
            d.other_white_fills
        ));
    }
    if d.images > 0 || d.text_missing > 0 {
        d.drawing.read_notes.push(format!("Omitted {} raster image(s) and {} unmapped or clipped text character(s).", d.images, d.text_missing));
    }
    Ok(d.drawing)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(content: &str) -> Vec<u8> {
        format!("%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> /ExtGState << /GS1 6 0 R >> >> /Contents 4 0 R >> endobj\n4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> endobj\n6 0 obj << /Type /ExtGState /CA 0.5 /ca 0.5 >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF",content.len()).into_bytes()
    }
    #[test]
    fn clip_and_embedded_visible_outlines_survive_dwg_roundtrip() {
        let p = fixture("q 10 10 40 40 re W n 0 20 m 80 20 l S BT /F1 5 Tf 1 0 0 1 20 30 Tm (Test) Tj ET Q");
        let drawing = crate::pdf_read::import_pdf(&p, 1).unwrap();
        let line = drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Line(l) if e.common.layer == "PDF_LINEWORK" => Some(l),
                _ => None,
            })
            .unwrap();
        let (lo, hi) = (line.a.x.min(line.b.x), line.a.x.max(line.b.x));
        assert!((lo - 10.0 * PT_TO_MM).abs() < 1e-6);
        assert!((hi - 50.0 * PT_TO_MM).abs() < 1e-6);
        assert!(drawing.model.iter().any(|e| e.common.layer == "PDF_TEXT_OUTLINES" && matches!(e.kind, EntityKind::Hatch(_))));
        let text: String = drawing
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Text(t) if e.common.layer == "PDF_TEXT_EDIT" => Some(t.value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Test");
        assert!(drawing.layers.iter().any(|l| l.name == "PDF_TEXT_EDIT" && !l.on));
        let bytes = crate::write(&drawing, "test.dwg").unwrap();
        let reopened = crate::read(&bytes, "test.dwg").unwrap();
        assert!(reopened.layers.iter().any(|l| l.name == "PDF_TEXT_EDIT" && !l.on));
        assert!(reopened.model.iter().any(|e| e.common.layer == "PDF_TEXT_OUTLINES" && matches!(e.kind, EntityKind::Hatch(_))));
    }
    #[test]
    fn clipped_transparent_paint_is_rejected() {
        let p = fixture("q 10 10 40 40 re W n /GS1 gs 0 20 m 80 20 l S Q");
        let e = crate::pdf_read::import_pdf(&p, 1).unwrap_err().to_string();
        assert!(e.contains("transparen"), "{e}");
    }
    #[test]
    fn complicated_nonzero_holes_are_rejected() {
        let p = fixture("q 10 10 40 40 re W n 12 12 30 30 re 15 15 10 10 re f Q");
        let e = crate::pdf_read::import_pdf(&p, 1).unwrap_err().to_string();
        assert!(e.contains("multi-contour"), "{e}");
    }
    #[test]
    fn even_odd_polygon_mask_clips_filled_region() {
        let p = fixture("q 10 10 m 80 10 l 10 80 l h W* n 0 0 100 100 re f Q");
        let d = crate::pdf_read::import_pdf(&p, 1).unwrap();
        let h = d
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Hatch(h) if e.common.layer == "PDF_FILLS" => Some(h),
                _ => None,
            })
            .unwrap();
        let pts = &h.loops[0].vertices;
        assert_eq!(pts.len(), 3);
        let area = pts.iter().zip(pts.iter().cycle().skip(1)).take(pts.len()).map(|(a, b)| a.p.x * b.p.y - b.p.x * a.p.y).sum::<f64>().abs() / 2.0;
        assert!((area - 2450.0 * PT_TO_MM * PT_TO_MM).abs() < 1e-4, "{area}");
    }
    #[test]
    fn partially_clipped_text_is_rejected() {
        let p = fixture("q 10 10 15 15 re W n 0 20 m 50 20 l S BT /F1 8 Tf 1 0 0 1 23 20 Tm (Test) Tj ET Q");
        let e = crate::pdf_read::import_pdf(&p, 1).unwrap_err().to_string();
        assert!(e.contains("text glyph crosses"), "{e}");
    }
    #[test]
    fn hostile_path_is_refused_before_flattening() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((1e20, 0.0), (0.0, 1e20), (1.0, 1.0));
        assert!(paths(&p, Affine::IDENTITY).unwrap_err().contains("excessive"));
        let mut many = BezPath::new();
        many.move_to((0.0, 0.0));
        for _ in 0..20_001 {
            many.line_to((1.0, 1.0));
        }
        assert!(paths(&many, Affine::IDENTITY).unwrap_err().contains("too many"));
    }
    #[test]
    fn removes_only_the_initial_page_sized_white_paper() {
        let p = fixture("1 g 0 0 100 100 re f 0 g q 10 10 40 40 re W n 0 20 m 80 20 l S 1 g 15 15 5 5 re f Q");
        let d = crate::pdf_read::import_pdf(&p, 1).unwrap();
        let fills: Vec<_> = d.model.iter().filter(|e| e.common.layer == "PDF_WHITE_MASKS" && matches!(e.kind, EntityKind::Hatch(_))).collect();
        assert_eq!(fills.len(), 1, "only the smaller white mask should remain");
        assert!(d.layers.iter().any(|l| l.name == "PDF_WHITE_MASKS" && !l.on));
        let bytes = crate::write(&d, "test.dwg").unwrap();
        let reopened = crate::read(&bytes, "test.dwg").unwrap();
        assert!(reopened.layers.iter().any(|l| l.name == "PDF_WHITE_MASKS" && !l.on));
        assert!(d.model.iter().any(|e| e.common.layer == "PDF_LINEWORK"));
        assert!(d.read_notes.iter().any(|n| n.contains("page-sized white paper")));
        assert!(d.read_notes.iter().any(|n| n.contains("initially hidden PDF_WHITE_MASKS")));
    }
    #[test]
    fn preserves_a_white_page_sized_fill_after_earlier_paint() {
        let p = fixture("0 g 0 10 m 100 10 l S 1 g 0 0 100 100 re f q 10 10 40 40 re W n 0 20 m 80 20 l S Q");
        let d = crate::pdf_read::import_pdf(&p, 1).unwrap();
        assert!(d.model.iter().any(|e| e.common.layer == "PDF_WHITE_MASKS" && matches!(e.kind, EntityKind::Hatch(_))));
        assert!(d.layers.iter().any(|l| l.name == "PDF_WHITE_MASKS" && !l.on));
        assert!(!d.read_notes.iter().any(|n| n.contains("page-sized white paper")));
    }
}
