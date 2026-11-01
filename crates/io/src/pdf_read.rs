//! PDF pages read for PDF underlays (PDFATTACH): the page count, a page's size and the page
//! drawn to pixels. The file's bytes are hostile: every failure is an error, never a panic, and
//! the pixel size is capped whatever the page claims.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_interpret::font::{FontData, FontQuery, StandardFont};
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::TRANSPARENT;
use hayro::{RenderCache, RenderSettings};

use crate::{IoError, Result};

/// Millimetres per PDF point (1/72 inch).
pub const MM_PER_POINT: f64 = 25.4 / 72.0;
/// The longest side, in pixels, a page is ever drawn at.
pub const MAX_SIDE: u32 = 8192;
/// The shortest longest side a page is drawn at.
pub const MIN_SIDE: u32 = 16;

/// A page drawn to pixels: straight RGBA, rows top to bottom. The paper is transparent: only
/// what the page draws covers what is under it.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfRaster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// An opened PDF file.
pub struct PdfFile {
    pdf: Pdf,
}

impl PdfFile {
    /// Open a PDF from its bytes (encrypted files are not read).
    pub fn open(bytes: &[u8]) -> Result<PdfFile> {
        let data = bytes.to_vec();
        let pdf = guarded(move || Pdf::new(data))?.map_err(|e| IoError::Format(format!("not a readable PDF file ({e:?})")))?;
        Ok(PdfFile { pdf })
    }

    /// The number of pages.
    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Page `page` (1-based) as shown: its width and height in millimetres, after the page's
    /// rotation.
    pub fn page_size_mm(&self, page: usize) -> Result<(f64, f64)> {
        let (w, h) = self.points(page)?;
        Ok((w * MM_PER_POINT, h * MM_PER_POINT))
    }

    /// Page `page`'s measuring scale, in units per point: the conversion factor (`/C` of the first
    /// `/X` number format) of its first viewport's `/Measure` (`/VP`, ISO 32000 §12.9), which
    /// sizes an underlay's page. `None` when it has none (or a hostile one).
    pub fn units_per_point(&self, page: usize) -> Result<Option<f64>> {
        use hayro::hayro_syntax::object::{Array, Dict};
        let p = self.page(page)?;
        let c = p
            .raw()
            .get::<Array>(b"VP")
            .and_then(|vp| vp.iter::<Dict>().next())
            .and_then(|v| v.get::<Dict>(b"Measure"))
            .and_then(|m| m.get::<Array>(b"X"))
            .and_then(|x| x.iter::<Dict>().next())
            .and_then(|n| n.get::<f64>(b"C"))
            .filter(|c| c.is_finite() && (1e-9..=1e9).contains(c));
        Ok(c)
    }

    pub(crate) fn points(&self, page: usize) -> Result<(f64, f64)> {
        let p = self.page(page)?;
        let (w, h) = p.render_dimensions();
        let (w, h) = (f64::from(w), f64::from(h));
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return Err(IoError::Format(format!("page {page} of the PDF has no usable size")));
        }
        Ok((w, h))
    }

    fn page(&self, page: usize) -> Result<&hayro::hayro_syntax::page::Page<'_>> {
        let n = self.page_count();
        page.checked_sub(1).and_then(|i| self.pdf.pages().get(i)).ok_or_else(|| IoError::Format(format!("the PDF has no page {page} (it has {n})")))
    }

    /// Draw page `page` (1-based), its paper transparent, so that its longest side is `max_side` pixels
    /// (kept within [`MIN_SIDE`]..=[`MAX_SIDE`]).
    pub fn render(&self, page: usize, max_side: u32) -> Result<PdfRaster> {
        let (w, h) = self.points(page)?;
        let side = f64::from(max_side.clamp(MIN_SIDE, MAX_SIDE));
        let scale = (side / w.max(h)) as f32;
        let p = self.page(page)?;
        let pm = guarded(|| {
            let cache = RenderCache::new();
            let settings = RenderSettings { x_scale: scale, y_scale: scale, bg_color: TRANSPARENT, ..RenderSettings::default() };
            hayro::render(p, &cache, &interpreter(), &settings)
        })?;
        let (width, height) = (u32::from(pm.width()), u32::from(pm.height()));
        if width == 0 || height == 0 {
            return Err(IoError::Format(format!("page {page} of the PDF is too small to draw")));
        }
        Ok(PdfRaster { width, height, rgba: straight(pm.data_as_u8_slice()) })
    }
}

