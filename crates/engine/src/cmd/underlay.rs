//! PDF underlays: PDFATTACH puts one page of a PDF file under the drawing, at its size on paper.
//! PDFADJUST fades it, sets its contrast, shows it in greys and adapts its colours to a dark background.
//!
//! The PDF reader belongs to a lower layer (io); the host installs it as [`PdfHooks`], so the
//! engine stays I/O-agnostic and builds without it report "not available".

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use cadcraft_doc::{Drawing, EntityKind, Handle, PdfUnderlay, UNDERLAY_MAX_FADE};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, point_value, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// The PDF reader, installed by the host.
pub struct PdfHooks {
    /// The number of pages, the size of page `page` (from 1) in millimetres and the units per
    /// point its measuring scale gives, if it has one (see [`PageSize`]).
    pub page: fn(&[u8], u32) -> std::result::Result<(usize, f64, f64, Option<f64>), String>,
    /// Page `page` drawn on white with its longest side `max_side` pixels: width, height and
    /// straight RGBA pixels, rows top to bottom.
    pub render: fn(&[u8], u32, u32) -> std::result::Result<(u32, u32, Vec<u8>), String>,
    /// The part `[left, bottom, right, top]` (page fractions) of page `page` drawn on white at
    /// `width` × `height` pixels: width, height and straight RGBA pixels.
    pub region: fn(&[u8], u32, [f64; 4], u32, u32) -> std::result::Result<(u32, u32, Vec<u8>), String>,
    /// The PDF's layers, as (name, shown by default).
    pub layers: fn(&[u8]) -> std::result::Result<Vec<(String, bool)>, String>,
    /// The PDF with the named layers shown or hidden (the others as by default).
    pub with_layers: fn(&[u8], &[(String, bool)]) -> std::result::Result<Vec<u8>, String>,
}

/// The bytes of the PDF `stored` (in the drawing saved at `document`) with `layers` shown or
/// hidden.
fn underlay_bytes(hooks: &PdfHooks, stored: &str, document: Option<&str>, layers: &[(String, bool)]) -> Result<Vec<u8>> {
    let path = underlay_file(stored, document)?;
    let bytes = read_pdf(&path.to_string_lossy())?;
    if layers.is_empty() {
        return Ok(bytes);
    }
    (hooks.with_layers)(&bytes, layers).map_err(|e| bad("pdfattach", format!("{stored}: {e}")))
}

/// Part `region` (`[left, bottom, right, top]` in page fractions) of page `page` of the PDF
/// `stored` in the drawing saved at `document`, drawn at `width` × `height` pixels: what the
/// canvas shows when zoomed in past the whole page's pixels.
pub fn underlay_region_pixels(
    stored: &str,
    document: Option<&str>,
    page: u32,
    region: [f64; 4],
    width: u32,
    height: u32,
    layers: &[(String, bool)],
) -> Result<(u32, u32, Vec<u8>)> {
    let hooks = pdf().ok_or_else(|| bad("pdfattach", "PDF underlays are not available in this build"))?;
    let bytes = underlay_bytes(hooks, stored, document, layers)?;
    (hooks.region)(&bytes, page, region, width, height).map_err(|e| bad("pdfattach", format!("{stored}: {e}")))
}

static PDF: OnceLock<PdfHooks> = OnceLock::new();

pub fn set_pdf(h: PdfHooks) {
    let _ = PDF.set(h);
}
pub fn pdf() -> Option<&'static PdfHooks> {
    PDF.get()
}

/// The largest PDF file PDFATTACH reads.
const MAX_PDF_BYTES: u64 = 512 * 1024 * 1024;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("pdfattach", "PDF Underlay...", run_pdfattach)
            .menu(&["Insert", "PDF Underlay..."])
            .params("{path: PDF file, page?: number from 1 (default 1), at?: [x,y] lower-left corner (default 0,0), scale?: 1 = the page at its size on paper (default; 1/50 = a fiftieth of it), rotation? (degrees), fade?: 0..80 (default 0), contrast?: 0..100 (default 100), monochrome?: bool (default false), adjustColors?: bool (default true)} → {handle, page, pages, width, height} (typed: path, page, insertion point, scale, rotation)")
            .interactive(|s| Ok(Box::new(AttachM::new(s)))),
        CommandSpec::new("pdfadjust", "Adjust PDF Underlay", run_pdfadjust)
            .menu(&["Modify", "Object", "PDF Underlay", "Adjust"])
            .params("{handles?: PDF underlays (default the selection), fade?: 0..80, contrast?: 0..100, monochrome?: bool, adjustColors?: bool (on a dark background, white paper shows dark and dark lines light)} → {adjusted}")
            .interactive(|_| Ok(Box::new(AdjustM::default()))),
    ]
}

/// Most pages one PDFATTACH attaches.
const MAX_ATTACH_PAGES: usize = 100;

