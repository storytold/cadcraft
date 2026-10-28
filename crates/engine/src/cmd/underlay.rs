//! PDF underlays' pages: read from their PDF files for showing them, and sized when a drawing
//! is opened (a file stores no page size).
//!
//! The PDF reader belongs to a lower layer (io); the host installs it as [`PdfHooks`], so the
//! engine stays I/O-agnostic and builds without it report "not available".

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::Vec2;

use super::bad;
use crate::{Result, Session};

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

/// The largest PDF file read for an underlay.
const MAX_PDF_BYTES: u64 = 512 * 1024 * 1024;

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
    let Some(hooks) = pdf() else { return };
    let Some(st) = s.docs.get_mut(i) else { return };
    let missing_size = |e: &cadcraft_doc::Entity| matches!(&e.kind, EntityKind::PdfUnderlay(u) if u.size == Vec2::ZERO);
    let d = &st.doc;
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    let todo: Vec<(String, u32)> = stores
        .flat_map(|s| s.iter())
        .filter(|e| missing_size(e))
        .filter_map(|e| match &e.kind {
            EntityKind::PdfUnderlay(u) => Some((u.path.clone(), u.page)),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if todo.is_empty() {
        return;
    }
    let mut sizes: HashMap<(String, u32), Vec2> = HashMap::new();
    let mut notes = Vec::new();
    for (stored, page) in todo {
        let size = underlay_file(&stored, st.path.as_deref())
            .and_then(|p| read_pdf(&p.to_string_lossy()))
            .and_then(|b| (hooks.page)(&b, page).map_err(|e| bad("pdfattach", format!("{stored}: {e}"))));
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
    if !sizes.is_empty() {
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
        st.saved = st.doc.clone();
    }
    for n in notes {
        s.echo(n);
    }
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

/// Millimetres per drawing unit: the drawing's INSUNITS, or its MEASUREMENT when unitless.
pub fn mm_per_unit(d: &Drawing) -> f64 {
    match d.header.i64("INSUNITS", 0) {
        1 => 25.4,
        2 => 304.8,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        14 => 100.0,
        _ if d.header.i64("MEASUREMENT", 0) == 1 => 1.0,
        _ => 25.4,
    }
}
