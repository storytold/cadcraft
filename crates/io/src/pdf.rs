//! Vector PDF plotting (PLOT / EXPORTPDF), written with craft-pdf's PDF writer (krilla; pure
//! Rust, no file access, so it runs on the web too).
//!
//! One page per sheet: model-space extents fitted to a sheet, or a layout at 1:1 (or the given
//! `scale`) on its paper (paper units are millimetres in metric drawings, inches otherwise).
//! Lines are stroked paths, fills are filled paths, colours are RGB with colour 7 printing
//! black on white paper, and line widths come from lineweights when enabled. The page setup's
//! plot style table (or the `plotStyleTable` option) sets the printed colours, linetypes and
//! lineweights.
//!
//! Several sheets go into one file ([`pdf_sheets`], the `layouts` option), each page labelled
//! with its layout name. Drawing layers become PDF layers (optional content groups) that a PDF
//! viewer can switch on and off, and entity hyperlinks become link annotations.

use std::fmt::Write as _;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Handle, PageSetup, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{DPrim, Kind, Sheet, clip, paper};
use serde_json::Value;

use craft_pdf::write::backend as kr;
use craft_pdf::write::{self, Metadata, WriteError, Writer};
use kr::action::LinkAction;
use kr::annotation::{Annotation, LinkAnnotation, Target};
use kr::error::KrillaError;
use kr::geom::{Path, PathBuilder, Point, Rect, Transform};
use kr::num::NormalizedF32;
use kr::optional_content::OcgId;
use kr::page::{PageLabel, PageSettings};
use kr::paint::{Fill, FillRule, LineCap, LineJoin, Stroke};
use kr::surface::Surface;
use kr::text::{GlyphId, KrillaGlyph};

use crate::{IoError, Result};

mod fonts;

/// PDF points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Largest sheet side we accept, in millimetres (PDF viewers cap pages at 200 inches anyway).
const MAX_SHEET_MM: f64 = 5080.0;
/// Most sheets (pages) in one file.
pub const MAX_PAGES: usize = 256;
/// Most PDF layers in one file; objects on further layers print without one.
const MAX_LAYERS: usize = 4096;
/// Longest PDF layer name, in characters.
const MAX_LAYER_NAME: usize = 1000;
/// Most link annotations in one file, and the longest link target (bytes).
const MAX_LINKS: usize = 10_000;
const MAX_URL: usize = 2048;
/// Longest title and page label written (characters).
const MAX_TEXT: usize = 1000;

/// Plot settings. `None` fields fall back to the layout's page setup (or sensible defaults for
/// model space).
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// Plot style table name, replacing the page setup's (`"None"` plots without one).
    pub plot_style_table: Option<String>,
    /// Paper name from [`cadcraft_render::PAPER_SIZES`] (`"A4"`, `"Letter"`, `"ANSI B"`…).
    pub paper: Option<String>,
    /// Custom paper size in millimetres (portrait width, height).
    pub paper_mm: Option<(f64, f64)>,
    pub landscape: Option<bool>,
    /// Fit the drawing to the printable area. Default: on for model space, off for layouts (1:1).
    pub fit: Option<bool>,
    /// Plot scale in paper units per drawing unit (when not fitting).
    pub scale: Option<f64>,
    /// Model-space plot placement. `None` keeps the legacy centered behavior.
    pub center: Option<bool>,
    /// Model-space X/Y plot offset in millimetres from the printable area's lower-left corner.
    pub offset: Option<[f64; 2]>,
    /// Plot object lineweights. Default: the layout's setting; on for model space.
    pub lineweights: Option<bool>,
    /// Flate-compress the content stream.
    pub compress: bool,
    pub title: String,
    /// Model space only: plot this rectangle (drawing units), fitted to the printable area,
    /// instead of the extents. Ignored for layouts.
    pub window: Option<Bounds2>,
    /// Drawing layers become PDF layers (optional content groups). Default on.
    pub layers: bool,
    /// With `layers`: objects on layers that are off are plotted on PDF layers that start
    /// hidden (frozen layers are never plotted). Default off: they aren't plotted.
    pub include_off_layers: bool,
    /// Hyperlinks: an entity's handle and its link target (a URL). The area the entity covers
    /// on the sheet becomes a link annotation.
    pub links: Vec<(Handle, String)>,
    /// Write text in installed TrueType fonts as filled outlines instead of as text in an
    /// embedded font subset (like AutoCAD's PDFSHX for its own fonts). Stroke-font text is
    /// always geometry.
    pub text_as_geometry: bool,
    /// Margins (left, bottom, right, top; mm) of the plot device's paper, bounding the printable
    /// area of a model-space plot. `None` keeps the page setup's (none for model space).
    pub margins: Option<[f64; 4]>,
    /// Entity hyperlinks read from a drawing file (`PE_URL` extended data) become link
    /// annotations too, after [`PdfOptions::links`]. Default on.
    pub hyperlinks: bool,
    /// Plot objects with their transparency (PDF alpha). `None` = the page setup's "Plot
    /// transparency" (off for model space).
    pub plot_transparency: Option<bool>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions {
            plot_style_table: None,
            paper: None,
            paper_mm: None,
            landscape: None,
            fit: None,
            scale: None,
            center: None,
            offset: None,
            lineweights: None,
            compress: false,
            title: String::new(),
            window: None,
            layers: true,
            include_off_layers: false,
            links: Vec::new(),
            text_as_geometry: false,
            margins: None,
            hyperlinks: true,
            plot_transparency: None,
        }
    }
}

