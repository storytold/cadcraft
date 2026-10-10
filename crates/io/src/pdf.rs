//! Vector PDF plotting (PLOT / EXPORTPDF): a minimal PDF 1.4 writer of our own.
//!
//! One page per plot: model-space extents fitted to a sheet, or a layout at 1:1 on its paper
//! (paper units are millimetres in metric drawings, inches otherwise). Lines are stroked paths
//! (`m`/`l`/`S`), fills are `f` paths, colours are RGB with colour 7 printing black on white
//! paper, and line widths come from lineweights when enabled.

use std::fmt::Write as _;

use cadcraft_color::{Rgb, display_rgb};
use cadcraft_doc::{Drawing, PageSetup, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{Kind, Sheet, clip, paper};
use serde_json::Value;

use crate::{IoError, Result};

/// PDF points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Largest sheet side we accept, in millimetres (PDF viewers cap pages at 200 inches anyway).
const MAX_SHEET_MM: f64 = 5080.0;

/// Plot settings. `None` fields fall back to the layout's page setup (or sensible defaults for
/// model space).
#[derive(Clone, Debug, Default)]
pub struct PdfOptions {
    /// Paper name from [`cadcraft_render::PAPER_SIZES`] (`"A4"`, `"Letter"`, `"ANSI B"`…).
    pub paper: Option<String>,
    /// Custom paper size in millimetres (portrait width, height).
    pub paper_mm: Option<(f64, f64)>,
    pub landscape: Option<bool>,
    /// Fit the drawing to the printable area. Default: on for model space, off for layouts (1:1).
    pub fit: Option<bool>,
    /// Plot scale in paper units per drawing unit (when not fitting).
    pub scale: Option<f64>,
    /// Plot object lineweights. Default: the layout's setting; on for model space.
    pub lineweights: Option<bool>,
    /// Flate-compress the content stream.
    pub compress: bool,
    pub title: String,
    /// Model space only: plot this rectangle (drawing units), fitted to the printable area,
    /// instead of the extents. Ignored for layouts.
    pub window: Option<Bounds2>,
}

impl PdfOptions {
    /// Parse `{paper?, width?, height?, landscape?, fit?, scale?, lineweights?, compress?, title?}`.
    pub fn from_json(v: &Value) -> PdfOptions {
        let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        let paper_mm = match (num("width"), num("height")) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        };
        PdfOptions {
            paper: v.get("paper").and_then(Value::as_str).map(str::to_string),
            paper_mm,
            landscape: v.get("landscape").and_then(Value::as_bool),
            fit: v.get("fit").and_then(Value::as_bool),
            scale: num("scale").filter(|s| *s > 0.0),
            lineweights: v.get("lineweights").and_then(Value::as_bool),
            compress: v.get("compress").and_then(Value::as_bool).unwrap_or(true),
            title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            window: None,
        }
    }
}

/// Plot with JSON options (the engine's `plot` hook).
pub fn plot(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    pdf(d, space, &PdfOptions::from_json(opts))
}

