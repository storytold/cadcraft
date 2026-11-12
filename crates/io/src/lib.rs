//! CADCraft file formats: DXF/DWG read, SVG and SVGZ import, SVG/PNG/PDF export (plotting).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// What a DXF file holds that CADCraft doesn't model, kept and written back (#528).
mod dxf_raw;

/// XCLIP boundaries as SPATIAL_FILTER objects.
mod dxf_clip;
mod dxf_ext;
mod dxf_image;
mod dxf_mleader;
mod dxf_read;
mod dxf_table;
mod dxf_write;
mod dxf_xref;
pub mod pdf;
pub mod plotter;
/// Raster image files (PNG, JPEG, BMP, TIFF) decoded to pixels.
pub mod raster;
/// Open → save → reopen comparison and the real-file corpus harness (gaps FF1, FF5).
pub mod roundtrip;
pub mod svg;
mod svg_read;

/// Multilines and their styles (MLINE, MLINESTYLE).
mod dxf_mline;

use cadcraft_doc::{Drawing, RawTag, Space};
use cadcraft_geom::Bounds2;
use std::path::{Path, PathBuf};

pub use cadcraft_doc::Format;
pub use craft_dxf::Version;
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

/// How a program runs the DWG reader and writer in a child process (`cadcraft_dwg::worker`).
pub mod dwg_worker {
    pub use cadcraft_dwg::{WORKER_ARG, serve_if_asked, use_own_worker};
    #[cfg(all(not(target_arch = "wasm32"), not(all(target_os = "freebsd", target_arch = "aarch64"))))]
    pub use cadcraft_dwg::{crash_worker_for_test, use_worker};
}

fn ext(name: impl AsRef<Path>) -> String {
    name.as_ref().extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Read a drawing; the format comes from the content (DXF ASCII/binary, DWG) or the name
/// (SVG and gzip-compressed SVGZ).
pub fn read(bytes: &[u8], name: impl AsRef<Path>) -> Result<Drawing> {
    read_source(bytes, name.as_ref()).map(|(d, _)| d)
}

/// [`read`], also returning the DXF rendition a DWG was read through (`None` for other files),
/// which the round-trip harness counts.
pub(crate) fn read_source(bytes: &[u8], name: &Path) -> Result<(Drawing, Option<Vec<u8>>)> {
    if cadcraft_dwg::is_dwg(bytes) {
        let (dxf, notes) = cadcraft_dwg::decode(bytes, cadcraft_dwg::Limits::DEFAULT).map_err(IoError::Format)?;
        let mut d = dxf_read::read(&dxf).map_err(|e| IoError::Format(format!("{e} (in the DXF converted from this DWG)")))?;
        d.read_notes = notes;
        attach_tianzheng(&mut d, bytes);
        return Ok((d, Some(dxf)));
    }
    let d = match (Format::from_path(name), ext(name).as_str()) {
        (Some(Format::Svg | Format::Svgz), _) => svg_read::read(bytes),
        (Some(Format::Dxf), _) => dxf_read::read(bytes),
        (_, e) if bytes.len() > 4 => dxf_read::read(bytes).map_err(|_| IoError::Unsupported(e.to_string())),
        (_, e) => Err(IoError::Unsupported(e.to_string())),
    }?;
    Ok((d, None))
}

/// 小写 hex 编码（DXF 310/319 的 binary group 值）。
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 小写 hex 解码（DXF 310/319 的 binary group 值）。
fn hex_decode(s: &str) -> Vec<u8> {
    let s = s.trim();
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap_or(0))
        .collect()
}