impl PdfOptions {
    /// Parse `{paper?, width?, height?, landscape?, fit?, scale?, center?, offset?: [x,y] mm,
    /// lineweights?, compress?, title?, window?: [[x1, y1], [x2, y2]], margins?: [l, b, r, t] (mm),
    /// hyperlinks?: bool, plotTransparency?: bool}`.
    pub fn from_json(v: &Value) -> PdfOptions {
        let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        let paper_mm = match (num("width"), num("height")) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        };
        PdfOptions {
            // `layers?` (default on), `offLayers?`, `links?: [{handle, url}]`, `text?: "text" |
            // "geometry"`.
            text_as_geometry: v.get("text").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("geometry")),
            layers: v.get("layers").and_then(Value::as_bool).unwrap_or(true),
            include_off_layers: v.get("offLayers").and_then(Value::as_bool).unwrap_or(false),
            links: links_json(v.get("links")),
            plot_style_table: v.get("plotStyleTable").and_then(Value::as_str).map(|s| s.chars().take(260).collect()),
            paper: v.get("paper").and_then(Value::as_str).map(str::to_string),
            paper_mm,
            landscape: v.get("landscape").and_then(Value::as_bool),
            fit: v.get("fit").and_then(Value::as_bool),
            scale: num("scale").filter(|s| *s > 0.0),
            center: v.get("center").and_then(Value::as_bool),
            offset: offset(v.get("offset")),
            lineweights: v.get("lineweights").and_then(Value::as_bool),
            compress: v.get("compress").and_then(Value::as_bool).unwrap_or(true),
            title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            window: window(v.get("window")),
            margins: margins(v.get("margins")),
            hyperlinks: v.get("hyperlinks").and_then(Value::as_bool).unwrap_or(true),
            plot_transparency: v.get("plotTransparency").and_then(Value::as_bool),
        }
    }
}

fn offset(v: Option<&Value>) -> Option<[f64; 2]> {
    let a = v?.as_array()?;
    if a.len() != 2 {
        return None;
    }
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    valid_plot_offset([x, y]).then_some([x, y])
}

fn valid_plot_offset(offset: [f64; 2]) -> bool {
    offset.iter().all(|v| v.is_finite() && v.abs() <= paper::MAX_PLOT_OFFSET_MM)
}

/// Reject malformed offsets at the JSON plotting boundary, before a permissive option parser
/// could treat them as absent.
pub(crate) fn validate_plot_offset_json(v: &Value) -> Result<()> {
    if let Some(raw) = v.get("offset")
        && offset(Some(raw)).is_none()
    {
        return Err(IoError::Format(format!(
            "plot offset must contain exactly two finite values from -{} to {} mm",
            paper::MAX_PLOT_OFFSET_MM,
            paper::MAX_PLOT_OFFSET_MM
        )));
    }
    Ok(())
}

/// `[l, b, r, t]` margins in mm (finite, 0–1000), else `None`.
pub(crate) fn margins(v: Option<&Value>) -> Option<[f64; 4]> {
    let a: Vec<f64> = v?.as_array()?.iter().take(5).filter_map(Value::as_f64).collect();
    match a[..] {
        [l, b, r, t] if a.iter().all(|x| x.is_finite() && (0.0..=1000.0).contains(x)) => Some([l, b, r, t]),
        _ => None,
    }
}

/// A plot window from `[[x1, y1], [x2, y2]]`: finite corners and a positive size, else `None`.
fn window(v: Option<&Value>) -> Option<Bounds2> {
    let corner = |i: usize| {
        let c = v?.get(i)?;
        let p = Vec2::new(c.get(0)?.as_f64()?, c.get(1)?.as_f64()?);
        p.is_finite().then_some(p)
    };
    let (a, b) = (corner(0)?, corner(1)?);
    let w = Bounds2::new(a, b);
    (w.width() > 0.0 && w.height() > 0.0).then_some(w)
}

