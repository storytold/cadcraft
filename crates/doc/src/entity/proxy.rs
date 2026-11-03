//! The proxy graphics an ACAD_PROXY_ENTITY carries (DXF groups 92/160 and 310): the picture
//! the owning application left for programs that don't have it, drawn as AutoCAD draws it with
//! PROXYSHOW 1. The layout follows the Open Design Alliance's public DWG specification
//! ("Proxy Entity Graphics"): a header (total size, chunk count), then chunks of (size, type,
//! data), sizes in bytes including the 8-byte chunk header, all little-endian.
//!
//! The picture becomes ordinary, read-only entities: drawn and measured like the contents of a
//! block, with layer 0, ByBlock colour, linetype and lineweight standing for the proxy's own
//! until a chunk sets them. Chunks CADCraft doesn't decode (meshes, shells, lightweight
//! polylines, materials) are skipped.

use super::{Arc, Circle, Common, Entity, EntityKind, Handle, Hatch, HatchLoop, Line, Lineweight, LwPolyline, Text, Unknown};
use cadcraft_color::{Color, Rgb};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

/// Bytes of proxy graphics read at most from one entity.
const MAX_BYTES: usize = 16 << 20;
/// Entities made at most from one entity's proxy graphics.
const MAX_ENTITIES: usize = 100_000;
/// Vertices read at most from one polyline or polygon chunk.
const MAX_VERTICES: usize = 1_000_000;
/// Matrices pushed at most.
const MAX_MATRICES: usize = 64;

/// The table names proxy graphics refer to by index, in the order of the file's tables.
#[derive(Clone, Debug, Default)]
pub struct ProxyTables {
    /// LAYER records in file order.
    pub layers: Vec<String>,
    /// LTYPE records in file order, without ByBlock and ByLayer (indices count only the others).
    pub linetypes: Vec<String>,
    /// Text styles (name, font file) in file order: a text chunk names its font, and the first
    /// style with that font draws it.
    pub styles: Vec<(String, String)>,
}

impl Unknown {
    /// The proxy graphics bytes of an ACAD_PROXY_ENTITY: the 310 groups after its size group
    /// (92, or 160 in newer files), cut to that size. Empty for other records, for an entity
    /// without graphics and for one whose data isn't hex.
    pub fn proxy_graphics_bytes(&self) -> Vec<u8> {
        if !self.dxf_type.eq_ignore_ascii_case("ACAD_PROXY_ENTITY") {
            return Vec::new();
        }
        let Some(at) = self.tags.iter().position(|t| t.code == 92 || t.code == 160) else { return Vec::new() };
        let size = self.tags.get(at).and_then(|t| t.value.trim().parse::<usize>().ok()).unwrap_or(0).min(MAX_BYTES);
        let mut out = Vec::with_capacity(size);
        for t in self.tags.iter().skip(at + 1).take_while(|t| t.code == 310) {
            // Damaged data draws nothing rather than a picture cut at a guess.
            if !push_hex(&mut out, &t.value) {
                return Vec::new();
            }
            if out.len() >= size {
                break;
            }
        }
        out.truncate(size);
        out
    }
}

/// Append the bytes of hex string `s`. False on a character that isn't a hex digit.
fn push_hex(out: &mut Vec<u8>, s: &str) -> bool {
    let digits: Vec<u8> = s.trim().bytes().collect();
    for pair in digits.chunks(2) {
        let (Some(&hi), Some(&lo)) = (pair.first(), pair.get(1)) else { return false };
        let (Some(hi), Some(lo)) = ((hi as char).to_digit(16), (lo as char).to_digit(16)) else { return false };
        out.push((hi * 16 + lo) as u8);
    }
    true
}

