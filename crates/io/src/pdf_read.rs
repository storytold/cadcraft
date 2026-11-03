//! PDF → Drawing (PDFIMPORT): the vector content of one page as editable objects.
//!
//! Stroked paths become `LINE`s and `LWPOLYLINE`s (a curve that is a circular arc becomes a bulge
//! vertex, any other curve is flattened into short segments), filled paths become solid `HATCH`es
//! and text becomes `TEXT`: the glyphs of one baseline are joined into a single string and fitted
//! to the width the PDF gave them. Each kind goes to its own layer (`PDF_Geometry`,
//! `PDF_Solid Fills`, `PDF_Text`) with the colour and line width the PDF used, in millimetres with
//! the page's lower-left corner at the origin.
//!
//! Only a PDF made from a DWG drawing is imported: one whose document information says a CAD
//! program wrote it (AutoCAD's "DWG To PDF" and its relatives, or CADCraft's own plot). Any other PDF
//! is refused with the name of the program that made it, since page content drawn by a word
//! processor or a browser is not a drawing and imports as a mess of text and rectangles.
//!
//! Not imported (each is named in a read note, so it is never left out silently): raster images,
//! Type 3 glyphs and glyphs without a Unicode value, gradient and pattern fills, and white fills
//! (they only hide what lies under them on paper). A rectangular clip (the usual page, viewport and
//! frame clip) cuts the lines and filled shapes it crosses and drops what lies outside it; other
//! clip shapes, transparency and dash patterns are not applied, and a read note says how often.

use std::collections::BTreeMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, Hatch, HatchLoop, Line, Lineweight, LwPolyline, Space, Text};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use craft_pdf::interpret::hayro_cmap::BfString;
use craft_pdf::interpret::{
    BlendMode, ClipPath, Context, Device, FillRule, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
    TransformExt, font::Glyph,
};
use kurbo::{Affine, BezPath, PathEl, Point, Rect};

use crate::{IoError, Result};

/// Objects one import creates.
const MAX_ENTITIES: usize = 1_000_000;
/// Vertices kept across the whole import, so a page of curves cannot grow without bound.
const MAX_TOTAL_VERTS: usize = 8_000_000;
/// Vertices in one polyline.
const MAX_POLY_VERTS: usize = 100_001;
/// Glyphs read from one page.
const MAX_GLYPHS: usize = 2_000_000;
/// A page is refused when its sides, in points, are not between these.
const MAX_PAGE_PT: f64 = 200_000.0;
/// Millimetres in a PDF point (1/72 inch).
pub const MM_PER_PT: f64 = 25.4 / 72.0;
/// How far a flattened curve may leave the real one, in points.
const FLATTEN_TOL_PT: f64 = 0.02;
/// Cap height of a typical text face, as a fraction of its em square.
const CAP_HEIGHT: f64 = 0.72;

const LAYER_GEOMETRY: &str = "PDF_Geometry";
const LAYER_FILLS: &str = "PDF_Solid Fills";
const LAYER_TEXT: &str = "PDF_Text";

/// What PDFIMPORT reads and how it places it.
#[derive(Clone, Debug)]
pub struct PdfImportOptions {
    /// The page to import, counted from 1.
    pub page: usize,
    /// The password of an encrypted file.
    pub password: Option<String>,
    /// Drawing units per PDF point (millimetres by default).
    pub unit: f64,
    /// Import text as `TEXT` (else it is left out).
    pub text: bool,
    /// Import filled shapes as solid hatches.
    pub fills: bool,
    /// Give strokes the line width the PDF drew them with.
    pub lineweights: bool,
}

impl Default for PdfImportOptions {
    fn default() -> Self {
        PdfImportOptions { page: 1, password: None, unit: MM_PER_PT, text: true, fills: true, lineweights: true }
    }
}