fn run_pdfattach(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "pdfattach";
    let hooks = pdf().ok_or_else(|| bad(ID, "PDF underlays are not available in this build"))?;
    let path = str_param(p, "path").filter(|s| !s.is_empty()).ok_or_else(|| bad(ID, "`path` (the PDF file) is required"))?;
    let page_no = |v: &Value| v.as_u64().filter(|n| *n >= 1).and_then(|n| u32::try_from(n).ok());
    let wanted: Vec<u32> = match (p.get("pages"), p.get("page")) {
        (Some(Value::Array(a)), _) if !a.is_empty() && a.len() <= MAX_ATTACH_PAGES => {
            a.iter().map(|v| page_no(v).ok_or_else(|| bad(ID, "`pages` must be page numbers from 1"))).collect::<Result<_>>()?
        }
        (Some(_), _) => return Err(bad(ID, format!("`pages` must be a list of 1 to {MAX_ATTACH_PAGES} page numbers"))),
        (None, Some(v)) => vec![page_no(v).ok_or_else(|| bad(ID, "`page` must be a page number from 1"))?],
        (None, None) => vec![1],
    };
    let at = match p.get("at") {
        Some(v) => point_value(v).ok_or_else(|| bad(ID, "`at` must be a point [x, y]"))?,
        None => Vec2::ZERO,
    };
    // Given as 1 = the page at its size on paper; stored as the file format's underlay scale.
    let scale = match p.get("scale") {
        Some(v) => v.as_f64().filter(|k| k.is_finite() && (1e-9..=1e9).contains(k)).ok_or_else(|| bad(ID, "`scale` must be a positive number"))?,
        None => 1.0,
    };
    // Strict like the other parameters: "90" or true is refused, not taken as 0.
    let rotation = match p.get("rotation") {
        None | Some(Value::Null) => 0.0,
        Some(v) => v.as_f64().filter(|r| r.is_finite()).ok_or_else(|| bad(ID, "`rotation` must be a number of degrees"))?,
    };
    let document = s.state().ok().and_then(|st| st.path.clone());
    let stored = stored_path(path, document.as_deref(), str_param(p, "pathType").unwrap_or("full")).map_err(|e| bad(ID, e))?;
    let adjust = adjust_params(ID, p)?;
    let bytes = read_pdf(path)?;
    // Several pages go side by side, left to right along the pages' bottom edge.
    let (sin, cos) = rotation.to_radians().sin_cos();
    let mut origin = at;
    let mut handles = Vec::new();
    let mut first: Option<(u32, usize, Vec2, f64, f64, f64)> = None;
    // Every page is read before any is attached: a page that doesn't exist attaches none.
    let mut sized = Vec::with_capacity(wanted.len());
    for page in wanted {
        let (pages, w_mm, h_mm, per_point) = (hooks.page)(&bytes, page).map_err(|e| bad(ID, format!("{path}: {e}")))?;
        let size = PageSize::new(w_mm, h_mm, per_point).ok_or_else(|| bad(ID, format!("{path}: page {page} has no usable size")))?;
        sized.push((page, pages, w_mm, h_mm, size));
    }
    // The file's scale needs only the drawing's units: read before any underlay is added.
    let d = s.doc()?;
    let sized: Vec<_> =
        sized.into_iter().map(|(page, pages, w_mm, h_mm, size)| (page, pages, w_mm, h_mm, size.native, size.file_scale(d, scale))).collect();
    for (page, pages, w_mm, h_mm, size, scale) in sized {
        let mut u = PdfUnderlay {
            insert: Vec3::new(origin.x, origin.y, 0.0),
            size,
            scale,
            rotation: rotation.to_radians(),
            path: stored.clone(),
            page,
            ..PdfUnderlay::default()
        };
        adjust.apply(&mut u);
        handles.push(s.add_entity(EntityKind::PdfUnderlay(u))?.hex());
        let step = size.x * scale * 1.05;
        origin += Vec2::new(cos * step, sin * step);
        first.get_or_insert((page, pages, size, scale, w_mm, h_mm));
    }
    let (page, pages, size, scale, w_mm, h_mm) = first.unwrap_or_default();
    let file = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let message = match handles.len() {
        1 => format!("Page {page} of {pages} of {file} attached ({w_mm:.0} × {h_mm:.0} mm)"),
        n => format!("{n} pages of {file} attached"),
    };
    Ok(json!({
        "handle": handles.first(),
        "handles": handles,
        "pages": pages,
        "width": size.x * scale,
        "height": size.y * scale,
        "path": stored,
        "message": message,
    }))
}

/// The path to store for the PDF `chosen` (a full path) in the drawing saved at `document`:
/// `full` as chosen, `relative` from the drawing's folder, `none` the file name alone (found
/// beside the drawing). The last two need a saved drawing.
pub fn stored_path(chosen: &str, document: Option<&str>, kind: &str) -> std::result::Result<String, String> {
    let file = std::path::Path::new(chosen);
    match kind.to_ascii_lowercase().as_str() {
        "full" => Ok(chosen.to_string()),
        "none" | "relative" if document.is_none() => Err("save the drawing first to store a relative path or the file name alone".into()),
        "none" => Ok(file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| chosen.to_string())),
        "relative" => {
            let dir = document.map(std::path::Path::new).and_then(std::path::Path::parent).unwrap_or(std::path::Path::new(""));
            let (a, b): (Vec<_>, Vec<_>) = (dir.components().collect(), file.components().collect());
            let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
            // On different drives (Windows) or roots there is no relative path: keep it full.
            if common == 0 {
                return Ok(chosen.to_string());
            }
            let mut rel = std::path::PathBuf::new();
            for _ in common..a.len() {
                rel.push("..");
            }
            for c in b.iter().skip(common) {
                rel.push(c.as_os_str());
            }
            Ok(rel.to_string_lossy().to_string())
        }
        other => Err(format!("`pathType` must be full, relative or none, got {other}")),
    }
}

