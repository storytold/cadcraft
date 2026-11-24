//! Convert the cached metafile in a Word OLE presentation to self-contained CAD solids.
//! The input is the OLE2FRAME 310 payload, never an executable OLE server.
//! MS-OLEDS OlePresentationStream and MS-WMF 2.3.6.25 describe the containers.
use std::io::{Cursor, Read};

use base64::Engine;
use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Solid, Space};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use craft_svg::tree::{self, usvg};
use emf_core::parser::{MapMode, ModifyWorldTransformMode};
use lyon_tessellation::geometry_builder::{FillGeometryBuilder, GeometryBuilder, GeometryBuilderError};
use lyon_tessellation::math::point;
use lyon_tessellation::{FillOptions, FillRule, FillTessellator, FillVertex, VertexId};

use crate::{IoError, Result};

const MAX_XML_NODES: u32 = 500_000;
const MAX_BYTES: usize = 32 << 20;
const MAX_RECORDS: usize = 100_000;
const MAX_SOLIDS: usize = 500_000;

fn bad(message: impl std::fmt::Display) -> IoError {
    IoError::Format(format!("OLE preview: {message}"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let end = at.checked_add(4).ok_or_else(|| bad("invalid offset"))?;
    let value: [u8; 4] = bytes.get(at..end).ok_or_else(|| bad("truncated record"))?.try_into().map_err(bad)?;
    Ok(u32::from_le_bytes(value))
}

fn i32_at(bytes: &[u8], at: usize) -> Result<i32> {
    Ok(u32_at(bytes, at)? as i32)
}

/// Decode a cached presentation and return normalized CAD geometry (unit square).
pub(super) fn read(bytes: &[u8]) -> Result<Drawing> {
    let emf = extract(bytes)?;
    let (gdi, images) = gdi_records(&emf)?;
    let svg = emf_core::converter::convert_to_svg(gdi.as_slice()).map_err(bad)?;
    if svg.len() > MAX_BYTES {
        return Err(bad("the converted preview is too large"));
    }
    let svg = normalize_svg(std::str::from_utf8(&svg).map_err(bad)?, &images)?;
    read_vector_svg(&svg, true)
}

/// Import a self-contained vector presentation as filled CAD geometry.
pub(super) fn read_vector_svg(svg: &str, normalized: bool) -> Result<Drawing> {
    if svg.len() > MAX_BYTES {
        return Err(bad("SVG exceeds limits"));
    }
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    // Metafile SVGs carry all their data. Never resolve paths from the embedded document.
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let limits =
        tree::Limits { bytes: MAX_BYTES, depth: 32, tree_depth: 32, refuse_entity_markup: true, xml_nodes: MAX_XML_NODES, ..tree::Limits::default() };
    let tree = tree::parse_text(svg, &limits, &options).map_err(bad)?;
    let mut drawing = Drawing::new_metric();
    let size = tree.size();
    let mm_per_pixel = 25.4 / 96.0;
    let (width, height, divisor) = if normalized {
        (1.0, 1.0, [size.width(), size.height()])
    } else {
        (f64::from(size.width() * mm_per_pixel), f64::from(size.height() * mm_per_pixel), [1.0 / mm_per_pixel; 2])
    };
    drawing
        .add(
            &Space::Model,
            Common { color: Color::True(Rgb(255, 255, 255)), ..Common::default() },
            EntityKind::Solid(Solid {
                corners: [Vec3::new(0.0, -height, 0.0), Vec3::new(width, -height, 0.0), Vec3::ZERO, Vec3::new(width, 0.0, 0.0)],
            }),
        )
        .map_err(bad)?;
    let start = drawing.model.len();
    let page_clip = (!normalized).then(|| Bounds2::new(Vec2::new(0.0, -height), Vec2::new(width, 0.0)));
    draw_group(tree.root(), &mut drawing, divisor, 0, page_clip)?;
    if normalized && drawing.model.len() == start {
        return Err(bad("the preview has no visible geometry"));
    }
    Ok(drawing)
}

fn extract(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_BYTES {
        return Err(bad("embedded data exceeds 32 MB"));
    }
    let magic = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    let offset = bytes.get(..bytes.len().min(4096)).and_then(|b| b.windows(8).position(|w| w == magic)).ok_or_else(|| bad("no compound document"))?;
    let mut file = cfb::CompoundFile::open(Cursor::new(bytes.get(offset..).ok_or_else(|| bad("invalid compound document"))?)).map_err(bad)?;
    let entry = file.walk().take(4096).find(|e| e.is_stream() && e.name() == "\u{2}OlePres000").ok_or_else(|| bad("no cached presentation"))?;
    if entry.len() > MAX_BYTES as u64 {
        return Err(bad("presentation exceeds 32 MB"));
    }
    let path = entry.path().to_owned();
    let mut presentation = Vec::new();
    file.open_stream(path).map_err(bad)?.take(MAX_BYTES as u64 + 1).read_to_end(&mut presentation).map_err(bad)?;
    if presentation.len() > MAX_BYTES {
        return Err(bad("presentation exceeds 32 MB"));
    }
    // ClipboardFormat must be a numeric ID and TargetDevice the four-byte empty device.
    if u32_at(&presentation, 0)? != u32::MAX || u32_at(&presentation, 8)? != 4 {
        return Err(bad("unsupported presentation header"));
    }
    let size = u32_at(&presentation, 36)? as usize;
    let payload = presentation.get(40..40usize.checked_add(size).ok_or_else(|| bad("invalid size"))?).ok_or_else(|| bad("truncated presentation"))?;
    if payload.get(40..44) == Some(b" EMF") {
        return Ok(payload.to_vec());
    }
    embedded_emf(payload)
}

fn embedded_emf(wmf: &[u8]) -> Result<Vec<u8>> {
    if wmf.get(2..4) != Some(&[9, 0]) {
        return Err(bad("unsupported cached metafile"));
    }
    let mut at = 18usize;
    let mut emf = Vec::new();
    let mut total = None;
    let mut expected_records = None;
    let mut chunks = 0usize;
    for _ in 0..MAX_RECORDS {
        let size = (u32_at(wmf, at)? as usize).checked_mul(2).ok_or_else(|| bad("record too large"))?;
        if size < 6 {
            return Err(bad("invalid WMF record"));
        }
        let end = at.checked_add(size).ok_or_else(|| bad("invalid record offset"))?;
        let record = wmf.get(at..end).ok_or_else(|| bad("truncated WMF record"))?;
        if record.get(4..10).is_some_and(|v| v.get(..4) == Some(&[0x26, 0x06, 0x0f, 0x00])) && record.get(10..14) == Some(b"WMFC") {
            let length = u32_at(record, 32)? as usize;
            let remaining = u32_at(record, 36)? as usize;
            let declared = u32_at(record, 40)? as usize;
            let count = u32_at(record, 28)? as usize;
            if declared > MAX_BYTES
                || length > 8192
                || count > MAX_RECORDS
                || total.is_some_and(|n| n != declared)
                || expected_records.is_some_and(|n| n != count)
                || emf.len().checked_add(length).and_then(|n| n.checked_add(remaining)) != Some(declared)
            {
                return Err(bad("inconsistent embedded EMF chunks"));
            }
            total = Some(declared);
            expected_records = Some(count);
            emf.extend_from_slice(record.get(44..44 + length).ok_or_else(|| bad("truncated EMF chunk"))?);
            chunks += 1;
            if remaining == 0 {
                if Some(chunks) != expected_records {
                    return Err(bad("missing EMF chunks"));
                }
                return Ok(emf);
            }
        }
        at = end;
    }
    Err(bad("no complete embedded EMF"))
}

#[derive(Clone)]
struct BitmapRect {
    id: String,
    size: [i32; 2],
}

/// The SVG player maps a bitmap's origin, but does not map its axes. Keep
/// enough GDI state to refuse a bitmap whenever its axes may have changed.
#[derive(Clone, Copy)]
struct BitmapMapping {
    world_translation_only: bool,
    mode: Option<MapMode>,
    window: [i32; 2],
    viewport: [i32; 2],
    scaled_extents: bool,
    layout: u32,
}

impl BitmapMapping {
    fn new(emf: &[u8]) -> Result<Self> {
        // The SVG player seeds both extents from the device-space header bounds.
        let width = i32_at(emf, 16)?.checked_sub(i32_at(emf, 8)?).ok_or_else(|| bad("invalid EMF width"))?;
        let height = i32_at(emf, 20)?.checked_sub(i32_at(emf, 12)?).ok_or_else(|| bad("invalid EMF height"))?;
        let extent = [width, height];
        Ok(Self { world_translation_only: true, mode: Some(MapMode::MM_TEXT), window: extent, viewport: extent, scaled_extents: false, layout: 0 })
    }

    fn uses_extents(self) -> bool {
        matches!(self.mode, Some(MapMode::MM_ISOTROPIC | MapMode::MM_ANISOTROPIC))
    }

    fn translation_only(self) -> bool {
        (self.mode == Some(MapMode::MM_TEXT) || self.uses_extents())
            && self.world_translation_only
            && !self.scaled_extents
            && self.layout == 0
            && self.window == self.viewport
            && !self.window.contains(&0)
    }

    fn world_transform(&mut self, record: &[u8], mode: ModifyWorldTransformMode) -> Result<()> {
        if mode == ModifyWorldTransformMode::MWT_IDENTITY {
            // The pinned SVG player leaves its transform unchanged in this mode.
            return Ok(());
        }
        let mut matrix = [0.0; 6];
        for (i, value) in matrix.iter_mut().enumerate() {
            *value = f32::from_bits(u32_at(record, 8 + i * 4)?);
        }
        let translation = matrix.starts_with(&[1.0, 0.0, 0.0, 1.0]) && matrix.iter().all(|v| v.is_finite());
        self.world_translation_only = translation && (mode == ModifyWorldTransformMode::MWT_SET || self.world_translation_only);
        Ok(())
    }
}

fn gdi_records(emf: &[u8]) -> Result<(Vec<u8>, Vec<BitmapRect>)> {
    use emf_core::parser::RecordType::*;

    if emf.len() > MAX_BYTES || emf.get(40..44) != Some(b" EMF") {
        return Err(bad("invalid EMF header"));
    }
    let mut out = Vec::new();
    let mut images = Vec::new();
    let mut at = 0usize;
    let mut count = 0u32;
    let mut mapping = BitmapMapping::new(emf)?;
    let mut saved_mappings = Vec::new();
    for _ in 0..MAX_RECORDS {
        let kind = u32_at(emf, at)?;
        let size = u32_at(emf, at + 4)? as usize;
        if size < 8 || !size.is_multiple_of(4) {
            return Err(bad("invalid EMF record size"));
        }
        let end = at.checked_add(size).ok_or_else(|| bad("invalid EMF offset"))?;
        let record = emf.get(at..end).ok_or_else(|| bad("truncated EMF record"))?;
        // Word emits EMF+ Dual with a complete GDI fallback. Its comments are not needed
        // by this converter; unsupported EMF+ Only presentations are rejected below.
        if kind != EMR_COMMENT as u32 {
            match emf_core::parser::RecordType::from_repr(kind) {
                Some(
                    kind @ (EMR_EXCLUDECLIPRECT
                    | EMR_EXTSELECTCLIPRGN
                    | EMR_INTERSECTCLIPRECT
                    | EMR_OFFSETCLIPRGN
                    | EMR_SELECTCLIPPATH
                    | EMR_SETMETARGN),
                ) => return Err(bad(format!("unsupported EMF clipping record {kind:?}"))),
                // emf-core's SVG player accepts these records but emits no artwork.
                Some(
                    kind @ (EMR_STRETCHBLT
                    | EMR_MASKBLT
                    | EMR_PLGBLT
                    | EMR_SETDIBITSTODEVICE
                    | EMR_TRANSPARENTBLT
                    | EMR_ANGLEARC
                    | EMR_ARC
                    | EMR_ARCTO
                    | EMR_CHORD
                    | EMR_EXTFLOODFILL
                    | EMR_EXTTEXTOUTA
                    | EMR_FILLRGN
                    | EMR_FRAMERGN
                    | EMR_GRADIENTFILL
                    | EMR_INVERTRGN
                    | EMR_PAINTRGN
                    | EMR_PIE
                    | EMR_POLYDRAW
                    | EMR_POLYDRAW16
                    | EMR_POLYPOLYGON
                    | EMR_POLYTEXTOUTA
                    | EMR_POLYTEXTOUTW
                    | EMR_ROUNDRECT
                    | EMR_SETPIXELV
                    | EMR_SMALLTEXTOUT
                    | EMR_DRAWESCAPE
                    | EMR_EXTESCAPE
                    | EMR_NAMEDESCAPE
                    | EMR_GLSRECORD
                    | EMR_GLSBOUNDEDRECORD),
                ) => {
                    return Err(bad(format!("unsupported EMF paint record {kind:?}")));
                }
                Some(EMR_BITBLT) if !matches!(u32_at(record, 40)?, 0x0000_0042 | 0x00ff_0062 | 0x00f0_0021) => {
                    return Err(bad("unsupported EMR_BITBLT raster operation"));
                }
                Some(EMR_ALPHABLEND) => return Err(bad("unsupported EMR_ALPHABLEND compositing")),
                Some(EMR_SETLAYOUT) => mapping.layout = u32_at(record, 8)?,
                Some(EMR_SETMAPMODE) => mapping.mode = MapMode::from_repr(u32_at(record, 8)?),
                Some(EMR_SETWINDOWEXTEX) if mapping.uses_extents() => mapping.window = [i32_at(record, 8)?, i32_at(record, 12)?],
                Some(EMR_SETVIEWPORTEXTEX) if mapping.uses_extents() => mapping.viewport = [i32_at(record, 8)?, i32_at(record, 12)?],
                Some(EMR_SCALEWINDOWEXTEX | EMR_SCALEVIEWPORTEXTEX) => {
                    let (x_num, x_den, y_num, y_den) = (i32_at(record, 8)?, i32_at(record, 12)?, i32_at(record, 16)?, i32_at(record, 20)?);
                    if x_den == 0 || y_den == 0 {
                        return Err(bad("invalid EMF extent scale"));
                    }
                    mapping.scaled_extents |= (x_num, y_num) != (x_den, y_den);
                }
                Some(EMR_SETWORLDTRANSFORM) => mapping.world_transform(record, ModifyWorldTransformMode::MWT_SET)?,
                Some(EMR_MODIFYWORLDTRANSFORM) => {
                    let mode = ModifyWorldTransformMode::from_repr(u32_at(record, 32)?).ok_or_else(|| bad("invalid EMF transform mode"))?;
                    mapping.world_transform(record, mode)?;
                }
                Some(EMR_SAVEDC) => saved_mappings.push(mapping),
                Some(EMR_RESTOREDC) => {
                    let depth = i32_at(record, 8)?;
                    if depth < 0 {
                        let index =
                            saved_mappings.len().checked_sub(depth.unsigned_abs() as usize).ok_or_else(|| bad("missing saved EMF context"))?;
                        mapping = *saved_mappings.get(index).ok_or_else(|| bad("missing saved EMF context"))?;
                        saved_mappings.truncate(index);
                    }
                }
                Some(EMR_STRETCHDIBITS) => {
                    if !mapping.translation_only() {
                        return Err(bad("unsupported EMF bitmap coordinate transform"));
                    }
                    images.push(stretch_dibits(record, count)?);
                }
                _ => {}
            }
            out.extend_from_slice(record);
            count += 1;
        }
        at = end;
        if kind == EMR_EOF as u32 {
            break;
        }
    }
    if at != emf.len() || count < 3 {
        return Err(bad("incomplete or unsupported EMF"));
    }
    let length = u32::try_from(out.len()).map_err(bad)?;
    out.get_mut(48..52).ok_or_else(|| bad("short EMF header"))?.copy_from_slice(&length.to_le_bytes());
    out.get_mut(52..56).ok_or_else(|| bad("short EMF header"))?.copy_from_slice(&count.to_le_bytes());
    Ok((out, images))
}

fn stretch_dibits(record: &[u8], index: u32) -> Result<BitmapRect> {
    const SRCCOPY: u32 = 0x00cc_0020;
    if u32_at(record, 68)? != SRCCOPY {
        return Err(bad("unsupported EMR_STRETCHDIBITS raster operation"));
    }
    let header_offset = u32_at(record, 48)? as usize;
    let header_len = u32_at(record, 52)? as usize;
    let header_end = header_offset.checked_add(header_len).ok_or_else(|| bad("invalid bitmap header"))?;
    let header = record.get(header_offset..header_end).ok_or_else(|| bad("truncated bitmap header"))?;
    let width = i32_at(header, 4)?;
    let height = i32_at(header, 8)?.unsigned_abs();
    if i32_at(record, 32)? != 0 || i32_at(record, 36)? != 0 || i32_at(record, 40)? != width || u32::try_from(i32_at(record, 44)?).ok() != Some(height)
    {
        return Err(bad("cropped or reversed EMR_STRETCHDIBITS source rectangle"));
    }
    if width <= 0 || height == 0 || u64::from(width.unsigned_abs()) * u64::from(height) > 1_000_000 {
        return Err(bad("invalid or oversized EMR_STRETCHDIBITS bitmap"));
    }
    Ok(BitmapRect { id: format!("elem{index}"), size: [i32_at(record, 72)?, i32_at(record, 76)?] })
}

fn normalize_svg(svg: &str, images: &[BitmapRect]) -> Result<String> {
    normalize_svg_bounded(svg, images, MAX_BYTES)
}

pub(super) fn parse_svg_xml(svg: &str) -> Result<roxmltree::Document<'_>> {
    let options = roxmltree::ParsingOptions { nodes_limit: MAX_XML_NODES, ..Default::default() };
    roxmltree::Document::parse_with_options(svg, options).map_err(bad)
}

fn normalize_svg_bounded(svg: &str, images: &[BitmapRect], limit: usize) -> Result<String> {
    checked_svg_size(0, 0, svg.len(), limit)?;
    let xml = parse_svg_xml(svg)?;
    let root = xml.root_element();
    let view: Vec<f32> = root
        .attribute("viewBox")
        .ok_or_else(|| bad("no view box"))?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .map_err(bad)?;
    let [_, _, width, height] = view.as_slice() else {
        return Err(bad("invalid view box"));
    };
    if !width.is_finite() || !height.is_finite() || *width <= 0.0 || *height <= 0.0 {
        return Err(bad("empty view box"));
    }
    let mut edits = Vec::new();
    let mut replacement_bytes = 0usize;
    let mut placements: std::collections::HashMap<_, _> = images.iter().map(|i| (i.id.as_str(), i.size)).collect();
    for node in root.descendants().filter(|n| n.has_tag_name("image")) {
        let [w, h] = node.attribute("id").and_then(|id| placements.remove(id)).ok_or_else(|| bad("unsupported bitmap placement"))?;
        let coordinate = |name| node.attribute(name).ok_or_else(|| bad("missing mapped bitmap origin"))?.parse::<i32>().map_err(bad);
        let (x, y) = (coordinate("x")?, coordinate("y")?);
        let href = node.attribute("href").ok_or_else(|| bad("missing embedded bitmap"))?;
        let data = href.strip_prefix("data:image/bmp;base64,").ok_or_else(|| bad("unsupported embedded image"))?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(bad)?;
        let (width, height) = crate::raster::probe(&bytes)?;
        if u64::from(width) * u64::from(height) > 1_000_000 {
            return Err(bad("bitmap is too large for CAD conversion"));
        }
        let raster = crate::raster::decode(&bytes)?;
        let mut replacement = format!(
            "<g transform=\"translate({x} {y}) scale({} {})\">",
            f64::from(w) / f64::from(raster.width()),
            f64::from(h) / f64::from(raster.height())
        );
        let mut runs = 0usize;
        for row in 0..raster.height() {
            let mut start = 0;
            while start < raster.width() {
                let color = raster.pixel(start, row).ok_or_else(|| bad("invalid bitmap"))?;
                let mut end = start + 1;
                while end < raster.width() && raster.pixel(end, row) == Some(color) {
                    end += 1;
                }
                let [r, g, b, _] = color;
                replacement
                    .push_str(&format!("<rect x=\"{start}\" y=\"{row}\" width=\"{}\" height=\"1\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>", end - start));
                runs += 1;
                if runs > MAX_SOLIDS / 2 || replacement.len() > limit {
                    return Err(bad("bitmap conversion exceeds limits"));
                }
                start = end;
            }
        }
        replacement.push_str("</g>");
        replacement_bytes = checked_svg_size(replacement_bytes, 0, replacement.len(), limit)?;
        edits.push((node.range(), replacement));
    }
    if !placements.is_empty() {
        return Err(bad("EMF bitmap missing from converted preview"));
    }
    let mut result = apply_svg_edits(svg, edits, limit)?;
    let end = result.find('>').ok_or_else(|| bad("invalid SVG root"))?;
    let dimensions = format!(" width=\"{width}\" height=\"{height}\"");
    checked_svg_size(result.len(), 0, dimensions.len(), limit)?;
    result.insert_str(end, &dimensions);
    if result.len() > limit {
        return Err(bad("SVG exceeds limits"));
    }
    Ok(result)
}

/// Account for edits before retaining or assembling their expanded SVG.
fn checked_svg_size(current: usize, removed: usize, added: usize, limit: usize) -> Result<usize> {
    current.checked_sub(removed).and_then(|n| n.checked_add(added)).filter(|n| *n <= limit).ok_or_else(|| bad("SVG replacements exceed limits"))
}

pub(super) fn apply_svg_edits(svg: &str, mut edits: Vec<(std::ops::Range<usize>, String)>, limit: usize) -> Result<String> {
    edits.sort_unstable_by_key(|(range, _)| range.start);
    let mut length = checked_svg_size(0, 0, svg.len(), limit)?;
    let mut previous = 0;
    for (range, replacement) in &edits {
        if range.start < previous || svg.get(range.clone()).is_none() {
            return Err(bad("overlapping or invalid SVG edits"));
        }
        length = checked_svg_size(length, range.len(), replacement.len(), limit)?;
        previous = range.end;
    }
    let mut output = String::with_capacity(length);
    let mut previous = 0;
    for (range, replacement) in edits {
        output.push_str(svg.get(previous..range.start).ok_or_else(|| bad("invalid SVG edit"))?);
        output.push_str(&replacement);
        previous = range.end;
    }
    output.push_str(svg.get(previous..).ok_or_else(|| bad("invalid SVG edit"))?);
    Ok(output)
}

/// The page and its embedded images use axis-aligned rectangular PDF clips.
fn clip_bounds(group: &usvg::Group, size: [f32; 2]) -> Result<Option<Bounds2>> {
    let Some(clip) = group.clip_path() else { return Ok(None) };
    let mut root = clip.root();
    let path = loop {
        match root.children() {
            [usvg::Node::Path(path)] => break path,
            [usvg::Node::Group(child)] if child.clip_path().is_none() => root = child,
            _ => return Err(bad("unsupported clip shape")),
        }
    };
    if clip.clip_path().is_some() {
        return Err(bad("unsupported nested clip path"));
    }
    use usvg::tiny_skia_path::PathSegment;
    let mut segments = path.data().segments();
    let Some(PathSegment::MoveTo(start)) = segments.next() else { return Err(bad("empty clip path")) };
    let mut points = vec![start];
    for segment in segments.by_ref() {
        match segment {
            PathSegment::LineTo(point) if points.len() < 5 => points.push(point),
            PathSegment::Close => break,
            _ => return Err(bad("unsupported clip contour")),
        }
    }
    if segments.next().is_some() {
        return Err(bad("unsupported multiple clip contours"));
    }
    if points.len() == 5 && points.first() == points.last() {
        points.pop();
    }
    let transform = group.abs_transform().pre_concat(clip.transform()).pre_concat(path.abs_transform());
    transform.map_points(&mut points);
    let [a, b, c, d] = points.as_slice() else { return Err(bad("unsupported nonrectangular clip")) };
    let vertical_first = a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y;
    let horizontal_first = a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x;
    if !(vertical_first || horizontal_first) || a.x == c.x || a.y == c.y || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(bad("unsupported nonrectangular clip"));
    }
    Ok(Some(Bounds2::from_points(points.into_iter().map(|p| Vec2::new(f64::from(p.x / size[0]), f64::from(-p.y / size[1]))))))
}