/// Most points [`PdfFile::lines`] returns for a page (hostile or very dense files).
pub const MAX_LINE_POINTS: usize = 2_000_000;

impl PdfFile {
    /// The lines drawn on page `page` (1-based): stroked and filled paths flattened to
    /// polylines, in page fractions (0..1 along the width from the left, along the height from
    /// the bottom, as shown after the page's rotation). Text is left out. At most
    /// [`MAX_LINE_POINTS`] points.
    pub fn lines(&self, page: usize) -> Result<Vec<Vec<(f64, f64)>>> {
        use hayro::hayro_interpret::util::TransformExt;
        let (w, h) = self.points(page)?;
        let p = self.page(page)?;
        guarded(|| {
            let cache = hayro::hayro_interpret::InterpreterCache::new();
            let bbox = kurbo::Rect::new(0.0, 0.0, w, h);
            let mut cx =
                hayro::hayro_interpret::Context::new(p.initial_transform(true).to_kurbo(), bbox, &cache, p.xref(), InterpreterSettings::default());
            let mut sink = Lines { out: Vec::new(), points: 0, size: (w, h) };
            hayro::hayro_interpret::interpret_page(p, &mut cx, &mut sink);
            sink.out
        })
    }
}

/// A PDF device that keeps the paths' geometry and nothing else.
struct Lines {
    out: Vec<Vec<(f64, f64)>>,
    points: usize,
    /// The page's size in points (device space, y down).
    size: (f64, f64),
}

impl Lines {
    /// Add `path` (device space) as polylines in page fractions, keeping at most `cap` points in
    /// all: a hostile path with a huge number of segments stops there.
    fn add(&mut self, path: &kurbo::BezPath, cap: usize) {
        use kurbo::PathEl;
        let room = cap.saturating_sub(self.points);
        if room == 0 {
            return;
        }
        let (w, h) = self.size;
        let mut current: Vec<(f64, f64)> = Vec::new();
        let mut start: Option<(f64, f64)> = None;
        // Points kept by this path so far.
        let mut used = 0usize;
        let out = &mut self.out;
        let mut flush = |line: &mut Vec<(f64, f64)>, used: &mut usize| {
            if line.len() >= 2 {
                *used = used.saturating_add(line.len());
                out.push(std::mem::take(line));
            } else {
                line.clear();
            }
        };
        kurbo::flatten(path.iter(), 0.1, |el| {
            let to = |p: kurbo::Point| (p.x / w, 1.0 - p.y / h);
            let full = |current: &Vec<(f64, f64)>, used: usize| used.saturating_add(current.len()) >= room;
            match el {
                PathEl::MoveTo(p) => {
                    flush(&mut current, &mut used);
                    let q = to(p);
                    start = Some(q);
                    if !full(&current, used) {
                        current.push(q);
                    }
                }
                PathEl::LineTo(p) => {
                    if !full(&current, used) {
                        current.push(to(p));
                    }
                }
                PathEl::ClosePath => {
                    if let Some(s) = start
                        && !full(&current, used)
                    {
                        current.push(s);
                    }
                    flush(&mut current, &mut used);
                }
                _ => {}
            }
        });
        flush(&mut current, &mut used);
        self.points = self.points.saturating_add(used);
    }
}