/// What the PDF Underlay dialog chose: the values given, and `None` for those to specify on
/// screen.
#[derive(Clone, Debug)]
pub struct AttachPreset {
    pub path: String,
    pub pages: Vec<u32>,
    pub at: Option<Vec2>,
    /// 1 = the page at its size on paper.
    pub scale: Option<f64>,
    /// Degrees.
    pub rotation: Option<f64>,
    /// `full`, `relative` or `none`.
    pub path_type: String,
}

/// Attach what the dialog chose: at once when nothing is to be specified on screen, else
/// PDFATTACH asks for what is (insertion point, scale, rotation) with the rest filled in.
pub fn start_attach(s: &mut Session, preset: AttachPreset) -> Result<()> {
    if let (Some(at), Some(scale), Some(rotation)) = (preset.at, preset.scale, preset.rotation) {
        let p = json!({"path": preset.path, "pages": preset.pages, "at": [at.x, at.y], "scale": scale, "rotation": rotation, "pathType": preset.path_type});
        let r = s.execute("pdfattach", &p)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m);
        }
        return Ok(());
    }
    s.attach_preset = Some(preset);
    let r = s.start("pdfattach");
    // Only the PDFATTACH started here gets it.
    s.attach_preset = None;
    r
}

/// The number of pages of the PDF at `path` (a path the user chose).
pub fn pdf_pages(path: &str) -> Result<usize> {
    let hooks = pdf().ok_or_else(|| bad("pdfattach", "PDF underlays are not available in this build"))?;
    let bytes = read_pdf(path)?;
    (hooks.page)(&bytes, 1).map(|(n, ..)| n).map_err(|e| bad("pdfattach", format!("{path}: {e}")))
}

#[derive(Default)]
enum AttachStage {
    #[default]
    Path,
    Page,
    Point,
    Scale,
    Rotation,
}

/// PDFATTACH typed (or after the menu's file dialog): path, page, insertion point, scale and
/// rotation, as the command line form asks them.
#[derive(Default)]
struct AttachM {
    stage: AttachStage,
    path: String,
    pages: usize,
    /// The chosen page's size.
    size: PageSize,
    page: u32,
    at: Vec2,
    scale: f64,
    /// From the dialog: every page to attach, the rotation and how to store the path.
    chosen: Vec<u32>,
    scale_given: bool,
    rotation: Option<f64>,
    path_type: Option<String>,
}

