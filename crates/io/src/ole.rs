//! Convert the cached metafile in a Word OLE presentation to self-contained CAD solids.
//! The input is the OLE2FRAME 310 payload, never an executable OLE server.
//! MS-OLEDS OlePresentationStream and MS-WMF 2.3.6.25 describe the containers.
use std::io::{Cursor, Read};

use base64::Engine;
use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Solid, Space};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use craft_svg::tree::{self, usvg};
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::{FillOptions, FillRule, FillTessellator, FillVertex};

use crate::{IoError, Result};

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
        tree::Limits { bytes: MAX_BYTES, depth: 32, tree_depth: 32, refuse_entity_markup: true, xml_nodes: 500_000, ..tree::Limits::default() };
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
    draw_group(tree.root(), &mut drawing, divisor, 0, None)?;
    if drawing.model.len() == start {
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
    rect: [i32; 4],
}

fn gdi_records(emf: &[u8]) -> Result<(Vec<u8>, Vec<BitmapRect>)> {
    if emf.len() > MAX_BYTES || emf.get(40..44) != Some(b" EMF") {
        return Err(bad("invalid EMF header"));
    }
    let mut out = Vec::new();
    let mut images = Vec::new();
    let mut at = 0usize;
    let mut count = 0u32;
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
        if kind != 70 {
            if kind == 81 {
                images.push(BitmapRect {
                    id: format!("elem{count}"),
                    rect: [i32_at(record, 24)?, i32_at(record, 28)?, i32_at(record, 72)?, i32_at(record, 76)?],
                });
            }
            out.extend_from_slice(record);
            count += 1;
        }
        at = end;
        if kind == 14 {
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

fn normalize_svg(svg: &str, images: &[BitmapRect]) -> Result<String> {
    normalize_svg_bounded(svg, images, MAX_BYTES)
}

fn normalize_svg_bounded(svg: &str, images: &[BitmapRect], limit: usize) -> Result<String> {
    checked_svg_size(0, 0, svg.len(), limit)?;
    let xml = roxmltree::Document::parse(svg).map_err(bad)?;
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
    for node in root.descendants().filter(|n| n.has_tag_name("image")) {
        let placement = images.iter().find(|i| Some(i.id.as_str()) == node.attribute("id")).ok_or_else(|| bad("unsupported bitmap placement"))?;
        let href = node.attribute("href").ok_or_else(|| bad("missing embedded bitmap"))?;
        let data = href.strip_prefix("data:image/bmp;base64,").ok_or_else(|| bad("unsupported embedded image"))?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(bad)?;
        let raster = crate::raster::decode(&bytes)?;
        if u64::from(raster.width()) * u64::from(raster.height()) > 1_000_000 {
            return Err(bad("bitmap is too large for CAD conversion"));
        }
        let [x, y, w, h] = placement.rect;
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
    let mut points = Vec::new();
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(point) | PathSegment::LineTo(point) => points.push(point),
            PathSegment::Close => {}
            _ => return Err(bad("unsupported curved clip")),
        }
    }
    if points.len() == 5 && points.first() == points.last() {
        points.pop();
    }
    let transform = group.abs_transform().pre_concat(clip.transform()).pre_concat(path.abs_transform());
    transform.map_points(&mut points);
    if points.len() != 4 || !points.iter().zip(points.iter().cycle().skip(1)).all(|(a, b)| a.x == b.x || a.y == b.y) {
        return Err(bad("unsupported nonrectangular clip"));
    }
    Ok(Some(Bounds2::from_points(points.into_iter().map(|p| Vec2::new(f64::from(p.x / size[0]), f64::from(-p.y / size[1]))))))
}

fn draw_group(group: &usvg::Group, drawing: &mut Drawing, size: [f32; 2], depth: usize, inherited: Option<Bounds2>) -> Result<()> {
    if depth > 64 {
        return Err(bad("SVG nesting exceeds limits"));
    }
    if group.mask().is_some() || !group.filters().is_empty() || group.opacity().get() != 1.0 {
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
                    // PDF can emit zero-length, butt-capped segments. The stroker
                    // returns no outline for these because they paint nothing.
                    if let Some(path) = p.data().stroke(&pen, 16.0).or_else(|| p.data().stroke(&pen, 1.0)) {
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
    let mut buffers: VertexBuffers<usvg::tiny_skia_path::Point, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path.build(),
            &FillOptions::default().with_fill_rule(rule).with_tolerance(tolerance),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| usvg::tiny_skia_path::Point::from_xy(v.position().x, v.position().y)),
        )
        .map_err(bad)?;
    for indices in buffers.indices.as_chunks::<3>().0 {
        let mut triangle = [Vec3::ZERO; 3];
        for (out, index) in triangle.iter_mut().zip(indices) {
            let mut p = *buffers.vertices.get(*index as usize).ok_or_else(|| bad("invalid mesh"))?;
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
        let image = |id: &str| format!(r#"<image id="{id}" href="data:image/bmp;base64,{encoded}"/>"#);
        let svg = |content: String| format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">{content}</svg>"#);
        let a = BitmapRect { id: "a".into(), rect: [0, 0, 64, 64] };
        let b = BitmapRect { id: "b".into(), rect: [0, 0, 64, 64] };
        let single = normalize_svg_bounded(&svg(image("a")), std::slice::from_ref(&a), 600_000).unwrap();
        let limit = single.len() + 1000;
        assert!(normalize_svg_bounded(&svg(image("a") + &image("b")), &[a, b], limit).is_err());
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