/// Read one page of a PDF. Returns the drawing and the number of pages in the file.
pub fn read(bytes: &[u8], o: &PdfImportOptions) -> Result<(Drawing, usize)> {
    if !o.unit.is_finite() || o.unit <= 0.0 {
        return Err(IoError::Format("PDF: the unit must be a positive number".into()));
    }
    let pdf = craft_pdf::open_slice(bytes, o.password.as_deref()).map_err(|e| IoError::Format(format!("PDF: {e}")))?;
    check_made_from_dwg(pdf.metadata())?;
    let pages = pdf.pages();
    let count = pages.len();
    let page = o.page.checked_sub(1).and_then(|i| pages.get(i)).ok_or_else(|| {
        IoError::Format(format!("PDF: page {} doesn't exist; the file has {count} page{}", o.page, if count == 1 { "" } else { "s" }))
    })?;
    let (w, h) = page.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w.is_finite() && h.is_finite() && (1.0..=MAX_PAGE_PT).contains(&w) && (1.0..=MAX_PAGE_PT).contains(&h)) {
        return Err(IoError::Format(format!("PDF: page size {w} × {h} points is not usable")));
    }
    let cache = InterpreterCache::new();
    let mut context =
        Context::new(page.initial_transform(false).to_kurbo(), Rect::new(0.0, 0.0, w, h), &cache, page.xref(), InterpreterSettings::default());
    let mut dev = Collector::new(o);
    craft_pdf::interpret_page(page, &mut context, &mut dev).map_err(|e| IoError::Format(format!("PDF: {e}")))?;
    dev.flush_text();
    if let Some(e) = dev.failure.take() {
        return Err(IoError::Format(e));
    }
    let notes = dev.note();
    dev.drawing.read_notes.extend(notes);
    Ok((dev.drawing, count))
}

/// Names that, in a PDF's creator, producer, title, subject or author, say a CAD program plotted it
/// from a drawing (lower case).
const DWG_PRODUCERS: &[&str] =
    &["autocad", "autodesk", "dwg", "cadcraft", "bricscad", "draftsight", "zwcad", "gstarcad", "progecad", "intellicad", "teigha", "open design"];

/// A document-information string as text: UTF-16BE when it starts with the byte order mark, else
/// the bytes as Latin-1 (which is ASCII for the names that matter here).
fn info_text(raw: &[u8]) -> String {
    if let Some(rest) = raw.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    raw.iter().map(|b| char::from(*b)).collect()
}

/// Refuse a PDF its document information doesn't show was made from a DWG drawing.
fn check_made_from_dwg(meta: &craft_pdf::syntax::metadata::Metadata) -> Result<()> {
    let fields = [&meta.creator, &meta.producer, &meta.title, &meta.subject, &meta.author];
    let texts: Vec<String> = fields.into_iter().flatten().map(|raw| info_text(raw)).collect();
    if texts.iter().any(|t| {
        let t = t.to_lowercase();
        DWG_PRODUCERS.iter().any(|name| t.contains(name))
    }) {
        return Ok(());
    }
    let maker = [&meta.producer, &meta.creator]
        .into_iter()
        .flatten()
        .map(|raw| info_text(raw))
        .map(|t| t.chars().filter(|c| !c.is_control()).take(60).collect::<String>())
        .find(|t| !t.trim().is_empty());
    Err(IoError::Format(match maker {
        Some(m) => format!("PDF: only a PDF made from a DWG drawing can be imported; this one was made by {}", m.trim()),
        None => "PDF: only a PDF made from a DWG drawing can be imported; this one doesn't say what made it".into(),
    }))
}

/// [`read`] with options as JSON, the form the engine's PDFIMPORT passes:
/// `{page?, password?, unit?, text?, fills?, lineweights?}`. A value of the wrong type is an error.
pub fn read_json(bytes: &[u8], opts: &serde_json::Value) -> Result<(Drawing, usize)> {
    let mut o = PdfImportOptions::default();
    let bad = |key: &str, want: &str| IoError::Format(format!("PDF: `{key}` must be {want}"));
    if let Some(v) = opts.get("page") {
        o.page = v.as_u64().and_then(|n| usize::try_from(n).ok()).filter(|n| *n >= 1).ok_or_else(|| bad("page", "a page number from 1"))?;
    }
    if let Some(v) = opts.get("password") {
        o.password = Some(v.as_str().ok_or_else(|| bad("password", "text"))?.to_string());
    }
    if let Some(v) = opts.get("unit") {
        o.unit = v.as_f64().filter(|u| u.is_finite() && *u > 0.0).ok_or_else(|| bad("unit", "a positive number"))?;
    }
    for (key, slot) in [("text", &mut o.text), ("fills", &mut o.fills), ("lineweights", &mut o.lineweights)] {
        if let Some(v) = opts.get(key) {
            *slot = v.as_bool().ok_or_else(|| bad(key, "true or false"))?;
        }
    }
    read(bytes, &o)
}