impl AttachM {
    /// PDFATTACH, with what the dialog chose when it started it.
    fn new(s: &Session) -> AttachM {
        // Scale 1 (the page's size on paper) until one is given: the scale prompt's default, and
        // the frame shown while it is asked.
        let mut m = AttachM { scale: 1.0, ..AttachM::default() };
        let Some(p) = s.attach_preset.clone() else { return m };
        let first = p.pages.first().copied().unwrap_or(1);
        let Ok((pages, size)) = m.page_size(&p.path, first) else { return m };
        m.path = p.path;
        m.pages = pages;
        m.size = size;
        m.page = first;
        m.chosen = p.pages;
        m.at = p.at.unwrap_or_default();
        m.scale = p.scale.unwrap_or(1.0);
        m.scale_given = p.scale.is_some();
        m.rotation = p.rotation;
        m.path_type = Some(p.path_type);
        m.stage = if p.at.is_none() {
            AttachStage::Point
        } else if p.scale.is_none() {
            AttachStage::Scale
        } else {
            AttachStage::Rotation
        };
        m
    }
    /// The file is chosen: read its page count and first page's size.
    fn file(&mut self, s: &mut Session, path: &str) -> Result<Step> {
        let path = path.trim().trim_matches('"');
        match self.page_size(path, 1) {
            Ok((pages, size)) => {
                self.path = path.to_string();
                self.pages = pages;
                self.size = size;
                self.page = 1;
                self.stage = if pages > 1 { AttachStage::Page } else { AttachStage::Point };
            }
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Continue)
    }
    fn page_size(&self, path: &str, page: u32) -> Result<(usize, PageSize)> {
        let hooks = pdf().ok_or_else(|| bad("pdfattach", "PDF underlays are not available in this build"))?;
        let bytes = read_pdf(path)?;
        let (pages, w, h, per_point) = (hooks.page)(&bytes, page).map_err(|e| bad("pdfattach", format!("{path}: {e}")))?;
        let size = PageSize::new(w, h, per_point).ok_or_else(|| bad("pdfattach", format!("{path}: page {page} has no usable size")))?;
        Ok((pages, size))
    }
    fn number(i: &Input) -> Option<f64> {
        match i {
            Input::Text(t) | Input::Keyword(t) => t.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
            _ => None,
        }
    }
    fn finish(&mut self, s: &mut Session, rotation: f64) -> Result<Step> {
        let pages = if self.chosen.is_empty() { vec![self.page] } else { self.chosen.clone() };
        let path_type = self.path_type.clone().unwrap_or_else(|| "full".into());
        let p = json!({"path": self.path, "pages": pages, "at": [self.at.x, self.at.y], "scale": self.scale, "rotation": rotation, "pathType": path_type});
        match run_pdfattach(s, &p) {
            Ok(r) => {
                if let Some(m) = r.get("message").and_then(Value::as_str) {
                    s.echo(m);
                }
            }
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }
}

impl Interactive for AttachM {
    fn name(&self) -> &'static str {
        "PDFATTACH"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            AttachStage::Path => Prompt::new("Enter path to PDF file", Accept::TEXT),
            AttachStage::Page => Prompt::new(format!("Enter page number (1-{})", self.pages), Accept::NUMBER).default("1"),
            AttachStage::Point => Prompt::new("Specify insertion point", Accept::POINT).default("0,0"),
            AttachStage::Scale => Prompt::new("Specify scale factor (1 = size on paper)", Accept::NUMBER).default("1"),
            AttachStage::Rotation => Prompt::new("Specify rotation", Accept::NUMBER).default("0"),
        }
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        if self.path.is_empty() {
            return Vec::new();
        }
        // Placing the page: at the scale the dialog gave (else 1, its size on paper).
        let (at, k) = match self.stage {
            AttachStage::Path | AttachStage::Page | AttachStage::Point => (cursor, if self.scale_given { self.scale } else { 1.0 }),
            _ => (self.at, self.scale),
        };
        let Ok(d) = s.doc() else { return Vec::new() };
        let u = PdfUnderlay {
            insert: Vec3::new(at.x, at.y, 0.0),
            size: self.size.native,
            scale: self.size.file_scale(d, k),
            // The rotation the dialog gave, until it is specified on screen.
            rotation: self.rotation.filter(|r| r.is_finite()).unwrap_or(0.0).to_radians(),
            ..PdfUnderlay::default()
        };
        let c = u.corners();
        (0..4).filter_map(|i| Some(EntityKind::Line(cadcraft_doc::Line { a: c.get(i)?.to3(0.0), b: c.get((i + 1) % 4)?.to3(0.0) }))).collect()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            AttachStage::Path => match &i {
                Input::Text(t) | Input::Keyword(t) if !t.trim().is_empty() => self.file(s, t),
                Input::Enter | Input::Cancel => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            AttachStage::Page => {
                let page = match &i {
                    Input::Enter => Some(1),
                    _ => Self::number(&i).filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= self.pages as f64).map(|n| n as u32),
                };
                match page {
                    Some(p) => match self.page_size(&self.path.clone(), p) {
                        Ok((_, size)) => {
                            self.page = p;
                            self.size = size;
                            self.stage = AttachStage::Point;
                        }
                        Err(e) => s.echo(e.to_string()),
                    },
                    None => s.echo(format!("Requires a page number from 1 to {}.", self.pages)),
                }
                Ok(Step::Continue)
            }
            AttachStage::Point => {
                match &i {
                    Input::Point(p) => self.at = *p,
                    Input::Enter => self.at = Vec2::ZERO,
                    _ => return Ok(Step::Continue),
                }
                // Values the dialog gave are not asked again.
                match (self.scale_given, self.rotation) {
                    (true, Some(r)) => return self.finish(s, r),
                    (true, None) => self.stage = AttachStage::Rotation,
                    (false, _) => self.stage = AttachStage::Scale,
                }
                Ok(Step::Continue)
            }
            AttachStage::Scale => {
                let k = match &i {
                    Input::Enter => Some(1.0),
                    _ => Self::number(&i).filter(|k| (1e-9..=1e9).contains(k)),
                };
                match k {
                    Some(k) => {
                        self.scale = k;
                        if let Some(r) = self.rotation {
                            return self.finish(s, r);
                        }
                        self.stage = AttachStage::Rotation;
                    }
                    None => s.echo("Requires a positive number."),
                }
                Ok(Step::Continue)
            }
            AttachStage::Rotation => match &i {
                Input::Enter => self.finish(s, 0.0),
                _ => match Self::number(&i) {
                    Some(r) => self.finish(s, r),
                    None => {
                        s.echo("Requires a number of degrees.");
                        Ok(Step::Continue)
                    }
                },
            },
        }
    }
}

/// The display settings PDFADJUST (or PDFATTACH) changes; `None` keeps an underlay's own.
#[derive(Clone, Copy, Default)]
struct Adjust {
    fade: Option<u8>,
    contrast: Option<u8>,
    monochrome: Option<bool>,
    colors: Option<bool>,
}

impl Adjust {
    fn apply(&self, u: &mut PdfUnderlay) {
        if let Some(f) = self.fade {
            u.fade = f.min(UNDERLAY_MAX_FADE);
        }
        if let Some(c) = self.contrast {
            u.contrast = c.min(100);
        }
        if let Some(m) = self.monochrome {
            u.monochrome = m;
        }
        if let Some(a) = self.colors {
            u.adjust_colors = a;
        }
    }
}