/// Plot with JSON options (the engine's `plot` hook).
pub fn plot(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    validate_plot_offset_json(opts)?;
    // `layouts: [name | "Model"] | "all"` plots those sheets into one file, one page each,
    // instead of `space`; "all" = Model if it has objects, then the layouts in tab order.
    let o = PdfOptions::from_json(opts);
    // `pages: [{window: [[x1,y1],[x2,y2]], paper?, landscape?, scale?}]` plots model space once
    // per window, one page each (a book of sheet frames), instead of `space`.
    if let Some(pages) = opts.get("pages") {
        let list = pages.as_array().ok_or_else(|| IoError::Format("`pages` must be a list of {window, paper?, landscape?}".into()))?;
        if list.len() > MAX_PAGES {
            return Err(IoError::Format(format!("at most {MAX_PAGES} sheets fit in one PDF")));
        }
        let items = list
            .iter()
            .map(|pg| {
                let w = window(pg.get("window")).ok_or_else(|| IoError::Format("each page needs a `window` with two corners".into()))?;
                let mut po = PdfOptions::from_json(&serde_json::json!({
                    "paper": pg.get("paper"), "landscape": pg.get("landscape"), "scale": pg.get("scale"),
                    "plotStyleTable": opts.get("plotStyleTable"), "lineweights": opts.get("lineweights"),
                    "margins": opts.get("margins"), "text": opts.get("text"),
                }));
                po.window = Some(w);
                po.fit = Some(true);
                po.compress = o.compress;
                Ok((Space::Model, po))
            })
            .collect::<Result<Vec<_>>>()?;
        return pdf_items(d, &items, &o);
    }
    match opts.get("layouts") {
        Some(Value::String(a)) if a.eq_ignore_ascii_case("all") => {
            let mut layouts: Vec<_> = d.layouts.iter().collect();
            layouts.sort_by_key(|l| l.tab_order);
            let model = (!d.model.is_empty()).then_some(Space::Model);
            let spaces: Vec<Space> = model.into_iter().chain(layouts.iter().map(|l| Space::Paper(l.name.clone()))).collect();
            pdf_sheets(d, &spaces, &o)
        }
        Some(v) => {
            let names = v.as_array().ok_or_else(|| IoError::Format("`layouts` must be a list of layout names".into()))?;
            if names.len() > MAX_PAGES {
                return Err(IoError::Format(format!("at most {MAX_PAGES} sheets fit in one PDF")));
            }
            let spaces = names
                .iter()
                .map(|n| match n.as_str() {
                    Some(n) if n.eq_ignore_ascii_case("model") => Ok(Space::Model),
                    Some(n) => d.layout(n).map(|l| Space::Paper(l.name.clone())).ok_or_else(|| IoError::Format(format!("no layout `{n}`"))),
                    None => Err(IoError::Format("`layouts` must be a list of layout names".into())),
                })
                .collect::<Result<Vec<_>>>()?;
            pdf_sheets(d, &spaces, &o)
        }
        None => pdf(d, space, &o),
    }
}

/// `[{handle: "1A" | 26, url}]` → (handle, url) pairs; malformed entries are skipped.
fn links_json(v: Option<&Value>) -> Vec<(Handle, String)> {
    let Some(a) = v.and_then(Value::as_array) else { return Vec::new() };
    a.iter()
        .take(MAX_LINKS)
        .filter_map(|l| {
            let h = l.get("handle")?;
            let h = h.as_str().and_then(Handle::parse_hex).or_else(|| h.as_u64().map(Handle))?;
            let url = l.get("url")?.as_str()?.trim();
            (!url.is_empty()).then(|| (h, url.to_string()))
        })
        .collect()
}

