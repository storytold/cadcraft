//! CADCraft file formats: DXF read/write, SVG, PNG and PDF export (plotting).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod dxf_ext;
mod dxf_read;
mod dxf_write;
pub mod pdf;
pub mod svg;

use cadcraft_doc::{Drawing, Space};

pub use pdf::{PdfOptions, pdf, plot};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Format(String),
    #[error("unsupported file type `{0}`")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

fn ext(name: &str) -> String {
    std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Read a drawing; the format comes from the content (DXF ASCII/binary) or the name.
pub fn read(bytes: &[u8], name: &str) -> Result<Drawing> {
    if cadcraft_dwg::is_dwg(bytes) {
        let dxf = cadcraft_dwg::dwg_to_dxf(bytes).map_err(IoError::Format)?;
        return dxf_read::read(&dxf).map_err(|e| IoError::Format(format!("{e} (in the DXF converted from this DWG)")));
    }
    match ext(name).as_str() {
        "dxf" | "" => dxf_read::read(bytes),
        e if bytes.len() > 4 => dxf_read::read(bytes).map_err(|_| IoError::Unsupported(e.to_string())),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// Write a drawing in the format chosen by the name's extension.
pub fn write(d: &Drawing, name: &str) -> Result<Vec<u8>> {
    match ext(name).as_str() {
        "dxf" | "" => Ok(dxf_write::write(d).into_bytes()),
        "dwg" => cadcraft_dwg::dxf_to_dwg(dxf_write::write(d).as_bytes()).map_err(IoError::Format),
        "svg" => Ok(svg::export(d, &Space::Model).into_bytes()),
        "png" => png(d, &Space::Model, 2400, 1600),
        "pdf" => pdf::pdf(d, &Space::Model, &PdfOptions { compress: true, ..PdfOptions::default() }),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// Render a space to PNG, fitted to its extents.
pub fn png(d: &Drawing, space: &Space, width: u32, height: u32) -> Result<Vec<u8>> {
    let list = cadcraft_render::build(d, space, &cadcraft_render::Options::default());
    let view = cadcraft_render::raster::View::fit(&list.bounds, width, height, 0.05);
    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default())
        .ok_or_else(|| IoError::Format("render failed".into()))
}

pub fn read_dxf(bytes: &[u8]) -> Result<Drawing> {
    dxf_read::read(bytes)
}
pub fn write_dxf(d: &Drawing) -> String {
    dxf_write::write(d)
}

#[cfg(test)]
mod tests;
