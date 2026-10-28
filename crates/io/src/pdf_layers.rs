//! A PDF's layers (optional content groups), and the same PDF with chosen layers turned on or
//! off, for PDF underlays' layer visibility. The page is drawn showing the layers the PDF's
//! default configuration turns on; to choose others, an incremental update (ISO 32000-1 §7.5.6)
//! is appended to a copy of the bytes, in memory: a new catalog whose default configuration
//! turns the chosen layers off. The file on disk is never changed. The bytes are hostile: every
//! failure is an error, never a panic.

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{Array, Dict, Name, ObjRef, Object, ObjectIdentifier};

use crate::{IoError, Result};

/// Most layers read from one PDF.
const MAX_LAYERS: usize = 4096;
/// Most object numbers tried for the new catalog past the first free-looking one.
const MAX_NUMBER_PROBES: u64 = 100_000;

/// One layer of a PDF: its name and whether the PDF shows it by default.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfLayer {
    pub name: String,
    pub on: bool,
}

/// A PDF's layers with the object each is, in the order the PDF lists them.
struct Layers {
    pages: ObjRef,
    groups: Vec<(ObjRef, PdfLayer)>,
}

fn open(bytes: &[u8]) -> Result<Pdf> {
    let data = bytes.to_vec();
    let pdf = std::panic::catch_unwind(move || Pdf::new(data)).map_err(|_| IoError::Format("the PDF could not be read".into()))?;
    pdf.map_err(|e| IoError::Format(format!("not a readable PDF file ({e:?})")))
}

/// A layer name: UTF-16 with a byte order mark, else PDFDocEncoding read as Latin-1.
fn text(b: &[u8]) -> String {
    match b {
        [0xFE, 0xFF, rest @ ..] => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
            String::from_utf16_lossy(&units)
        }
        _ => b.iter().map(|&c| char::from(c)).collect(),
    }
}