/// The page setup a plot of `space` uses, with overrides applied.
pub fn page_for(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<PageSetup> {
    let mut page = match space {
        Space::Paper(n) => d.layout(n).map(|l| l.page.clone()).ok_or_else(|| IoError::Format(format!("no layout `{n}`")))?,
        Space::Model => {
            let mut p = PageSetup::default();
            if paper::paper_unit_mm(d) == 1.0
                && let Some(a4) = paper::paper_size("A4")
            {
                p.paper = a4.name.into();
                p.width_mm = a4.width_mm;
                p.height_mm = a4.height_mm;
            }
            p
        }
    };
    if let Some(name) = &o.paper {
        let ps = paper::paper_size(name).ok_or_else(|| IoError::Format(format!("unknown paper size `{name}`")))?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    if let Some((w, h)) = o.paper_mm {
        page.paper = format!("User ({w:.2} x {h:.2} MM)");
        page.width_mm = w.min(MAX_SHEET_MM);
        page.height_mm = h.min(MAX_SHEET_MM);
    }
    if let Some(l) = o.landscape {
        page.landscape = l;
    }
    page.width_mm = page.width_mm.clamp(1.0, MAX_SHEET_MM);
    page.height_mm = page.height_mm.clamp(1.0, MAX_SHEET_MM);
    Ok(page)
}

/// World → paper-unit mapping: `p' = (p - from) * s + to`.
#[derive(Clone, Copy)]
struct Map {
    from: Vec2,
    s: f64,
    to: Vec2,
    /// Points per paper unit.
    k: f64,
}

impl Map {
    fn pt(&self, p: Vec2) -> Vec2 {
        ((p - self.from) * self.s + self.to) * self.k
    }
}

/// Write a one-page vector PDF of a space.
pub fn pdf(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<Vec<u8>> {
    let page = page_for(d, space, o)?;
    let unit_mm = paper::paper_unit_mm(d);
    let sheet = Sheet::from_page(&page, unit_mm);
    let lineweights = o.lineweights.unwrap_or(match space {
        Space::Paper(_) => page.lineweights,
        Space::Model => true,
    });
    let ropts = cadcraft_render::Options { tolerance: 0.001, min_dash: 0.0, text: true, fill: true, lineweights };
    let k = unit_mm * PT_PER_MM;
    let window = o.window.filter(|w| matches!(space, Space::Model) && !w.is_empty());
    let fit = window.is_some() || o.fit.unwrap_or(matches!(space, Space::Model));
    // Chord tolerance: about 0.05 mm on paper.
    let est = plot_scale(&window.unwrap_or_else(|| d.extents(space)), &sheet, fit, o.scale);
    let tol = 0.05 / unit_mm / est.max(1e-300);
    let tolerance = if tol.is_finite() && tol > 0.0 { tol } else { ropts.tolerance };
    let list = cadcraft_render::build_plot(d, space, &cadcraft_render::Options { tolerance, ..ropts });
    let b = window.unwrap_or(list.bounds);
    let s = plot_scale(&b, &sheet, fit, o.scale);
    let map = if fit || matches!(space, Space::Model) {
        let from = if b.is_empty() { Vec2::ZERO } else { b.center() };
        Map { from, s, to: sheet.printable.center(), k }
    } else {
        Map { from: Vec2::ZERO, s: 1.0, to: Vec2::ZERO, k }
    };
    let media = sheet.size * k;
    let clip_pt = if fit || matches!(space, Space::Model) {
        Bounds2::new(sheet.printable.min * k, sheet.printable.max * k)
    } else {
        Bounds2::new(Vec2::ZERO, media)
    };
    let content = content_stream(&list, &map, &clip_pt);
    let title = if o.title.is_empty() {
        match space {
            Space::Model => "Model".to_string(),
            Space::Paper(n) => n.clone(),
        }
    } else {
        o.title.clone()
    };
    Ok(assemble(media, content.as_bytes(), o.compress, &title))
}

/// Paper units per drawing unit for a plot.
fn plot_scale(b: &Bounds2, sheet: &Sheet, fit: bool, scale: Option<f64>) -> f64 {
    if let Some(s) = scale.filter(|s| s.is_finite() && *s > 0.0)
        && !fit
    {
        return s;
    }
    if !fit || b.is_empty() {
        return 1.0;
    }
    let pw = sheet.printable.width();
    let ph = sheet.printable.height();
    let s = (pw / b.width().max(1e-12)).min(ph / b.height().max(1e-12));
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// A number for a content stream: short, finite, no exponent.
fn num(out: &mut String, v: f64) {
    let v = if v.is_finite() { v.clamp(-1.0e7, 1.0e7) } else { 0.0 };
    let mut s = format!("{v:.3}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    out.push_str(&s);
}

fn pt(out: &mut String, p: Vec2) {
    num(out, p.x);
    out.push(' ');
    num(out, p.y);
}

fn color(out: &mut String, c: Rgb, op: &str) {
    for v in [c.0, c.1, c.2] {
        num(out, f64::from(v) / 255.0);
        out.push(' ');
    }
    out.push_str(op);
    out.push('\n');
}

fn content_stream(list: &cadcraft_render::DisplayList, map: &Map, clip_pt: &Bounds2) -> String {
    let white = Rgb(255, 255, 255);
    let mut c = String::new();
    c.push_str("q\n1 J 1 j\n");
    // Clip to the plot area.
    pt(&mut c, clip_pt.min);
    c.push(' ');
    num(&mut c, clip_pt.width());
    c.push(' ');
    num(&mut c, clip_pt.height());
    c.push_str(" re W n\n");
    let mut stroke: Option<Rgb> = None;
    let mut fill: Option<Rgb> = None;
    let mut width: Option<f64> = None;
    let mut pending = false;
    let flush = |c: &mut String, pending: &mut bool| {
        if *pending {
            c.push_str("S\n");
            *pending = false;
        }
    };
    for p in &list.prims {
        let rgb = display_rgb(p.color, white);
        let raw = list.points(p);
        if raw.iter().any(|q| !q.is_finite()) {
            continue;
        }
        let w = f64::from(p.lw) * PT_PER_MM;
        let w = if w.is_finite() && w > 0.0 { w.min(100.0) } else { 0.0 };
        match p.kind {
            Kind::Polyline | Kind::Infinite { .. } => {
                let pts: Vec<Vec2> = match p.kind {
                    Kind::Infinite { ray } => match (raw.first(), raw.get(1)) {
                        (Some(base), Some(dir)) => {
                            let a = map.pt(*base);
                            let dir_pt = map.pt(*base + *dir) - a;
                            match clip::clip_infinite(a, dir_pt, ray, clip_pt) {
                                Some((x, y)) => vec![x, y],
                                None => continue,
                            }
                        }
                        _ => continue,
                    },
                    _ => raw.iter().map(|q| map.pt(*q)).collect(),
                };
                if pts.len() < 2 {
                    continue;
                }
                if stroke != Some(rgb) || width != Some(w) {
                    flush(&mut c, &mut pending);
                    if stroke != Some(rgb) {
                        color(&mut c, rgb, "RG");
                        stroke = Some(rgb);
                    }
                    if width != Some(w) {
                        num(&mut c, w);
                        c.push_str(" w\n");
                        width = Some(w);
                    }
                }
                for (i, q) in pts.iter().enumerate() {
                    pt(&mut c, *q);
                    c.push_str(if i == 0 { " m\n" } else { " l\n" });
                }
                pending = true;
            }
            Kind::Tris => {
                flush(&mut c, &mut pending);
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                let mut any = false;
                for t in raw.as_chunks::<3>().0 {
                    for (i, q) in t.iter().enumerate() {
                        pt(&mut c, map.pt(*q));
                        c.push_str(if i == 0 { " m\n" } else { " l\n" });
                    }
                    c.push_str("h\n");
                    any = true;
                }
                if any {
                    c.push_str("f\n");
                }
            }
            Kind::Point => {
                let Some(q) = raw.first() else { continue };
                flush(&mut c, &mut pending);
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                let half = (w.max(0.5)) / 2.0;
                pt(&mut c, map.pt(*q) - Vec2::new(half, half));
                c.push(' ');
                num(&mut c, half * 2.0);
                c.push(' ');
                num(&mut c, half * 2.0);
                c.push_str(" re f\n");
            }
        }
    }
    flush(&mut c, &mut pending);
    c.push_str("Q\n");
    c
}

/// A PDF literal string with `(`, `)` and `\` escaped; non-ASCII becomes `?`.
fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for ch in s.chars().take(200) {
        match ch {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// Assemble the file: header, five objects, cross-reference table and trailer.
fn assemble(media: Vec2, content: &[u8], compress: bool, title: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(content.len() + 1024);
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets: Vec<usize> = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let mut mb = String::new();
    num(&mut mb, media.x);
    mb.push(' ');
    num(&mut mb, media.y);
    obj(&mut out, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {mb}] /Resources << >> /Contents 4 0 R >>").as_bytes());
    let (data, filter) =
        if compress { (miniz_oxide::deflate::compress_to_vec_zlib(content, 6), " /Filter /FlateDecode") } else { (content.to_vec(), "") };
    let mut stream = format!("<< /Length {}{filter} >>\nstream\n", data.len()).into_bytes();
    stream.extend_from_slice(&data);
    stream.extend_from_slice(b"\nendstream");
    obj(&mut out, &stream);
    obj(&mut out, format!("<< /Producer (CADCraft) /Creator (CADCraft) /Title {} >>", pdf_string(title)).as_bytes());
    let xref = out.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(x, "{o:010} 00000 n ");
    }
    let _ = write!(x, "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out.extend_from_slice(x.as_bytes());
    out
}