fn draw_group(group: &usvg::Group, drawing: &mut Drawing, size: [f32; 2], depth: usize, inherited: Option<Bounds2>) -> Result<()> {
    if depth > 64 {
        return Err(bad("SVG nesting exceeds limits"));
    }
    if group.mask().is_some() || !group.filters().is_empty() || group.opacity().get() != 1.0 || group.blend_mode() != usvg::BlendMode::Normal {
        return Err(bad("unsupported SVG clipping or compositing"));
    }
    let clip = match (inherited, clip_bounds(group, size)?) {
        (Some(a), Some(b)) => {
            if !a.intersects(&b) {
                return Ok(());
            }
            Some(Bounds2::new(Vec2::new(a.min.x.max(b.min.x), a.min.y.max(b.min.y)), Vec2::new(a.max.x.min(b.max.x), a.max.y.min(b.max.y))))
        }
        (a, b) => a.or(b),
    };
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => draw_group(g, drawing, size, depth + 1, clip)?,
            usvg::Node::Text(t) => draw_group(t.flattened(), drawing, size, depth + 1, clip)?,
            usvg::Node::Path(p) if p.is_visible() => {
                if let Some(fill) = p.fill() {
                    if fill.opacity().get() != 1.0 {
                        return Err(bad("nonopaque path fills are not supported"));
                    }
                    let rule = match fill.rule() {
                        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                        usvg::FillRule::NonZero => FillRule::NonZero,
                    };
                    add_path(drawing, p.data(), p.abs_transform(), fill.paint(), rule, size, clip)?;
                }
                if let Some(stroke) = p.stroke() {
                    if stroke.opacity().get() != 1.0 {
                        return Err(bad("nonopaque path strokes are not supported"));
                    }
                    let pen = stroke.to_tiny_skia();
                    let dashed;
                    let centerline = if let Some(pattern) = &pen.dash {
                        let Some(path) = p.data().dash(pattern, 16.0) else {
                            // A short contour can fall entirely within a gap. tiny-skia also
                            // returns None for excessive dash counts. Use the control polygon
                            // length as an upper bound before treating the stroke as empty.
                            let length: f64 = p
                                .data()
                                .points()
                                .array_windows()
                                .map(|[a, b]| (f64::from(a.x) - f64::from(b.x)).hypot(f64::from(a.y) - f64::from(b.y)))
                                .sum();
                            let intervals = stroke.dasharray().unwrap_or_default();
                            let period: f64 = intervals.iter().copied().map(f64::from).sum();
                            if length * intervals.len() as f64 / period > MAX_SOLIDS as f64 {
                                return Err(bad("dash geometry exceeds limits"));
                            }
                            continue;
                        };
                        dashed = path;
                        &dashed
                    } else {
                        p.data()
                    };
                    // PDF can emit zero-length, butt-capped segments. The stroker
                    // returns no outline for these because they paint nothing.
                    if let Some(path) = centerline.stroke(&pen, 16.0).or_else(|| centerline.stroke(&pen, 1.0)) {
                        add_path(drawing, &path, p.abs_transform(), stroke.paint(), FillRule::NonZero, size, clip)?;
                    }
                }
            }
            usvg::Node::Path(_) => {}
            usvg::Node::Image(_) => return Err(bad("unconverted image")),
        }
    }
    Ok(())
}