/// The entities the proxy graphics `bytes` draw, all with `handle` (the proxy's): selecting one
/// selects the proxy.
pub fn proxy_entities(bytes: &[u8], tables: &ProxyTables, handle: Handle) -> Vec<Entity> {
    let mut r = Decoder {
        tables,
        handle,
        pen: Common { layer: "0".into(), color: Color::ByBlock, linetype: "ByBlock".into(), lineweight: Lineweight::ByBlock, ..Common::default() },
        fill: false,
        matrices: Vec::new(),
        out: Vec::new(),
    };
    let mut at = 8;
    while let (Some(size), Some(kind)) = (u32_at(bytes, at), u32_at(bytes, at + 4)) {
        let size = size as usize;
        if size < 8 || r.out.len() >= MAX_ENTITIES {
            break;
        }
        let Some(data) = bytes.get(at + 8..at.saturating_add(size)) else { break };
        r.chunk(kind, data);
        at += size;
    }
    r.out
}

struct Decoder<'a> {
    tables: &'a ProxyTables,
    handle: Handle,
    /// Layer, colour, linetype and lineweight for the entities that follow.
    pen: Common,
    fill: bool,
    /// Model transforms pushed by PUSH_MATRIX, innermost last (row-major 3x4 of the 4x4).
    matrices: Vec<[f64; 12]>,
    out: Vec<Entity>,
}

