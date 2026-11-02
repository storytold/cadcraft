//! Conservative import of vector PDF paths at paper scale. Unsupported clipping is an error.
use crate::{IoError, Result};
use cadcraft_doc::Drawing;
use craft_pdf::syntax::{
    content::{TypedIter, ops::TypedInstruction as Op},
    object::{Array, Dict, Name, Number, Object, String as PdfString},
    page::Resources,
};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

const LIMIT: usize = 32 * 1024 * 1024;
const MAX_OPS: usize = 1_000_000;
const MAX_DEPTH: usize = 16;
fn fail(s: &str) -> IoError {
    IoError::Format(format!("PDF import: {s}"))
}
fn pdfstr(b: &[u8]) -> String {
    if let Some(b) = b.strip_prefix(&[0xfe, 0xff]) {
        String::from_utf16_lossy(&b.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
    } else {
        b.iter().map(|&c| char::from(c)).collect()
    }
}
#[derive(Clone, Copy, Debug)]
struct Matrix([f64; 6]);
impl Matrix {
    fn mul(self, b: Self) -> Self {
        let [a, b1, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = b.0;
        Self([a * g + c * h, b1 * g + d * h, a * i + c * j, b1 * i + d * j, a * k + c * l + e, b1 * k + d * l + f])
    }
    fn pt(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
    fn inverse(self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        Some(Self([d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det]))
    }
}
#[derive(Clone)]
struct Frame {
    matrix: Matrix,
    boxes: Vec<(Matrix, [f64; 4])>,
}
struct Importer {
    width: f64,
    height: f64,
    frame: Frame,
    path: String,
    current: Option<(f64, f64)>,
    start: Option<(f64, f64)>,
    path_bounds: Option<[f64; 4]>,
    path_rect: bool,
    outside: bool,
    layer: String,
    layer_stack: Vec<String>,
    layer_ids: HashMap<String, String>,
    svg: String,
    notes: HashSet<&'static str>,
    ops: usize,
    bytes: usize,
}
impl Importer {
    fn point(&mut self, x: f64, y: f64) -> Result<(f64, f64)> {
        let p = self.frame.matrix.pt(x, y);
        if !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 1e9 || p.1.abs() > 1e9 {
            return Err(fail("non-finite or excessively large coordinate"));
        }
        for (inv, b) in &self.frame.boxes {
            let (x, y) = inv.pt(p.0, p.1);
            if x < b[0] - 0.01 || x > b[2] + 0.01 || y < b[1] - 0.01 || y > b[3] + 0.01 {
                self.outside = true;
            }
        }
        self.path_bounds = Some(match self.path_bounds {
            Some(v) => [v[0].min(p.0), v[1].min(p.1), v[2].max(p.0), v[3].max(p.1)],
            None => [p.0, p.1, p.0, p.1],
        });
        Ok(p)
    }
    fn moveto(&mut self, x: f64, y: f64) -> Result<()> {
        self.path_rect = false;
        let p = self.point(x, y)?;
        write!(self.path, "M {} {} ", p.0, p.1).map_err(|_| fail("path formatting"))?;
        self.current = Some(p);
        self.start = Some(p);
        Ok(())
    }
    fn lineto(&mut self, x: f64, y: f64) -> Result<()> {
        if self.current.is_none() {
            return Err(fail("line without a current point"));
        }
        self.path_rect = false;
        let p = self.point(x, y)?;
        write!(self.path, "L {} {} ", p.0, p.1).map_err(|_| fail("path formatting"))?;
        self.current = Some(p);
        Ok(())
    }
    fn cubic(&mut self, c1: (f64, f64), c2: (f64, f64), end: (f64, f64)) -> Result<()> {
        if self.current.is_none() {
            return Err(fail("curve without a current point"));
        }
        self.path_rect = false;
        let a = self.point(c1.0, c1.1)?;
        let b = self.point(c2.0, c2.1)?;
        let c = self.point(end.0, end.1)?;
        write!(self.path, "C {} {} {} {} {} {} ", a.0, a.1, b.0, b.1, c.0, c.1).map_err(|_| fail("path formatting"))?;
        self.current = Some(c);
        Ok(())
    }
    fn close(&mut self) {
        self.path.push_str("Z ");
        self.current = self.start;
    }
    fn clear(&mut self) {
        self.path.clear();
        self.current = None;
        self.start = None;
        self.path_bounds = None;
        self.path_rect = false;
        self.outside = false;
    }
    fn emit(&mut self, stroke: bool, fill: bool) -> Result<()> {
        if fill {
            self.notes.insert("fills and hatches");
        }
        if stroke && !self.path.is_empty() {
            if self.outside {
                return Err(fail("content crosses a page or form boundary; clipping is not yet supported"));
            }
            let next = self.layer_ids.len() + 1;
            if next > 255 && !self.layer_ids.contains_key(&self.layer) {
                return Err(fail("more than 255 PDF layers are not supported"));
            }
            let id = self.layer_ids.entry(self.layer.clone()).or_insert_with(|| format!("PDFL{next}"));
            write!(self.svg, "<g inkscape:groupmode=\"layer\" inkscape:label=\"{}\"><path d=\"{}\"/></g>", id, self.path)
                .map_err(|_| fail("SVG output"))?;
            if self.svg.len() > LIMIT {
                return Err(fail("generated geometry is too large"));
            }
        }
        self.clear();
        Ok(())
    }
    fn content<'a>(&mut self, data: &[u8], res: &Resources<'a>, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(fail("nested form limit exceeded"));
        }
        self.bytes = self.bytes.saturating_add(data.len());
        if self.bytes > LIMIT {
            return Err(fail("decoded content exceeds the limit"));
        }
        let mut iter = TypedIter::new(data);
        let mut stack: Vec<Frame> = Vec::new();
        let mut clip = false;
        while let Some(op) = iter.next() {
            self.ops += 1;
            if self.ops > MAX_OPS {
                return Err(fail("too many drawing instructions"));
            }
            if clip
                && matches!(
                    &op,
                    Op::StrokePath(_)
                        | Op::CloseAndStrokePath(_)
                        | Op::FillPathNonZero(_)
                        | Op::FillPathNonZeroCompatibility(_)
                        | Op::FillPathEvenOdd(_)
                        | Op::FillAndStrokeNonZero(_)
                        | Op::FillAndStrokeEvenOdd(_)
                        | Op::CloseFillAndStrokeNonZero(_)
                        | Op::CloseFillAndStrokeEvenOdd(_)
                )
            {
                return Err(fail("painted clipping paths are unsupported"));
            }
            match op {
                Op::SaveState(_) => stack.push(self.frame.clone()),
                Op::RestoreState(_) => self.frame = stack.pop().ok_or_else(|| fail("unbalanced graphics state"))?,
                Op::Transform(t) => {
                    self.frame.matrix =
                        self.frame.matrix.mul(Matrix([t.0.as_f64(), t.1.as_f64(), t.2.as_f64(), t.3.as_f64(), t.4.as_f64(), t.5.as_f64()]))
                }
                Op::MoveTo(v) => self.moveto(v.0.as_f64(), v.1.as_f64())?,
                Op::LineTo(v) => self.lineto(v.0.as_f64(), v.1.as_f64())?,
                Op::CubicTo(v) => self.cubic((v.0.as_f64(), v.1.as_f64()), (v.2.as_f64(), v.3.as_f64()), (v.4.as_f64(), v.5.as_f64()))?,
                Op::CubicStartTo(v) => {
                    let cur = self.current.ok_or_else(|| fail("curve without a current point"))?;
                    let inv = self.frame.matrix.inverse().ok_or_else(|| fail("singular transform"))?;
                    let start = inv.pt(cur.0, cur.1);
                    self.cubic(start, (v.0.as_f64(), v.1.as_f64()), (v.2.as_f64(), v.3.as_f64()))?;
                }
                Op::CubicEndTo(v) => self.cubic((v.0.as_f64(), v.1.as_f64()), (v.2.as_f64(), v.3.as_f64()), (v.2.as_f64(), v.3.as_f64()))?,
                Op::RectPath(v) => {
                    let (x, y, w, h) = (v.0.as_f64(), v.1.as_f64(), v.2.as_f64(), v.3.as_f64());
                    let was_empty = self.path.is_empty();
                    self.moveto(x, y)?;
                    self.lineto(x + w, y)?;
                    self.lineto(x + w, y + h)?;
                    self.lineto(x, y + h)?;
                    self.close();
                    self.path_rect = was_empty;
                }
                Op::ClosePath(_) => self.close(),
                Op::ClipNonZero(_) | Op::ClipEvenOdd(_) => {
                    let b = self.path_bounds.ok_or_else(|| fail("empty clipping path"))?;
                    let m = self.frame.matrix.0;
                    let axis_aligned = (m[1].abs() < 1e-9 && m[2].abs() < 1e-9) || (m[0].abs() < 1e-9 && m[3].abs() < 1e-9);
                    if !self.path_rect || !axis_aligned || b[0] > 0.01 || b[1] > 0.01 || b[2] < self.width - 0.01 || b[3] < self.height - 0.01 {
                        return Err(fail("this page uses clipping that cannot yet be converted safely"));
                    }
                    clip = true;
                }
                Op::StrokePath(_) => {
                    if clip {
                        return Err(fail("painted clipping path is unsupported"));
                    }
                    self.emit(true, false)?
                }
                Op::CloseAndStrokePath(_) => {
                    self.close();
                    self.emit(true, false)?
                }
                Op::FillPathNonZero(_) | Op::FillPathNonZeroCompatibility(_) | Op::FillPathEvenOdd(_) => self.emit(false, true)?,
                Op::FillAndStrokeNonZero(_) | Op::FillAndStrokeEvenOdd(_) => self.emit(true, true)?,
                Op::CloseFillAndStrokeNonZero(_) | Op::CloseFillAndStrokeEvenOdd(_) => {
                    self.close();
                    self.emit(true, true)?
                }
                Op::EndPath(_) => {
                    self.clear();
                    clip = false
                }
                Op::SetGraphicsState(v) => {
                    let g = res.get_ext_g_state(v.0).ok_or_else(|| fail("unresolved graphics state"))?;
                    for key in [b"CA".as_slice(), b"ca".as_slice()] {
                        if g.get::<Number>(key).is_some_and(|n| n.as_f64() < 0.999999) {
                            return Err(fail("transparent graphics cannot be safely converted"));
                        }
                    }
                    if g.get::<Object<'_>>(b"SMask").is_some_and(|o| !matches!(o,Object::Name(n) if n.as_ref()==b"None")) {
                        return Err(fail("soft masks cannot be safely converted"));
                    }
                    if g.get::<Name<'_>>(b"BM").is_some_and(|n| n.as_ref() != b"Normal") {
                        return Err(fail("blend modes cannot be safely converted"));
                    }
                }
                Op::Fallback(_) => return Err(fail("the page contains an unsupported drawing instruction")),
                Op::BeginText(_) => {
                    self.notes.insert("text");
                }
                Op::InlineImage(_) => {
                    self.notes.insert("raster images");
                }
                Op::Shading(_) => {
                    self.notes.insert("shadings");
                }
                Op::BeginMarkedContentWithProperties(v) => {
                    self.layer_stack.push(self.layer.clone());
                    if v.0.as_ref() == b"OC" {
                        let dict = match v.1 {
                            Object::Name(n) => res.properties.get::<Dict<'_>>(n.as_ref()),
                            Object::Dict(d) => d.get::<Dict<'_>>(b"OC"),
                            _ => None,
                        }
                        .ok_or_else(|| fail("unresolved PDF layer"))?;
                        if dict.get::<Name<'_>>(b"Type").is_some_and(|n| n.as_ref() == b"OCMD") {
                            return Err(fail("compound optional-content layers are not supported"));
                        }
                        let name = dict.get::<PdfString<'_>>(b"Name").map(|s| pdfstr(s.as_bytes())).ok_or_else(|| fail("unnamed PDF layer"))?;
                        if name.trim().is_empty() || name.len() > 255 || name.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c)) {
                            return Err(fail("PDF layer name is invalid for CAD"));
                        }
                        if self.layer != "0" && self.layer != name {
                            return Err(fail("nested PDF layers cannot be assigned to one CAD layer safely"));
                        }
                        if self.layer_ids.keys().any(|v| v != &name && v.eq_ignore_ascii_case(&name)) {
                            return Err(fail("PDF contains layer names that differ only by case"));
                        }
                        self.layer = name;
                    }
                }
                Op::BeginMarkedContent(_) => self.layer_stack.push(self.layer.clone()),
                Op::EndMarkedContent(_) => self.layer = self.layer_stack.pop().ok_or_else(|| fail("unbalanced marked content"))?,
                Op::XObject(v) => {
                    let stream = res.get_x_object(v.0).ok_or_else(|| fail("missing external object"))?;
                    let dict = stream.dict();
                    let kind = dict.get::<Name<'_>>(b"Subtype").map(|n| n.as_ref().to_vec()).unwrap_or_default();
                    if kind == b"Image" {
                        self.notes.insert("raster images");
                    } else if kind == b"Form" {
                        if !self.path.is_empty() {
                            return Err(fail("form encountered inside an unfinished path"));
                        }
                        let saved = self.frame.clone();
                        let start_layer = self.layer.clone();
                        if dict.get::<Dict<'_>>(b"Group").and_then(|g| g.get::<Name<'_>>(b"S")).is_some_and(|n| n.as_ref() == b"Transparency") {
                            return Err(fail("transparent form groups cannot be safely converted"));
                        }
                        if let Some(oc) = dict.get::<Dict<'_>>(b"OC") {
                            if oc.get::<Name<'_>>(b"Type").is_some_and(|n| n.as_ref() == b"OCMD") {
                                return Err(fail("compound form visibility is unsupported"));
                            }
                            let name = oc.get::<PdfString<'_>>(b"Name").map(|s| pdfstr(s.as_bytes())).ok_or_else(|| fail("unnamed form layer"))?;
                            if self.layer != "0" && self.layer != name {
                                return Err(fail("nested form layers cannot be preserved"));
                            }
                            self.layer = name;
                        }
                        let a = dict
                            .get::<Array<'_>>(b"Matrix")
                            .map(|a| a.iter::<Number>().map(|n| n.as_f64()).collect::<Vec<_>>())
                            .unwrap_or_else(|| vec![1., 0., 0., 1., 0., 0.]);
                        if a.len() != 6 {
                            return Err(fail("invalid form matrix"));
                        }
                        self.frame.matrix = self.frame.matrix.mul(Matrix([a[0], a[1], a[2], a[3], a[4], a[5]]));
                        let bbox = dict
                            .get::<Array<'_>>(b"BBox")
                            .ok_or_else(|| fail("form has no bounding box"))?
                            .iter::<Number>()
                            .map(|n| n.as_f64())
                            .collect::<Vec<_>>();
                        if bbox.len() != 4 {
                            return Err(fail("invalid form bounding box"));
                        }
                        let inv = self.frame.matrix.inverse().ok_or_else(|| fail("singular form matrix"))?;
                        self.frame.boxes.push((inv, [bbox[0].min(bbox[2]), bbox[1].min(bbox[3]), bbox[0].max(bbox[2]), bbox[1].max(bbox[3])]));
                        let resources = Resources::from_scope(dict.get::<Dict<'_>>(b"Resources"), res.clone());
                        let bytes = stream.decoded_within(LIMIT - self.bytes).map_err(|_| fail("form stream could not be decoded within limit"))?;
                        self.content(&bytes, &resources, depth + 1)?;
                        if !self.path.is_empty() {
                            return Err(fail("form contains an unfinished path"));
                        }
                        self.frame = saved;
                        self.layer = start_layer;
                    } else {
                        return Err(fail("unsupported external object"));
                    }
                }
                _ => {}
            }
        }
        if !stack.is_empty() || clip {
            return Err(fail("incomplete graphics state or clipping"));
        }
        Ok(())
    }
}
/// Import one PDF page as editable paths, using millimetres at PDF paper scale.
pub fn import_pdf(bytes: &[u8], page_number: usize) -> Result<Drawing> {
    match import_pdf_strict(bytes, page_number) {
        Err(IoError::Format(ref message)) if message.contains("clipping") || message.contains("boundary") => {
            crate::pdf_fallback::import_pdf(bytes, page_number)
        }
        other => other,
    }
}