/// Drawn content that isn't imported, named in the drawing's read note.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LeftOut {
    Image,
    Type3,
    NoUnicode,
    PatternFill,
    WhiteFill,
}

impl LeftOut {
    fn describe(self, n: usize) -> String {
        let (one, many) = match self {
            Self::Image => ("raster image", "raster images"),
            Self::Type3 => ("Type 3 glyph", "Type 3 glyphs"),
            Self::NoUnicode => ("glyph without a Unicode value", "glyphs without a Unicode value"),
            Self::PatternFill => ("gradient or pattern fill", "gradient or pattern fills"),
            Self::WhiteFill => ("white fill", "white fills"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }
}

/// The glyphs of one baseline gathered so far.
struct Run {
    start: Vec2,
    /// Unit vector along the baseline.
    dir: Vec2,
    em: f64,
    color: Color,
    text: String,
    glyphs: usize,
    /// Where the last glyph's advance ends, when the font said.
    end: Option<Vec2>,
    last: Vec2,
}

struct Collector {
    drawing: Drawing,
    opts: PdfImportOptions,
    entities: usize,
    verts_left: usize,
    glyphs: usize,
    run: Option<Run>,
    left_out: BTreeMap<LeftOut, usize>,
    failure: Option<String>,
    /// The clips in force, outermost first: a rectangle in page points, or `None` for another shape.
    clips: Vec<Option<Rect>>,
    /// What was drawn only approximately, by description.
    approx: BTreeMap<&'static str, usize>,
}

fn v3(p: Vec2) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0)
}

fn pt(p: Point) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// A PDF colour as a drawing colour; black and white (which a sheet of paper and a drawing screen
/// swap) become the automatic colour 7.
fn color_of(paint: &Paint<'_>) -> Option<Color> {
    let Paint::Color(c) = paint else { return None };
    let [r, g, b, a] = c.to_rgba().to_rgba8();
    if a == 0 {
        return None;
    }
    Some(match (r, g, b) {
        (0, 0, 0) | (255, 255, 255) => Color::Index(7),
        _ => Color::True(Rgb(r, g, b)),
    })
}

fn is_white(paint: &Paint<'_>) -> bool {
    matches!(paint, Paint::Color(c) if { let [r, g, b, a] = c.to_rgba().to_rgba8(); a > 0 && r == 255 && g == 255 && b == 255 })
}

/// The nearest standard lineweight to a width in millimetres.
fn lineweight(mm: f64) -> Lineweight {
    let hundredths = (mm * 100.0).clamp(0.0, 211.0);
    let nearest =
        Lineweight::STANDARD.iter().copied().min_by(|a, b| (f64::from(*a) - hundredths).abs().total_cmp(&(f64::from(*b) - hundredths).abs()));
    Lineweight::Mm100(nearest.unwrap_or(0))
}

impl Collector {
    fn new(o: &PdfImportOptions) -> Self {
        Collector {
            drawing: Drawing::new_metric(),
            opts: o.clone(),
            entities: 0,
            verts_left: MAX_TOTAL_VERTS,
            glyphs: 0,
            run: None,
            left_out: BTreeMap::new(),
            failure: None,
            clips: Vec::new(),
            approx: BTreeMap::new(),
        }
    }

    fn approximated(&mut self, what: &'static str) {
        *self.approx.entry(what).or_default() += 1;
    }

    /// The intersection of the rectangular clips in force, as corners in drawing units.
    fn clip(&self) -> Option<(Vec2, Vec2)> {
        let r = self.clips.iter().flatten().copied().reduce(|a, b| a.intersect(b))?;
        let u = self.opts.unit;
        Some((Vec2::new(r.x0 * u, r.y0 * u), Vec2::new(r.x1 * u, r.y1 * u)))
    }