impl Decoder<'_> {
    /// Decode one chunk of type `kind`: draw it, or change the pen for what follows.
    fn chunk(&mut self, kind: u32, data: &[u8]) {
        let mut b = Bytes { data, at: 0 };
        match kind {
            // CIRCLE: centre, radius, normal.
            2 => {
                if let (Some(c), Some(r)) = (b.point(), b.f64()) {
                    self.circle(c, r);
                }
            }
            // CIRCLE_3P: three points on it.
            3 => {
                if let (Some(p1), Some(p2), Some(p3)) = (b.point(), b.point(), b.point())
                    && let Some((c, r)) = circle_through(self.xy(p1), self.xy(p2), self.xy(p3))
                {
                    self.push(EntityKind::Circle(Circle { center: c.to3(0.0), radius: r }));
                }
            }
            // CIRCULAR_ARC: centre, radius, normal, start vector, sweep angle (around the normal).
            4 => {
                if let (Some(c), Some(r), Some(n), Some(v), Some(sweep)) = (b.point(), b.f64(), b.point(), b.point(), b.f64()) {
                    self.arc(c, r, n, v, sweep);
                }
            }
            // CIRCULAR_ARC_3P: start, a point on the arc, end.
            5 => {
                if let (Some(p1), Some(p2), Some(p3)) = (b.point(), b.point(), b.point()) {
                    self.arc_3p(self.xy(p1), self.xy(p2), self.xy(p3));
                }
            }
            // POLYLINE and POLYGON: a count, then the points.
            6 | 7 => {
                let Some(n) = b.u32() else { return };
                let n = (n as usize).min(MAX_VERTICES);
                let pts: Vec<Vec2> = (0..n).map_while(|_| b.point()).map(|p| self.xy(p)).collect();
                self.polyline(pts, kind == 7);
            }
            // TEXT: position, normal, direction, height, width factor, oblique, string.
            10 => {
                if let (Some(p), Some(_), Some(dir), Some(h), Some(w), Some(o)) = (b.point(), b.point(), b.point(), b.f64(), b.f64(), b.f64()) {
                    let s = b.string();
                    self.text(TextChunk { p, dir, height: h, width: w, oblique: o, value: s, font: String::new() });
                }
            }
            // TEXT2: position, normal, direction, string, length (the characters drawn), raw
            // flag, height, width factor, oblique, tracking, five style flags, font, big font.
            11 => {
                if let (Some(p), Some(_), Some(dir)) = (b.point(), b.point(), b.point()) {
                    let s = b.string();
                    let s = first_chars(s, b.u32());
                    b.skip(4);
                    if let (Some(h), Some(w), Some(o)) = (b.f64(), b.f64(), b.f64()) {
                        b.skip(8 + 20);
                        let font = b.string();
                        self.text(TextChunk { p, dir, height: h, width: w, oblique: o, value: s, font });
                    }
                }
            }
            // UNICODE_TEXT: as TEXT with a UTF-16 string.
            36 => {
                if let (Some(p), Some(_), Some(dir), Some(h), Some(w), Some(o)) = (b.point(), b.point(), b.point(), b.f64(), b.f64(), b.f64()) {
                    let s = b.unicode();
                    self.text(TextChunk { p, dir, height: h, width: w, oblique: o, value: s, font: String::new() });
                }
            }
            // UNICODE_TEXT2: as TEXT2 with UTF-16 strings, and bold, italic, charset, pitch and
            // the typeface before the font file.
            38 => {
                if let (Some(p), Some(_), Some(dir)) = (b.point(), b.point(), b.point()) {
                    let s = b.unicode();
                    let s = first_chars(s, b.u32());
                    b.skip(4);
                    if let (Some(h), Some(w), Some(o)) = (b.f64(), b.f64(), b.f64()) {
                        b.skip(8 + 20 + 16);
                        let _typeface = b.unicode();
                        let font = b.unicode();
                        self.text(TextChunk { p, dir, height: h, width: w, oblique: o, value: s, font });
                    }
                }
            }
            // ATTRIBUTE_COLOR: ACI, 0 ByBlock, 256 ByLayer.
            14 => {
                if let Some(c) = b.u32() {
                    self.pen.color = Color::from_aci(i16::try_from(c).unwrap_or(256));
                }
            }
            // ATTRIBUTE_LAYER: index into the layer table; an index past it leaves the layer.
            16 => {
                if let Some(name) = b.u32().and_then(|i| self.tables.layers.get(i as usize)) {
                    self.pen.layer = name.clone();
                }
            }
            // ATTRIBUTE_LINETYPE: index into the linetypes other than ByBlock and ByLayer;
            // 32766 is ByBlock, 32767 and anything past the table ByLayer.
            18 => {
                if let Some(i) = b.u32() {
                    self.pen.linetype = match i {
                        32766 => "ByBlock".into(),
                        _ => self.tables.linetypes.get(i as usize).cloned().unwrap_or_else(|| "ByLayer".into()),
                    };
                }
            }
            // ATTRIBUTE_FILL: the next polygon is filled.
            20 => self.fill = b.u32().is_some_and(|f| f != 0),
            // ATTRIBUTE_TRUE_COLOR: a colour with its method in the top byte.
            22 => {
                if let Some(v) = b.u32() {
                    self.pen.color = match v >> 24 {
                        0xC0 => Color::ByLayer,
                        0xC1 => Color::ByBlock,
                        0xC2 => Color::True(Rgb::from_u32(v & 0x00FF_FFFF)),
                        0xC3 => Color::from_aci((v & 0xFF) as i16),
                        _ => self.pen.color,
                    };
                }
            }
            // ATTRIBUTE_LINEWEIGHT: hundredths of a millimetre, or -1 ByLayer, -2 ByBlock,
            // -3 the default.
            23 => {
                if let Some(v) = b.u32() {
                    self.pen.lineweight = Lineweight::from_dxf(i16::try_from(v as i32).unwrap_or(-1));
                }
            }
            // ATTRIBUTE_LTSCALE.
            24 => {
                if let Some(s) = b.f64().filter(|s| s.is_finite() && *s > 0.0) {
                    self.pen.ltscale = s;
                }
            }
            // PUSH_MATRIX, PUSH_MATRIX2: a 4x4 model transform for what follows; POP_MATRIX.
            29 | 30 => {
                let m: Vec<f64> = (0..16).map_while(|_| b.f64()).collect();
                if let Some(m) = m.get(..12).and_then(|m| <[f64; 12]>::try_from(m).ok())
                    && m.iter().all(|x| x.is_finite())
                    && self.matrices.len() < MAX_MATRICES
                {
                    self.matrices.push(m);
                }
            }
            31 => {
                self.matrices.pop();
            }
            _ => {}
        }
    }

    /// Add an entity drawn with the current pen (up to [`MAX_ENTITIES`]).
    fn push(&mut self, kind: EntityKind) {
        if self.out.len() < MAX_ENTITIES {
            self.out.push(Entity { handle: self.handle, common: self.pen.clone(), kind });
        }
    }

    /// `p` through the pushed matrices, innermost first, in the XY plane.
    fn xy(&self, p: Vec3) -> Vec2 {
        let mut p = p;
        for m in self.matrices.iter().rev() {
            p = Vec3::new(
                m[0] * p.x + m[1] * p.y + m[2] * p.z + m[3],
                m[4] * p.x + m[5] * p.y + m[6] * p.z + m[7],
                m[8] * p.x + m[9] * p.y + m[10] * p.z + m[11],
            );
        }
        p.xy()
    }

    /// How much the pushed matrices scale lengths (1 without any).
    fn scale(&self) -> f64 {
        self.matrices.iter().map(|m| (m[0] * m[5] - m[1] * m[4]).abs().sqrt()).product()
    }

    /// A full circle around `c`, through the pushed matrices.
    fn circle(&mut self, c: Vec3, r: f64) {
        let r = r * self.scale();
        if r.is_finite() && r > 0.0 {
            let c = self.xy(c);
            self.push(EntityKind::Circle(Circle { center: c.to3(0.0), radius: r }));
        }
    }

    /// An arc around `c` from the direction `start`, turned by `sweep` around `normal`.
    fn arc(&mut self, c: Vec3, r: f64, normal: Vec3, start: Vec3, sweep: f64) {
        if !(r.is_finite() && r > 0.0 && sweep.is_finite() && sweep != 0.0) || start.xy().len() < 1e-12 {
            return;
        }
        // The start point and the end point (the start vector turned by the sweep around the
        // normal: clockwise in plan for a normal pointing down), through the matrices.
        let dir = if normal.z < 0.0 { -1.0 } else { 1.0 };
        let a0 = start.y.atan2(start.x);
        let a1 = a0 + dir * sweep;
        let on = |a: f64| c + Vec3::new(r * a.cos(), r * a.sin(), 0.0);
        let mid = on((a0 + a1) / 2.0);
        if sweep.abs() >= std::f64::consts::TAU - 1e-9 {
            return self.circle(c, r);
        }
        self.arc_3p(self.xy(on(a0)), self.xy(mid), self.xy(on(a1)));
    }

    /// The arc from `p1` through `p2` to `p3`.
    fn arc_3p(&mut self, p1: Vec2, p2: Vec2, p3: Vec2) {
        let Some((c, r)) = circle_through(p1, p2, p3) else {
            return self.polyline(vec![p1, p3], false);
        };
        let ang = |p: Vec2| (p.y - c.y).atan2(p.x - c.x);
        let (a1, a2, a3) = (ang(p1), ang(p2), ang(p3));
        let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
        // Counter-clockwise from p1 to p3 when p2 lies on that way, else from p3 to p1.
        let (start, end) = if ccw(a1, a2) <= ccw(a1, a3) { (a1, a3) } else { (a3, a1) };
        self.push(EntityKind::Arc(Arc { center: c.to3(0.0), radius: r, start, end }));
    }

    /// A polyline, or a closed polygon: filled (a solid hatch) when ATTRIBUTE_FILL asked for it,
    /// which applies to this one polygon only.
    fn polyline(&mut self, pts: Vec<Vec2>, polygon: bool) {
        let fill = std::mem::take(&mut self.fill);
        if pts.iter().any(|p| !p.is_finite()) || pts.is_empty() {
            return;
        }
        if polygon && fill && pts.len() >= 3 {
            let vertices = pts.iter().map(|&p| PolyVertex { p, bulge: 0.0, start_width: 0.0, end_width: 0.0 }).collect();
            return self.push(EntityKind::Hatch(Hatch {
                pattern: "SOLID".into(),
                solid: true,
                loops: vec![HatchLoop { vertices, outer: true, sources: Vec::new() }],
                scale: 1.0,
                angle: 0.0,
                associative: false,
                style: 0,
                elevation: 0.0,
                gradient: None,
                origin: Vec2::ZERO,
                background: None,
                pattern_lines: Vec::new(),
            }));
        }
        match pts.as_slice() {
            [a, b] if !polygon => self.push(EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) })),
            _ => {
                let vertices = pts.iter().map(|&p| PolyVertex { p, bulge: 0.0, start_width: 0.0, end_width: 0.0 }).collect();
                self.push(EntityKind::LwPolyline(LwPolyline { vertices, closed: polygon, const_width: 0.0, elevation: 0.0, plinegen: false }));
            }
        }
    }

    /// A single-line text, its style the first one whose font file the chunk names.
    fn text(&mut self, t: TextChunk) {
        let TextChunk { p, dir, height, width, oblique, value, font } = t;
        let font = font.as_str();
        let height = height * self.scale();
        if !(height.is_finite() && height > 0.0) || value.is_empty() {
            return;
        }
        let at = self.xy(p);
        let towards = self.xy(p + dir) - at;
        let rotation = if towards.len() > 1e-12 { towards.y.atan2(towards.x) } else { 0.0 };
        let stem = |f: &str| f.trim().rsplit(['/', '\\']).next().unwrap_or_default().split('.').next().unwrap_or_default().to_ascii_lowercase();
        let style = self
            .tables
            .styles
            .iter()
            .find(|(_, f)| !font.is_empty() && stem(f) == stem(font))
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "Standard".into());
        self.push(EntityKind::Text(Text {
            insert: at.to3(0.0),
            align_pt: None,
            height,
            value,
            rotation,
            width_factor: if width.is_finite() && width > 0.0 { width } else { 1.0 },
            oblique: if oblique.is_finite() { oblique } else { 0.0 },
            style,
            halign: Default::default(),
            valign: Default::default(),
        }));
    }
}

