//! Vector PDF plotting (PLOT / EXPORTPDF): a minimal PDF writer of our own (pure Rust, no file
//! access, so it runs on the web too).
//!
//! One page per sheet: model-space extents fitted to a sheet, or a layout at 1:1 (or the given
//! `scale`) on its paper (paper units are millimetres in metric drawings, inches otherwise).
//! Lines are stroked paths (`m`/`l`/`S`), fills are `f` paths, colours are RGB with colour 7 printing black on white
//! paper, and line widths come from lineweights when enabled. The page setup's plot style
//! table (or the `plotStyleTable` option) sets the printed colours, linetypes and lineweights.
//!
//! Several sheets go into one file ([`pdf_sheets`], the `layouts` option), each page labelled
//! with its layout name. Drawing layers become PDF layers (optional content groups) that a PDF
//! viewer can switch on and off, and entity hyperlinks become link annotations.

use std::fmt::Write as _;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Handle, PageSetup, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{Kind, Sheet, clip, paper};
use serde_json::Value;

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
/// Most link annotations in one file, and the longest link target (bytes).
const MAX_LINKS: usize = 10_000;
const MAX_URL: usize = 2048;

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
    /// Model space only: draw it in this 3D view (`window` is then in its view coordinates)
    /// instead of in plan. Ignored for layouts.
    pub view: Option<crate::View3d>,
    /// Drawing layers become PDF layers (optional content groups). Default on.
    pub layers: bool,
    /// With `layers`: objects on layers that are off are included on PDF layers that start
    /// hidden (frozen layers are never included). Default off: they aren't plotted.
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
            view: None,
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
    /// hyperlinks?: bool, plotTransparency?: bool, viewDir?: [x, y, z], visualStyle?}` (a
    /// model-space 3D view: [`PdfOptions::view`]).
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
            view: view3d(v),
            margins: margins(v.get("margins")),
            hyperlinks: v.get("hyperlinks").and_then(Value::as_bool).unwrap_or(true),
            plot_transparency: v.get("plotTransparency").and_then(Value::as_bool),
        }
    }
}

/// The 3D view of `viewDir: [x, y, z]` (from the target towards the viewer; any length) in the
/// `visualStyle` (`2dwireframe` default, `shaded`, `shadedwithedges`). `None` for a missing,
/// non-finite or zero direction, or a plan view.
fn view3d(v: &Value) -> Option<crate::View3d> {
    let a = v.get("viewDir")?.as_array()?;
    let [x, y, z] = a.as_slice() else { return None };
    let dir = cadcraft_geom::Vec3::new(x.as_f64()?, y.as_f64()?, z.as_f64()?);
    let len = dir.len();
    if !(len.is_finite() && len > 1e-12) {
        return None;
    }
    let projection = cadcraft_geom::Mat4::view(dir * (1.0 / len), cadcraft_geom::Vec3::ZERO, 0.0);
    if projection.is_plan() {
        return None;
    }
    let style = v.get("visualStyle").and_then(Value::as_str).map(str::to_ascii_lowercase);
    let (shaded, shaded_edges) = match style.as_deref() {
        Some("shaded") => (true, false),
        Some("shadedwithedges") => (true, true),
        _ => (false, false),
    };
    Some(crate::View3d { projection, shaded, shaded_edges })
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
    // Layers that are off plot on PDF layers that start hidden: draw them from a copy with them on.
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
    let pages = items.iter().map(|(s, po)| sheet(drawn, s, po, &links)).collect::<Result<Vec<_>>>()?;
    let title = if o.title.is_empty() { pages.first().map(|p| p.label.clone()).unwrap_or_default() } else { o.title.clone() };
    let fonts = if o.text_as_geometry { fonts::Fonts::default() } else { fonts::Fonts::collect(pages.iter().map(|p| &p.list)) };
    Ok(assemble(d, &pages, &fonts, o, &title))
}