    fn add_poly(&mut self, verts: Vec<PolyVertex>, closed: bool, color: Color, weight: Lineweight) {
        let n = verts.len();
        let straight = verts.iter().all(|v| v.bulge == 0.0);
        let kind = if n == 2 && !closed && straight {
            EntityKind::Line(Line { a: v3(verts[0].p), b: v3(verts[1].p) })
        } else {
            EntityKind::LwPolyline(LwPolyline { vertices: verts, closed, const_width: 0.0, elevation: 0.0, plinegen: false })
        };
        self.add(LAYER_GEOMETRY, color, weight, kind, n);
    }

    fn left_out(&mut self, kind: LeftOut) {
        *self.left_out.entry(kind).or_default() += 1;
    }

    fn note(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if !self.left_out.is_empty() {
            let list: Vec<String> = self.left_out.iter().map(|(k, n)| k.describe(*n)).collect();
            notes.push(format!("PDF: CADCraft doesn't import these, so they were left out: {}", list.join(", ")));
        }
        if !self.approx.is_empty() {
            let list: Vec<String> = self.approx.iter().map(|(what, n)| format!("{n} {what}")).collect();
            notes.push(format!("PDF: drawn only approximately: {}", list.join(", ")));
        }
        notes
    }

    fn fail(&mut self, why: &str) {
        self.failure.get_or_insert_with(|| format!("PDF: {why}"));
    }

    fn add(&mut self, layer: &str, color: Color, lineweight: Lineweight, kind: EntityKind, verts: usize) {
        if self.failure.is_some() {
            return;
        }
        self.entities += 1;
        if self.entities > MAX_ENTITIES {
            return self.fail(&format!("more than {MAX_ENTITIES} objects on the page"));
        }
        let Some(left) = self.verts_left.checked_sub(verts) else { return self.fail("too much geometry on the page") };
        self.verts_left = left;
        let common = Common { layer: layer.into(), color, lineweight, ..Common::default() };
        if self.drawing.add(&Space::Model, common, kind).is_err() {
            self.fail("the drawing refused an object");
        }
    }

    /// The path's subpaths in drawing units: vertices (curves flattened or kept as bulges) and
    /// whether each is closed.
    fn subpaths(&mut self, path: &BezPath, to_page: Affine) -> Vec<(Vec<PolyVertex>, bool)> {
        let m = Affine::scale(self.opts.unit) * to_page;
        let tol = FLATTEN_TOL_PT * self.opts.unit;
        let mut out: Vec<(Vec<PolyVertex>, bool)> = Vec::new();
        let mut cur: Vec<PolyVertex> = Vec::new();
        let mut start = Vec2::ZERO;
        let finish = |cur: &mut Vec<PolyVertex>, closed: bool, out: &mut Vec<(Vec<PolyVertex>, bool)>| {
            let mut v = std::mem::take(cur);
            if closed && v.len() > 1 && v.first().zip(v.last()).is_some_and(|(a, b)| (a.p - b.p).len() < 1e-9) {
                v.pop();
            }
            if v.len() >= 2 {
                out.push((v, closed));
            }
        };
        for el in path.elements() {
            if cur.len() > MAX_POLY_VERTS {
                self.fail("one path has too many vertices");
                break;
            }
            match *el {
                PathEl::MoveTo(p) => {
                    finish(&mut cur, false, &mut out);
                    start = pt(m * p);
                    cur.push(PolyVertex::new(start));
                }
                PathEl::LineTo(p) => {
                    if cur.is_empty() {
                        cur.push(PolyVertex::new(start));
                    }
                    cur.push(PolyVertex::new(pt(m * p)));
                }
                PathEl::QuadTo(c, p) => {
                    let Some(p0) = cur.last().map(|v| v.p) else { continue };
                    let q = pt(m * c);
                    let p3 = pt(m * p);
                    let c1 = p0 + (q - p0) * (2.0 / 3.0);
                    let c2 = p3 + (q - p3) * (2.0 / 3.0);
                    curve(&mut cur, [p0, c1, c2, p3], tol);
                }
                PathEl::CurveTo(a, b, p) => {
                    let Some(p0) = cur.last().map(|v| v.p) else { continue };
                    curve(&mut cur, [p0, pt(m * a), pt(m * b), pt(m * p)], tol);
                }
                PathEl::ClosePath => {
                    finish(&mut cur, true, &mut out);
                    // A path may go on from the point it closed to.
                }
            }
        }
        finish(&mut cur, false, &mut out);
        out
    }

