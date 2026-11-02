//! CADCraft file formats: DXF/DWG read, SVG and SVGZ import, SVG/PNG/PDF export (plotting).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// What a DXF file holds that CADCraft doesn't model, kept and written back (#528).
mod dxf_raw;

mod dxf_ext;
mod dxf_image;
mod dxf_mleader;
mod dxf_read;
mod dxf_table;
mod dxf_write;
mod dxf_xref;
pub mod pdf;
pub mod plotter;
pub mod svg;
mod svg_read;

/// Multilines and their styles (MLINE, MLINESTYLE).
mod dxf_mline;

use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::Bounds2;
use std::path::{Path, PathBuf};

pub use cadcraft_doc::Format;
pub use cadcraft_dxf::Version;
pub use dxf_read::entity_from_groups;
pub use dxf_write::entity_groups;
pub use pdf::{PdfOptions, pdf, plot};

/// How a drawing is saved as DXF or DWG.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// The file version; `None` saves in the drawing's own version ([`save_version`]).
    pub version: Option<Version>,
    /// Binary instead of ASCII DXF (DXF only; DWG is always binary).
    pub binary: bool,
}

/// The version a drawing is saved in when none is asked for: the version of the file it was
/// read from (`$ACADVER`, the nearest one CADCraft writes), and 2018 for a new drawing.
pub fn save_version(d: &Drawing) -> Version {
    d.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(Version::for_acadver).unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Format(String),
    #[error("unsupported file type `{0}`")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

fn ext(name: impl AsRef<Path>) -> String {
    name.as_ref().extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Read a drawing; the format comes from the content (DXF ASCII/binary, DWG) or the name
/// (SVG and gzip-compressed SVGZ).
pub fn read(bytes: &[u8], name: impl AsRef<Path>) -> Result<Drawing> {
    let name = name.as_ref();
    if cadcraft_dwg::is_dwg(bytes) {
        let (dxf, notes) = cadcraft_dwg::dwg_to_dxf_noted(bytes, cadcraft_dwg::Limits::DEFAULT).map_err(IoError::Format)?;
        let mut d = dxf_read::read(&dxf).map_err(|e| IoError::Format(format!("{e} (in the DXF converted from this DWG)")))?;
        d.read_notes = notes;
        return Ok(d);
    }
    match (Format::from_path(name), ext(name).as_str()) {
        (Some(Format::Svg | Format::Svgz), _) => svg_read::read(bytes),
        (Some(Format::Dxf), _) => dxf_read::read(bytes),
        (_, e) if bytes.len() > 4 => dxf_read::read(bytes).map_err(|_| IoError::Unsupported(e.to_string())),
        (_, e) => Err(IoError::Unsupported(e.to_string())),
    }
}

/// Write a drawing in the format chosen by the name's extension. Images (PNG, SVG, PDF) show
/// model space fitted to its extents. DXF and DWG are written in the drawing's version
/// ([`save_version`]), DXF as ASCII.
pub fn write(d: &Drawing, name: impl AsRef<Path>) -> Result<Vec<u8>> {
    write_framed(d, name, None)
}

/// Like [`write`], with the DXF/DWG version and binary DXF chosen by `opts`. Binary applies to
/// DXF only; asking for it with another format is an error.
pub fn write_with(d: &Drawing, name: impl AsRef<Path>, opts: &SaveOptions) -> Result<Vec<u8>> {
    let name = name.as_ref();
    let version = opts.version.unwrap_or_else(|| save_version(d));
    match Format::from_path(name) {
        Some(Format::Dxf) => {
            let tags = dxf_write::tags(d, version);
            // Before 2007 the text is in the code page the header declares.
            let code_page = dxf_write::code_page(d, version);
            Ok(match (opts.binary, version.utf8()) {
                (true, true) => cadcraft_dxf::write_binary(&tags, true),
                (true, false) => cadcraft_dxf::write_binary_in(&tags, &code_page),
                (false, true) => cadcraft_dxf::write_ascii_as(&tags, true).into_bytes(),
                (false, false) => cadcraft_dxf::write_ascii_in(&tags, &code_page),
            })
        }
        // The DWG bridge reads our R2000 DXF and writes the DWG at the version asked for. Without
        // one, at least 2004: R2000 DWG has no true colours, transparency, table styles or
        // gradients, so a drawing from an R2000 DXF doesn't lose them by default.
        Some(Format::Dwg) if !opts.binary => {
            let version = opts.version.unwrap_or_else(|| version.max(Version::R2004));
            cadcraft_dwg::dxf_to_dwg_as(dxf_write::dwg_bridge(d, version).as_bytes(), version.code()).map_err(IoError::Format)
        }
        _ if opts.binary => Err(IoError::Format(format!("binary output is for DXF files, not `{}`", ext(name)))),
        _ => write_framed(d, name, None),
    }
}

/// Like [`write`], but image formats (PNG, SVG, PDF) show `window` (a model-space rectangle in
/// drawing units, centred and scaled to fit the image without distortion) instead of the
/// extents. Formats that are not images (DXF, DWG) ignore the window. The window is checked with
/// [`check_window`].
pub fn write_framed(d: &Drawing, name: impl AsRef<Path>, window: Option<Bounds2>) -> Result<Vec<u8>> {
    let name = name.as_ref();
    // A drawing template (.dwt) is DWG content.
    let format = Format::from_path(name);
    if format == Some(Format::Dwt) {
        let mut dwg = name.as_os_str().to_owned();
        dwg.push(".dwg");
        return write_framed(d, PathBuf::from(dwg), None);
    }
    if let Some(w) = &window
        && is_image(name)
    {
        check_window(w)?;
    }
    match format {
        Some(Format::Dxf | Format::Dwg) => write_with(d, name, &SaveOptions::default()),
        Some(Format::Svg) => Ok(svg::export_window(d, &Space::Model, window).into_bytes()),
        Some(Format::Png) => png_framed(d, &Space::Model, 2400, 1600, window),
        Some(Format::Pdf) => pdf::pdf(d, &Space::Model, &PdfOptions { compress: true, window, ..PdfOptions::default() }),
        _ => Err(IoError::Unsupported(ext(name))),
    }
}

/// Whether `name` is written as an image (PNG, SVG, PDF), i.e. whether framing applies to it.
pub fn is_image(name: impl AsRef<Path>) -> bool {
    Format::from_path(name).is_some_and(Format::is_image)
}

/// Largest coordinate magnitude accepted in an export window.
pub const MAX_WINDOW_COORD: f64 = 1.0e12;

/// Validate an export window: finite corners within ±[`MAX_WINDOW_COORD`], and a width and height
/// that are positive and not vanishingly small next to the coordinates (at least 1e-9 of the
/// largest coordinate magnitude, and at least 1e-9 drawing units).
pub fn check_window(w: &Bounds2) -> Result<()> {
    let coords = [w.min.x, w.min.y, w.max.x, w.max.y];
    if coords.iter().any(|c| !c.is_finite()) {
        return Err(IoError::Format("export window: coordinates must be finite numbers".into()));
    }
    let mag = coords.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    if mag > MAX_WINDOW_COORD {
        return Err(IoError::Format(format!("export window: coordinates must lie within ±{MAX_WINDOW_COORD:e}")));
    }
    let min_side = mag.max(1.0) * 1e-9;
    let (width, height) = (w.max.x - w.min.x, w.max.y - w.min.y);
    if !(width >= min_side && height >= min_side) {
        return Err(IoError::Format(format!("export window: width and height must be positive (got {width} x {height})")));
    }
    Ok(())
}

/// Render a space to PNG, fitted to its extents.
pub fn png(d: &Drawing, space: &Space, width: u32, height: u32) -> Result<Vec<u8>> {
    png_framed(d, space, width, height, None)
}

/// Render a space to PNG showing `window` (centred, fitted without margin), or fitted to its
/// extents (5% margin) when `window` is `None`. Objects are drawn with their transparency, as on
/// screen.
pub fn png_framed(d: &Drawing, space: &Space, width: u32, height: u32, window: Option<Bounds2>) -> Result<Vec<u8>> {
    let list = cadcraft_render::build(d, space, &cadcraft_render::Options { transparency: true, ..cadcraft_render::Options::default() });
    let view = png_view(&list.bounds, width, height, window);
    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default())
        .ok_or_else(|| IoError::Format("render failed".into()))
}

/// The raster view a PNG export uses.
pub fn png_view(extents: &Bounds2, width: u32, height: u32, window: Option<Bounds2>) -> cadcraft_render::raster::View {
    match window {
        Some(w) => cadcraft_render::raster::View::fit(&w, width, height, 0.0),
        None => cadcraft_render::raster::View::fit(extents, width, height, 0.05),
    }
}

pub fn read_dxf(bytes: &[u8]) -> Result<Drawing> {
    dxf_read::read(bytes)
}
/// The drawing as R2000 ASCII DXF, whatever its own version ([`write_with`] chooses one).
pub fn write_dxf(d: &Drawing) -> String {
    dxf_write::write(d)
}

/// Regions, written as closed polylines.
mod dxf_region;

#[cfg(test)]
mod tests;