/// `fade?`, `contrast?`, `monochrome?` and `adjustColors?`.
fn adjust_params(id: &str, p: &Value) -> Result<Adjust> {
    let percent = |key: &str, max: u8| -> Result<Option<u8>> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .and_then(|f| u8::try_from(f).ok())
                .filter(|f| *f <= max)
                .map(Some)
                .ok_or_else(|| bad(id, format!("`{key}` must be a whole number from 0 to {max}"))),
        }
    };
    let flag = |key: &str| -> Result<Option<bool>> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_bool().map(Some).ok_or_else(|| bad(id, format!("`{key}` must be true or false"))),
        }
    };
    Ok(Adjust {
        fade: percent("fade", UNDERLAY_MAX_FADE)?,
        contrast: percent("contrast", 100)?,
        monochrome: flag("monochrome")?,
        colors: flag("adjustColors")?,
    })
}

fn run_pdfadjust(s: &mut Session, p: &Value) -> Result<Value> {
    let adjust = adjust_params("pdfadjust", p)?;
    let hs = targets(s, p)?;
    let n = set_adjust(s, &hs, adjust)?;
    if n == 0 {
        return Err(bad("pdfadjust", "no PDF underlay to adjust (select one, or pass `handles`)"));
    }
    Ok(json!({ "adjusted": n }))
}

/// Change the display settings of the PDF underlays among `hs` (those on locked layers are left
/// alone). Returns how many were changed.
fn set_adjust(s: &mut Session, hs: &[Handle], adjust: Adjust) -> Result<usize> {
    let d = s.doc()?;
    let ids: Vec<Handle> = hs
        .iter()
        .copied()
        .filter(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::PdfUnderlay(_)) && !d.layer(&e.common.layer).is_some_and(|l| l.locked)))
        .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let d = s.doc_mut()?;
    for h in &ids {
        d.modify_entity(*h, |e| {
            if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                adjust.apply(u);
            }
        })?;
    }
    Ok(ids.len())
}

#[derive(Default)]
enum AdjustStage {
    #[default]
    Select,
    Option,
    Fade,
    Contrast,
    Monochrome,
    Colors,
}

/// PDFADJUST typed or from a menu: select the underlays, pick Fade, Contrast, Monochrome or
/// adjustColors, give the value.
#[derive(Default)]
struct AdjustM {
    stage: AdjustStage,
    sel: SelectPhase,
    objs: Vec<Handle>,
}

impl AdjustM {
    fn current(&self, s: &Session) -> Option<PdfUnderlay> {
        let d = s.doc().ok()?;
        self.objs.iter().find_map(|h| match &d.entity(*h)?.kind {
            EntityKind::PdfUnderlay(u) => Some(u.clone()),
            _ => None,
        })
    }
    fn selected(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        s.set_selection(Vec::new());
        self.objs = hs;
        if self.current(s).is_none() {
            s.echo("No PDF underlay selected.");
            return Ok(Step::Done);
        }
        self.stage = AdjustStage::Option;
        Ok(Step::Continue)
    }
    fn apply(&mut self, s: &mut Session, adjust: Adjust) -> Result<Step> {
        let objs = std::mem::take(&mut self.objs);
        set_adjust(s, &objs, adjust)?;
        Ok(Step::Done)
    }
}

impl Interactive for AdjustM {
    fn name(&self) -> &'static str {
        "PDFADJUST"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.selected(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let u = self.current(s).unwrap_or_default();
        match self.stage {
            AdjustStage::Select => self.sel.prompt(),
            AdjustStage::Option => {
                Prompt::new("Enter PDF underlay option", Accept::TEXT).kw(&["Fade", "Contrast", "Monochrome", "adjustColors"]).default("Fade")
            }
            AdjustStage::Fade => Prompt::new(format!("Enter fade value (0-{UNDERLAY_MAX_FADE})"), Accept::NUMBER).default(u.fade.to_string()),
            AdjustStage::Contrast => Prompt::new("Enter contrast value (0-100)", Accept::NUMBER).default(u.contrast.to_string()),
            AdjustStage::Monochrome => {
                Prompt::new("Show in shades of grey?", Accept::TEXT).kw(&["Yes", "No"]).default(if u.monochrome { "Yes" } else { "No" })
            }
            AdjustStage::Colors => {
                Prompt::new("Adjust colors for the background?", Accept::TEXT).kw(&["Yes", "No"]).default(if u.adjust_colors { "Yes" } else { "No" })
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
            _ => None,
        };
        match self.stage {
            AdjustStage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.selected(s, hs),
            },
            AdjustStage::Option => {
                match (&i, text.as_deref()) {
                    (Input::Enter, _) | (_, Some("fade" | "f")) => self.stage = AdjustStage::Fade,
                    (_, Some("contrast" | "c")) => self.stage = AdjustStage::Contrast,
                    (_, Some("monochrome" | "m")) => self.stage = AdjustStage::Monochrome,
                    (_, Some("adjustcolors" | "a" | "adjust")) => self.stage = AdjustStage::Colors,
                    _ => s.echo("Invalid option keyword."),
                }
                Ok(Step::Continue)
            }
            AdjustStage::Fade | AdjustStage::Contrast => {
                let max = if matches!(self.stage, AdjustStage::Fade) { UNDERLAY_MAX_FADE } else { 100 };
                match (&i, text.as_deref()) {
                    (Input::Enter, _) => Ok(Step::Done),
                    (_, Some(t)) => match t.parse::<f64>().ok().filter(|f| f.is_finite() && (0.0..=f64::from(max)).contains(f)) {
                        Some(f) => {
                            let v = Some(f.round() as u8);
                            let adjust = if matches!(self.stage, AdjustStage::Fade) {
                                Adjust { fade: v, ..Adjust::default() }
                            } else {
                                Adjust { contrast: v, ..Adjust::default() }
                            };
                            self.apply(s, adjust)
                        }
                        None => {
                            s.echo(format!("Requires a number from 0 to {max}."));
                            Ok(Step::Continue)
                        }
                    },
                    _ => Ok(Step::Continue),
                }
            }
            AdjustStage::Monochrome | AdjustStage::Colors => {
                let yes = match (&i, text.as_deref()) {
                    (Input::Enter, _) => return Ok(Step::Done),
                    (_, Some("yes" | "y")) => true,
                    (_, Some("no" | "n")) => false,
                    _ => {
                        s.echo("Invalid option keyword.");
                        return Ok(Step::Continue);
                    }
                };
                let adjust = if matches!(self.stage, AdjustStage::Monochrome) {
                    Adjust { monochrome: Some(yes), ..Adjust::default() }
                } else {
                    Adjust { colors: Some(yes), ..Adjust::default() }
                };
                self.apply(s, adjust)
            }
        }
    }
}