    fn stroke(&mut self, path: &BezPath, to_page: Affine, paint: &Paint<'_>, width: f32) {
        let Some(color) = color_of(paint) else { return };
        let det = to_page.determinant().abs().sqrt();
        let mm = f64::from(width) * det * MM_PER_PT;
        let weight = if self.opts.lineweights { lineweight(mm) } else { Lineweight::ByLayer };
        if translucent(paint) {
            self.approximated("translucent objects drawn opaque");
        }
        let clip = self.clip();
        for (verts, closed) in self.subpaths(path, to_page) {
            let Some((lo, hi)) = clip else {
                self.add_poly(verts, closed, color, weight);
                continue;
            };
            match relation(&verts, lo, hi) {
                Relation::Inside => self.add_poly(verts, closed, color, weight),
                Relation::Outside => {}
                Relation::Crossing if verts.iter().all(|v| v.bulge == 0.0) => {
                    for run in clipped_runs(&verts, closed, lo, hi) {
                        self.add_poly(run, false, color, weight);
                    }
                }
                Relation::Crossing => {
                    self.approximated("curved lines crossing a clip kept whole");
                    self.add_poly(verts, closed, color, weight);
                }
            }
        }
    }

    fn fill(&mut self, path: &BezPath, to_page: Affine, paint: &Paint<'_>, rule: &FillRule) {
        if !self.opts.fills {
            return;
        }
        if is_white(paint) {
            return self.left_out(LeftOut::WhiteFill);
        }
        let Some(color) = color_of(paint) else {
            if matches!(paint, Paint::Pattern(_)) {
                self.left_out(LeftOut::PatternFill);
            }
            return;
        };
        if translucent(paint) {
            self.approximated("translucent objects drawn opaque");
        }
        let clip = self.clip();
        let mut kept_whole = false;
        let loops: Vec<HatchLoop> = self
            .subpaths(path, to_page)
            .into_iter()
            .filter(|(v, _)| v.len() >= 3)
            .filter_map(|(v, closed)| {
                let Some((lo, hi)) = clip else { return Some((v, closed)) };
                match relation(&v, lo, hi) {
                    Relation::Inside => Some((v, closed)),
                    Relation::Outside => None,
                    Relation::Crossing if v.iter().all(|p| p.bulge == 0.0) => {
                        let cut = clip_polygon(v.iter().map(|p| p.p).collect(), lo, hi);
                        (cut.len() >= 3).then(|| (cut.into_iter().map(PolyVertex::new).collect(), closed))
                    }
                    Relation::Crossing => {
                        kept_whole = true;
                        Some((v, closed))
                    }
                }
            })
            .enumerate()
            .map(|(i, (vertices, _))| HatchLoop { vertices, outer: i == 0, sources: Vec::new() })
            .collect();
        if kept_whole {
            self.approximated("curved filled shapes crossing a clip kept whole");
        }
        if loops.is_empty() {
            return;
        }
        let verts = loops.iter().map(|l| l.vertices.len()).sum();
        // Both rules read nested loops as islands; even-odd is what the hatch's normal style does.
        let _ = rule;
        let hatch = Hatch {
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
            pattern_lines: Vec::new(),
        };
        self.add(LAYER_FILLS, color, Lineweight::ByLayer, EntityKind::Hatch(hatch), verts);
    }