/// One plotted sheet, ready to be written as a page.
struct SheetPage {
    /// Page size in points.
    media: Vec2,
    /// What it draws, the mapping to points and the plot area (points).
    list: cadcraft_render::DisplayList,
    map: Map,
    clip_pt: Bounds2,
    /// The drawing layers (indices in the layer table) of the content's `/L<i>` marks.
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
        projection: None,
        shaded: false,
        shaded_edges: false,
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
    let ropts = crate::View3d::apply(o.view.as_ref(), space, cadcraft_render::Options { tolerance, view_height, ..ropts });
    let list = cadcraft_render::build_plot(d, space, &ropts);
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
    let mut areas: std::collections::HashMap<Handle, Bounds2> = std::collections::HashMap::new();
    for p in &list.prims {
        if matches!(p.kind, Kind::Infinite { .. }) || !links.iter().any(|(h, _)| *h == p.handle) {
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

/// The page's marks; those of primitives on `layers` are marked as optional content
/// (`/OC /L<layer> BDC` … `EMC`).
///
/// Primitives in `texts` (filled glyph outlines) are written as text instead: the glyph in its
/// embedded font (`/F<font>`), placed by a text matrix.
///
/// Transparent primitives are drawn with a graphics state of their alpha (`/T<percent> gs`);
/// the transparencies used are returned for the page's `/ExtGState` (see [`ext_gstates`]). A
/// list without transparency writes no graphics states.
fn content_stream(
    list: &cadcraft_render::DisplayList,
    map: &Map,
    clip_pt: &Bounds2,
    layers: &[u32],
    texts: &std::collections::HashMap<u32, (usize, &cadcraft_render::PlotText)>,
) -> (String, Vec<u8>) {
    let marked: std::collections::HashSet<u32> = layers.iter().copied().collect();
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
    // The PDF layer (optional content) the marks are in: `/OC /L<i> BDC` … `EMC`.
    let mut layer: Option<u32> = None;
    // Inside a text object (`BT` … `ET`), with this font set.
    let mut text_font: Option<Option<usize>> = None;
    // The transparency (percent) marks are drawn with, and those used on the page.
    let mut alpha: u8 = 0;
    let mut alphas: Vec<u8> = Vec::new();
    for (i, p) in list.prims.iter().enumerate() {
        let want = list.prim_layers.get(i).copied().filter(|l| marked.contains(l));
        let text = u32::try_from(i).ok().and_then(|i| texts.get(&i));
        if text_font.is_some() && (text.is_none() || want != layer) {
            c.push_str("ET\n");
            text_font = None;
        }
        if want != layer {
            flush(&mut c, &mut pending);
            if layer.is_some() {
                c.push_str("EMC\n");
            }
            if let Some(l) = want {
                let _ = writeln!(c, "/OC /L{l} BDC");
            }
            layer = want;
        }
        let rgb = p.display_rgb(white);
        let raw = list.points(p);
        if raw.iter().any(|q| !q.is_finite()) {
            continue;
        }
        // A wipeout's mask always hides what is under it.
        let t = if p.kind == Kind::Mask { 0 } else { p.transparency.min(cadcraft_render::MAX_TRANSPARENCY) };
        if t != alpha {
            flush(&mut c, &mut pending);
            let _ = writeln!(c, "/T{t} gs");
            if !alphas.contains(&t) {
                alphas.push(t);
            }
            alpha = t;
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
                // A transparent primitive is stroked on its own (one path is painted once), so
                // where primitives overlap they build up, as in PNG plots and on screen. Block
                // contents and viewport contents share their reference's handle, so this goes
                // by primitive, not by object.
                if t > 0 {
                    flush(&mut c, &mut pending);
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
            Kind::Tris | Kind::Mask => {
                flush(&mut c, &mut pending);
                // A wipeout's mask is paper white.
                let rgb = if p.kind == Kind::Mask { white } else { rgb };
                if fill != Some(rgb) {
                    color(&mut c, rgb, "rg");
                    fill = Some(rgb);
                }
                if let Some((font, t)) = text {
                    glyph(&mut c, &mut text_font, *font, t, map);
                    continue;
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
    if text_font.is_some() {
        c.push_str("ET\n");
    }
    if layer.is_some() {
        c.push_str("EMC\n");
    }
    c.push_str("Q\n");
    (c, alphas)
}

/// The page resources' graphics states for transparencies `alphas` (percent): `/T<percent>`
/// sets the stroke and fill alpha (`/CA`, `/ca`). Empty when there are none.
fn ext_gstates(alphas: &[u8]) -> String {
    if alphas.is_empty() {
        return String::new();
    }
    let mut out = String::from(" /ExtGState <<");
    for t in alphas {
        let mut a = String::new();
        num(&mut a, f64::from(100 - u32::from((*t).min(cadcraft_render::MAX_TRANSPARENCY))) / 100.0);
        let _ = write!(out, " /T{t} << /Type /ExtGState /CA {a} /ca {a} >>");
    }
    out.push_str(" >>");
    out
}

/// Write one glyph as text: opens a text object and sets the font when needed, then places the
/// glyph with a text matrix (font size 1: the matrix axes are one em).
fn glyph(c: &mut String, text_font: &mut Option<Option<usize>>, font: usize, t: &cadcraft_render::PlotText, map: &Map) {
    if text_font.is_none() {
        c.push_str("BT\n");
        *text_font = Some(None);
    }
    if *text_font != Some(Some(font)) {
        let _ = writeln!(c, "/F{font} 1 Tf");
        *text_font = Some(Some(font));
    }
    let s = map.s * map.k;
    let g = &t.glyph;
    for v in [g.x.x * s, g.x.y * s, g.y.x * s, g.y.y * s] {
        num(c, v);
        c.push(' ');
    }
    pt(c, map.pt(g.origin));
    let _ = writeln!(c, " Tm <{:04X}> Tj", g.gid);
}

/// A PDF text string: a literal with `(`, `)` and `\` escaped when it is printable ASCII,
/// else UTF-16BE with a byte order mark (hex). At most 1000 characters.
fn pdf_string(s: &str) -> String {
    let s: String = s.chars().take(1000).collect();
    if s.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        let mut out = String::from("(");
        for ch in s.chars() {
            if matches!(ch, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push(')');
        return out;
    }
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out.push('>');
    out
}

/// A link target as a PDF URI string: 7-bit ASCII (other bytes percent-encoded), `(`, `)` and
/// `\` escaped. `None` for script and data URLs, which a plot never carries.
fn uri_string(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    if url.is_empty() || url.len() > MAX_URL || matches!(scheme.as_str(), "javascript" | "vbscript" | "data") {
        return None;
    }
    let mut out = String::from("(");
    for b in url.bytes() {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(b));
            }
            0x21..=0x7E => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out.push(')');
    Some(out)
}

/// A stream object body, Flate-compressed when asked.
fn stream(data: &[u8], compress: bool, extra: &str) -> Vec<u8> {
    let (data, filter) = if compress { (miniz_oxide::deflate::compress_to_vec_zlib(data, 6), " /Filter /FlateDecode") } else { (data.to_vec(), "") };
    let mut out = format!("<< /Length {}{filter}{extra} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(&data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// PDF objects being written: bodies by object number - 1.
#[derive(Default)]
struct Objects {
    bodies: Vec<Vec<u8>>,
}

impl Objects {
    /// Reserve an object number (its body is set later).
    fn reserve(&mut self) -> usize {
        self.bodies.push(Vec::new());
        self.bodies.len()
    }
    fn set(&mut self, n: usize, body: impl Into<Vec<u8>>) {
        if let Some(b) = n.checked_sub(1).and_then(|i| self.bodies.get_mut(i)) {
            *b = body.into();
        }
    }
    fn add(&mut self, body: impl Into<Vec<u8>>) -> usize {
        let n = self.reserve();
        self.set(n, body);
        n
    }
}

/// `n 0 R` references, space-separated.
fn refs(ids: impl IntoIterator<Item = usize>) -> String {
    ids.into_iter().map(|i| format!("{i} 0 R")).collect::<Vec<_>>().join(" ")
}

/// Assemble the file: catalog, page tree, pages with their content streams and link
/// annotations, the PDF layers (one optional content group per drawing layer used, named
/// like it), page labels, document information, cross-reference table and trailer.
fn assemble(d: &Drawing, pages: &[SheetPage], fonts: &fonts::Fonts, o: &PdfOptions, title: &str) -> Vec<u8> {
    let mut objs = Objects::default();
    let catalog = objs.reserve();
    let tree = objs.reserve();
    // Optional content groups shared by all pages, by layer index: (layer, object).
    let mut ocg_ids: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut ocgs: Vec<(u32, usize)> = Vec::new();
    let mut kids = Vec::new();
    // Page dictionaries are completed once the fonts are written: (page, its fonts, the rest).
    let mut page_dicts: Vec<(usize, Vec<usize>, String, String, String)> = Vec::new();
    for p in pages {
        let page = objs.reserve();
        let texts = fonts.texts(&p.list);
        let (marks, alphas) = content_stream(&p.list, &p.map, &p.clip_pt, &p.layers, &texts);
        let content = objs.add(stream(marks.as_bytes(), o.compress, ""));
        let mut used: Vec<usize> = texts.values().map(|(f, _)| *f).collect();
        used.sort_unstable();
        used.dedup();
        let mut props = String::new();
        for l in &p.layers {
            let Some(layer) = d.layers.get(*l as usize) else { continue };
            let id = match ocg_ids.get(l) {
                Some(id) => *id,
                None => {
                    let id = objs.add(format!("<< /Type /OCG /Name {} >>", pdf_string(&layer.name)));
                    ocg_ids.insert(*l, id);
                    ocgs.push((*l, id));
                    id
                }
            };
            let _ = write!(props, " /L{l} {id} 0 R");
        }
        let mut annots = Vec::new();
        for (r, url) in &p.links {
            let Some(uri) = uri_string(url) else { continue };
            let mut rect = String::new();
            pt(&mut rect, r.min);
            rect.push(' ');
            pt(&mut rect, r.max);
            annots.push(objs.add(format!("<< /Type /Annot /Subtype /Link /Rect [{rect}] /Border [0 0 0] /A << /S /URI /URI {uri} >> >>")));
        }
        let mut mb = String::new();
        pt(&mut mb, p.media);
        let annots = if annots.is_empty() { String::new() } else { format!(" /Annots [{}]", refs(annots)) };
        page_dicts.push((page, used, props, ext_gstates(&alphas), format!("/MediaBox [0 0 {mb}] /Contents {content} 0 R{annots}")));
        kids.push(page);
    }
    let font_ids = if fonts.is_empty() { Vec::new() } else { fonts.write(&mut objs, o.compress) };
    for (page, used, props, gstates, rest) in page_dicts {
        let mut res = String::new();
        if !props.is_empty() {
            let _ = write!(res, " /Properties <<{props} >>");
        }
        res.push_str(&gstates);
        let f: Vec<String> = used.iter().filter_map(|i| Some(format!("/F{i} {} 0 R", font_ids.get(*i)?))).collect();
        if !f.is_empty() {
            let _ = write!(res, " /Font << {} >>", f.join(" "));
        }
        objs.set(page, format!("<< /Type /Page /Parent {tree} 0 R /Resources <<{res} >> {rest} >>"));
    }
    objs.set(tree, format!("<< /Type /Pages /Kids [{}] /Count {} >>", refs(kids.iter().copied()), kids.len()));
    let mut cat = format!("<< /Type /Catalog /Pages {tree} 0 R");
    if !ocgs.is_empty() {
        // Optional content needs PDF 1.5; the catalog's version overrides the header's.
        let off: Vec<usize> = ocgs.iter().filter(|(l, _)| d.layers.get(*l as usize).is_some_and(|l| !l.on)).map(|(_, id)| *id).collect();
        let all = refs(ocgs.iter().map(|(_, id)| *id));
        let _ = write!(cat, " /Version /1.5 /OCProperties << /OCGs [{all}] /D << /Name (Layers) /Order [{all}] /OFF [{}] >> >>", refs(off));
        if pages.len() == 1 {
            cat.push_str(" /PageMode /UseOC");
        }
    }
    let labels: Vec<String> = pages.iter().enumerate().map(|(i, p)| format!("{i} << /P {} >>", pdf_string(&p.label))).collect();
    let _ = write!(cat, " /PageLabels << /Nums [{}] >> >>", labels.join(" "));
    objs.set(catalog, cat);
    let info = objs.add(format!("<< /Producer (CADCraft) /Creator (CADCraft) /Title {} >>", pdf_string(title)));
    let size: usize = objs.bodies.iter().map(Vec::len).sum();
    let mut out: Vec<u8> = Vec::with_capacity(size + 64 * objs.bodies.len() + 256);
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objs.bodies.len());
    for (i, body) in objs.bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(x, "{o:010} 00000 n ");
    }
    let _ = write!(x, "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out.extend_from_slice(x.as_bytes());
    out
}