/// 把修改后的 text 流写回 Tianzheng 的 tags（310 data 的 text_payload 部分），供保存 DWG 用。
///
/// proxy DXF 的 data 310 = payload（对象数据，前 `payload_bits.div_ceil(8)` 字节）+ text_payload
/// （字符串流，其余）。字段名和字符串值都在 text 流里，数值字段在 payload（main 流）里；因此
/// 改字符串字段只需替换 data 310 的 text_payload 部分，payload 和 graphics 保持不变。
/// `new_text` 的末尾按 `new_text_bits` 清零 padding（`ProxyPayload::data` 的做法）。
fn sync_tianzheng_text(tags: &mut Vec<RawTag>, new_text_bits: i64, new_text: &[u8]) -> bool {
    // payload 的 bit 数（group 161，R2004 为 93）。
    let mut payload_bits = 0u32;
    for tag in tags.iter() {
        if tag.code == 161 || tag.code == 93 {
            payload_bits = tag.value.trim().parse().unwrap_or(0);
            break;
        }
    }
    if payload_bits == 0 {
        return false;
    }
    let payload_bytes = (payload_bits as usize).div_ceil(8);

    // 收集 data 310 的前 payload_bytes 字节（payload 部分，保持不变）。
    let mut in_data = false;
    let mut payload: Vec<u8> = Vec::new();
    for tag in tags.iter() {
        if tag.code == 161 || tag.code == 93 {
            in_data = true;
            continue;
        }
        if in_data && (tag.code == 310 || tag.code == 319) {
            payload.extend_from_slice(&hex_decode(&tag.value));
            if payload.len() >= payload_bytes {
                break;
            }
        }
    }
    payload.truncate(payload_bytes);

    // 新 text 流：末尾按精确 bit 数清零 padding。
    let mut text = new_text.to_vec();
    let rem = (new_text_bits.max(0) as usize) % 8;
    if rem != 0 {
        if let Some(last) = text.last_mut() {
            *last &= 0xFF << (8 - rem);
        }
    }

    let mut data = payload;
    data.extend_from_slice(&text);
    let chunks: Vec<RawTag> = data.chunks(127).map(|c| RawTag { code: 310, value: hex_encode(c) }).collect();

    // 重建 tags：更新 text_bits（162 / 96），替换 data 310 段为新的 chunk。
    let mut out: Vec<RawTag> = Vec::with_capacity(tags.len() + chunks.len());
    let mut i = 0;
    while i < tags.len() {
        let code = tags[i].code;
        if code == 162 || code == 96 {
            out.push(RawTag { code, value: new_text_bits.to_string() });
            i += 1;
            continue;
        }
        if code == 161 || code == 93 {
            out.push(tags[i].clone());
            i += 1;
            while i < tags.len() && (tags[i].code == 310 || tags[i].code == 319) {
                i += 1;
            }
            out.extend(chunks.iter().cloned());
            continue;
        }
        out.push(tags[i].clone());
        i += 1;
    }
    *tags = out;
    true
}

/// 修改天正实体的一个字段值，并把改动写回 `raw_data` 和 tags（供保存 DWG 用）。
///
/// 从 `raw_data` 重新提取 text 流 → 解析字符串列表 → 改值 → 重新编码 → 替换回 `raw_data`，
/// 同时把新 text 流同步回 `tags` 的 310（否则保存走 tags 原始值，改动不落盘）。
/// 返回是否成功（实体非天正、无 raw_data、或字段不存在时返回 false）。
pub fn set_tianzheng_field(d: &mut Drawing, handle: cadcraft_doc::Handle, field_name: &str, new_value: &str) -> bool {
    use cadcraft_doc::EntityKind;
    let t = match d.model.get(handle).map(|e| &e.kind) {
        Some(EntityKind::Tianzheng(t)) => t.clone(),
        _ => return false,
    };
    let raw = match &t.raw_data {
        Some(r) => r.clone(),
        None => return false,
    };
    let text = match cadcraft_dwg::tianzheng::extract_text_stream(&raw, t.handle_bits) {
        Some(t) => t,
        None => return false,
    };
    let strings = cadcraft_dwg::tianzheng::parse_text_strings(&text);
    // 找字段名，改其后的字符串值
    let mut new_strings = strings;
    let mut changed = false;
    for i in 0..new_strings.len() {
        if new_strings[i] == field_name
            && i + 1 < new_strings.len()
            && !new_strings[i + 1].starts_with("T20")
            && !new_strings[i + 1].starts_with("TDBX")
        {
            new_strings[i + 1] = new_value.to_string();
            changed = true;
        }
    }
    if !changed {
        return false;
    }
    let (new_text, new_text_bits) = cadcraft_dwg::tianzheng::encode_text_strings_with_bits(&new_strings);
    let new_raw = match cadcraft_dwg::tianzheng::replace_text_stream(&raw, t.handle_bits, &new_text) {
        Some(r) => r,
        None => return false,
    };
    d.model.modify(handle, |entity| {
        if let EntityKind::Tianzheng(t) = &mut entity.kind {
            t.raw_data = Some(new_raw);
            sync_tianzheng_text(&mut t.tags, new_text_bits, &new_text);
            for (name, value) in &mut t.fields {
                if name == field_name {
                    *value = new_value.to_string();
                }
            }
        }
    });
    true
}