    fn glyph_text(&mut self, glyph: &Glyph<'_>, to_page: Affine, glyph_to_text: Affine, paint: &Paint<'_>) {
        if !self.opts.text || self.failure.is_some() {
            return;
        }
        self.glyphs += 1;
        if self.glyphs > MAX_GLYPHS {
            return self.fail(&format!("more than {MAX_GLYPHS} glyphs on the page"));
        }
        let Glyph::Outline(g) = glyph else { return self.left_out(LeftOut::Type3) };
        let Some(color) = color_of(paint) else { return };
        let ch = match g.as_unicode() {
            Some(BfString::Char(c)) => c.to_string(),
            Some(BfString::String(s)) => s,
            None => return self.left_out(LeftOut::NoUnicode),
        };
        let ch: String = ch.chars().filter(|c| !c.is_control() && *c != '\u{FFFD}').collect();
        if ch.is_empty() {
            return;
        }
        // The outline's 1000-unit em square on the page, in drawing units.
        let m = Affine::scale(self.opts.unit) * to_page * glyph_to_text;
        let origin = pt(m * Point::ZERO);
        let ex = pt(m * Point::new(1000.0, 0.0)) - origin;
        let ey = pt(m * Point::new(0.0, 1000.0)) - origin;
        let (lx, em) = (ex.len(), ey.len());
        if !(origin.x.is_finite() && origin.y.is_finite() && lx > 0.0 && em > 0.0 && em.is_finite()) {
            return;
        }
        if let Some((lo, hi)) = self.clip()
            && !((lo.x - 1e-9..=hi.x + 1e-9).contains(&origin.x) && (lo.y - 1e-9..=hi.y + 1e-9).contains(&origin.y))
        {
            return;
        }
        let dir = ex * (1.0 / lx);
        let end = g.advance_width().map(|w| pt(m * Point::new(f64::from(w), 0.0)));

        if let Some(run) = &mut self.run {
            let rel = origin - run.last;
            let along = rel.x * run.dir.x + rel.y * run.dir.y;
            let across = (rel.x * run.dir.y - rel.y * run.dir.x).abs();
            let same = run.color == color && (run.em - em).abs() <= 0.02 * em && run.dir.x * dir.x + run.dir.y * dir.y > 0.9999 && across <= 0.1 * em;
            // Where the previous glyph ended, else the previous origin plus an average glyph.
            let gap = match run.end {
                Some(e) => {
                    let d = origin - e;
                    d.x * run.dir.x + d.y * run.dir.y
                }
                None => along - 0.5 * em,
            };
            if same && (-0.3 * em..=1.5 * em).contains(&gap) {
                if gap > 0.15 * em && !run.text.ends_with(' ') && !ch.starts_with(' ') {
                    run.text.push(' ');
                }
                // A fill and its stroke arrive as two calls for one glyph.
                if !(along.abs() < 1e-9 * em.max(1.0) && run.text.ends_with(&ch)) {
                    run.text.push_str(&ch);
                    run.glyphs += 1;
                }
                run.last = origin;
                run.end = end;
                return;
            }
        }
        self.flush_text();
        self.run = Some(Run { start: origin, dir, em, color, text: ch, glyphs: 1, end, last: origin });
    }

    fn flush_text(&mut self) {
        let Some(run) = self.run.take() else { return };
        let value = run.text.trim_end().to_string();
        if value.trim().is_empty() {
            return;
        }
        let fit_end = run.end.filter(|e| run.glyphs >= 2 && (*e - run.start).len() > 1e-9);
        let text = Text {
            insert: v3(run.start),
            align_pt: fit_end.map(v3),
            height: run.em * CAP_HEIGHT,
            value,
            rotation: run.dir.y.atan2(run.dir.x),
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: if fit_end.is_some() { HAlign::Fit } else { HAlign::Left },
            valign: Default::default(),
        };
        self.add(LAYER_TEXT, run.color, Lineweight::ByLayer, EntityKind::Text(text), 1);
    }
}

/// Whether the paint is a colour that lets what is under it show through.
fn translucent(paint: &Paint<'_>) -> bool {
    matches!(paint, Paint::Color(c) if { let [_, _, _, a] = c.to_rgba().to_rgba8(); a > 0 && a < 255 })
}