/// Lyon reports vertex overflow through a Result, while triangle callbacks
/// return nothing. Stop retaining triangles at the limit and reject the mesh.
#[derive(Default)]
struct PageMesh {
    vertices: Vec<usvg::tiny_skia_path::Point>,
    triangles: Vec<[VertexId; 3]>,
    limit: usize,
    overflow: bool,
}

impl GeometryBuilder for PageMesh {
    fn add_triangle(&mut self, a: VertexId, b: VertexId, c: VertexId) {
        if self.triangles.len() < self.limit {
            self.triangles.push([a, b, c]);
        } else {
            self.overflow = true;
        }
    }
}

impl FillGeometryBuilder for PageMesh {
    fn add_fill_vertex(&mut self, vertex: FillVertex<'_>) -> std::result::Result<VertexId, GeometryBuilderError> {
        if self.overflow || self.vertices.len() >= self.limit * 3 {
            return Err(GeometryBuilderError::TooManyVertices);
        }
        let id = VertexId::from_usize(self.vertices.len());
        let p = vertex.position();
        self.vertices.push(usvg::tiny_skia_path::Point::from_xy(p.x, p.y));
        Ok(id)
    }
}

fn add_path(
    drawing: &mut Drawing,
    data: &usvg::tiny_skia_path::Path,
    transform: usvg::Transform,
    paint: &usvg::Paint,
    rule: FillRule,
    size: [f32; 2],
    clip: Option<Bounds2>,
) -> Result<()> {
    let usvg::Paint::Color(color) = paint else {
        return Err(bad("unsupported gradient or pattern"));
    };
    let mut path = lyon_tessellation::path::Path::builder();
    let mut open = false;
    for segment in data.segments() {
        use usvg::tiny_skia_path::PathSegment;
        match segment {
            PathSegment::MoveTo(p) => {
                if open {
                    path.end(false);
                }
                path.begin(point(p.x, p.y));
                open = true;
            }
            PathSegment::LineTo(p) => {
                path.line_to(point(p.x, p.y));
            }
            PathSegment::QuadTo(a, b) => {
                path.quadratic_bezier_to(point(a.x, a.y), point(b.x, b.y));
            }
            PathSegment::CubicTo(a, b, c) => {
                path.cubic_bezier_to(point(a.x, a.y), point(b.x, b.y), point(c.x, c.y));
            }
            PathSegment::Close => {
                path.end(true);
                open = false;
            }
        }
    }
    if open {
        path.end(false);
    }
    // PDF glyphs are often stored in a unit-sized path and enlarged by a transform.
    // Flattening by 0.1 in that path's coordinates can erase thin serifs and round strokes.
    let bounds = data.bounds();
    let stretch = transform.sx.hypot(transform.ky).hypot(transform.kx.hypot(transform.sy));
    let tolerance = (0.035 / stretch.max(1e-6)).min(bounds.width().max(bounds.height()) / 1024.0).clamp(1e-6, 0.1);
    let mut mesh = PageMesh { limit: MAX_SOLIDS, ..Default::default() };
    FillTessellator::new()
        .tessellate_path(&path.build(), &FillOptions::default().with_fill_rule(rule).with_tolerance(tolerance), &mut mesh)
        .map_err(bad)?;
    if mesh.overflow {
        return Err(bad("CAD geometry exceeds limits"));
    }
    for indices in &mesh.triangles {
        let mut triangle = [Vec3::ZERO; 3];
        for (out, index) in triangle.iter_mut().zip(indices) {
            let mut p = *mesh.vertices.get(index.to_usize()).ok_or_else(|| bad("invalid mesh"))?;
            transform.map_point(&mut p);
            *out = Vec3::new(f64::from(p.x / size[0]), f64::from(-p.y / size[1]), 0.0);
            if !out.x.is_finite() || !out.y.is_finite() {
                return Err(bad("invalid geometry"));
            }
        }
        let color = Rgb(color.red, color.green, color.blue);
        if let Some(clip) = clip {
            for points in cadcraft_render::clip::clip_triangles(&triangle.map(|p| p.xy()), &clip).as_chunks::<3>().0 {
                add_triangle(drawing, points.map(|p| Vec3::new(p.x, p.y, 0.0)), color)?;
            }
        } else {
            add_triangle(drawing, triangle, color)?;
        }
    }
    Ok(())
}