fn read(pdf: &Pdf) -> Result<Option<Layers>> {
    let xref = pdf.xref();
    let catalog: Dict<'_> = xref.get(xref.root_id()).ok_or_else(|| IoError::Format("the PDF has no catalog".into()))?;
    let pages = catalog.get_ref(b"Pages").ok_or_else(|| IoError::Format("the PDF has no page tree".into()))?;
    let Some(props) = catalog.get::<Dict<'_>>(b"OCProperties") else { return Ok(None) };
    let Some(ocgs) = props.get::<Array<'_>>(b"OCGs") else { return Ok(None) };
    let config = props.get::<Dict<'_>>(b"D");
    let listed = |key: &[u8]| -> Vec<ObjRef> {
        config.as_ref().and_then(|c| c.get::<Array<'_>>(key)).map(|a| a.raw_iter().filter_map(|i| i.as_obj_ref()).collect()).unwrap_or_default()
    };
    let base_off = config.as_ref().and_then(|c| c.get::<Name<'_>>(b"BaseState")).is_some_and(|n| n.as_ref() == b"OFF");
    let (on, off) = (listed(b"ON"), listed(b"OFF"));
    let mut groups = Vec::new();
    for item in ocgs.raw_iter().take(MAX_LAYERS) {
        let Some(r) = item.as_obj_ref() else { continue };
        let Some(group) = xref.get::<Dict<'_>>(ObjectIdentifier::from(r)) else { continue };
        let name = group.get::<hayro::hayro_syntax::object::String<'_>>(b"Name").map(|s| text(s.as_bytes())).unwrap_or_default();
        let shown = if base_off { on.contains(&r) } else { !off.contains(&r) };
        groups.push((r, PdfLayer { name, on: shown }));
    }
    Ok(Some(Layers { pages, groups }))
}

/// The layers of the PDF `bytes`, as its default configuration shows them; empty for a PDF
/// without layers.
pub fn layers(bytes: &[u8]) -> Result<Vec<PdfLayer>> {
    let pdf = open(bytes)?;
    Ok(read(&pdf)?.map(|l| l.groups.into_iter().map(|(_, layer)| layer).collect()).unwrap_or_default())
}

/// [`layers`] as (name, shown) pairs, for the engine's PDF hooks.
pub fn layer_list(bytes: &[u8]) -> std::result::Result<Vec<(String, bool)>, String> {
    layers(bytes).map(|l| l.into_iter().map(|l| (l.name, l.on)).collect()).map_err(|e| e.to_string())
}

/// [`with_layers`], for the engine's PDF hooks.
pub fn apply_layers(bytes: &[u8], visible: &[(String, bool)]) -> std::result::Result<Vec<u8>, String> {
    with_layers(bytes, visible).map_err(|e| e.to_string())
}

/// The last number after `key` in `bytes` (the final trailer's or cross-reference stream's).
fn last_number(bytes: &[u8], key: &[u8]) -> Option<u64> {
    let at = bytes.windows(key.len()).rposition(|w| w == key)? + key.len();
    let digits: Vec<u8> = bytes.get(at..)?.iter().copied().skip_while(u8::is_ascii_whitespace).take_while(u8::is_ascii_digit).take(19).collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// A number for one new object of the PDF `bytes`: from its trailer's size (read from the raw
/// bytes, which may name the size of an older section or match bytes inside a stream) or its
/// count of objects, whichever is larger, then past any number the PDF already uses.
fn new_object_number(pdf: &Pdf, bytes: &[u8]) -> Result<u64> {
    let start = last_number(bytes, b"/Size").unwrap_or(0).max(pdf.len() as u64).max(1);
    let in_use = |n: u64| {
        let Ok(n) = i32::try_from(n) else { return true };
        (0..4).any(|g| pdf.xref().get::<Object<'_>>(ObjectIdentifier::new(n, g)).is_some())
    };
    (start..start.saturating_add(MAX_NUMBER_PROBES)).find(|n| !in_use(*n)).ok_or_else(|| IoError::Format("the PDF has no free object number".into()))
}

/// The PDF `bytes` with each layer named in `visible` turned on or off (layers not named keep
/// the PDF's default). The bytes come back unchanged when nothing changes.
pub fn with_layers(bytes: &[u8], visible: &[(String, bool)]) -> Result<Vec<u8>> {
    if visible.is_empty() {
        return Ok(bytes.to_vec());
    }
    let pdf = open(bytes)?;
    let Some(l) = read(&pdf)? else { return Ok(bytes.to_vec()) };
    let shown = |layer: &PdfLayer| visible.iter().find(|(n, _)| *n == layer.name).map_or(layer.on, |(_, v)| *v);
    if l.groups.iter().all(|(_, layer)| shown(layer) == layer.on) {
        return Ok(bytes.to_vec());
    }
    let size = new_object_number(&pdf, bytes)?;
    let prev = last_number(bytes, b"startxref").ok_or_else(|| IoError::Format("the PDF has no cross-reference".into()))?;
    let r = |o: &ObjRef| format!("{} {} R", o.obj_number, o.gen_number);
    let all: Vec<String> = l.groups.iter().map(|(o, _)| r(o)).collect();
    let off: Vec<String> = l.groups.iter().filter(|(_, layer)| !shown(layer)).map(|(o, _)| r(o)).collect();
    let mut out = bytes.to_vec();
    out.extend_from_slice(b"\n");
    let at = out.len();
    out.extend_from_slice(
        format!(
            "{size} 0 obj\n<< /Type /Catalog /Pages {} /OCProperties << /OCGs [{}] /D << /BaseState /ON /OFF [{}] /Order [{}] >> >> >>\nendobj\n",
            r(&l.pages),
            all.join(" "),
            off.join(" "),
            all.join(" ")
        )
        .as_bytes(),
    );
    let xref = out.len();
    out.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \n{size} 1\n{at:010} 00000 n \ntrailer\n<< /Size {} /Root {size} 0 R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n",
            size + 1
        )
        .as_bytes(),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
    use cadcraft_geom::Vec3;

    /// A PDF written by CADCraft's own export: drawing layers MURS (red) and COTES (green)
    /// become PDF layers.
    fn layered_pdf() -> Vec<u8> {
        let mut d = Drawing::new_metric();
        for (name, color, y) in [("MURS", 1, 50.0), ("COTES", 3, 120.0)] {
            d.layers.push(cadcraft_doc::Layer { name: name.into(), color: cadcraft_color::Color::Index(color), ..cadcraft_doc::Layer::default() });
            let c = Common { layer: name.into(), lineweight: cadcraft_doc::Lineweight::Mm100(100), ..Common::default() };
            d.add(&Space::Model, c, EntityKind::Line(Line { a: Vec3::new(10.0, y, 0.0), b: Vec3::new(280.0, y, 0.0) })).unwrap();
        }
        let o = crate::PdfOptions { paper_mm: Some((297.0, 210.0)), fit: Some(false), lineweights: Some(true), ..crate::PdfOptions::default() };
        crate::pdf(&d, &Space::Model, &o).unwrap()
    }

    /// Pixels of the page drawn with `layers` visibility that are mostly red, mostly green.
    fn colours(bytes: &[u8]) -> (usize, usize) {
        let r = crate::pdf_read::PdfFile::open(bytes).unwrap().render(1, 600).unwrap();
        let px = r.rgba.as_chunks::<4>().0;
        let red = px.iter().filter(|[r, g, b, a]| *a > 128 && *r > 150 && *g < 100 && *b < 100).count();
        let green = px.iter().filter(|[r, g, b, a]| *a > 128 && *g > 150 && *r < 100 && *b < 100).count();
        (red, green)
    }

    #[test]
    fn the_layers_of_a_pdf_are_read_and_can_be_turned_off() {
        let pdf = layered_pdf();
        let names: Vec<(String, bool)> = layers(&pdf).unwrap().into_iter().map(|l| (l.name, l.on)).collect();
        assert!(names.contains(&("MURS".into(), true)) && names.contains(&("COTES".into(), true)), "{names:?}");

        let (red, green) = colours(&pdf);
        assert!(red > 50 && green > 50, "both drawn: {red} {green}");
        let off = with_layers(&pdf, &[("COTES".into(), false)]).unwrap();
        assert!(off.len() > pdf.len() && off.starts_with(&pdf), "an update appended, the file kept");
        let (red2, green2) = colours(&off);
        assert!(red2 > 50 && green2 == 0, "COTES hidden: {red2} {green2}");
        assert_eq!(layers(&off).unwrap().iter().find(|l| l.name == "COTES").map(|l| l.on), Some(false));
        // Turned on again from the updated file.
        let on = with_layers(&off, &[("COTES".into(), true)]).unwrap();
        assert!(colours(&on).1 > 50);
        // Nothing to change: the same bytes.
        assert_eq!(with_layers(&pdf, &[("COTES".into(), true), ("NOPE".into(), false)]).unwrap(), pdf);
    }

    /// A one-page PDF with layers MURS and COTES whose trailer understates its size (`/Size 3`
    /// for objects 1 to 5), as some writers do.
    fn understated_size_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] /D << /Order [4 0 R 5 0 R] >> >> >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>",
            "<< /Type /OCG /Name (MURS) >>",
            "<< /Type /OCG /Name (COTES) >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut at = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            at.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for a in at {
            out.extend_from_slice(format!("{a:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    /// A PDF 1.5 whose cross-reference is a stream (as most recent PDF writers make), with layers
    /// MURS (a red square) and COTES (a green one).
    fn xref_stream_pdf() -> Vec<u8> {
        let content = b"/OC /L1 BDC 1 0 0 rg 10 10 80 80 re f EMC /OC /L2 BDC 0 1 0 rg 110 10 80 80 re f EMC";
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << /BaseState /ON /Order [5 0 R 6 0 R] >> >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Properties << /L1 5 0 R /L2 6 0 R >> >> /Contents 4 0 R >>"
                .to_vec(),
            [format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(), content.to_vec(), b"\nendstream".to_vec()].concat(),
            b"<< /Type /OCG /Name (MURS) >>".to_vec(),
            b"<< /Type /OCG /Name (COTES) >>".to_vec(),
        ];
        let mut out = b"%PDF-1.5\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(u32::try_from(out.len()).unwrap());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        // The cross-reference stream (object 7, uncompressed): type 1, offset (4 bytes), generation (2).
        let xref_at = u32::try_from(out.len()).unwrap();
        offsets.push(xref_at);
        let mut rows = vec![0u8, 0, 0, 0, 0, 0xff, 0xff];
        for o in &offsets {
            rows.push(1);
            rows.extend_from_slice(&o.to_be_bytes());
            rows.extend_from_slice(&[0, 0]);
        }
        out.extend_from_slice(format!("7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n", rows.len()).as_bytes());
        out.extend_from_slice(&rows);
        out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
        out
    }

    #[test]
    fn layers_are_turned_off_in_a_pdf_whose_cross_reference_is_a_stream() {
        let pdf = xref_stream_pdf();
        let names: Vec<String> = layers(&pdf).unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["MURS", "COTES"]);
        let (red, green) = colours(&pdf);
        assert!(red > 1000 && green > 1000, "both shown at first: {red} red, {green} green");
        // A classic update chained to the stream: MURS off.
        let murs_off = with_layers(&pdf, &[("MURS".into(), false)]).unwrap();
        assert!(murs_off.starts_with(&pdf), "an incremental update");
        let (red, green) = colours(&murs_off);
        assert!(red == 0 && green > 1000, "MURS hidden: {red} red, {green} green");
        assert_eq!(layers(&murs_off).unwrap().iter().map(|l| (l.name.as_str(), l.on)).collect::<Vec<_>>(), [("MURS", false), ("COTES", true)]);
        // A second update on top of the first: MURS back, COTES off.
        let cotes_off = with_layers(&murs_off, &[("MURS".into(), true), ("COTES".into(), false)]).unwrap();
        let (red, green) = colours(&cotes_off);
        assert!(red > 1000 && green == 0, "COTES hidden: {red} red, {green} green");
        // Nothing to change: the same bytes.
        assert_eq!(with_layers(&cotes_off, &[("COTES".into(), false)]).unwrap(), cotes_off);
    }

    #[test]
    fn the_new_catalog_takes_a_number_no_object_uses() {
        let pdf = understated_size_pdf();
        assert_eq!(layers(&pdf).unwrap().len(), 2);
        let off = with_layers(&pdf, &[("COTES".into(), false)]).unwrap();
        // The catalog went past object 5 instead of replacing the page (object 3).
        let update = String::from_utf8_lossy(off.get(pdf.len()..).unwrap_or_default()).into_owned();
        assert!(update.contains("6 0 obj") && !update.contains("\n3 0 obj"), "{update}");
        let pdf_file = crate::pdf_read::PdfFile::open(&off).unwrap();
        assert_eq!(pdf_file.page_count(), 1);
        let (w, h) = pdf_file.page_size_mm(1).unwrap();
        assert!((w - 209.9).abs() < 0.5 && (h - 297.0).abs() < 0.5, "the page keeps its size: {w} × {h}");
        let after: Vec<(String, bool)> = layers(&off).unwrap().into_iter().map(|l| (l.name, l.on)).collect();
        assert_eq!(after, vec![("MURS".into(), true), ("COTES".into(), false)]);
    }

    #[test]
    fn hostile_bytes_are_errors_or_unchanged() {
        assert!(layers(b"not a pdf").is_err());
        assert!(with_layers(b"%PDF-1.7 junk", &[("A".into(), false)]).is_err());
        assert_eq!(layers(&crate::pdf(&Drawing::new_metric(), &Space::Model, &crate::PdfOptions::default()).unwrap()).unwrap().len(), 0);
    }
}