/// The rectangle a clip path is, when it is an axis-aligned one (four or five straight points).
fn rect_of(path: &BezPath) -> Option<Rect> {
    let mut pts: Vec<Point> = Vec::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push(p),
            PathEl::ClosePath => {}
            _ => return None,
        }
    }
    if !(4..=5).contains(&pts.len()) {
        return None;
    }
    let (x0, x1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
    let (y0, y1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let on = |v: f64, a: f64, b: f64| (v - a).abs() < 1e-6 || (v - b).abs() < 1e-6;
    (pts.iter().all(|p| on(p.x, x0, x1) && on(p.y, y0, y1)) && x0.is_finite() && y0.is_finite()).then(|| Rect::new(x0, y0, x1, y1))
}

/// How a shape lies against a clip rectangle.
enum Relation {
    Inside,
    Outside,
    Crossing,
}

fn relation(verts: &[PolyVertex], lo: Vec2, hi: Vec2) -> Relation {
    const EPS: f64 = 1e-9;
    let (mut min, mut max) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for v in verts {
        min = Vec2::new(min.x.min(v.p.x), min.y.min(v.p.y));
        max = Vec2::new(max.x.max(v.p.x), max.y.max(v.p.y));
    }
    if min.x >= lo.x - EPS && min.y >= lo.y - EPS && max.x <= hi.x + EPS && max.y <= hi.y + EPS {
        Relation::Inside
    } else if max.x < lo.x || min.x > hi.x || max.y < lo.y || min.y > hi.y {
        Relation::Outside
    } else {
        Relation::Crossing
    }
}

/// The part of the segment a to b inside the rectangle (Liang-Barsky).
fn clip_segment(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }
    Some((a + d * t0, a + d * t1))
}

/// The pieces of a straight-sided polyline inside the rectangle, each an open run of vertices.
fn clipped_runs(verts: &[PolyVertex], closed: bool, lo: Vec2, hi: Vec2) -> Vec<Vec<PolyVertex>> {
    fn flush(cur: &mut Vec<PolyVertex>, runs: &mut Vec<Vec<PolyVertex>>) {
        let run = std::mem::take(cur);
        if run.windows(2).any(|w| (w[0].p - w[1].p).len() > 1e-9) {
            runs.push(run);
        }
    }
    let n = verts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    let (mut runs, mut cur) = (Vec::new(), Vec::new());
    for i in 0..segments {
        match clip_segment(verts[i].p, verts[(i + 1) % n].p, lo, hi) {
            None => flush(&mut cur, &mut runs),
            Some((p, q)) => {
                if cur.last().is_none_or(|l: &PolyVertex| (l.p - p).len() > 1e-9) {
                    flush(&mut cur, &mut runs);
                    cur.push(PolyVertex::new(p));
                }
                cur.push(PolyVertex::new(q));
            }
        }
    }
    flush(&mut cur, &mut runs);
    runs
}

/// A polygon cut to the rectangle (Sutherland-Hodgman).
fn clip_polygon(mut poly: Vec<Vec2>, lo: Vec2, hi: Vec2) -> Vec<Vec2> {
    // Each edge: the axis (0 = x, 1 = y), its value and whether the kept side is the greater one.
    for (axis, value, keep_greater) in [(0, lo.x, true), (0, hi.x, false), (1, lo.y, true), (1, hi.y, false)] {
        let get = |v: Vec2| if axis == 0 { v.x } else { v.y };
        let inside = |v: Vec2| if keep_greater { get(v) >= value } else { get(v) <= value };
        let mut out = Vec::with_capacity(poly.len() + 4);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if inside(a) {
                out.push(a);
            }
            if inside(a) != inside(b) {
                out.push(a + (b - a) * ((value - get(a)) / (get(b) - get(a))));
            }
        }
        poly = out;
        if poly.is_empty() {
            break;
        }
    }
    poly
}

/// Append a cubic from the last vertex of `cur`: a circular arc becomes a bulge on that vertex,
/// any other curve is flattened to `tol`.
fn curve(cur: &mut Vec<PolyVertex>, c: [Vec2; 4], tol: f64) {
    if let Some(bulge) = arc_bulge(c, tol) {
        if let Some(last) = cur.last_mut() {
            last.bulge = bulge;
        }
        cur.push(PolyVertex::new(c[3]));
        return;
    }
    let path = [
        PathEl::MoveTo(Point::new(c[0].x, c[0].y)),
        PathEl::CurveTo(Point::new(c[1].x, c[1].y), Point::new(c[2].x, c[2].y), Point::new(c[3].x, c[3].y)),
    ];
    kurbo::flatten(path, tol.max(1e-9), |el| {
        if let PathEl::LineTo(p) = el {
            cur.push(PolyVertex::new(pt(p)));
        }
    });
}