fn import_pdf_strict(bytes: &[u8], page_number: usize) -> Result<Drawing> {
    if bytes.len() > LIMIT {
        return Err(fail("input exceeds the 32 MB limit"));
    }
    let pdf = craft_pdf::open_slice(bytes, None).map_err(|e| fail(&e.to_string()))?;
    let mut hidden = HashSet::new();
    if let Some(catalog) = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id())
        && let Some(oc) = catalog.get::<Dict<'_>>(b"OCProperties")
        && let Some(config) = oc.get::<Dict<'_>>(b"D")
    {
        let mut off: HashSet<_> = if config.get::<Name<'_>>(b"BaseState").is_some_and(|n| n.as_ref() == b"OFF") {
            oc.get::<Array<'_>>(b"OCGs").map(|a| a.raw_iter().filter_map(|o| o.as_obj_ref()).collect()).unwrap_or_default()
        } else {
            HashSet::new()
        };
        if let Some(a) = config.get::<Array<'_>>(b"OFF") {
            off.extend(a.raw_iter().filter_map(|o| o.as_obj_ref()));
        }
        if let Some(a) = config.get::<Array<'_>>(b"ON") {
            for r in a.raw_iter().filter_map(|o| o.as_obj_ref()) {
                off.remove(&r);
            }
        }
        if let Some(groups) = oc.get::<Array<'_>>(b"OCGs") {
            for group in groups.iter::<Dict<'_>>() {
                if let Some(id) = group.obj_id()
                    && off.contains(&id.into())
                    && let Some(name) = group.get::<PdfString<'_>>(b"Name")
                {
                    hidden.insert(pdfstr(name.as_bytes()));
                }
            }
        }
    }
    let page = pdf
        .pages()
        .get(page_number.checked_sub(1).ok_or_else(|| fail("page numbers start at 1"))?)
        .ok_or_else(|| fail("page number does not exist"))?;
    let (w, h) = page.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
        return Err(fail("invalid page dimensions"));
    }
    let initial = Matrix(page.initial_transform(true).as_coeffs());
    let inv = initial.inverse().ok_or_else(|| fail("singular page transform"))?;
    let crop = page.intersected_crop_box();
    let mut parser = Importer {
        width: w,
        height: h,
        frame: Frame { matrix: initial, boxes: vec![(inv, [crop.x0, crop.y0, crop.x1, crop.y1])] },
        path: String::new(),
        current: None,
        start: None,
        path_bounds: None,
        path_rect: false,
        outside: false,
        layer: "0".into(),
        layer_stack: vec![],
        layer_ids: HashMap::new(),
        svg: String::new(),
        notes: HashSet::new(),
        ops: 0,
        bytes: 0,
    };
    let mut reader = page.content_reader();
    let mut content = Vec::new();
    reader.read_into(&mut content, LIMIT + 1);
    if content.len() > LIMIT || reader.truncated() {
        return Err(fail("page content could not be decoded completely within limit"));
    }
    parser.content(&content, page.resources(), 0)?;
    if !parser.layer_stack.is_empty() {
        return Err(fail("unbalanced marked content"));
    }
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{w}pt\" height=\"{h}pt\" viewBox=\"0 0 {w} {h}\">{}</svg>",
        parser.svg
    );
    let mut drawing = crate::svg_read::read(svg.as_bytes())?;
    for layer in &mut drawing.layers {
        if let Some((name, _)) = parser.layer_ids.iter().find(|(_, id)| **id == layer.name) {
            layer.name = name.clone();
            layer.on = !hidden.contains(name);
        }
    }
    let mut seen = HashSet::new();
    drawing.layers.retain(|l| seen.insert(l.name.to_ascii_lowercase()));
    for layer in &mut drawing.layers {
        if hidden.contains(&layer.name) {
            layer.on = false;
        }
    }
    for handle in drawing.model.handles() {
        drawing.model.modify(handle, |entity| {
            if let Some((name, _)) = parser.layer_ids.iter().find(|(_, id)| **id == entity.common.layer) {
                entity.common.layer = name.clone();
            }
        });
    }
    if drawing.model.is_empty() {
        return Err(fail("no supported stroked vector paths were found; scanned or fill-only pages are not converted"));
    }
    drawing
        .read_notes
        .push("PDF vectors are imported at paper size in millimetres. Verify and scale against a known dimension before CAD use.".into());
    if !parser.notes.is_empty() {
        let mut notes: Vec<_> = parser.notes.into_iter().collect();
        notes.sort();
        drawing.read_notes.push(format!("PDF import omitted: {}. Colours and line styles are not retained.", notes.join(", ")));
    } else {
        drawing.read_notes.push("PDF import: colours and line styles are not retained.".into());
    }
    Ok(drawing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::EntityKind;
    fn fixture(content: &str, page_extra: &str) -> Vec<u8> {
        format!("%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [6 0 R 7 0 R] /D << /OFF [7 0 R] >> >> >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] {page_extra} /Resources << /Properties << /L1 6 0 R /L2 7 0 R >> /XObject << /Fm 8 0 R >> >> /Contents 4 0 R >> endobj\n4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n6 0 obj << /Type /OCG /Name (Walls and doors) >> endobj\n7 0 obj << /Type /OCG /Name (Hidden) >> endobj\n8 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [1 0 0 1 5 5] /Length 16 >> stream\n0 0 m 10 10 l S\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF",content.len()).into_bytes()
    }
    #[test]
    fn imports_layers_paper_scale_and_hidden_state() {
        let p = fixture("/OC /L1 BDC 0 0 m 72 0 l S EMC /OC /L2 BDC 0 10 m 72 10 l S EMC", "");
        let d = import_pdf(&p, 1).unwrap();
        assert_eq!(d.model.len(), 2);
        assert!(d.layers.iter().any(|l| l.name == "Walls and doors" && l.on));
        assert!(d.layers.iter().any(|l| l.name == "Hidden" && !l.on));
        assert!(d.model.iter().any(|e| e.common.layer == "Walls and doors"));
        let first = d.model.iter().find(|e| e.common.layer == "Walls and doors").unwrap();
        let length = match &first.kind {
            EntityKind::Line(l) => (l.b.x - l.a.x).hypot(l.b.y - l.a.y),
            EntityKind::LwPolyline(p) => (p.vertices[1].p.x - p.vertices[0].p.x).hypot(p.vertices[1].p.y - p.vertices[0].p.y),
            k => panic!("unexpected {k:?}"),
        };
        assert!((length - 25.4).abs() < 0.001, "{length}");
        let dwg = crate::write(&d, "test.dwg").unwrap();
        let reopened = crate::read(&dwg, "test.dwg").unwrap();
        assert!(reopened.layers.iter().any(|l| l.name == "Walls and doors"));
        assert_eq!(reopened.model.len(), 2);
    }
    #[test]
    fn imports_a_transformed_form_and_rotated_scaled_page() {
        let p = fixture("q 2 0 0 2 20 20 cm /OC /L1 BDC /Fm Do EMC Q", "/Rotate 90 /UserUnit 2");
        let d = import_pdf(&p, 1).unwrap();
        assert_eq!(d.model.len(), 1);
        assert_eq!(d.model.iter().next().unwrap().common.layer, "Walls and doors");
    }
    #[test]
    fn refuses_unsafe_clip_and_image_only_pages() {
        assert!(import_pdf(&fixture("0 0 20 20 re W n 0 0 m 10 10 l S", ""), 1).unwrap_err().to_string().contains("clipping"));
        assert!(import_pdf(&fixture("0 0 10 10 re f", ""), 1).unwrap_err().to_string().contains("no supported"));
        assert!(import_pdf(&fixture("0 0 m 10 10 l S", ""), 2).is_err());
    }
    #[test]
    fn notes_omitted_text_and_fills() {
        let d = import_pdf(&fixture("0 0 m 10 10 l S 1 1 3 3 re f BT ET", ""), 1).unwrap();
        assert!(d.read_notes.iter().any(|n| n.contains("text") && n.contains("fills")));
    }
}