/// The page setup a plot of `space` uses, with overrides applied.
pub fn page_for(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<PageSetup> {
    let mut page = match space.layout() {
        Some(n) => d.layout(n).map(|l| l.page.clone()).ok_or_else(|| IoError::Format(format!("no layout `{n}`")))?,
        None => {
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
    if let Some(t) = &o.plot_style_table {
        page.plot_style_table = t.clone();
    }
    // The plot device's margins bound a model-space plot (a layout's come with its page setup).
    if let (Space::Model, Some(m)) = (space, o.margins) {
        page.margins_mm = m;
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
    pdf_sheets(d, std::slice::from_ref(space), o)
}

/// The name of a sheet: `Model` or the layout's.
fn sheet_name(space: &Space) -> String {
    space.tab_name().to_string()
}

/// Write a vector PDF with one page per space (at most [`MAX_PAGES`]), each labelled with its
/// layout name. The options apply to every sheet; layouts use their own page setups.
pub fn pdf_sheets(d: &Drawing, spaces: &[Space], o: &PdfOptions) -> Result<Vec<u8>> {
    let items: Vec<(Space, PdfOptions)> = spaces.iter().take(MAX_PAGES + 1).map(|s| (s.clone(), o.clone())).collect();
    pdf_items(d, &items, o)
}

/// Write a vector PDF with one page per `(space, options)` item (at most [`MAX_PAGES`]): each
/// page plots with its own options (window, paper, orientation); `o` supplies the file-wide
/// ones (compression, title, layers, links).
pub fn pdf_items(d: &Drawing, items: &[(Space, PdfOptions)], o: &PdfOptions) -> Result<Vec<u8>> {
    if items.iter().any(|(_, io)| io.offset.is_some_and(|offset| !valid_plot_offset(offset))) {
        return Err(IoError::Format("plot offset must contain two finite values within ±100000 mm".into()));
    }
    if items.is_empty() {
        return Err(IoError::Format("there are no sheets to plot".into()));
    }
    if items.len() > MAX_PAGES {
        return Err(IoError::Format(format!("at most {MAX_PAGES} sheets fit in one PDF")));
    }
    match write_pdf(d, items, o, o.text_as_geometry) {
        // A font the PDF writer can't embed after all, or one that makes it fail internally:
        // plot the text as outlines instead. Outlines embed no font, so this runs at most once.
        Err(Failure::Write(e)) if !o.text_as_geometry && font_failure(&e) => write_pdf(d, items, o, true).map_err(Failure::into_io),
        r => r.map_err(Failure::into_io),
    }
}

/// Whether a writer failure may come from embedding a font: a font the writer refused, or a
/// panic inside the writer (caught by `guard`), which a malformed font can cause.
fn font_failure(e: &WriteError) -> bool {
    match e {
        WriteError::Serialization(e) => matches!(**e, kr::error::KrillaError::Font(..)),
        WriteError::BackendPanic(_) => true,
        _ => false,
    }
}

/// Why writing failed: our own error, or the PDF writer's.
enum Failure {
    Io(IoError),
    Write(WriteError),
}

impl From<IoError> for Failure {
    fn from(e: IoError) -> Self {
        Failure::Io(e)
    }
}

impl From<WriteError> for Failure {
    fn from(e: WriteError) -> Self {
        Failure::Write(e)
    }
}

impl Failure {
    fn into_io(self) -> IoError {
        match self {
            Failure::Io(e) => e,
            Failure::Write(e) => IoError::Format(format!("PDF export failed: {e}")),
        }
    }
}

/// Write the file, one sheet at a time: each sheet's display list is built, written as its page
/// and dropped before the next.
fn write_pdf(d: &Drawing, items: &[(Space, PdfOptions)], o: &PdfOptions, text_as_geometry: bool) -> std::result::Result<Vec<u8>, Failure> {
    write::guard(|| {
        // Layers that are off plot on PDF layers that start hidden: draw them from a copy with
        // them on.
        let with_off;
        let drawn = if o.layers && o.include_off_layers && d.layers.iter().any(|l| !l.on && !l.frozen) {
            let mut c = d.clone();
            for l in c.layers.iter_mut().filter(|l| !l.frozen) {
                l.on = true;
            }
            with_off = c;
            &with_off
        } else {
            d
        };
        let links = all_links(d, o);
        let title = if o.title.is_empty() { items.first().map(|(s, _)| sheet_name(s)).unwrap_or_default() } else { o.title.clone() };
        let metadata = Metadata {
            creator: "CADCraft".into(),
            producer: "CADCraft".into(),
            title: Some(title.chars().take(MAX_TEXT).collect()),
            ..Metadata::default()
        };
        let settings = kr::SerializeSettings {
            compress_content_streams: o.compress,
            xmp_metadata: false,
            enable_tagging: false,
            ..kr::SerializeSettings::default()
        };
        let mut writer = Writer::new(settings, metadata);
        let mut fonts = fonts::Fonts::new(!text_as_geometry);
        let mut groups = Groups::default();
        for (s, po) in items {
            let page = sheet(drawn, s, po, &links)?;
            let doc = writer.document_mut();
            // Groups are added to the document, so they must exist before the page starts.
            groups.add(doc, d, &page.layers);
            write_page(doc, &page, &groups, &mut fonts)?;
        }
        if items.len() == 1 && !groups.ids.is_empty() {
            writer.document_mut().set_optional_content_page_mode(true);
        }
        Ok(writer.finish()?)
    })
}

/// The PDF layers (optional content groups) of a file: one per drawing layer used, shared by
/// all pages, at most [`MAX_LAYERS`].
#[derive(Default)]
struct Groups {
    ids: std::collections::HashMap<u32, OcgId>,
}

impl Groups {
    /// Add groups for the drawing layers in `layers` that don't have one yet, named after the
    /// layer and visible when the layer is on in `d`, the drawing as given (not the copy that
    /// turns layers that are off on to plot them).
    fn add(&mut self, doc: &mut kr::Document, d: &Drawing, layers: &[u32]) {
        for l in layers {
            if self.ids.len() >= MAX_LAYERS || self.ids.contains_key(l) {
                continue;
            }
            let Some(layer) = d.layers.get(*l as usize) else { continue };
            let name: String = layer.name.chars().take(MAX_LAYER_NAME).collect();
            let id = doc.add_optional_content_group(name, layer.on);
            self.ids.insert(*l, id);
        }
    }
}

/// One plotted sheet, ready to be written as a page.
struct SheetPage {
    /// Page size in points.
    media: Vec2,
    /// What it draws, the mapping to points and the plot area (points).
    list: cadcraft_render::DisplayList,
    map: Map,
    clip_pt: Bounds2,
    /// The drawing layers (indices in the layer table) drawn on, in order of first use: the
    /// candidates for PDF layers.
    layers: Vec<u32>,
    /// Link annotations: rectangle in points and target.
    links: Vec<(Bounds2, String)>,
    /// Page label: the layout name.
    label: String,
}

/// Plot one space.
fn sheet(d: &Drawing, space: &Space, o: &PdfOptions, links_to: &[(Handle, String)]) -> Result<SheetPage> {
    let page = page_for(d, space, o)?;
    let unit_mm = paper::page_unit_mm(d, &page);
    let sheet = Sheet::from_page(&page, unit_mm);
    let lineweights = o.lineweights.unwrap_or(if space.is_paper() { page.lineweights } else { true });
    let plot_style_table = cadcraft_doc::plot_style_table(d, &page.plot_style_table).map(std::sync::Arc::new);
    let ropts = cadcraft_render::Options {
        transparency: o.plot_transparency.unwrap_or(page.plot_transparency),
        tolerance: 0.001,
        min_dash: 0.0,
        text: true,
        fill: true,
        lineweights,
        view_height: 0.0,
        plot_style_table,
        view: None,
        skip_viewport: None,
        cancel: None,
    };
    let k = unit_mm * PT_PER_MM;
    let window = o.window.filter(|w| matches!(space, Space::Model) && !w.is_empty());
    let fit = o.fit.unwrap_or(window.is_some() || matches!(space, Space::Model));
    // Chord tolerance: about 0.05 mm on paper.
    let est = plot_scale(&window.unwrap_or_else(|| d.extents(space)), &sheet, fit, o.scale);
    let tol = 0.05 / unit_mm / est.max(1e-300);
    let tolerance = if tol.is_finite() && tol > 0.0 { tol } else { ropts.tolerance };
    // Relative point sizes follow the plotted window (else the extents).
    let view_height = window.map(|w| w.height()).unwrap_or(0.0);
    let list = cadcraft_render::build_plot(d, space, &cadcraft_render::Options { tolerance, view_height, ..ropts });
    let b = window.unwrap_or(list.bounds);
    let s = plot_scale(&b, &sheet, fit, o.scale);
    let map = if fit || matches!(space, Space::Model) {
        let (from, to) = placement(space, &b, fit, &sheet, o);
        Map { from, s, to, k }
    } else {
        Map { from: Vec2::ZERO, s, to: Vec2::ZERO, k }
    };
    // Plot style ends, joins and fill patterns at the plot scale (a layout's are already drawn).
    let mut list = list;
    cadcraft_render::pen::apply(&mut list, map.s * unit_mm);
    let media = sheet.size * k;
    let clip_pt = if fit || matches!(space, Space::Model) {
        Bounds2::new(sheet.printable.min * k, sheet.printable.max * k)
    } else {
        Bounds2::new(Vec2::ZERO, media)
    };
    // The layers drawn on, in order of first use.
    let mut layers: Vec<u32> = Vec::new();
    if o.layers {
        let mut seen = std::collections::HashSet::new();
        for l in &list.prim_layers {
            if (*l as usize) < d.layers.len() && layers.len() < MAX_LAYERS && seen.insert(*l) {
                layers.push(*l);
            }
        }
    }
    let links = links(&list, &map, &clip_pt, links_to);
    Ok(SheetPage { media, list, map, clip_pt, layers, links, label: sheet_name(space) })
}

/// Source and destination origins for the shared model-space PDF/PNG placement behavior.
pub(crate) fn placement(space: &Space, bounds: &Bounds2, fit: bool, sheet: &Sheet, o: &PdfOptions) -> (Vec2, Vec2) {
    if matches!(space, Space::Model) {
        let from = if bounds.is_empty() {
            Vec2::ZERO
        } else if o.center.unwrap_or(true) {
            bounds.center()
        } else {
            bounds.min
        };
        let to = if o.center.unwrap_or(true) {
            sheet.printable.center()
        } else {
            let offset = o.offset.unwrap_or([0.0, 0.0]);
            sheet.printable.min + Vec2::new(offset[0] / sheet.unit_mm, offset[1] / sheet.unit_mm)
        };
        (from, to)
    } else if fit {
        (if bounds.is_empty() { Vec2::ZERO } else { bounds.center() }, sheet.printable.center())
    } else {
        (Vec2::ZERO, Vec2::ZERO)
    }
}

/// The hyperlinks to plot: the options' own, then (with [`PdfOptions::hyperlinks`]) those the
/// drawing file gave its entities as `PE_URL` extended data, at most [`MAX_LINKS`] in all.
fn all_links(d: &Drawing, o: &PdfOptions) -> Vec<(Handle, String)> {
    let mut out: Vec<(Handle, String)> = o.links.iter().take(MAX_LINKS).cloned().collect();
    if !o.hyperlinks {
        return out;
    }
    let given: std::collections::HashSet<Handle> = out.iter().map(|(h, _)| *h).collect();
    let mut read: Vec<(Handle, String)> = std::iter::once(&d.model)
        .chain(d.layouts.iter().map(|l| &l.entities))
        .chain(d.blocks.values().map(|b| &b.entities))
        .flat_map(|s| s.iter())
        .filter(|e| !given.contains(&e.handle))
        .filter_map(|e| Some((e.handle, pe_url(&e.common.xdata)?)))
        .collect();
    read.sort_by_key(|(h, _)| *h);
    read.dedup_by_key(|(h, _)| *h);
    out.extend(read.into_iter().take(MAX_LINKS.saturating_sub(out.len())));
    out
}

/// The link target of `PE_URL` extended data: the first string (group 1000) after
/// `1001 PE_URL`, before the next application's data.
fn pe_url(xdata: &[cadcraft_doc::RawTag]) -> Option<String> {
    let start = xdata.iter().position(|t| t.code == 1001 && t.value.trim().eq_ignore_ascii_case("PE_URL"))?;
    let url = xdata.get(start + 1..)?.iter().take_while(|t| t.code != 1001).find(|t| t.code == 1000)?.value.trim();
    (!url.is_empty() && url.len() <= MAX_URL).then(|| url.to_string())
}

/// Link annotation rectangles: the area (in points) each linked entity covers on the sheet,
/// clipped to the plot area.
fn links(list: &cadcraft_render::DisplayList, map: &Map, clip_pt: &Bounds2, links: &[(Handle, String)]) -> Vec<(Bounds2, String)> {
    if links.is_empty() {
        return Vec::new();
    }
    // Membership by handle, built once, so scanning primitives is O(primitives) rather than
    // O(primitives × links): otherwise the two counts multiply into excessive work (CWE-407).
    let linked: std::collections::HashSet<Handle> = links.iter().map(|(h, _)| *h).collect();
    let mut areas: std::collections::HashMap<Handle, Bounds2> = std::collections::HashMap::new();
    for p in &list.prims {
        if matches!(p.kind, Kind::Infinite { .. }) || !linked.contains(&p.handle) {
            continue;
        }
        let b = areas.entry(p.handle).or_insert(Bounds2::EMPTY);
        for q in list.points(p) {
            let q = map.pt(*q);
            if q.is_finite() {
                b.add(q);
            }
        }
    }
    let mut out = Vec::new();
    for (h, url) in links {
        let Some(b) = areas.get(h).filter(|b| !b.is_empty()) else { continue };
        // A little slack so thin lines are easy to click.
        let b = b.expand(2.0);
        let r = Bounds2 { min: b.min.max(clip_pt.min), max: b.max.min(clip_pt.max) };
        if r.min.x < r.max.x && r.min.y < r.max.y {
            out.push((r, url.clone()));
        }
    }
    out
}

/// Paper units per drawing unit for a plot.
pub(crate) fn plot_scale(b: &Bounds2, sheet: &Sheet, fit: bool, scale: Option<f64>) -> f64 {
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

/// Largest coordinate written (points); a primitive reaching further is clipped to the plot
/// area first. The PDF writer's geometry is `f32`.
const MAX_COORD: f64 = 1.0e7;

/// A point in points as the writer's `f32`, or `None` when it is not finite or out of range.
fn coord(p: Vec2) -> Option<(f32, f32)> {
    let ok = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
    (ok(p.x) && ok(p.y)).then_some((p.x as f32, p.y as f32))
}

/// A rectangle path between two corners (points), if it has an area.
fn rect_path(min: Vec2, max: Vec2) -> Option<Path> {
    let (x0, y0) = coord(min)?;
    let (x1, y1) = coord(max)?;
    let mut pb = PathBuilder::new();
    pb.push_rect(Rect::from_ltrb(x0, y0, x1, y1)?);
    pb.finish()
}

/// Diameter (points) of the dot a zero-length hairline plots: a hairline has no width of its
/// own, and this is about the thinnest mark that still prints.
const HAIRLINE_DOT_PT: f32 = 0.25;

/// A circle path (points) around `(x, y)`: four cubic arcs.
fn circle_path(x: f32, y: f32, r: f32) -> Option<Path> {
    // Control-point distance of a quarter-circle cubic, as a fraction of the radius.
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.cubic_to(x + r, y + k, x + k, y + r, x, y + r);
    pb.cubic_to(x - k, y + r, x - r, y + k, x - r, y);
    pb.cubic_to(x - r, y - k, x - k, y - r, x, y - r);
    pb.cubic_to(x + k, y - r, x + r, y - k, x + r, y);
    pb.close();
    pb.finish()
}

/// Write one sheet as a page: its size, label, link annotations and marks.
fn write_page(doc: &mut kr::Document, p: &SheetPage, groups: &Groups, fonts: &mut fonts::Fonts) -> std::result::Result<(), Failure> {
    let (w, h) = (p.media.x as f32, p.media.y as f32);
    let label = PageLabel::new(None, Some(p.label.chars().take(MAX_TEXT).collect()), None);
    let settings = PageSettings::from_wh(w, h).ok_or_else(|| IoError::Format("the sheet has no area".into()))?.with_page_label(label);
    let mut page = doc.start_page_with(settings);
    for (r, url) in &p.links {
        let Some(uri) = uri(url) else { continue };
        // Annotation rectangles are given top-down, like the writer's page surface.
        let Some(rect) = Rect::from_ltrb(r.min.x as f32, h - r.max.y as f32, r.max.x as f32, h - r.min.y as f32) else { continue };
        let link = LinkAnnotation::new(rect, Target::Action(LinkAction::new(uri).into()));
        page.add_annotation(Annotation::new_link(link, None));
    }
    let mut surface = page.surface();
    // The surface's y axis points down; flip it so we draw in PDF points (y up).
    surface.push_transform(&Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, h));
    // Clip to the plot area.
    let mut drawn = Ok(());
    if let Some(clip) = rect_path(p.clip_pt.min, p.clip_pt.max) {
        surface.push_clip_path(&clip, &FillRule::NonZero);
        drawn = marks(&mut surface, p, groups, fonts);
        surface.pop();
    }
    surface.pop();
    surface.finish();
    page.finish();
    drawn
}

/// What consecutive strokes share to be painted as one path: colour, width (points) and
/// transparency (percent).
#[derive(Clone, Copy, PartialEq)]
struct StrokeStyle {
    rgb: Rgb,
    width: f32,
    transparency: u8,
}

/// Opacity of a transparency in percent.
fn opacity(t: u8) -> NormalizedF32 {
    NormalizedF32::new(f32::from(100 - t.min(cadcraft_render::MAX_TRANSPARENCY)) / 100.0).unwrap_or(NormalizedF32::ONE)
}

fn paint(c: Rgb) -> kr::paint::Paint {
    kr::color::rgb::Color::new(c.0, c.1, c.2).into()
}

fn fill_path(s: &mut Surface<'_>, path: &Path, rgb: Rgb, t: u8) {
    s.set_stroke(None);
    s.set_fill(Some(Fill { paint: paint(rgb), opacity: opacity(t), rule: FillRule::NonZero }));
    s.draw_path(path);
}

/// Paint the strokes collected so far as one path.
fn flush(s: &mut Surface<'_>, batch: &mut Option<(StrokeStyle, PathBuilder)>) {
    let Some((style, pb)) = batch.take() else { return };
    let Some(path) = pb.finish() else { return };
    s.set_fill(None);
    s.set_stroke(Some(Stroke {
        paint: paint(style.rgb),
        width: style.width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        opacity: opacity(style.transparency),
        ..Stroke::default()
    }));
    s.draw_path(&path);
}

/// The PDF layer of the following marks changed from `from` to `to` (`None`: no layer): end
/// the old one's marked content and start the new one's.
fn layer_changed(s: &mut Surface<'_>, from: Option<OcgId>, to: Option<OcgId>) -> std::result::Result<(), Failure> {
    if from.is_some() {
        s.end_optional_content().map_err(krilla_failure)?;
    }
    if let Some(id) = to {
        s.start_optional_content(id).map_err(krilla_failure)?;
    }
    Ok(())
}

fn krilla_failure(e: KrillaError) -> Failure {
    Failure::Io(IoError::Format(format!("PDF export failed: {e}")))
}

/// The page's marks, clipped to the plot area by the caller. Those of primitives on a layer
/// with a PDF layer in `groups` are marked as its optional content.
///
/// Consecutive strokes with the same colour, width and transparency are one path; a
/// transparent primitive is stroked on its own (one path is painted once), so where
/// primitives overlap they build up, as in PNG plots and on screen. Block contents and
/// viewport contents share their reference's handle, so this goes by primitive, not by object.
/// Each filled primitive is one path of its triangles.
///
/// Primitives that stand for a glyph in an embeddable font (see [`fonts::Fonts`]) are written
/// as text instead: the glyph at font size 1 under its glyph matrix (the matrix axes are one
/// em).
fn marks(s: &mut Surface<'_>, p: &SheetPage, groups: &Groups, fonts: &mut fonts::Fonts) -> std::result::Result<(), Failure> {
    let list = &p.list;
    let texts = fonts::texts(list);
    let white = Rgb(255, 255, 255);
    let mut batch: Option<(StrokeStyle, PathBuilder)> = None;
    let mut layer: Option<OcgId> = None;
    for (i, prim) in list.prims.iter().enumerate() {
        let want = list.prim_layers.get(i).and_then(|l| groups.ids.get(l)).copied();
        if want != layer {
            flush(s, &mut batch);
            layer_changed(s, layer, want)?;
            layer = want;
        }
        let rgb = prim.display_rgb(white);
        let raw = list.points(prim);
        if raw.iter().any(|q| !q.is_finite()) {
            continue;
        }
        // A wipeout's mask always hides what is under it.
        let t = if prim.kind == Kind::Mask { 0 } else { prim.transparency.min(cadcraft_render::MAX_TRANSPARENCY) };
        let w = f64::from(prim.lw) * PT_PER_MM;
        let w = if w.is_finite() && w > 0.0 { w.min(100.0) } else { 0.0 };
        let m = Mark { rgb, t, w };
        match prim.kind {
            Kind::Polyline | Kind::Infinite { .. } => polyline(s, &mut batch, p, prim, raw, m),
            Kind::Tris | Kind::Mask => {
                flush(s, &mut batch);
                // A wipeout's mask is paper white.
                let m = if prim.kind == Kind::Mask { Mark { rgb: white, ..m } } else { m };
                let text_font =
                    u32::try_from(i).ok().and_then(|i| texts.get(&i).copied()).and_then(|text| fonts.font(&text.glyph.face).map(|font| (text, font)));
                match text_font {
                    Some((text, font)) => glyph(s, font, text, &p.map, m.rgb, m.t),
                    None => fill_tris(s, p, raw, m.rgb, m.t),
                }
            }
            Kind::Point => point(s, &mut batch, &p.map, raw, m),
        }
    }
    flush(s, &mut batch);
    layer_changed(s, layer, None)
}

/// How a primitive is painted: colour, transparency (percent) and lineweight (points).
#[derive(Clone, Copy)]
struct Mark {
    rgb: Rgb,
    t: u8,
    w: f64,
}

/// Stroke a polyline or infinite line, clipped to the plot area, into the open batch.
fn polyline(s: &mut Surface<'_>, batch: &mut Option<(StrokeStyle, PathBuilder)>, p: &SheetPage, prim: &DPrim, raw: &[Vec2], m: Mark) {
    let Some(pts) = polyline_points(prim, raw, &p.map, &p.clip_pt) else { return };
    if pts.len() < 2 {
        return;
    }
    // With a vertex out of range, keep the pieces inside the plot area, widened so
    // a wide stroke just outside it still shows its edge.
    let runs = if pts.iter().all(|q| coord(*q).is_some()) { vec![pts] } else { clip::clip_polyline(&pts, &p.clip_pt.expand(m.w + 1.0)) };
    let style = StrokeStyle { rgb: m.rgb, width: m.w as f32, transparency: m.t };
    if m.t > 0 || batch.as_ref().is_some_and(|(b, _)| *b != style) {
        flush(s, batch);
    }
    for run in runs {
        stroke_run(s, batch, &run, style, m);
    }
}

/// The page points of a polyline, or of an infinite line clipped to the plot area; `None` when
/// there is nothing to draw.
fn polyline_points(prim: &DPrim, raw: &[Vec2], map: &Map, clip_pt: &Bounds2) -> Option<Vec<Vec2>> {
    match prim.kind {
        Kind::Infinite { ray } => {
            let (Some(base), Some(dir)) = (raw.first(), raw.get(1)) else { return None };
            let a = map.pt(*base);
            let dir_pt = map.pt(*base + *dir) - a;
            let (x, y) = clip::clip_infinite(a, dir_pt, ray, clip_pt)?;
            Some(vec![x, y])
        }
        _ => Some(raw.iter().map(|q| map.pt(*q)).collect()),
    }
}

/// Add one run of a polyline to the batch, or paint it as a dot when it has no extent.
fn stroke_run(s: &mut Surface<'_>, batch: &mut Option<(StrokeStyle, PathBuilder)>, run: &[Vec2], style: StrokeStyle, m: Mark) {
    let Some(pts) = run.iter().map(|q| coord(*q)).collect::<Option<Vec<_>>>() else { return };
    // The writer leaves out a path without extent, so a zero-length line is
    // painted as the round-cap dot its stroke would make.
    if let Some(&(x, y)) = pts.first()
        && pts.iter().all(|q| *q == (x, y))
    {
        flush(s, batch);
        let d = if m.w > 0.0 { m.w as f32 } else { HAIRLINE_DOT_PT };
        if let Some(path) = circle_path(x, y, d / 2.0) {
            fill_path(s, &path, m.rgb, m.t);
        }
        return;
    }
    let (_, pb) = batch.get_or_insert_with(|| (style, PathBuilder::new()));
    for (j, (x, y)) in pts.iter().enumerate() {
        if j == 0 {
            pb.move_to(*x, *y);
        } else {
            pb.line_to(*x, *y);
        }
    }
}

/// Fill a set of triangles, clipped to the plot area.
fn fill_tris(s: &mut Surface<'_>, p: &SheetPage, raw: &[Vec2], rgb: Rgb, t: u8) {
    let mapped: Vec<Vec2> = raw.iter().map(|q| p.map.pt(*q)).collect();
    // With a vertex out of range, keep the parts inside the plot area.
    let tris = if mapped.iter().all(|q| coord(*q).is_some()) { mapped } else { clip::clip_triangles(&mapped, &p.clip_pt) };
    let mut pb = PathBuilder::new();
    for tri in tris.as_chunks::<3>().0 {
        let Some(v) = tri.iter().map(|q| coord(*q)).collect::<Option<Vec<_>>>() else { continue };
        for (j, (x, y)) in v.iter().enumerate() {
            if j == 0 {
                pb.move_to(*x, *y);
            } else {
                pb.line_to(*x, *y);
            }
        }
        pb.close();
    }
    if let Some(path) = pb.finish() {
        fill_path(s, &path, rgb, t);
    }
}

/// Paint a point marker: a small square at the point.
fn point(s: &mut Surface<'_>, batch: &mut Option<(StrokeStyle, PathBuilder)>, map: &Map, raw: &[Vec2], m: Mark) {
    let Some(q) = raw.first() else { return };
    flush(s, batch);
    let half = (m.w.max(0.5)) / 2.0;
    let c = map.pt(*q);
    if let Some(path) = rect_path(c - Vec2::new(half, half), c + Vec2::new(half, half)) {
        fill_path(s, &path, m.rgb, m.t);
    }
}

/// Write one glyph as text in its embedded font: font size 1, placed by the glyph's matrix (one
/// em along each axis). A glyph whose matrix is out of range or collapses is left out.
fn glyph(s: &mut Surface<'_>, font: kr::text::Font, t: &cadcraft_render::PlotText, map: &Map, rgb: Rgb, transparency: u8) {
    let k = map.s * map.k;
    let g = &t.glyph;
    let (Some((a, b)), Some((c, d)), Some((e, f))) = (coord(g.x * k), coord(g.y * k), coord(map.pt(g.origin))) else { return };
    let det = a * d - b * c;
    if !det.is_finite() || det == 0.0 {
        return;
    }
    let text = g.ch.to_string();
    let glyphs = [KrillaGlyph::new(GlyphId::new(u32::from(g.gid)), 0.0, 0.0, 0.0, 0.0, 0..text.len(), None)];
    // The writer sets glyphs upright on a y-down surface: flip the em's y axis back.
    s.push_transform(&Transform::from_row(a, b, -c, -d, e, f));
    s.set_stroke(None);
    s.set_fill(Some(Fill { paint: paint(rgb), opacity: opacity(transparency), rule: FillRule::NonZero }));
    s.draw_glyphs(Point::from_xy(0.0, 0.0), &glyphs, font, &text, 1.0, false);
    s.pop();
}

/// A link target as a URI: 7-bit printable ASCII, other bytes percent-encoded. `None` for
/// script and data URLs, which a plot never carries.
fn uri(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    if url.is_empty() || url.len() > MAX_URL || matches!(scheme.as_str(), "javascript" | "vbscript" | "data") {
        return None;
    }
    let mut out = String::with_capacity(url.len());
    for b in url.bytes() {
        match b {
            0x21..=0x7E => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    Some(out)
}