fn cubic_at(c: &[Vec2; 4], t: f64) -> Vec2 {
    let u = 1.0 - t;
    c[0] * (u * u * u) + c[1] * (3.0 * u * u * t) + c[2] * (3.0 * u * t * t) + c[3] * (t * t * t)
}

/// The bulge of the circular arc a cubic stays within `tol` of, if it is one (and under a full turn).
fn arc_bulge(c: [Vec2; 4], tol: f64) -> Option<f64> {
    let (a, m, b) = (c[0], cubic_at(&c, 0.5), c[3]);
    let chord = b - a;
    let len = chord.len();
    if !(len.is_finite() && len > 100.0 * tol) {
        return None;
    }
    // The circle through the ends and the middle.
    let d = 2.0 * (a.x * (m.y - b.y) + m.x * (b.y - a.y) + b.x * (a.y - m.y));
    if d.abs() < 1e-12 * len * len {
        return None;
    }
    let (a2, m2, b2) = (a.x * a.x + a.y * a.y, m.x * m.x + m.y * m.y, b.x * b.x + b.y * b.y);
    let center =
        Vec2::new((a2 * (m.y - b.y) + m2 * (b.y - a.y) + b2 * (a.y - m.y)) / d, (a2 * (b.x - m.x) + m2 * (a.x - b.x) + b2 * (m.x - a.x)) / d);
    let r = (a - center).len();
    if !r.is_finite() || r < len * 0.5 * (1.0 - 1e-9) {
        return None;
    }
    // Every sample must lie on it.
    for t in [0.125, 0.25, 0.375, 0.625, 0.75, 0.875] {
        if ((cubic_at(&c, t) - center).len() - r).abs() > tol {
            return None;
        }
    }
    // The arc from a to b through m runs counter-clockwise when m lies to the right of a→b.
    let ccw = chord.x * (m.y - a.y) - chord.y * (m.x - a.x) < 0.0;
    let half = (len * 0.5 / r).clamp(-1.0, 1.0).asin();
    // The middle lies on the minor arc, unless it is further from the chord than the radius.
    let sag = ((m - a).x * chord.y - (m - a).y * chord.x).abs() / len;
    let theta = if sag > r { std::f64::consts::TAU - 2.0 * half } else { 2.0 * half };
    if theta >= std::f64::consts::TAU - 1e-6 {
        return None;
    }
    let bulge = (theta / 4.0).tan();
    Some(if ccw { bulge } else { -bulge })
}

impl<'a> Device<'a> for Collector {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, path: &BezPath, transform: Affine, paint: &Paint<'a>, mode: &PathDrawMode) {
        if self.failure.is_some() {
            return;
        }
        match mode {
            PathDrawMode::Fill(rule) => self.fill(path, transform, paint, rule),
            PathDrawMode::Stroke(p) => self.stroke(path, transform, paint, p.line_width),
        }
    }
    fn push_clip_path(&mut self, clip: &ClipPath) {
        let rect = rect_of(&clip.path);
        if rect.is_none() {
            self.approximated("clips by a shape other than a rectangle not applied");
        }
        self.clips.push(rect);
    }
    fn push_transparency_group(&mut self, opacity: f32, _: Option<SoftMask<'a>>, _: BlendMode) {
        if opacity < 1.0 {
            self.approximated("translucent groups drawn opaque");
        }
    }
    fn draw_glyph(&mut self, glyph: &Glyph<'a>, transform: Affine, glyph_transform: Affine, paint: &Paint<'a>, mode: &GlyphDrawMode) {
        if matches!(mode, GlyphDrawMode::Fill | GlyphDrawMode::Stroke(_)) {
            self.glyph_text(glyph, transform, glyph_transform, paint);
        }
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: Affine) {
        self.left_out(LeftOut::Image);
    }
    fn pop_clip_path(&mut self) {
        self.clips.pop();
    }
    fn pop_transparency_group(&mut self) {}
}