/// Turn over the lightness of straight RGBA pixels, keeping their hues: white paper becomes
/// black and black lines white, a red line stays red (an underlay's "adjust colors" on a dark
/// background).
pub fn turn_lightness(rgba: &mut [u8]) {
    for [r, g, b, _] in rgba.as_chunks_mut::<4>().0 {
        let (lo, hi) = ((*r).min(*g).min(*b), (*r).max(*g).max(*b));
        // HSL lightness L = (max + min) / 2 becomes 1 - L: shift every channel by 255 - max - min.
        let shift = 255 - i32::from(hi) - i32::from(lo);
        for c in [r, g, b] {
            *c = (i32::from(*c) + shift).clamp(0, 255) as u8;
        }
    }
}

/// Read a PDF file named in a drawing or a command (capped at [`MAX_PDF_BYTES`]).
pub fn read_pdf(path: &str) -> Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let f = std::fs::File::open(path).map_err(|e| bad("pdfattach", format!("{path}: {e}")))?;
        let mut bytes = Vec::new();
        f.take(MAX_PDF_BYTES.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| bad("pdfattach", format!("{path}: {e}")))?;
        if bytes.len() as u64 > MAX_PDF_BYTES {
            return Err(bad("pdfattach", format!("{path}: the file is larger than {} MB", MAX_PDF_BYTES / (1024 * 1024))));
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, MAX_PDF_BYTES);
        Err(bad("pdfattach", "files are not available on the web"))
    }
}

/// The file to read for the PDF path `stored` in the drawing saved at `document` (`None` for a
/// drawing without a file). A path stored in a drawing goes through [`crate::doc_path::resolve`]
/// (a relative path from the drawing's folder, other computers refused); when it names no file,
/// the same file name beside the drawing is tried.
pub fn underlay_file(stored: &str, document: Option<&str>) -> Result<std::path::PathBuf> {
    let host = document.map(std::path::Path::new);
    let refused = match crate::doc_path::resolve(stored, host) {
        Ok(p) if p.is_file() => return Ok(p),
        Ok(_) | Err(crate::doc_path::Refused::Invalid) => None,
        Err(r) => Some(r),
    };
    let name = stored.trim().rsplit(['/', '\\']).next().unwrap_or("");
    if let Some(dir) = host.and_then(|h| h.parent())
        && !name.is_empty()
    {
        let beside = dir.join(name);
        if beside.is_file() {
            return Ok(beside);
        }
    }
    Err(bad(
        "pdfattach",
        match refused {
            Some(r) => format!("{stored}: not opened ({r})"),
            None => format!("{stored}: file not found"),
        },
    ))
}

/// Page `page` of the PDF `stored` in the drawing saved at `document`, drawn with its longest
/// side `max_side` pixels, for showing an underlay: width, height and straight RGBA pixels.
pub fn underlay_pixels(stored: &str, document: Option<&str>, page: u32, max_side: u32, layers: &[(String, bool)]) -> Result<(u32, u32, Vec<u8>)> {
    let hooks = pdf().ok_or_else(|| bad("pdfattach", "PDF underlays are not available in this build"))?;
    let bytes = underlay_bytes(hooks, stored, document, layers)?;
    (hooks.render)(&bytes, page, max_side).map_err(|e| bad("pdfattach", format!("{stored}: {e}")))
}

/// Give the PDF underlays of drawing `i`, just opened, their page sizes (a file stores none):
/// read from each PDF, in the drawing's units. Underlays whose file can't be read keep no size
/// and are reported. The opened state counts as saved.
pub(crate) fn after_open(s: &mut Session, i: usize) {
    fill_sizes(s, i, true);
}