impl<'a> hayro::hayro_interpret::Device<'a> for Lines {
    fn set_soft_mask(&mut self, _: Option<hayro::hayro_interpret::SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_path(
        &mut self,
        path: &kurbo::BezPath,
        transform: kurbo::Affine,
        _: &hayro::hayro_interpret::Paint<'a>,
        _: &hayro::hayro_interpret::PathDrawMode,
    ) {
        self.add(&(transform * path.clone()), MAX_LINE_POINTS);
    }
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<hayro::hayro_interpret::SoftMask<'a>>, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_glyph(
        &mut self,
        _: &hayro::hayro_interpret::font::Glyph<'a>,
        _: kurbo::Affine,
        _: kurbo::Affine,
        _: &hayro::hayro_interpret::Paint<'a>,
        _: &hayro::hayro_interpret::GlyphDrawMode,
    ) {
    }
    fn draw_image(&mut self, _: hayro::hayro_interpret::Image<'a, '_>, _: kurbo::Affine) {}
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// How pages are read: fonts a PDF names without embedding them are found among the installed
/// fonts (or craft-fonts), see [`font_for`].
fn interpreter() -> InterpreterSettings {
    InterpreterSettings { font_resolver: std::sync::Arc::new(font_for), ..InterpreterSettings::default() }
}

/// An installed font for a font a PDF doesn't embed: one of the 14 standard fonts, matched to a
/// font of the same widths (Arial for Helvetica, Times New Roman for Times, Courier New for
/// Courier, or their Liberation, Arimo/Tinos/Cousine, Nimbus and DejaVu counterparts), or another
/// font, by its name when it is installed, else by the standard font of its kind. `None` when
/// none is installed: the text is then not drawn.
fn font_for(query: &FontQuery) -> Option<(FontData, u32)> {
    let installed = |names: &[&str]| names.iter().find_map(|n| cadcraft_fonts::ttf::find(n));
    let face = match query {
        FontQuery::Standard(f) => installed(standard_names(*f)),
        FontQuery::Fallback(q) => {
            let named = [q.post_script_name.as_deref(), q.font_family.as_deref(), q.font_name.as_deref()];
            named.into_iter().flatten().find_map(cadcraft_fonts::ttf::find).or_else(|| installed(standard_names(q.pick_standard_font())))
        }
    }?;
    let (bytes, index) = face.data();
    Some((bytes, index))
}

/// Installed fonts with a standard font's widths and style, best first (file names).
fn standard_names(f: StandardFont) -> &'static [&'static str] {
    use StandardFont as S;
    match f {
        S::Helvetica => &["arial", "liberationsans-regular", "arimo-regular", "nimbussans-regular", "helvetica", "dejavusans"],
        S::HelveticaBold => &["arialbd", "liberationsans-bold", "arimo-bold", "nimbussans-bold", "dejavusans-bold", "arial"],
        S::HelveticaOblique => &["ariali", "liberationsans-italic", "arimo-italic", "nimbussans-italic", "dejavusans-oblique", "arial"],
        S::HelveticaBoldOblique => {
            &["arialbi", "liberationsans-bolditalic", "arimo-bolditalic", "nimbussans-bolditalic", "dejavusans-boldoblique", "arial"]
        }
        S::TimesRoman => &["times", "liberationserif-regular", "tinos-regular", "nimbusroman-regular", "dejavuserif"],
        S::TimesBold => &["timesbd", "liberationserif-bold", "tinos-bold", "nimbusroman-bold", "dejavuserif-bold", "times"],
        S::TimesItalic => &["timesi", "liberationserif-italic", "tinos-italic", "nimbusroman-italic", "dejavuserif-italic", "times"],
        S::TimesBoldItalic => {
            &["timesbi", "liberationserif-bolditalic", "tinos-bolditalic", "nimbusroman-bolditalic", "dejavuserif-bolditalic", "times"]
        }
        S::Courier => &["cour", "liberationmono-regular", "cousine-regular", "nimbusmonops-regular", "dejavusansmono"],
        S::CourierBold => &["courbd", "liberationmono-bold", "cousine-bold", "nimbusmonops-bold", "dejavusansmono-bold", "cour"],
        S::CourierOblique => &["couri", "liberationmono-italic", "cousine-italic", "nimbusmonops-italic", "dejavusansmono-oblique", "cour"],
        S::CourierBoldOblique => {
            &["courbi", "liberationmono-bolditalic", "cousine-bolditalic", "nimbusmonops-bolditalic", "dejavusansmono-boldoblique", "cour"]
        }
        S::Symbol => &["symbol", "standardsymbolsps"],
        S::ZapfDingBats => &["d050000l"],
    }
}

/// The lines of page `page` in page fractions (the engine's PDF hook).
pub fn page_lines(bytes: &[u8], page: u32) -> std::result::Result<Vec<Vec<(f64, f64)>>, String> {
    PdfFile::open(bytes).and_then(|f| f.lines(page as usize)).map_err(|e| e.to_string())
}

/// The number of pages, page `page`'s size in millimetres and its measuring scale in units per
/// point (the engine's PDF hook).
pub fn page_info(bytes: &[u8], page: u32) -> std::result::Result<(usize, f64, f64, Option<f64>), String> {
    let f = PdfFile::open(bytes).map_err(|e| e.to_string())?;
    let (w, h) = f.page_size_mm(page as usize).map_err(|e| e.to_string())?;
    let c = f.units_per_point(page as usize).map_err(|e| e.to_string())?;
    Ok((f.page_count(), w, h, c))
}