fn add_triangle(drawing: &mut Drawing, points: [Vec3; 3], rgb: Rgb) -> Result<()> {
    if drawing.entity_count() >= MAX_SOLIDS {
        return Err(bad("CAD geometry exceeds limits"));
    }
    let [a, b, c] = points;
    drawing
        .add(&Space::Model, Common { color: Color::True(rgb), ..Common::default() }, EntityKind::Solid(Solid { corners: [a, b, c, c] }))
        .map_err(bad)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use emf_core::parser::RecordType;

    fn emf_record(kind: RecordType, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        bytes[..4].copy_from_slice(&(kind as u32).to_le_bytes());
        bytes[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        bytes
    }

    fn emf_header() -> Vec<u8> {
        let mut header = emf_record(RecordType::EMR_HEADER, 88);
        for offset in [16, 20, 32, 36, 72, 76, 80, 84] {
            header[offset..offset + 4].copy_from_slice(&100i32.to_le_bytes());
        }
        header[40..44].copy_from_slice(b" EMF");
        header[44..48].copy_from_slice(&0x10000u32.to_le_bytes());
        header
    }

    #[test]
    fn gdi_preflight_rejects_paint_and_clipping_that_the_svg_converter_would_drop() {
        use emf_core::parser::RecordType::*;

        let mut header = emf_header();
        header.extend(emf_record(EMR_RECTANGLE, 24));
        let eof = emf_record(EMR_EOF, 20);
        assert!(gdi_records(&[header.as_slice(), &eof].concat()).is_ok());
        for kind in [EMR_STRETCHBLT, EMR_TRANSPARENTBLT, EMR_GRADIENTFILL, EMR_INTERSECTCLIPRECT, EMR_SELECTCLIPPATH] {
            let unsupported = emf_record(kind, 108);
            let emf = [header.as_slice(), &unsupported, &eof].concat();
            assert!(gdi_records(&emf).err().unwrap().to_string().contains(&format!("{kind:?}")));
        }
    }

    #[test]
    fn emf_bitmap_uses_the_mapped_origin_and_refuses_changed_axes() {
        use emf_core::parser::RecordType::*;

        let header = emf_header();
        let mut bitmap = emf_record(EMR_STRETCHDIBITS, 124);
        for (offset, value) in [
            (24, 5i32),
            (28, 6),
            (40, 1),
            (44, 1),
            (48, 80),
            (52, 40),
            (56, 120),
            (60, 4),
            (68, 0x00cc_0020),
            (72, 20),
            (76, 10),
            (80, 40),
            (84, 1),
            (88, 1),
            (92, (24 << 16) | 1),
            (100, 4),
        ] {
            bitmap[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bitmap[122] = 255;
        let mut eof = emf_record(EMR_EOF, 20);
        eof[16..20].copy_from_slice(&20u32.to_le_bytes());
        let mut transform = emf_record(EMR_SETWORLDTRANSFORM, 32);
        for (i, value) in [1.0f32, 0.0, 0.0, 1.0, 30.0, 10.0].into_iter().enumerate() {
            let start = 8 + i * 4;
            transform[start..start + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (records, expected) in [(&[][..], "translate(5 6)"), (transform.as_slice(), "translate(35 16)")] {
            let emf = [header.as_slice(), records, &bitmap, &eof].concat();
            let (gdi, images) = gdi_records(&emf).unwrap();
            let svg = emf_core::converter::convert_to_svg(gdi.as_slice()).unwrap();
            let rewritten = normalize_svg(std::str::from_utf8(&svg).unwrap(), &images).unwrap();
            assert!(rewritten.contains(expected), "{rewritten}");
        }
        transform[8..12].copy_from_slice(&2.0f32.to_le_bytes());
        let emf = [header.as_slice(), &transform, &bitmap, &eof].concat();
        assert!(gdi_records(&emf).err().unwrap().to_string().contains("bitmap coordinate transform"));

        let mut mapping_mode = emf_record(EMR_SETMAPMODE, 12);
        mapping_mode[8..12].copy_from_slice(&(MapMode::MM_ANISOTROPIC as u32).to_le_bytes());
        let mut viewport = emf_record(EMR_SETVIEWPORTEXTEX, 16);
        viewport[8..12].copy_from_slice(&1i32.to_le_bytes());
        viewport[12..16].copy_from_slice(&1i32.to_le_bytes());
        let emf = [header.as_slice(), &mapping_mode, &viewport, &bitmap, &eof].concat();
        assert!(gdi_records(&emf).err().unwrap().to_string().contains("bitmap coordinate transform"));
    }

    #[test]
    fn stretch_dibits_requires_a_full_source_copy() {
        let mut record = vec![0u8; 120];
        for (offset, value) in [(24, 5i32), (28, 6), (40, 2), (44, 1), (48, 80), (52, 40), (68, 0x00cc_0020), (72, 20), (76, 10), (84, 2), (88, 1)] {
            record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(stretch_dibits(&record, 0).unwrap().size, [20, 10]);
        record[32..36].copy_from_slice(&1i32.to_le_bytes());
        record[40..44].copy_from_slice(&1i32.to_le_bytes());
        assert!(stretch_dibits(&record, 0).err().unwrap().to_string().contains("source rectangle"));
    }

    fn chunk(data: &[u8], count: u32, remaining: u32, total: u32) -> Vec<u8> {
        let mut record = vec![0; 44];
        record[..4].copy_from_slice(&((44 + data.len()) as u32 / 2).to_le_bytes());
        record[4..8].copy_from_slice(&[0x26, 0x06, 0x0f, 0]);
        record[10..14].copy_from_slice(b"WMFC");
        record[28..32].copy_from_slice(&count.to_le_bytes());
        record[32..36].copy_from_slice(&(data.len() as u32).to_le_bytes());
        record[36..40].copy_from_slice(&remaining.to_le_bytes());
        record[40..44].copy_from_slice(&total.to_le_bytes());
        record.extend_from_slice(data);
        record
    }

    #[test]
    fn multiple_bitmap_expansions_share_one_svg_budget() {
        let pixels = image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([(x * 3) as u8, (y * 3) as u8, ((x + y) * 2) as u8]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(pixels).write_to(&mut bytes, image::ImageFormat::Bmp).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.into_inner());
        let image = |id: &str| format!(r#"<image id="{id}" x="35" y="6" href="data:image/bmp;base64,{encoded}"/>"#);
        let svg = |content: String| format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">{content}</svg>"#);
        let a = BitmapRect { id: "a".into(), size: [64, 64] };
        let b = BitmapRect { id: "b".into(), size: [64, 64] };
        let single = normalize_svg_bounded(&svg(image("a")), std::slice::from_ref(&a), 600_000).unwrap();
        assert!(single.contains("translate(35 6) scale(1 1)"));
        let limit = single.len() + 1000;
        assert!(normalize_svg_bounded(&svg(image("a") + &image("b")), &[a, b], limit).is_err());
    }

    #[test]
    fn blank_page_keeps_the_paper_but_an_empty_ole_preview_is_rejected() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"/>"#;
        let page = read_vector_svg(svg, false).unwrap();
        assert_eq!(page.model.len(), 1);
        assert!((page.extents(&Space::Model).width() - 25.4).abs() < 1e-5);
        assert!(read_vector_svg(svg, true).unwrap_err().to_string().contains("no visible geometry"));
    }

    #[test]
    fn rectangular_clip_needs_four_corners_and_nonzero_area() {
        let svg = |path| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <defs><clipPath id="clip"><path d="{path}"/></clipPath></defs>
            <g clip-path="url(#clip)"><rect width="10" height="10" fill="#ff0000"/></g></svg>"##
            )
        };
        let red = |drawing: &Drawing| drawing.model.iter().any(|e| e.common.color == Color::True(Rgb(255, 0, 0)));
        assert!(red(&read_vector_svg(&svg("M1 1H6V6H1Z"), false).unwrap()));
        for path in ["M1 1H6V6V1Z", "M1 1H6 M6 6H1Z"] {
            assert!(!read_vector_svg(&svg(path), false).is_ok_and(|drawing| red(&drawing)), "{path}");
        }
    }

    #[test]
    fn cumulative_bitmap_edits_are_bounded_before_assembly() {
        let first = checked_svg_size(0, 0, 6, 10).unwrap();
        assert!(checked_svg_size(first, 0, 6, 10).is_err());
        assert!(checked_svg_size(usize::MAX, 0, 1, usize::MAX).is_err());
        assert!(apply_svg_edits("ab", vec![(0..1, "xxxxxx".into()), (1..2, "yyyyyy".into())], 10).is_err());
        assert_eq!(apply_svg_edits("ab", vec![(0..1, "xx".into())], 10).unwrap(), "xxb");
    }

    #[test]
    fn nonopaque_fill_and_stroke_are_refused_instead_of_becoming_opaque() {
        for paint in [r#"fill="black" fill-opacity="0.5""#, r#"fill="none" stroke="black" stroke-opacity="0.5""#] {
            let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M1 1H9V9H1Z" {paint}/></svg>"#);
            assert!(read_vector_svg(&svg, false).is_err(), "{paint}");
        }
    }

    #[test]
    fn dashed_stroke_keeps_its_gaps_and_phase() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
            <path d="M2 5H18" fill="none" stroke="red" stroke-width="1" stroke-dasharray="2 2" stroke-dashoffset="1"/></svg>"#;
        let drawing = read_vector_svg(svg, true).unwrap();
        let spans: Vec<_> = drawing
            .model
            .iter()
            .filter(|e| e.common.color == Color::True(Rgb(255, 0, 0)))
            .map(|e| {
                let EntityKind::Solid(s) = &e.kind else { panic!("expected solid") };
                Bounds2::from_points(s.corners.map(|p| p.xy()))
            })
            .collect();
        for (x, painted) in [(2.5, true), (4.5, false), (5.5, true), (7.5, false), (10.5, true)] {
            assert_eq!(spans.iter().any(|b| b.min.x < x / 20.0 && x / 20.0 < b.max.x), painted, "at {x}");
        }
    }

    #[test]
    fn a_tessellation_cannot_retain_more_than_its_mesh_budget() {
        let mut path = lyon_tessellation::path::Path::builder();
        path.begin(point(0.0, 0.0));
        path.line_to(point(2.0, 0.0));
        path.line_to(point(2.0, 2.0));
        path.line_to(point(0.0, 2.0));
        path.close();
        let mut mesh = PageMesh { limit: 1, ..Default::default() };
        let result = FillTessellator::new().tessellate_path(&path.build(), &FillOptions::default(), &mut mesh);
        assert!(result.is_err() || mesh.overflow);
        assert!(mesh.vertices.len() <= 3 && mesh.triangles.len() <= 1);
    }

    #[test]
    fn fragmented_metafile_is_reassembled_and_missing_chunks_are_rejected() {
        let mut wmf = vec![0; 18];
        wmf[2..4].copy_from_slice(&[9, 0]);
        wmf.extend(chunk(b"abcd", 2, 4, 8));
        assert!(embedded_emf(&wmf).is_err());
        wmf.extend(chunk(b"efgh", 2, 0, 8));
        assert_eq!(embedded_emf(&wmf).unwrap(), b"abcdefgh");
        let last = wmf.len() - 12;
        wmf[last..last + 4].copy_from_slice(&1u32.to_le_bytes());
        assert!(embedded_emf(&wmf).is_err());
    }

    #[test]
    fn malformed_inputs_do_not_panic() {
        for n in 0..256 {
            let bytes = vec![0xff; n];
            assert!(extract(&bytes).is_err());
            assert!(gdi_records(&bytes).is_err());
        }
        assert!(u32_at(&[], usize::MAX).is_err());
    }

    #[test]
    fn native_geometry_preserves_filled_holes_and_colours() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
          <path fill="#f09000" fill-rule="evenodd" d="M0 0H100V100H0Z M25 25H75V75H25Z"/>
        </svg>"##;
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
        let mut drawing = Drawing::default();
        draw_group(tree.root(), &mut drawing, [100.0, 100.0], 0, None).unwrap();
        let mut area = 0.0;
        for e in drawing.model.iter() {
            assert_eq!(e.common.color, Color::True(Rgb(240, 144, 0)));
            let EntityKind::Solid(solid) = &e.kind else {
                panic!("expected CAD solid");
            };
            let [a, b, c, _] = solid.corners;
            area += ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0;
        }
        assert!((area - 0.75).abs() < 1e-6, "hole must remain empty: {area}");
        let bytes = crate::write(&drawing, "antet.dxf").unwrap();
        let reopened = crate::read(&bytes, "antet.dxf").unwrap();
        assert_eq!(reopened.model.len(), drawing.model.len());
        assert!(reopened.model.iter().all(|e| matches!(e.kind, EntityKind::Solid(_))));
    }

    #[test]
    fn enlarged_small_curves_keep_their_shape() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
          <path transform="scale(100)" d="M0.5 0 C0.776142 0 1 0.223858 1 0.5
          C1 0.776142 0.776142 1 0.5 1 C0.223858 1 0 0.776142 0 0.5 C0 0.223858 0.223858 0 0.5 0 Z"/>
        </svg>"#;
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
        let mut drawing = Drawing::default();
        draw_group(tree.root(), &mut drawing, [100.0, 100.0], 0, None).unwrap();
        let area: f64 = drawing
            .model
            .iter()
            .map(|entity| {
                let EntityKind::Solid(solid) = &entity.kind else {
                    panic!("expected CAD solid");
                };
                let [a, b, c, _] = solid.corners;
                ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0
            })
            .sum();
        assert!((area - std::f64::consts::FRAC_PI_4).abs() < 0.001, "{area}");
    }

    #[test]
    fn empty_stroke_does_not_discard_the_page() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
          <path d="M20 20L20 20" fill="none" stroke="black" stroke-width="1"/>
          <path d="M20 30h1" fill="none" stroke="black" stroke-dasharray="2 8" stroke-dashoffset="4"/>
          <path d="M10 10H90" fill="none" stroke="black" stroke-width="1"/>
        </svg>"#;
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
        let Some(usvg::Node::Path(empty)) = tree.root().children().first() else {
            panic!("expected the empty stroke");
        };
        let stroke = empty.stroke().unwrap().to_tiny_skia();
        assert!(empty.data().stroke(&stroke, 16.0).is_none());
        let mut drawing = Drawing::default();
        draw_group(tree.root(), &mut drawing, [100.0, 100.0], 0, None).unwrap();
        assert!(!drawing.model.is_empty(), "the visible line is still imported");
    }

    #[test]
    fn failed_compositing_is_explicit() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
        <g opacity="0.5"><rect width="10" height="10"/></g></svg>"#;
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
        assert!(draw_group(tree.root(), &mut Drawing::default(), [10.0, 10.0], 0, None).is_err());
    }
}
