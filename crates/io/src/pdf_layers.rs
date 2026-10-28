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

/// A layer name, a PDF text string: UTF-16 with a byte order mark, UTF-8 with one (PDF 2.0), else
/// PDFDocEncoding.
fn text(b: &[u8]) -> String {
    match b {
        [0xFE, 0xFF, rest @ ..] => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
            String::from_utf16_lossy(&units)
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => b.iter().map(|&c| pdf_doc_char(c)).collect(),
    }
}

/// PDFDocEncoding's character for byte `c` (ISO 32000, annex D): Latin-1 but for the accents at
/// 0x18–0x1F and the punctuation, ligatures and letters at 0x80–0xA0. Bytes it leaves undefined
/// keep their Latin-1 character.
fn pdf_doc_char(c: u8) -> char {
    const LOW: [char; 8] = ['\u{2d8}', '\u{2c7}', '\u{2c6}', '\u{2d9}', '\u{2dd}', '\u{2db}', '\u{2da}', '\u{2dc}'];
    const HIGH: [char; 33] = [
        '\u{2022}', '\u{2020}', '\u{2021}', '\u{2026}', '\u{2014}', '\u{2013}', '\u{192}', '\u{2044}', '\u{2039}', '\u{203a}', '\u{2212}',
        '\u{2030}', '\u{201e}', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '\u{201a}', '\u{2122}', '\u{fb01}', '\u{fb02}', '\u{141}', '\u{152}',
        '\u{160}', '\u{178}', '\u{17d}', '\u{131}', '\u{142}', '\u{153}', '\u{161}', '\u{17e}', '\u{9f}', '\u{20ac}',
    ];
    match c {
        0x18..=0x1F => LOW.get(usize::from(c - 0x18)).copied(),
        0x80..=0xA0 => HIGH.get(usize::from(c - 0x80)).copied(),
        _ => None,
    }
    .unwrap_or(char::from(c))
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

/// Most bytes of a trailer dictionary read to carry its entries over.
const MAX_TRAILER_BYTES: usize = 64 << 10;

/// The end of the PDF value (a dictionary, array, string, name, number or reference) starting
/// at `at` in `b`, skipping leading white space; `None` when it doesn't end within `b`.
fn value_end(b: &[u8], at: usize) -> Option<usize> {
    let mut i = at;
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    let delimited = |c: u8| c.is_ascii_whitespace() || b"()<>[]{}/%".contains(&c);
    match (*b.get(i)?, b.get(i + 1).copied()) {
        (b'<', Some(b'<')) | (b'[', _) | (b'(', _) => {
            // Nested dictionaries, arrays and strings (with their escapes) up to the matching end.
            let mut stack: Vec<u8> = Vec::new();
            let mut j = i;
            loop {
                let c = *b.get(j)?;
                if stack.last() == Some(&b'(') {
                    match c {
                        b'\\' => j += 1,
                        b'(' => stack.push(b'('),
                        b')' => {
                            stack.pop();
                        }
                        _ => {}
                    }
                } else {
                    match (c, b.get(j + 1).copied()) {
                        (b'<', Some(b'<')) => {
                            stack.push(b'd');
                            j += 1;
                        }
                        (b'>', Some(b'>')) if stack.last() == Some(&b'd') => {
                            stack.pop();
                            j += 1;
                        }
                        (b'<', _) => {
                            j += b.get(j..)?.iter().position(|&x| x == b'>')?;
                        }
                        (b'[', _) | (b'(', _) => stack.push(c),
                        (b']', _) if stack.last() == Some(&b'[') => {
                            stack.pop();
                        }
                        _ => {}
                    }
                }
                j += 1;
                if stack.is_empty() {
                    return Some(j);
                }
            }
        }
        (b'<', _) => Some(i + b.get(i..)?.iter().position(|&x| x == b'>')? + 1),
        (b'/', _) => Some(i + 1 + b.get(i + 1..)?.iter().position(|&c| delimited(c)).unwrap_or(b.len() - i - 1)),
        _ => {
            // A number, or a reference: two numbers and R.
            let word = |from: usize| -> Option<(usize, usize)> {
                let mut k = from;
                while b.get(k).is_some_and(u8::is_ascii_whitespace) {
                    k += 1;
                }
                let len = b.get(k..)?.iter().position(|&c| delimited(c)).unwrap_or(b.len() - k);
                (len > 0).then_some((k, k + len))
            };
            let (_, end) = word(i)?;
            match (word(end), word(end).and_then(|(_, e)| word(e))) {
                (Some((gs, ge)), Some((rs, re)))
                    if b.get(gs..ge).is_some_and(|g| g.iter().all(u8::is_ascii_digit)) && b.get(rs..re) == Some(b"R".as_slice()) =>
                {
                    Some(re)
                }
                _ => Some(end),
            }
        }
    }
}

/// The `/Encrypt`, `/ID` and `/Info` entries of the trailer of the cross-reference section at
/// `prev` in `bytes` (a `trailer` dictionary, or a cross-reference stream's), as written there:
/// an update's trailer must repeat them, or an encrypted PDF is no longer decrypted.
fn kept_trailer_entries(bytes: &[u8], prev: usize) -> Vec<u8> {
    let Some(region) = bytes.get(prev..).map(|r| &r[..r.len().min(MAX_TRAILER_BYTES)]) else { return Vec::new() };
    let Some(start) = region.windows(2).position(|w| w == b"<<") else { return Vec::new() };
    let Some(end) = value_end(region, start) else { return Vec::new() };
    let dict = &region[start + 2..end.saturating_sub(2)];
    let mut out = Vec::new();
    let mut i = 0;
    // Each entry of the dictionary itself (not of one nested in it): a name, then its value.
    while i < dict.len() {
        if dict[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if dict[i] != b'/' {
            match value_end(dict, i) {
                Some(e) if e > i => i = e,
                _ => i += 1,
            }
            continue;
        }
        let Some(key_end) = value_end(dict, i) else { break };
        let Some(val_end) = value_end(dict, key_end) else { break };
        let key = &dict[i..key_end];
        if [b"/Encrypt".as_slice(), b"/ID", b"/Info"].contains(&key) {
            out.push(b' ');
            out.extend_from_slice(&dict[i..val_end]);
        }
        i = val_end;
    }
    out
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
    let kept = usize::try_from(prev).map(|p| kept_trailer_entries(bytes, p)).unwrap_or_default();
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
        format!("xref\n0 1\n0000000000 65535 f \n{size} 1\n{at:010} 00000 n \ntrailer\n<< /Size {} /Root {size} 0 R /Prev {prev}", size + 1)
            .as_bytes(),
    );
    out.extend_from_slice(&kept);
    out.extend_from_slice(format!(" >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
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
        xref_stream_pdf_named(b"(MURS)", b"(COTES)")
    }

    /// [`xref_stream_pdf`] with its two layers' names written as `red` and `green` (PDF strings).
    fn xref_stream_pdf_named(red: &[u8], green: &[u8]) -> Vec<u8> {
        let content = b"/OC /L1 BDC 1 0 0 rg 10 10 80 80 re f EMC /OC /L2 BDC 0 1 0 rg 110 10 80 80 re f EMC";
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << /BaseState /ON /Order [5 0 R 6 0 R] >> >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Properties << /L1 5 0 R /L2 6 0 R >> >> /Contents 4 0 R >>"
                .to_vec(),
            [format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(), content.to_vec(), b"\nendstream".to_vec()].concat(),
            [b"<< /Type /OCG /Name ".as_slice(), red, b" >>"].concat(),
            [b"<< /Type /OCG /Name ".as_slice(), green, b" >>"].concat(),
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
    fn layer_names_are_decoded_as_pdf_text_strings() {
        // PDFDocEncoding: 0x84 is an em dash and 0xA0 the euro sign (not Latin-1's no-break space).
        assert_eq!(text(b"Co\xfbt \x84 5 \xa0"), "Coût — 5 €");
        assert_eq!(text(b"\x18\x8d\x8e\x92\x93\x95\x9e"), "˘\u{201c}\u{201d}™ﬁŁž");
        // UTF-16 with its byte order mark, and UTF-8 with its own (PDF 2.0).
        assert_eq!(text(b"\xfe\xff\x00C\x00\xf4\x20\xac"), "Cô€");
        assert_eq!(text(b"\xef\xbb\xbfCo\xc3\xbbt \xe2\x82\xac"), "Coût €");
        // A PDF whose layer is named in PDFDocEncoding: listed by its name, and turned off by it.
        let pdf = xref_stream_pdf_named(b"(Co\\373t \\204 5 \\240)", b"(COTES)");
        let names: Vec<String> = layers(&pdf).unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["Coût — 5 €", "COTES"]);
        let off = with_layers(&pdf, &[("Coût — 5 €".into(), false)]).unwrap();
        let (red, green) = colours(&off);
        assert!(red == 0 && green > 1000, "the layer named in PDFDocEncoding hidden: {red} red, {green} green");
    }

    /// MD5 (RFC 1321), for [`encrypted_pdf`].
    fn md5(data: &[u8]) -> [u8; 16] {
        let s: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11,
            16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        let k: Vec<u32> = (0..64).map(|i| ((f64::from(i + 1)).sin().abs() * 4_294_967_296.0) as u32).collect();
        let mut msg = data.to_vec();
        let bits = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bits.to_le_bytes());
        let (mut a0, mut b0, mut c0, mut d0) = (0x6745_2301u32, 0xefcd_ab89u32, 0x98ba_dcfeu32, 0x1032_5476u32);
        for chunk in msg.chunks(64) {
            let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
            let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
            for i in 0..64 {
                let (f, g) = match i / 16 {
                    0 => ((b & c) | (!b & d), i),
                    1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                    2 => (b ^ c ^ d, (3 * i + 5) % 16),
                    _ => (c ^ (b | !d), (7 * i) % 16),
                };
                let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
                a = d;
                d = c;
                c = b;
                b = b.wrapping_add(f.rotate_left(s[i]));
            }
            a0 = a0.wrapping_add(a);
            b0 = b0.wrapping_add(b);
            c0 = c0.wrapping_add(c);
            d0 = d0.wrapping_add(d);
        }
        let mut out = [0u8; 16];
        for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// RC4, for [`encrypted_pdf`].
    fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut st: Vec<u8> = (0..=255).collect();
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(st[i]).wrapping_add(key[i % key.len()]);
            st.swap(i, usize::from(j));
        }
        let (mut i, mut j) = (0u8, 0u8);
        data.iter()
            .map(|b| {
                i = i.wrapping_add(1);
                j = j.wrapping_add(st[usize::from(i)]);
                st.swap(usize::from(i), usize::from(j));
                b ^ st[usize::from(st[usize::from(i)].wrapping_add(st[usize::from(j)]))]
            })
            .collect()
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02X}")).collect()
    }

    /// The PDF of [`xref_stream_pdf`] encrypted with the standard security handler (revision 2,
    /// 40-bit RC4, empty user password, ISO 32000 §7.6.3), with a classic cross-reference table:
    /// its content and its layers' names are encrypted.
    fn encrypted_pdf() -> Vec<u8> {
        const PAD: [u8; 32] = [
            0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E,
            0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
        ];
        let id = *b"cadcraft-test-id";
        let p: i32 = -4;
        let o = rc4(&md5(&PAD)[..5], &PAD);
        let key: Vec<u8> = md5(&[PAD.as_slice(), &o, &p.to_le_bytes(), &id].concat())[..5].to_vec();
        let u = rc4(&key, &PAD);
        let obj_key = |n: u32| md5(&[key.as_slice(), &n.to_le_bytes()[..3], &[0, 0]].concat())[..10].to_vec();
        let content = rc4(&obj_key(4), b"/OC /L1 BDC 1 0 0 rg 10 10 80 80 re f EMC /OC /L2 BDC 0 1 0 rg 110 10 80 80 re f EMC");
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << /BaseState /ON /Order [5 0 R 6 0 R] >> >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Properties << /L1 5 0 R /L2 6 0 R >> >> /Contents 4 0 R >>"
                .to_vec(),
            [format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(), content, b"\nendstream".to_vec()].concat(),
            format!("<< /Type /OCG /Name <{}> >>", hex(&rc4(&obj_key(5), b"MURS"))).into_bytes(),
            format!("<< /Type /OCG /Name <{}> >>", hex(&rc4(&obj_key(6), b"COTES"))).into_bytes(),
            format!("<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P {p} >>", hex(&o), hex(&u)).into_bytes(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, ob) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(ob);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Encrypt 7 0 R /ID [<{}> <{}>] >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1,
                hex(&id),
                hex(&id)
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn an_update_repeats_the_trailer_s_encryption_identity_and_info() {
        let kept = |trailer: &[u8]| String::from_utf8_lossy(&kept_trailer_entries(trailer, 0)).into_owned();
        // A reference, a two-string array and a reference; other entries left out.
        assert_eq!(
            kept(
                b"xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 8 /Root 1 0 R /Encrypt 7 0 R /ID [<AB> <CD>] /Info 3 0 R /Prev 10 >>\nstartxref"
            ),
            " /Encrypt 7 0 R /ID [<AB> <CD>] /Info 3 0 R"
        );
        // A direct encryption dictionary, an identity with literal strings holding brackets and an
        // escaped parenthesis, in a cross-reference stream's dictionary (its data not read).
        assert_eq!(
            kept(
                b"7 0 obj\n<< /Type /XRef /Encrypt << /Filter /Standard /O <00ff> /P -4 >> /ID [(a]b\\)c) (x>>y)] /W [1 4 2] >>\nstream\n\x01<<\x02"
            ),
            " /Encrypt << /Filter /Standard /O <00ff> /P -4 >> /ID [(a]b\\)c) (x>>y)]"
        );
        // An /Encrypt inside a nested dictionary is not the trailer's.
        assert_eq!(kept(b"trailer\n<< /Root 1 0 R /X << /Encrypt 9 0 R >> >>"), "");
        // Hostile or cut short: nothing, no panic.
        for b in [b"".as_slice(), b"<<", b"<< /Encrypt", b"<< /ID [(unclosed", b"<< /Encrypt 7 0", b"<<<<<<>>", b"<< /ID <ab"] {
            let _ = kept_trailer_entries(b, 0);
            let _ = kept_trailer_entries(b, 1000);
        }
        assert_eq!(kept(b"<< /Encrypt 7 0"), "");
    }

    #[test]
    fn layers_are_turned_off_in_an_encrypted_pdf() {
        let pdf = encrypted_pdf();
        let names: Vec<String> = layers(&pdf).unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["MURS", "COTES"], "the names are decrypted");
        let (red, green) = colours(&pdf);
        assert!(red > 1000 && green > 1000, "both shown at first: {red} red, {green} green");
        // The update keeps the file's encryption: its trailer names the same /Encrypt and /ID.
        let off = with_layers(&pdf, &[("MURS".into(), false)]).unwrap();
        assert_eq!(layers(&off).unwrap().iter().map(|l| (l.name.as_str(), l.on)).collect::<Vec<_>>(), [("MURS", false), ("COTES", true)]);
        let (red, green) = colours(&off);
        assert!(red == 0 && green > 1000, "MURS hidden, the page still decrypted: {red} red, {green} green");
        // And a second update on top of it.
        let both = with_layers(&off, &[("COTES".into(), false)]).unwrap();
        let (red, green) = colours(&both);
        assert!(red == 0 && green == 0, "both hidden: {red} red, {green} green");
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
