//! Embed the first PDF page as CAD geometry. Word documents are rendered to PDF first.
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use cadcraft_doc::{Drawing, Raster};
use roxmltree::Node;

use crate::{IoError, Result, ole, raster};

const MAX_BYTES: usize = 32 << 20;

fn error(message: impl std::fmt::Display) -> IoError {
    IoError::Format(format!("Page import: {message}"))
}

pub(super) fn read(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(error("the DOCX exceeds 32 MB"));
    }
    let dir = tempfile::tempdir().map_err(error)?;
    let path = dir.path();
    std::fs::write(path.join("antet.docx"), bytes).map_err(error)?;
    let profile = url::Url::from_directory_path(path.join("profile")).map_err(|()| error("invalid temporary directory"))?;
    let mut office = converter("CADCRAFT_SOFFICE", "soffice");
    office
        .arg(format!("-env:UserInstallation={profile}"))
        .args(["--headless", "--norestore", "--convert-to", "pdf:writer_pdf_Export"])
        .arg("--outdir")
        .arg(path)
        .arg(path.join("antet.docx"));
    run(office, "LibreOffice (soffice)")?;
    let pdf = path.join("antet.pdf");
    if !pdf.is_file() {
        return Err(error("LibreOffice could not render the DOCX"));
    }
    read_pdf_page(&pdf, path)
}

pub(super) fn read_pdf(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(error("the PDF exceeds 32 MB"));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(error("invalid PDF header"));
    }
    let dir = tempfile::tempdir().map_err(error)?;
    let pdf = dir.path().join("page.pdf");
    std::fs::write(&pdf, bytes).map_err(error)?;
    read_pdf_page(&pdf, dir.path())
}

fn read_pdf_page(pdf: &Path, path: &Path) -> Result<Drawing> {
    let mut mupdf = converter("CADCRAFT_MUTOOL", "mutool");
    mupdf.args(["draw", "-F", "svg", "-o"]).arg(path.join("page-%d.svg")).arg(pdf).arg("1");
    run(mupdf, "MuPDF (mutool)")?;
    let svg = read_svg(&path.join("page-1.svg"))?;
    let svg = inline_bitmaps(&svg)?;
    ole::read_vector_svg(&svg, false)
}

fn converter(variable: &str, program: &str) -> Command {
    Command::new(std::env::var_os(variable).unwrap_or_else(|| program.into()))
}

fn run(mut command: Command, name: &str) -> Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| error(format!("{name} is required and could not be started ({e})")))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(error(format!("{name} failed ({status})"))),
            Ok(None) if start.elapsed() < Duration::from_secs(45) => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error(result.err().map_or_else(|| format!("{name} timed out"), |e| e.to_string())));
            }
        }
    }
}

fn read_svg(path: &Path) -> Result<String> {
    let mut svg = String::new();
    std::fs::File::open(path).map_err(error)?.take(MAX_BYTES as u64 + 1).read_to_string(&mut svg).map_err(error)?;
    if svg.len() > MAX_BYTES {
        return Err(error("the rendered page exceeds 32 MB"));
    }
    Ok(svg)
}