/// The pages (stored path, page) of drawing `d`'s PDF underlays that have no page size.
fn pages_without_size(d: &Drawing) -> Vec<(String, u32)> {
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    stores
        .flat_map(|s| s.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::PdfUnderlay(u) if u.size == Vec2::ZERO => Some((u.path.clone(), u.page)),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The pages (stored path, page) of the active drawing's PDF underlays that have no page size
/// (their PDF couldn't be read when it opened), and the drawing's file: what [`page_size`]
/// reads, off the UI thread, for [`set_sizes`].
pub fn unsized_pages(s: &Session) -> Unsized {
    s.state().map(|st| Unsized { drawing: st.uid, document: st.path.clone(), pages: pages_without_size(&st.doc) }).unwrap_or_default()
}

/// The PDF underlays of a drawing that have no page size ([`unsized_pages`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Unsized {
    /// The drawing (its `uid`), so sizes read for it go to it alone ([`set_sizes`]).
    pub drawing: u64,
    /// The file the drawing is saved at: relative PDF paths are read from its folder.
    pub document: Option<String>,
    /// Their stored PDF path and page.
    pub pages: Vec<(String, u32)>,
}

/// What reading a page's size gave ([`page_size`]).
#[derive(Clone, Debug, PartialEq)]
pub enum SizeRead {
    /// The size at underlay scale 1, in the units files store the scale against.
    Found(Vec2),
    /// The PDF can't be found or opened (not there yet, a share not reachable): worth trying again.
    Missing,
    /// The PDF is there but gives no size for the page (damaged, or without that page); its
    /// modification time, so it is read again only once it changes.
    Unreadable(Option<std::time::SystemTime>, String),
    /// Unreadable last time and not changed since: not read again.
    Unchanged,
}

/// Page `page`'s size of the PDF `stored` in the drawing saved at `document`. It reads files and
/// nothing else, so it can run on a worker thread. `unreadable_since`: the modification time of
/// the file when it was last found unreadable; while it has that time, it isn't read again.
pub fn page_size(stored: &str, document: Option<&str>, page: u32, unreadable_since: Option<std::time::SystemTime>) -> SizeRead {
    let Some(hooks) = pdf() else { return SizeRead::Missing };
    let Ok(file) = underlay_file(stored, document) else { return SizeRead::Missing };
    let meta = std::fs::metadata(&file).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok());
    if unreadable_since.is_some() && modified == unreadable_since {
        return SizeRead::Unchanged;
    }
    // Too large to read is no reason to try again: it stays so until the file changes.
    if meta.is_some_and(|m| m.len() > MAX_PDF_BYTES) {
        return SizeRead::Unreadable(modified, format!("{stored}: the file is larger than {} MB", MAX_PDF_BYTES / (1024 * 1024)));
    }
    let Ok(bytes) = read_pdf(&file.to_string_lossy()) else { return SizeRead::Missing };
    match (hooks.page)(&bytes, page) {
        Ok((_, w, h, per_point)) => match PageSize::new(w, h, per_point) {
            Some(size) => SizeRead::Found(size.native),
            None => SizeRead::Unreadable(modified, format!("{stored}: page {page} has no usable size")),
        },
        Err(e) => SizeRead::Unreadable(modified, format!("{stored}: {e}")),
    }
}

/// Give the PDF underlays that have no page size theirs from `sizes` (by stored path and page),
/// in the drawing they were read for ([`Unsized::drawing`]), open or not active, as long as it
/// is still saved at `document` (relative paths would name other PDFs otherwise). The drawing
/// is redrawn (a new revision) and stays unmodified if it was. Returns whether a size was set.
pub fn set_sizes(s: &mut Session, drawing: u64, document: Option<&str>, sizes: &HashMap<(String, u32), Vec2>) -> bool {
    s.docs.iter_mut().find(|st| st.uid == drawing && st.path.as_deref() == document).is_some_and(|st| set_sizes_in(st, sizes, false))
}

/// [`set_sizes`] on one drawing; `opened`: it was just opened, so its state counts as saved.
fn set_sizes_in(st: &mut crate::DocState, sizes: &HashMap<(String, u32), Vec2>, opened: bool) -> bool {
    let missing_size = |e: &cadcraft_doc::Entity| matches!(&e.kind, EntityKind::PdfUnderlay(u) if u.size == Vec2::ZERO && sizes.contains_key(&(u.path.clone(), u.page)));
    let d = &st.doc;
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    if !stores.flat_map(|s| s.iter()).any(|e| missing_size(e)) {
        return false;
    }
    // Reading a size is no change: an unmodified drawing stays so.
    let unmodified = !st.is_dirty();
    let d = Arc::make_mut(&mut st.doc);
    let fill = |store: &mut cadcraft_doc::EntityStore| {
        let hs: Vec<Handle> = store.iter().filter(|e| missing_size(e)).map(|e| e.handle).collect();
        for h in hs {
            store.modify(h, |e| {
                if let EntityKind::PdfUnderlay(u) = &mut e.kind
                    && let Some(sz) = sizes.get(&(u.path.clone(), u.page))
                {
                    u.size = *sz;
                }
            });
        }
    };
    fill(&mut d.model);
    d.layouts.iter_mut().for_each(|l| fill(&mut l.entities));
    for b in d.blocks.values_mut() {
        if b.entities.iter().any(|e| missing_size(e)) {
            fill(&mut Arc::make_mut(b).entities);
        }
    }
    if unmodified || opened {
        st.saved = st.doc.clone();
    }
    st.revision += 1;
    true
}