/// The first `length` characters of `s`: a text chunk may carry more text than it draws (the
/// rest of a note, drawn by the chunks after it). No length, or one past the end, keeps it all.
fn first_chars(s: String, length: Option<u32>) -> String {
    match length.map(|n| n as usize) {
        Some(n) if n > 0 && n < s.chars().count() => s.chars().take(n).collect(),
        _ => s,
    }
}

/// A text chunk: position, direction, height, width factor, oblique, string and font file.
struct TextChunk {
    p: Vec3,
    dir: Vec3,
    height: f64,
    width: f64,
    oblique: f64,
    value: String,
    font: String,
}

/// The circle through three points (centre, radius), if they aren't on one line.
fn circle_through(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if !d.is_finite() || d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
    let center =
        Vec2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d);
    let r = (a - center).len();
    (center.is_finite() && r.is_finite() && r > 0.0).then_some((center, r))
}

/// The little-endian u32 at `at`, if `bytes` holds one there.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at.checked_add(4)?).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
}

/// A reader over one chunk's data. Strings are padded to four bytes.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    /// The next `n` bytes, if there are that many.
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    /// Pass over `n` bytes (reads after the end then find nothing).
    fn skip(&mut self, n: usize) {
        self.at = self.at.saturating_add(n);
    }
    /// A little-endian u32.
    fn u32(&mut self) -> Option<u32> {
        self.take(4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
    }
    /// A little-endian double.
    fn f64(&mut self) -> Option<f64> {
        self.take(8).and_then(|b| b.try_into().ok()).map(f64::from_le_bytes)
    }
    /// Three doubles, if all are finite.
    fn point(&mut self) -> Option<Vec3> {
        let p = Vec3::new(self.f64()?, self.f64()?, self.f64()?);
        (p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).then_some(p)
    }
    /// Move on to the next multiple of four bytes, where padded strings end.
    fn align(&mut self) {
        self.at = self.at.saturating_add((4 - self.at % 4) % 4);
    }
    /// A zero-terminated string in the drawing's code page; bytes above 0x7F as Latin-1.
    fn string(&mut self) -> String {
        let rest = self.data.get(self.at..).unwrap_or_default();
        let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        let s: String = rest.iter().take(end).map(|&c| c as char).collect();
        self.at += end + 1;
        self.align();
        s
    }
    /// A UTF-16 string ended by a zero code unit.
    fn unicode(&mut self) -> String {
        let rest = self.data.get(self.at..).unwrap_or_default();
        let (pairs, _) = rest.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|&p| u16::from_le_bytes(p)).take_while(|&u| u != 0).collect();
        self.at += units.len() * 2 + 2;
        self.align();
        String::from_utf16_lossy(&units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RawTag;

    /// A chunk of type `kind`, its data padded to four bytes.
    fn chunk(kind: u32, data: &[u8]) -> Vec<u8> {
        let mut d = data.to_vec();
        d.resize(d.len().div_ceil(4) * 4, 0);
        let mut out = ((8 + d.len()) as u32).to_le_bytes().to_vec();
        out.extend(kind.to_le_bytes());
        out.extend(d);
        out
    }
    /// Proxy graphics: the header (total size, chunk count), then the chunks.
    fn graphics(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut out = ((8 + body.len()) as u32).to_le_bytes().to_vec();
        out.extend((chunks.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }
    /// A point in the XY plane as three doubles.
    fn pt(x: f64, y: f64) -> Vec<u8> {
        [x, y, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    /// Two layers, two linetypes and two text styles.
    fn tables() -> ProxyTables {
        ProxyTables {
            layers: vec!["0".into(), "BALLOONS".into()],
            linetypes: vec!["Continuous".into(), "DASHED".into()],
            styles: vec![("Standard".into(), "txt".into()), ("ISO".into(), "isocp.shx".into())],
        }
    }

    /// A balloon as AutoCAD Mechanical stores one: layer, colour, a circle, the leader and the
    /// number as TEXT2.
    #[test]
    fn a_balloon_becomes_circle_line_and_text() {
        let mut text2 = pt(173.0, 112.0);
        text2.extend(pt(0.0, 0.0).iter().take(16).chain(1.0f64.to_le_bytes().iter()));
        text2.extend(pt(1.0, 0.0));
        text2.extend(b"12\0\0");
        text2.extend(2i32.to_le_bytes());
        text2.extend(0i32.to_le_bytes());
        for v in [6.0f64, 1.0, 0.0, 0.0] {
            text2.extend(v.to_le_bytes());
        }
        text2.extend([0u8; 20]);
        text2.extend(b"ISOCP.SHX\0\0\0\0\0");
        let mut circle = pt(175.0, 115.0);
        circle.extend(6.0f64.to_le_bytes());
        circle.extend(pt(0.0, 0.0).iter().take(16).chain(1.0f64.to_le_bytes().iter()));
        let mut leader = 2u32.to_le_bytes().to_vec();
        leader.extend(pt(170.0, 111.0));
        leader.extend(pt(150.0, 90.0));
        let g = graphics(&[chunk(16, &1u32.to_le_bytes()), chunk(14, &1u32.to_le_bytes()), chunk(2, &circle), chunk(6, &leader), chunk(11, &text2)]);
        let es = proxy_entities(&g, &tables(), Handle(0x35));
        assert_eq!(es.len(), 3);
        assert!(es.iter().all(|e| e.handle == Handle(0x35) && e.common.layer == "BALLOONS" && e.common.color == Color::Index(1)));
        assert!(matches!(&es[0].kind, EntityKind::Circle(c) if c.center == Vec3::new(175.0, 115.0, 0.0) && c.radius == 6.0));
        assert!(matches!(&es[1].kind, EntityKind::Line(l) if l.b == Vec3::new(150.0, 90.0, 0.0)));
        assert!(matches!(&es[2].kind, EntityKind::Text(t) if t.value == "12" && t.height == 6.0 && t.style == "ISO" && t.rotation == 0.0));
    }

    /// A TEXT2 chunk draws only as many characters as its length says.
    #[test]
    fn text2_draws_its_length() {
        let text2 = |value: &[u8], length: i32| {
            let mut t = pt(0.0, 0.0);
            t.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
            t.extend(pt(1.0, 0.0));
            t.extend(value);
            t.extend(vec![0u8; 4 - value.len() % 4]);
            t.extend(length.to_le_bytes());
            t.extend(1i32.to_le_bytes());
            for v in [2.5f64, 1.0, 0.0, 0.0] {
                t.extend(v.to_le_bytes());
            }
            t.extend([0u8; 20]);
            t.extend(b"txt\0");
            let es = proxy_entities(&graphics(&[chunk(11, &t)]), &tables(), Handle(1));
            match es.first().map(|e| &e.kind) {
                Some(EntityKind::Text(t)) => t.value.clone(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(text2(b"two lines\\Pof text", 9), "two lines");
        assert_eq!(text2(b"whole", 0), "whole");
        assert_eq!(text2(b"whole", 99), "whole");
    }

    /// Without layer and colour chunks the picture takes the proxy's own (layer 0, ByBlock).
    #[test]
    fn unset_traits_are_the_proxys() {
        let mut leader = 3u32.to_le_bytes().to_vec();
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)] {
            leader.extend(pt(x, y));
        }
        let es = proxy_entities(
            &graphics(&[chunk(6, &leader), chunk(16, &u32::MAX.to_le_bytes()), chunk(18, &32766u32.to_le_bytes())]),
            &tables(),
            Handle(1),
        );
        let e = es.first().expect("polyline");
        assert_eq!(
            (e.common.layer.as_str(), e.common.color, e.common.linetype.as_str(), e.common.lineweight),
            ("0", Color::ByBlock, "ByBlock", Lineweight::ByBlock)
        );
        assert!(matches!(&e.kind, EntityKind::LwPolyline(p) if p.vertices.len() == 3 && !p.closed));
    }

    #[test]
    fn a_filled_polygon_is_a_solid_hatch_and_fill_applies_once() {
        let mut tri = 3u32.to_le_bytes().to_vec();
        for (x, y) in [(0.0, 0.0), (4.0, 0.0), (2.0, 3.0)] {
            tri.extend(pt(x, y));
        }
        let es = proxy_entities(&graphics(&[chunk(20, &1u32.to_le_bytes()), chunk(7, &tri), chunk(7, &tri)]), &tables(), Handle(1));
        assert!(matches!(&es[0].kind, EntityKind::Hatch(h) if h.solid && h.loops[0].vertices.len() == 3));
        assert!(matches!(&es[1].kind, EntityKind::LwPolyline(p) if p.closed));
    }

    /// A quarter arc counter-clockwise from 0°, and the same sweep around a downward normal,
    /// which runs clockwise in plan (from 0° to -90°).
    #[test]
    fn arcs_follow_their_normal() {
        let arc = |nz: f64| {
            let mut a = pt(0.0, 0.0);
            a.extend(2.0f64.to_le_bytes());
            a.extend([0.0f64, 0.0, nz].iter().flat_map(|v| v.to_le_bytes()));
            a.extend(pt(1.0, 0.0));
            a.extend(std::f64::consts::FRAC_PI_2.to_le_bytes());
            a.extend(0u32.to_le_bytes());
            match proxy_entities(&graphics(&[chunk(4, &a)]), &tables(), Handle(1)).first().map(|e| e.kind.clone()) {
                Some(EntityKind::Arc(a)) => (a.start, a.end, a.radius),
                other => panic!("{other:?}"),
            }
        };
        let (s, e, r) = arc(1.0);
        assert!(s.abs() < 1e-9 && (e - std::f64::consts::FRAC_PI_2).abs() < 1e-9 && (r - 2.0).abs() < 1e-9);
        let (s, e, _) = arc(-1.0);
        assert!((s + std::f64::consts::FRAC_PI_2).abs() < 1e-9 && e.abs() < 1e-9, "{s} {e}");
    }

    #[test]
    fn a_pushed_matrix_moves_what_follows_until_popped() {
        let mut m = [0.0f64; 16];
        (m[0], m[5], m[10], m[15], m[3], m[7]) = (2.0, 2.0, 1.0, 1.0, 100.0, 50.0);
        let mut circle = pt(1.0, 1.0);
        circle.extend(1.0f64.to_le_bytes());
        circle.extend(pt(0.0, 0.0));
        let push: Vec<u8> = m.iter().flat_map(|v| v.to_le_bytes()).collect();
        let es = proxy_entities(&graphics(&[chunk(29, &push), chunk(2, &circle), chunk(31, &[]), chunk(2, &circle)]), &tables(), Handle(1));
        assert!(matches!(&es[0].kind, EntityKind::Circle(c) if c.center == Vec3::new(102.0, 52.0, 0.0) && c.radius == 2.0));
        assert!(matches!(&es[1].kind, EntityKind::Circle(c) if c.center == Vec3::new(1.0, 1.0, 0.0) && c.radius == 1.0));
    }

    #[test]
    fn the_bytes_come_from_the_310_groups_after_the_size() {
        let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
        let u = Unknown {
            dxf_type: "ACAD_PROXY_ENTITY".into(),
            tags: vec![tag(90, "498"), tag(92, "5"), tag(310, "0102"), tag(310, "030405FF"), tag(93, "0")],
            graphics: Vec::new(),
        };
        assert_eq!(u.proxy_graphics_bytes(), [1, 2, 3, 4, 5]);
        let other = Unknown { dxf_type: "ACAD_PROXY_OBJECT".into(), ..u.clone() };
        assert!(other.proxy_graphics_bytes().is_empty());
        let bad = Unknown { tags: vec![tag(92, "4"), tag(310, "01zz")], ..u };
        assert!(bad.proxy_graphics_bytes().is_empty());
    }

    /// Truncated chunks, sizes past the end, huge counts and garbage draw what they can and
    /// never panic.
    #[test]
    fn hostile_graphics_do_not_panic() {
        let mut huge = u32::MAX.to_le_bytes().to_vec();
        huge.extend(pt(1.0, 1.0));
        let mut nan = pt(f64::NAN, 0.0);
        nan.extend(1.0f64.to_le_bytes());
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![0; 7],
            graphics(&[chunk(6, &huge)]),
            graphics(&[chunk(2, &nan)]),
            graphics(&[chunk(11, &[0xFF; 30])]),
            graphics(&[chunk(38, &[0x41; 77])]),
            graphics(&[chunk(29, &[1; 40])]),
            graphics(&[chunk(31, &[]), chunk(31, &[])]),
            [graphics(&[chunk(2, &pt(0.0, 0.0))]), vec![0xFF; 9]].concat(),
            {
                let mut g = graphics(&[]);
                g.extend(4u32.to_le_bytes());
                g.extend(2u32.to_le_bytes());
                g
            },
            {
                let mut g = graphics(&[]);
                g.extend(1_000u32.to_le_bytes());
                g.extend(2u32.to_le_bytes());
                g
            },
        ];
        for g in cases {
            let _ = proxy_entities(&g, &tables(), Handle(1));
        }
    }
}