fn bitmap(node: Node<'_, '_>) -> Result<Raster> {
    let uri = node.attribute(("http://www.w3.org/1999/xlink", "href")).or_else(|| node.attribute("href"));
    let encoded = uri
        .and_then(|s| s.strip_prefix("data:image/png;base64,").or_else(|| s.strip_prefix("data:image/jpeg;base64,")))
        .ok_or_else(|| error("unsupported inline image"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.split_ascii_whitespace().collect::<String>()).map_err(error)?;
    let (width, height) = raster::probe(&bytes)?;
    if u64::from(width) * u64::from(height) > 1_000_000 {
        return Err(error("an inline image exceeds one megapixel"));
    }
    raster::decode(&bytes)
}

/// MuPDF writes image soft masks as a second inline bitmap. Composite the pixels
/// against the white paper and convert their runs to rectangles in image coordinates.
fn inline_bitmaps(svg: &str) -> Result<String> {
    let xml = roxmltree::Document::parse(svg).map_err(error)?;
    let root = xml.root_element();
    let mut edits = Vec::new();
    let mut replacement_bytes = 0usize;
    for image in root.descendants().filter(|n| n.has_tag_name("image") && !n.ancestors().any(|p| p.has_tag_name("defs"))) {
        let pixels = bitmap(image)?;
        let mask_group = image.ancestors().find(|n| n.has_attribute("mask"));
        let mask = mask_group.map(|group| image_mask(root, group, image, &pixels)).transpose()?;
        let replacement = bitmap_rectangles(&pixels, mask.as_ref())?;
        replacement_bytes = replacement_bytes.saturating_add(replacement.len());
        if replacement_bytes > MAX_BYTES {
            return Err(error("bitmap geometry exceeds limits"));
        }
        edits.push((image.range(), replacement));
        if let Some(attribute) = mask_group.and_then(|g| g.attribute_node("mask")) {
            edits.push((attribute.range(), String::new()));
        }
    }
    // MuPDF uses one SVG unit per PDF point. Preserve the physical page dimensions.
    for name in ["width", "height"] {
        let dimension = root.attribute_node(name).ok_or_else(|| error("missing page dimensions"))?;
        let points: f64 = dimension.value().parse().map_err(error)?;
        if !points.is_finite() || !(1.0..=14_400.0).contains(&points) {
            return Err(error("invalid page dimensions"));
        }
        edits.push((dimension.range(), format!("{name}=\"{}mm\"", points * 25.4 / 72.0)));
    }
    edits.sort_unstable_by_key(|(range, _)| range.start);
    if edits.windows(2).any(|pair| matches!(pair, [(left, _), (right, _)] if left.end > right.start)) {
        return Err(error("overlapping bitmap masks"));
    }
    let mut result = svg.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        result.replace_range(range, &replacement);
    }
    Ok(result)
}

fn image_mask(root: Node<'_, '_>, group: Node<'_, '_>, image: Node<'_, '_>, pixels: &Raster) -> Result<Raster> {
    let id = group
        .attribute("mask")
        .and_then(|s| s.strip_prefix("url(#"))
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| error("unsupported image mask"))?;
    let definition =
        root.descendants().find(|n| n.has_tag_name("mask") && n.attribute("id") == Some(id)).ok_or_else(|| error("missing image mask"))?;
    let mut images = definition.descendants().filter(|n| n.has_tag_name("image"));
    let source = images.next().ok_or_else(|| error("missing mask bitmap"))?;
    let transform = |n: Node<'_, '_>| n.parent_element().and_then(|p| p.attribute("transform")).map(str::to_owned);
    if images.next().is_some() || transform(source) != transform(image) {
        return Err(error("unsupported mask placement"));
    }
    let mask = bitmap(source)?;
    if (mask.width(), mask.height()) != (pixels.width(), pixels.height()) {
        return Err(error("image and mask sizes differ"));
    }
    Ok(mask)
}

fn bitmap_rectangles(pixels: &Raster, mask: Option<&Raster>) -> Result<String> {
    use std::fmt::Write;
    let mut svg = String::from("<g>");
    let composite = |x, y| {
        let [r, g, b, alpha] = pixels.pixel(x, y)?;
        let alpha = u32::from(alpha) * u32::from(mask.and_then(|m| m.pixel(x, y)).map_or(255, |p| p[0])) / 255;
        Some([r, g, b].map(|channel| (255 - (255 - u32::from(channel)) * alpha / 255) as u8))
    };
    for row in 0..pixels.height() {
        let mut start = 0;
        while start < pixels.width() {
            let color = composite(start, row).ok_or_else(|| error("incomplete bitmap"))?;
            let mut end = start + 1;
            while end < pixels.width() && composite(end, row) == Some(color) {
                end += 1;
            }
            let [r, g, b] = color;
            if color != [255; 3] {
                write!(svg, "<rect x=\"{start}\" y=\"{row}\" width=\"{}\" height=\"1\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>", end - start)
                    .map_err(error)?;
            }
            if svg.len() > MAX_BYTES {
                return Err(error("bitmap geometry exceeds limits"));
            }
            start = end;
        }
    }
    svg.push_str("</g>");
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_point_page_becomes_metric_cad_geometry() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="72" height="144" viewBox="0 0 72 144">
        <defs><clipPath id="paper"><path d="M0 0H72V144H0Z"/></clipPath></defs>
        <g clip-path="url(#paper)"><path d="M0 0H73V145H0Z"/></g></svg>"#;
        let svg = inline_bitmaps(svg).unwrap();
        let drawing = ole::read_vector_svg(&svg, false).unwrap();
        let extents = drawing.extents(&cadcraft_doc::Space::Model);
        assert!((extents.width() - 25.4).abs() < 0.0001);
        assert!((extents.height() - 50.8).abs() < 0.0001);
    }
}