/// [`unsized_pages`], [`page_size`] and [`set_sizes`] in one go, on the calling thread: for
/// hosts without a UI thread to keep free (the canvas runs [`page_size`] on a worker). Returns
/// whether a size was set.
pub fn retry_sizes(s: &mut Session) -> bool {
    let todo = unsized_pages(s);
    let sizes: HashMap<(String, u32), Vec2> = todo
        .pages
        .into_iter()
        .filter_map(|(stored, page)| match page_size(&stored, todo.document.as_deref(), page, None) {
            SizeRead::Found(size) => Some(((stored, page), size)),
            _ => None,
        })
        .collect();
    set_sizes(s, todo.drawing, todo.document.as_deref(), &sizes)
}

/// The page sizes of drawing `i`'s PDF underlays that have none, read from their PDFs; the PDFs
/// that can't be read are reported when `report`. Returns whether a size was found.
fn fill_sizes(s: &mut Session, i: usize, report: bool) -> bool {
    let Some(st) = s.docs.get_mut(i) else { return false };
    let todo = pages_without_size(&st.doc);
    if todo.is_empty() {
        return false;
    }
    let mut sizes: HashMap<(String, u32), Vec2> = HashMap::new();
    let mut notes = Vec::new();
    for (stored, page) in todo {
        let size = underlay_file(&stored, st.path.as_deref()).and_then(|p| read_pdf(&p.to_string_lossy())).and_then(|b| {
            pdf()
                .ok_or_else(|| bad("pdfattach", "PDF underlays are not available in this build"))
                .and_then(|h| (h.page)(&b, page).map_err(|e| bad("pdfattach", format!("{stored}: {e}"))))
        });
        match size {
            Ok((_, w, h, per_point)) => match PageSize::new(w, h, per_point) {
                Some(size) => {
                    sizes.insert((stored, page), size.native);
                }
                None => notes.push(format!("PDF underlay {stored}: page {page} has no usable size")),
            },
            // The reason alone: the drawing is being opened, no command was typed.
            Err(crate::EngineError::BadParams { msg, .. }) => notes.push(format!("PDF underlay not shown: {msg}")),
            Err(e) => notes.push(format!("PDF underlay not shown: {e}")),
        }
    }
    let found = set_sizes_in(st, &sizes, report);
    if report {
        for n in notes {
            s.echo(n);
        }
    }
    found
}

/// A PDF page's size: on paper, in millimetres, and at underlay scale 1 (`native`), in the units
/// DXF and DWG files store the underlay scale against, whatever the drawing's units: a page is
/// its size in inches, or, when it has viewports (`/VP`), its size in points times the first
/// viewport's measuring scale (`/Measure` `/X` conversion factor, units per point), whatever
/// that viewport covers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PageSize {
    pub mm: Vec2,
    pub native: Vec2,
}

impl PageSize {
    /// The size of a `w_mm` × `h_mm` page whose measuring scale gives `per_point` units per point;
    /// `None` for an unusable size.
    pub fn new(w_mm: f64, h_mm: f64, per_point: Option<f64>) -> Option<PageSize> {
        let mm = Vec2::new(w_mm, h_mm);
        let native = match per_point.filter(|c| c.is_finite() && *c > 0.0) {
            Some(c) => mm / MM_PER_POINT * c,
            None => mm / 25.4,
        };
        let ok = |v: Vec2| v.x.is_finite() && v.y.is_finite() && v.x > 0.0 && v.y > 0.0;
        (ok(mm) && ok(native)).then_some(PageSize { mm, native })
    }

    /// The underlay scale a file stores for the page shown at `k` times its size on paper in
    /// drawing `d` (commands take `k`: 1 = size on paper).
    pub fn file_scale(&self, d: &Drawing, k: f64) -> f64 {
        k * self.mm.x / mm_per_unit(d) / self.native.x
    }
}

/// Millimetres per PDF point.
const MM_PER_POINT: f64 = 25.4 / 72.0;

/// Millimetres per drawing unit: the drawing's INSUNITS (the DXF reference's values; the survey
/// units are the US survey foot's, 1200/3937 m), or its MEASUREMENT when unitless or unknown.
pub fn mm_per_unit(d: &Drawing) -> f64 {
    const SURVEY_FOOT: f64 = 1_200_000.0 / 3937.0;
    match d.header.i64("INSUNITS", 0) {
        1 => 25.4,
        2 => 304.8,
        3 => 1_609_344.0,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        7 => 1e6,
        8 => 25.4e-6,
        9 => 0.0254,
        10 => 914.4,
        11 => 1e-7,
        12 => 1e-6,
        13 => 1e-3,
        14 => 100.0,
        15 => 1e4,
        16 => 1e5,
        17 => 1e12,
        // Astronomical unit, light year, parsec.
        18 => 1.495_978_707e14,
        19 => 9.460_730_472_580_8e18,
        20 => 3.085_677_581_491_367e19,
        21 => SURVEY_FOOT,
        22 => SURVEY_FOOT / 12.0,
        23 => SURVEY_FOOT * 3.0,
        24 => SURVEY_FOOT * 5280.0,
        _ if d.header.i64("MEASUREMENT", 0) == 1 => 1.0,
        _ => 25.4,
    }
}