/// Page `page` drawn with its longest side `max_side` pixels (the engine's PDF hook).
pub fn render_page(bytes: &[u8], page: u32, max_side: u32) -> std::result::Result<(u32, u32, Vec<u8>), String> {
    let r = PdfFile::open(bytes).and_then(|f| f.render(page as usize, max_side)).map_err(|e| e.to_string())?;
    Ok((r.width, r.height, r.rgba))
}

impl PdfFile {
    /// The part `region` of page `page` (1-based) drawn, its paper transparent, at `width` × `height` pixels
    /// (each at most [`MAX_SIDE`]): `region` is `[left, bottom, right, top]` in page fractions
    /// (0..1 from the left along the width, from the bottom along the height).
    pub fn render_region(&self, page: usize, region: [f64; 4], width: u32, height: u32) -> Result<PdfRaster> {
        let (w, h) = self.points(page)?;
        let [l, b, r, t] = region;
        let (rw, rh) = ((r - l) * w, (t - b) * h);
        if !(region.iter().all(|v| v.is_finite()) && rw > 0.0 && rh > 0.0) {
            return Err(IoError::Format("an empty part of the page".into()));
        }
        let (pw, ph) = (width.clamp(1, MAX_SIDE), height.clamp(1, MAX_SIDE));
        let p = self.page(page)?;
        let pm = guarded(|| {
            let cache = RenderCache::new();
            // Page points (y down) → the region's pixels.
            let (sx, sy) = (f64::from(pw) / rw, f64::from(ph) / rh);
            let settings = RenderSettings {
                x_scale: sx as f32,
                y_scale: sy as f32,
                width: Some(pw as u16),
                height: Some(ph as u16),
                bg_color: TRANSPARENT,
                x_offset: (l * w * sx) as f32,
                y_offset: ((1.0 - t) * h * sy) as f32,
                ..RenderSettings::default()
            };
            hayro::render(p, &cache, &interpreter(), &settings)
        })?;
        Ok(PdfRaster { width: u32::from(pm.width()), height: u32::from(pm.height()), rgba: straight(pm.data_as_u8_slice()) })
    }
}

/// Part `region` of page `page` drawn at `width` × `height` pixels (the engine's PDF hook).
pub fn render_page_region(bytes: &[u8], page: u32, region: [f64; 4], width: u32, height: u32) -> std::result::Result<(u32, u32, Vec<u8>), String> {
    let r = PdfFile::open(bytes).and_then(|f| f.render_region(page as usize, region, width, height)).map_err(|e| e.to_string())?;
    Ok((r.width, r.height, r.rgba))
}

/// Premultiplied RGBA (the renderer's) as straight RGBA.
fn straight(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = premultiplied.to_vec();
    for [r, g, b, a] in out.as_chunks_mut::<4>().0 {
        if *a != 0 && *a != 255 {
            let k = 255.0 / f32::from(*a);
            for c in [r, g, b] {
                *c = (f32::from(*c) * k).round().min(255.0) as u8;
            }
        }
    }
    out
}