/// 打开 DWG 后，把天正自定义实体（`TCH_*`）从 `Unknown` 解析成 `Tianzheng` 实体（含字段名和值）。
fn attach_tianzheng(d: &mut Drawing, dwg_bytes: &[u8]) {
    let entities = match cadcraft_dwg::tianzheng::read_tianzheng(dwg_bytes) {
        Ok(e) => e,
        Err(_) => return,
    };
    for e in entities {
        let fields = cadcraft_dwg::tianzheng::extract_text_stream(&e.raw_data, e.handle_bits)
            .map(|text| cadcraft_dwg::tianzheng::parse_tianzheng_fields(&text))
            .unwrap_or_default();
        let handle = cadcraft_doc::Handle(e.handle);
        let class_name = e.dxf_class.clone();
        let raw_data = e.raw_data.clone();
        let handle_bits = e.handle_bits;
        d.model.modify(handle, |entity| {
            if let cadcraft_doc::EntityKind::Unknown(u) = &mut entity.kind {
                let tags = std::mem::take(&mut u.tags);
                let dxf_type = std::mem::take(&mut u.dxf_type);
                entity.kind = cadcraft_doc::EntityKind::Tianzheng(cadcraft_doc::Tianzheng {
                    class_name: class_name.clone(),
                    fields: fields.clone(),
                    raw_data: Some(raw_data.clone()),
                    handle_bits,
                    tags,
                    dxf_type,
                });
            }
        });
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
                (true, true) => craft_dxf::write_binary(&tags, true),
                (true, false) => craft_dxf::write_binary_in(&tags, &code_page),
                (false, true) => craft_dxf::write_ascii_as(&tags, true).into_bytes(),
                (false, false) => craft_dxf::write_ascii_in(&tags, &code_page),
            })
        }
        // The DWG bridge reads our R2000 DXF and writes the DWG at the version asked for. Without
        // one, at least 2004: R2000 DWG has no true colours, transparency, table styles or
        // gradients, so a drawing from an R2000 DXF doesn't lose them by default.
        Some(Format::Dwg) if !opts.binary => write_dwg(d, opts.version.unwrap_or_else(|| version.max(Version::R2004))).map(|(dwg, _)| dwg),
        _ if opts.binary => Err(IoError::Format(format!("binary output is for DXF files, not `{}`", ext(name)))),
        _ => write_framed(d, name, None),
    }
}

/// [`write_with`], also returning what the file leaves out of the drawing: for a DWG, how many
/// records of each DXF type the DWG writer can't write (other programs' custom entities and
/// objects); for other formats, nothing.
pub fn write_noted(d: &Drawing, name: impl AsRef<Path>, opts: &SaveOptions) -> Result<(Vec<u8>, cadcraft_dwg::Unwritten)> {
    let name = name.as_ref();
    if matches!(Format::from_path(name), Some(Format::Dwg)) && !opts.binary {
        return write_dwg(d, opts.version.unwrap_or_else(|| save_version(d).max(Version::R2004)));
    }
    write_with(d, name, opts).map(|b| (b, cadcraft_dwg::Unwritten::new()))
}

/// The DWG bridge reads our R2000 DXF and writes the DWG at `version`, in the DWG worker when
/// there is one. The objects CADCraft keeps without modelling them are still in the groups of the
/// file they came from (`Foreign::version`, not `$ACADVER`, which SAVEAS changes), and the bridge
/// reads them as that version.
fn write_dwg(d: &Drawing, version: Version) -> Result<(Vec<u8>, cadcraft_dwg::Unwritten)> {
    let bridge = dxf_write::dwg_bridge(d, version);
    cadcraft_dwg::encode_kept_noted(bridge.as_bytes(), version.code(), d.foreign.version.as_deref()).map_err(IoError::Format)
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