/// Run the PDF library on hostile data: a panic inside it becomes an error.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|_| IoError::Format("the PDF file could not be read (it is damaged or unsupported)".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hostile_path_keeps_no_more_points_than_the_cap() {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        for i in 0..1000 {
            path.line_to((f64::from(i), f64::from(i % 2)));
        }
        path.close_path();
        path.move_to((5.0, 5.0));
        path.line_to((6.0, 6.0));
        let mut sink = Lines { out: Vec::new(), points: 0, size: (100.0, 100.0) };
        sink.add(&path, 100);
        let kept: usize = sink.out.iter().map(Vec::len).sum();
        assert!(kept <= 100 && sink.points == kept, "{kept} points kept, {} counted", sink.points);
        // Full: later paths add nothing.
        sink.add(&path, 100);
        assert_eq!(sink.out.iter().map(Vec::len).sum::<usize>(), kept);
    }
    use cadcraft_doc::{Common, Drawing, EntityKind, Line, Lineweight, Space};
    use cadcraft_geom::Vec3;

    /// A one-page A4 landscape PDF written by CADCraft's own PDF export: a thick diagonal line.
    fn a4_with_a_line() -> Vec<u8> {
        let mut d = Drawing::new_metric();
        let line = Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 70.0, 0.0) };
        let lw = Common { lineweight: Lineweight::Mm100(200), ..Common::default() };
        d.add(&Space::Model, lw, EntityKind::Line(line)).ok();
        let o = crate::PdfOptions { paper_mm: Some((297.0, 210.0)), fit: Some(true), lineweights: Some(true), ..crate::PdfOptions::default() };
        crate::pdf(&d, &Space::Model, &o).unwrap_or_default()
    }

    /// A one-page A4 landscape PDF whose page dictionary has `extra` entries, with a correct
    /// cross-reference table.
    fn pdf_with(extra: &str) -> Vec<u8> {
        pdf_objects(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 842 595] {extra} >>"),
        ])
    }

    /// A PDF of objects `objs` (1, 2, … in order; 1 is the catalog), with a correct
    /// cross-reference table.
    fn pdf_objects(objs: &[String]) -> Vec<u8> {
        let mut out = String::from("%PDF-1.7\n");
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
        }
        let xref = out.len();
        out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
        for o in offsets {
            out.push_str(&format!("{o:010} 00000 n \n"));
        }
        out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1));
        out.into_bytes()
    }

    /// A page with large black text in font `base_font`, not embedded in the file.
    fn text_pdf(base_font: &str) -> Vec<u8> {
        let content = "BT /F1 120 Tf 60 250 Td (HELLO) Tj ET";
        pdf_objects(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 842 595] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} >>"),
        ])
    }

    fn inked(r: &PdfRaster) -> usize {
        r.rgba.as_chunks::<4>().0.iter().filter(|[.., a]| *a > 128).count()
    }

    #[test]
    fn text_in_fonts_the_pdf_doesnt_embed_is_drawn_with_installed_ones() {
        // Only where a matching font is installed (Arial, Liberation Sans, …): tests run without.
        let sans = ["arial", "liberationsans-regular", "arimo-regular", "dejavusans"].iter().any(|n| cadcraft_fonts::ttf::find(n).is_some());
        if !sans {
            return;
        }
        for font in ["Helvetica", "Helvetica-Bold", "Times-Roman", "Courier", "ArialMT"] {
            let f = PdfFile::open(&text_pdf(font)).unwrap();
            let r = f.render(1, 842).unwrap();
            assert!(inked(&r) > 2000, "{font}: {} pixels of text", inked(&r));
        }
    }

    #[test]
    fn the_first_viewports_measuring_scale_sizes_the_page() {
        let vp = |bbox: &str, c: &str| {
            format!(
                "<< /Type /Viewport /BBox [{bbox}] /Measure << /Type /Measure /Subtype /RL /R (1:1) /X [<< /Type /NumberFormat /U (mm) /C {c} >>] >> >>"
            )
        };
        // The first viewport counts, whatever it covers.
        let two = format!("/VP [{} {}]", vp("100 100 400 300", "17.6391"), vp("0 0 842 595", ".35282"));
        let f = PdfFile::open(&pdf_with(&two)).unwrap();
        assert_eq!(f.units_per_point(1).unwrap(), Some(17.6391));
        let (_, w, h, c) = page_info(&pdf_with(&format!("/VP [{}]", vp("0 0 842 595", ".35282"))), 1).unwrap();
        assert!((w - 297.04).abs() < 0.01 && (h - 209.9).abs() < 0.01 && c == Some(0.35282), "{w} {h} {c:?}");
        // None without viewports, and for hostile ones.
        for extra in
            ["", "/VP []", "/VP 5", "/VP [<< /Measure 3 >>]", &format!("/VP [{}]", vp("0 0 1 1", "-2")), &format!("/VP [{}]", vp("0 0 1 1", "(x)"))]
        {
            assert_eq!(page_info(&pdf_with(extra), 1).unwrap().3, None, "{extra}");
        }
    }

    #[test]
    fn a_pdf_page_is_read_and_drawn() {
        let f = PdfFile::open(&a4_with_a_line()).unwrap();
        assert_eq!(f.page_count(), 1);
        let (w, h) = f.page_size_mm(1).unwrap();
        assert!((w - 297.0).abs() < 0.5 && (h - 210.0).abs() < 0.5, "{w} x {h}");
        let r = f.render(1, 1000).unwrap();
        assert_eq!((r.width, r.height), (1000, 707));
        assert_eq!(r.rgba.len(), 1000 * 707 * 4);
        // White paper, and the line drawn in dark pixels.
        let dark = r.rgba.chunks(4).filter(|p| p.get(3).copied().unwrap_or(0) > 128 && p.first().copied().unwrap_or(255) < 128).count();
        assert!(dark > 500, "{dark} dark pixels");
        // The paper is transparent.
        assert_eq!(r.rgba.get(3), Some(&0), "a corner of the page");
    }

    #[test]
    fn a_page_gives_its_lines_for_snapping() {
        let f = PdfFile::open(&a4_with_a_line()).unwrap();
        let lines = f.lines(1).unwrap();
        let (w, h) = f.page_size_mm(1).unwrap();
        // The drawn line: its slope on paper is the drawing's, 70 / 100.
        let slope = |l: &Vec<(f64, f64)>| {
            let (a, b) = (l.first()?, l.last()?);
            let (dx, dy) = ((b.0 - a.0) * w, (b.1 - a.1) * h);
            (dx.abs() > 10.0).then(|| dy / dx)
        };
        assert!(lines.iter().filter_map(slope).any(|s| (s - 0.7).abs() < 1e-3), "{lines:?}");
        assert!(lines.iter().flatten().all(|(x, y)| x.is_finite() && y.is_finite()));
    }

    #[test]
    fn a_part_of_a_page_is_drawn_sharper() {
        let f = PdfFile::open(&a4_with_a_line()).unwrap();
        let dark =
            |r: &PdfRaster| r.rgba.chunks(4).filter(|p| p.get(3).copied().unwrap_or(0) > 128 && p.first().copied().unwrap_or(255) < 128).count();
        let whole = f.render(1, 1000).unwrap();
        let same = f.render_region(1, [0.0, 0.0, 1.0, 1.0], 1000, 707).unwrap();
        assert_eq!((same.width, same.height), (1000, 707));
        let (a, b) = (dark(&whole) as f64, dark(&same) as f64);
        assert!((a - b).abs() <= a * 0.02, "{a} {b}");
        // The lower-left quarter at the same pixel size: the line is drawn twice as large.
        let quarter = f.render_region(1, [0.0, 0.0, 0.5, 0.5], 1000, 707).unwrap();
        assert!(dark(&quarter) as f64 > b * 1.2, "{} {b}", dark(&quarter));
        for bad in [[0.5, 0.5, 0.5, 0.9], [f64::NAN, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]] {
            assert!(f.render_region(1, bad, 100, 100).is_err(), "{bad:?}");
        }
        assert_eq!(f.render_region(1, [0.0, 0.0, 1.0, 1.0], u32::MAX, 0).unwrap().width, MAX_SIDE);
    }

    #[test]
    fn the_pixel_size_is_capped() {
        let f = PdfFile::open(&a4_with_a_line()).unwrap();
        let r = f.render(1, u32::MAX).unwrap();
        assert!((MAX_SIDE - 1..=MAX_SIDE).contains(&r.width), "{}", r.width);
        let r = f.render(1, 0).unwrap();
        assert!(r.width >= 1 && r.height >= 1);
    }

    #[test]
    fn hostile_pdf_does_not_panic() {
        let good = a4_with_a_line();
        let mut cases: Vec<Vec<u8>> = vec![Vec::new(), b"%PDF-1.7".to_vec(), b"not a pdf at all".to_vec(), vec![0xff; 4096]];
        for cut in [10, good.len() / 3, good.len() / 2, good.len().saturating_sub(20)] {
            cases.push(good.get(..cut).unwrap_or_default().to_vec());
        }
        let mut flipped = good.clone();
        for (i, b) in flipped.iter_mut().enumerate() {
            if i % 97 == 0 {
                *b = b.wrapping_add(31);
            }
        }
        cases.push(flipped);
        for bytes in &cases {
            if let Ok(f) = PdfFile::open(bytes) {
                for page in [0, 1, 2, usize::MAX] {
                    let _ = f.page_size_mm(page);
                    let _ = f.render(page, 256);
                    let _ = f.lines(page);
                }
            }
        }
        let f = PdfFile::open(&good).unwrap();
        assert!(f.page_size_mm(0).is_err());
        assert!(f.render(2, 100).is_err());
    }
}
