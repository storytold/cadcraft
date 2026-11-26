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

use super::{Arc, Circle, Common, Ellipse, Entity, EntityKind, Handle, Hatch, HatchLoop, Line, Lineweight, LwPolyline, Text, Unknown};
use crate::RawTag;
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
    /// The proxy graphics bytes of an entity: the 310 groups after its size group (92, or 160 in
    /// newer files), cut to that size. An ACAD_PROXY_ENTITY holds them in its proxy data: the size
    /// group is the first after the `AcDbProxyEntity` subclass marker (from the start in a record
    /// without one), so a 92 or 160 among the entity's own groups before it isn't taken for it.
    /// An entity of another type CADCraft keeps as read holds them in its `AcDbEntity` part when
    /// it was saved with a picture of itself (a LARGE_RADIAL_DIMENSION or an ARC_DIMENSION, or an
    /// object of another application; #3438). Empty for an entity without graphics and for one
    /// whose data isn't hex.
    pub fn proxy_graphics_bytes(&self) -> Vec<u8> {
        let Some((at, size)) = self.graphics_size_group() else { return Vec::new() };
        // Reserve what the 310 groups hold, not what the size claims: a size far past the data
        // would otherwise reserve up to MAX_BYTES for nothing.
        let held: usize = self.tags.iter().skip(at + 1).take_while(|t| t.code == 310).map(|t| t.value.trim().len() / 2).sum();
        let mut out = Vec::with_capacity(size.min(held));
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

impl Unknown {
    /// The `*D` block a dimension CADCraft keeps as read names (group 2 of its `AcDbDimension`
    /// part): a LARGE_RADIAL_DIMENSION or an ARC_DIMENSION. AutoCAD draws a dimension from that
    /// block, so CADCraft draws it from there too, and saving keeps it (#3438, #1161). `None`
    /// for other records.
    pub fn dimension_block(&self) -> Option<&str> {
        let m = self.tags.iter().position(|t| t.code == 100 && t.value.trim() == "AcDbDimension")?;
        self.tags[m + 1..].iter().take_while(|t| t.code != 100).find(|t| t.code == 2).map(|t| t.value.trim()).filter(|n| !n.is_empty())
    }

    /// Where the proxy graphics size group (92, or 160 in newer files) is, and the size it gives
    /// (at most [`MAX_BYTES`]): in an ACAD_PROXY_ENTITY the first after the `AcDbProxyEntity`
    /// marker, in another kept entity the one in its `AcDbEntity` part.
    fn graphics_size_group(&self) -> Option<(usize, usize)> {
        let at = if self.dxf_type.eq_ignore_ascii_case("ACAD_PROXY_ENTITY") {
            let from = self.tags.iter().position(|t| t.code == 100 && t.value.trim() == "AcDbProxyEntity").map_or(0, |m| m + 1);
            self.tags.iter().skip(from).position(|t| t.code == 92 || t.code == 160).map(|i| from + i)
        } else {
            // The groups of the AcDbEntity subclass, up to the entity's own subclass marker.
            self.tags.iter().position(|t| t.code == 100 && t.value.trim() == "AcDbEntity").and_then(|m| {
                let own = &self.tags[m + 1..];
                own.iter().take_while(|t| t.code != 100).position(|t| t.code == 92 || t.code == 160).map(|i| m + 1 + i)
            })
        }?;
        let size = self.tags.get(at).and_then(|t| t.value.trim().parse::<usize>().ok()).unwrap_or(0).min(MAX_BYTES);
        Some((at, size))
    }

    /// The record's groups with the layer indices of its proxy graphics (ATTRIBUTE_LAYER) turned
    /// into the written LAYER table `written`, by name; everything else as read. The name of an
    /// index is the one the record's own decoded graphics draw on after it (so a proxy copied from
    /// another drawing, or one whose layer was renamed, keeps the layer it shows); an index no
    /// drawn entity follows takes the name from the file's table `read` (see
    /// [`crate::Drawing::proxy_layers`]). A layer that is gone becomes layer 0 (the proxy's own
    /// layer, as in a block); an index past the table, or one of a record without a name, stays
    /// past the written one, which keeps the layer before it as AutoCAD does. Only the 310 groups
    /// that hold the declared size are read and rewritten. `None` when no index changes, or the
    /// graphics aren't readable.
    pub fn with_proxy_layers(&self, read: &[String], written: &[String]) -> Option<Vec<RawTag>> {
        // Only a proxy entity's own graphics: a kept dimension draws from its block instead.
        if !self.dxf_type.eq_ignore_ascii_case("ACAD_PROXY_ENTITY") {
            return None;
        }
        let (at, size) = self.graphics_size_group()?;
        let (mut bytes, mut groups) = (Vec::new(), Vec::new());
        for i in (at + 1..self.tags.len()).take_while(|&i| self.tags[i].code == 310) {
            if bytes.len() >= size {
                break;
            }
            let before = bytes.len();
            if !push_hex(&mut bytes, &self.tags[i].value) {
                return None;
            }
            groups.push((i, bytes.len() - before));
        }
        let end = size.min(bytes.len());
        // The ATTRIBUTE_LAYER chunks: where each index sits.
        let mut refs: Vec<(usize, u32)> = Vec::new();
        let mut chunk = 8usize;
        while let (Some(len), Some(kind)) = (u32_at(&bytes[..end], chunk), u32_at(&bytes[..end], chunk + 4)) {
            let len = len as usize;
            if len < 8 || chunk.saturating_add(len) > end {
                break;
            }
            if kind == 16
                && let Some(i) = u32_at(&bytes[..chunk + len], chunk + 8)
            {
                refs.push((chunk + 8, i));
            }
            chunk += len;
        }
        if refs.is_empty() {
            return None;
        }
        let indices: Vec<u32> = refs.iter().map(|r| r.1).collect();
        let learned = self.layers_by_index(&bytes[..end], &indices);
        if read.is_empty() && learned.is_empty() {
            return None;
        }
        // Each written name's first place, found once.
        let mut place: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for (p, n) in written.iter().enumerate() {
            place.entry(n.to_ascii_lowercase()).or_insert(u32::try_from(p).unwrap_or(u32::MAX));
        }
        let zero = place.get("0").copied();
        let past = u32::try_from(written.len()).unwrap_or(u32::MAX).max(u32::try_from(read.len()).unwrap_or(u32::MAX));
        let mut resolved: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut changed = false;
        for (offset, i) in refs {
            let j = *resolved.entry(i).or_insert_with(|| {
                let name = match learned.get(&i) {
                    Some(name) => name.as_deref(),
                    None => read.get(i as usize).map(String::as_str),
                };
                match name.filter(|n| !n.is_empty()) {
                    Some(name) => place.get(&name.to_ascii_lowercase()).copied().or(zero).unwrap_or(i),
                    None if i >= past => i,
                    None => past,
                }
            });
            if j != i {
                bytes[offset..offset + 4].copy_from_slice(&j.to_le_bytes());
                changed = true;
            }
        }
        if !changed {
            return None;
        }
        let mut tags = self.tags.clone();
        let mut from = 0;
        for (g, n) in groups {
            tags[g].value = bytes[from..from + n].iter().map(|b| format!("{b:02X}")).collect();
            from += n;
        }
        Some(tags)
    }

    /// The layer each ATTRIBUTE_LAYER index of `bytes` (the record's `indices`) stands for in
    /// this record's decoded graphics: the bytes decoded again with index `i` named `#i` give the
    /// same entities in the same order, so each one's marker meets its real layer. `None` for an
    /// index after which the graphics stay on the layer before (one past the table it was read
    /// with). An index above 65,535 isn't named here (it resolves through the file's table, like
    /// one no entity follows). Empty when the graphics don't match
    /// (not decoded from these bytes); an index whose entities disagree is left out.
    fn layers_by_index(&self, bytes: &[u8], indices: &[u32]) -> std::collections::HashMap<u32, Option<String>> {
        /// What the entities after an index say about it.
        enum Seen {
            /// They are on this layer, and the ones before them weren't.
            Named(String),
            /// They are on the same layer as the ones before them: the index keeps that layer,
            /// or names it.
            Kept(String),
            /// It keeps the layer before (two kept observations on different layers).
            Keeps,
        }
        const MAX: u32 = 65_535;
        let mut out: std::collections::HashMap<u32, Option<String>> = std::collections::HashMap::new();
        if self.graphics.is_empty() {
            return out;
        }
        let max = indices.iter().copied().filter(|&i| i <= MAX).max().unwrap_or(0);
        let markers = ProxyTables { layers: (0..=max).map(|i| format!("\u{1}{i}")).collect(), ..ProxyTables::default() };
        let marked = proxy_entities(bytes, &markers, Handle(0));
        if marked.len() != self.graphics.len() {
            return out;
        }
        let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        let mut seen: std::collections::HashMap<u32, Seen> = std::collections::HashMap::new();
        let mut conflict = std::collections::HashSet::new();
        let mut before: Option<(&str, &str)> = None;
        for (m, g) in marked.iter().zip(&self.graphics) {
            let (mark, real) = (m.common.layer.as_str(), g.common.layer.as_str());
            let changed_here = before.is_none_or(|(prev_mark, _)| prev_mark != mark);
            let kept = before.is_some_and(|(_, prev_real)| same(prev_real, real));
            before = Some((mark, real));
            let Some(i) = mark.strip_prefix('\u{1}').and_then(|i| i.parse::<u32>().ok()) else { continue };
            if !changed_here {
                // The same marker on another real layer: an index in between (one past the table
                // the marker decoding doesn't see as such) changed it, so this one can't be named.
                if !kept {
                    conflict.insert(i);
                }
                continue;
            }
            let real = real.to_string();
            let merged = match (seen.remove(&i), kept) {
                (None, false) => Some(Seen::Named(real)),
                (None, true) => Some(Seen::Kept(real)),
                (Some(Seen::Named(a) | Seen::Kept(a)), false) | (Some(Seen::Named(a)), true) if same(&a, &real) => Some(Seen::Named(a)),
                (Some(Seen::Kept(a)), true) => Some(if same(&a, &real) { Seen::Kept(a) } else { Seen::Keeps }),
                (Some(Seen::Keeps), true) => Some(Seen::Keeps),
                _ => None,
            };
            match merged {
                Some(s) => {
                    seen.insert(i, s);
                }
                None => {
                    conflict.insert(i);
                }
            }
        }
        for (i, s) in seen {
            if !conflict.contains(&i) {
                out.insert(i, if let Seen::Named(n) = s { Some(n) } else { None });
            }
        }
        out
    }
}

impl crate::Drawing {
    /// Rename layer `old` to `new` where proxy graphics use it: in [`crate::Drawing::proxy_layers`],
    /// so saving finds the layer under its new name, and in the decoded graphics, which draw on
    /// it at once. RENAME calls it, LAYMRG for each merged layer (its graphics move to the
    /// target, as its objects do) and LAYDEL with layer 0.
    pub fn rename_proxy_layer(&mut self, old: &str, new: &str) {
        for n in &mut self.proxy_layers {
            if n.eq_ignore_ascii_case(old) {
                *n = new.to_string();
            }
        }
        let uses = |e: &Entity| matches!(&e.kind, EntityKind::Unknown(u) if u.graphics.iter().any(|g| g.common.layer.eq_ignore_ascii_case(old)));
        let fix = |e: &mut Entity| {
            if let EntityKind::Unknown(u) = &mut e.kind {
                for g in u.graphics.iter_mut().filter(|g| g.common.layer.eq_ignore_ascii_case(old)) {
                    g.common.layer = new.to_string();
                }
            }
        };
        let mut stores: Vec<&mut crate::EntityStore> = vec![&mut self.model];
        stores.extend(self.layouts.iter_mut().map(|l| &mut l.entities));
        for b in self.blocks.values_mut() {
            if b.entities.iter().any(|e| uses(e)) {
                stores.push(&mut std::sync::Arc::make_mut(b).entities);
            }
        }
        for store in stores {
            let hs: Vec<Handle> = store.iter().filter(|e| uses(e)).map(|e| e.handle).collect();
            for h in hs {
                store.modify(h, fix);
            }
        }
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
    /// Whether polygons and circles that follow are filled (ATTRIBUTE_FILL).
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
                    && let Some(fit) = Fit3::through(p1, p2, p3)
                {
                    self.circle_in(fit.center, fit.radius, fit.e1, fit.e2);
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
                    self.arc_3p_local(p1, p2, p3);
                }
            }
            // POLYLINE and POLYGON: a count, then the points; POLYLINE_WITH_NORMAL: a count, the
            // points and a normal (the dimension lines of a LARGE_RADIAL_DIMENSION's picture).
            6 | 7 | 32 => {
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
            // ATTRIBUTE_LAYER: index into the layer table; an index past it, or one of a record
            // without a name, leaves the layer.
            16 => {
                if let Some(name) = b.u32().and_then(|i| self.tables.layers.get(i as usize)).filter(|n| !n.is_empty()) {
                    self.pen.layer = name.clone();
                }
            }
            // ATTRIBUTE_LINETYPE: index into the linetypes other than ByBlock and ByLayer;
            // 32766 is ByBlock, 32767 and anything past the table ByLayer.
            18 => {
                if let Some(i) = b.u32() {
                    self.pen.linetype = match i {
                        32766 => "ByBlock".into(),
                        _ => self.tables.linetypes.get(i as usize).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| "ByLayer".into()),
                    };
                }
            }
            // ATTRIBUTE_FILL: a trait like the colour, in force until the next one: 1 fills the
            // polygons and circles that follow, 2 (and anything else) doesn't (AcGiFillType
            // kAcGiFillAlways, kAcGiFillNever).
            20 => {
                if let Some(f) = b.u32() {
                    self.fill = f == 1;
                }
            }
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

    /// Where the pushed matrices take `c` and the unit vectors `e1`, `e2` of a circle's plane: the
    /// circle of radius 1 around `c` is drawn as `c' + u·cos a + v·sin a`. The vectors go through
    /// the matrices' linear parts only, so a small scale isn't lost against a large translation.
    fn axes(&self, c: Vec3, e1: Vec3, e2: Vec3) -> (Vec2, Vec2, Vec2) {
        (self.xy(c), self.linear(e1), self.linear(e2))
    }

    /// Direction `v` through the pushed matrices' linear parts, innermost first, in the XY plane.
    fn linear(&self, v: Vec3) -> Vec2 {
        let mut v = v;
        for m in self.matrices.iter().rev() {
            v = Vec3::new(m[0] * v.x + m[1] * v.y + m[2] * v.z, m[4] * v.x + m[5] * v.y + m[6] * v.z, m[8] * v.x + m[9] * v.y + m[10] * v.z);
        }
        v.xy()
    }

    /// A full circle around `c`, through the pushed matrices: an ellipse when they scale its
    /// plane unevenly or shear it. While ATTRIBUTE_FILL is on, a solid hatch under it: AutoCAD
    /// draws a filled shape's outline too, with its lineweight.
    fn circle(&mut self, c: Vec3, r: f64) {
        self.circle_in(c, r, X, Y);
    }

    /// The circle of radius `r` around `c` in the plane of the unit vectors `e1` and `e2`, as
    /// `circle` draws one in the chunk's XY plane.
    fn circle_in(&mut self, c: Vec3, r: f64, e1: Vec3, e2: Vec3) {
        let (at, u, v) = self.axes(c, e1, e2);
        if !(r.is_finite() && r > 0.0 && at.is_finite()) {
            return;
        }
        match round(u * r, v * r) {
            Some(r) => {
                if self.fill {
                    // Two half-circle arcs (bulge 1).
                    let side = |p: Vec2| PolyVertex { p, bulge: 1.0, start_width: 0.0, end_width: 0.0 };
                    self.solid(vec![side(at + Vec2::new(r, 0.0)), side(at - Vec2::new(r, 0.0))]);
                }
                self.push(EntityKind::Circle(Circle { center: at.to3(0.0), radius: r }));
            }
            None => {
                let Some(e) = ellipse(at, u * r, v * r, 0.0, std::f64::consts::TAU) else {
                    return self.edge_on(at, u * r, v * r, 0.0, std::f64::consts::TAU);
                };
                if self.fill {
                    // Bulges draw circular arcs only: the fill follows the ellipse in 72
                    // straight sides, finer than a plotted pixel at the sizes proxies use.
                    let side = |i: u32| {
                        let a = std::f64::consts::TAU * f64::from(i) / 72.0;
                        PolyVertex { p: at + u * (r * a.cos()) + v * (r * a.sin()), bulge: 0.0, start_width: 0.0, end_width: 0.0 }
                    };
                    self.solid((0..72).map(side).collect());
                }
                self.push(EntityKind::Ellipse(e));
            }
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
        if sweep.abs() >= std::f64::consts::TAU - 1e-9 {
            return self.circle(c, r);
        }
        self.arc_between(c, r, X, Y, a0, a1);
    }

    /// The arc of radius `r` around `c` from angle `a0` to `a1` (from `e1` towards `e2`, the unit
    /// vectors of its plane), either way round, through the pushed matrices: an elliptical arc
    /// when they don't keep it round.
    fn arc_between(&mut self, c: Vec3, r: f64, e1: Vec3, e2: Vec3, a0: f64, a1: f64) {
        let (at, u, v) = self.axes(c, e1, e2);
        let on = |a: f64| at + u * (r * a.cos()) + v * (r * a.sin());
        if round(u * r, v * r).is_some() {
            return self.arc_3p(on(a0), on((a0 + a1) / 2.0), on(a1));
        }
        match ellipse(at, u * r, v * r, a0.min(a1), a0.max(a1)) {
            Some(e) => self.push(EntityKind::Ellipse(e)),
            None => self.edge_on(at, u * r, v * r, a0.min(a1), a0.max(a1)),
        }
    }

    /// A circle or arc `c + u·cos a + v·sin a` (`a` from `a0` to `a1`) seen edge-on: `u` and `v`
    /// on one line, so it draws as the line between its two farthest points along it.
    fn edge_on(&mut self, c: Vec2, u: Vec2, v: Vec2, a0: f64, a1: f64) {
        use std::f64::consts::{PI, TAU};
        let d = if u.len() >= v.len() { u } else { v };
        if !(c.is_finite() && d.is_finite() && v.is_finite() && u.is_finite() && d.len() > 0.0) {
            return;
        }
        let d = d * (1.0 / d.len());
        // Along d the point is at R·cos(a − φ): farthest out at φ and φ + π, where the sweep has them.
        let phi = v.dot(d).atan2(u.dot(d));
        let inside = |a: f64| (a - a0).rem_euclid(TAU) <= a1 - a0;
        let on = |a: f64| c + u * a.cos() + v * a.sin();
        let mut ends = [a0, a1, phi, phi + PI].into_iter().filter(|&a| a == a0 || a == a1 || inside(a)).map(on).map(|p| (p - c).dot(d));
        let first = ends.next().unwrap_or(0.0);
        let (lo, hi) = ends.fold((first, first), |(lo, hi), t| (lo.min(t), hi.max(t)));
        if hi > lo {
            self.push(EntityKind::Line(Line { a: (c + d * lo).to3(0.0), b: (c + d * hi).to3(0.0) }));
        }
    }

    /// The arc from `p1` through `p2` to `p3`, chunk coordinates: fitted in the plane of the
    /// three points and drawn through the pushed matrices, which may make it elliptical.
    fn arc_3p_local(&mut self, p1: Vec3, p2: Vec3, p3: Vec3) {
        if self.matrices.is_empty() && p1.z == p2.z && p2.z == p3.z {
            return self.arc_3p(p1.xy(), p2.xy(), p3.xy());
        }
        let Some(fit) = Fit3::through(p1, p2, p3) else {
            return self.polyline(vec![self.xy(p1), self.xy(p3)], false);
        };
        let (a1, a2, a3) = (fit.angle(p1), fit.angle(p2), fit.angle(p3));
        let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
        // Counter-clockwise from p1 to p3 when p2 lies on that way, else from p3 to p1.
        let (from, sweep) = if ccw(a1, a2) <= ccw(a1, a3) { (a1, ccw(a1, a3)) } else { (a3, ccw(a3, a1)) };
        self.arc_between(fit.center, fit.radius, fit.e1, fit.e2, from, from + sweep);
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

    /// A polyline, or a closed polygon: on a solid hatch while ATTRIBUTE_FILL is on (the outline
    /// is drawn too, as for a circle).
    fn polyline(&mut self, pts: Vec<Vec2>, polygon: bool) {
        if pts.iter().any(|p| !p.is_finite()) || pts.is_empty() {
            return;
        }
        if polygon && self.fill && pts.len() >= 3 {
            let vertices = pts.iter().map(|&p| PolyVertex { p, bulge: 0.0, start_width: 0.0, end_width: 0.0 }).collect();
            self.solid(vertices);
        }
        match pts.as_slice() {
            [a, b] if !polygon => self.push(EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) })),
            _ => {
                let vertices = pts.iter().map(|&p| PolyVertex { p, bulge: 0.0, start_width: 0.0, end_width: 0.0 }).collect();
                self.push(EntityKind::LwPolyline(LwPolyline { vertices, closed: polygon, const_width: 0.0, elevation: 0.0, plinegen: false }));
            }
        }
    }

    /// A solid hatch with the one boundary `vertices`.
    fn solid(&mut self, vertices: Vec<PolyVertex>) {
        // At the cap the fill gives way to the outline that follows it: a shape left with only
        // its fill couldn't be picked or snapped to.
        if self.out.len() + 1 >= MAX_ENTITIES {
            return;
        }
        self.push(EntityKind::Hatch(Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops: vec![HatchLoop { vertices, outer: true, boundary_flags: None, sources: Vec::new() }],
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

    /// A single-line text, its style the first one whose font file the chunk names.
    fn text(&mut self, t: TextChunk) {
        let TextChunk { p, dir, height, width, oblique, value, font } = t;
        let font = font.as_str();
        let height = height * self.scale();
        if !(height.is_finite() && height > 0.0) || value.is_empty() {
            return;
        }
        let at = self.xy(p);
        if !at.is_finite() {
            return;
        }
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

/// The radius of the circle with conjugate radii `u` and `v` when they are one: as long as each
/// other and at right angles (a matrix that turns, mirrors and scales evenly).
fn round(u: Vec2, v: Vec2) -> Option<f64> {
    let (lu, lv) = (u.len(), v.len());
    let tol = 1e-9 * lu.max(lv);
    ((lu - lv).abs() <= tol && u.dot(v).abs() <= tol * lu.max(lv) && lu.is_finite() && lu > 0.0).then_some(lu)
}

/// The ellipse `c + u·cos a + v·sin a` for `a` from `a0` to `a1` (counter-clockwise in the
/// parameter): its axes from the conjugate radii `u` and `v`, the arc carried over to the
/// ellipse's own parameter (counter-clockwise from its major axis).
fn ellipse(c: Vec2, u: Vec2, v: Vec2, a0: f64, a1: f64) -> Option<Ellipse> {
    use std::f64::consts::{FRAC_PI_2, TAU};
    // c + u·cos a + v·sin a = c + p·cos(a − t) + q·sin(a − t): p and q at right angles.
    let t = 0.5 * (2.0 * u.dot(v)).atan2(u.len2() - v.len2());
    let p = u * t.cos() + v * t.sin();
    let q = v * t.cos() - u * t.sin();
    // The ellipse turns counter-clockwise with its parameter only when q is p turned left;
    // a mirroring matrix reverses it.
    let mirrored = p.cross(q) < 0.0;
    let q = if mirrored { -q } else { q };
    let (mut s0, mut s1) = if mirrored { (t - a1, t - a0) } else { (a0 - t, a1 - t) };
    // The major axis is the longer of p and q.
    let (major, minor) = if q.len() > p.len() {
        s0 -= FRAC_PI_2;
        s1 -= FRAC_PI_2;
        (q, p.len())
    } else {
        (p, q.len())
    };
    let ratio = minor / major.len();
    if !(c.is_finite() && major.is_finite() && major.len() > 0.0 && ratio.is_finite() && ratio > 0.0) {
        return None;
    }
    let full = (a1 - a0) >= TAU - 1e-9;
    let (start, end) = if full { (0.0, TAU) } else { (s0.rem_euclid(TAU), s1.rem_euclid(TAU)) };
    Some(Ellipse { center: c.to3(0.0), major: major.to3(0.0), ratio, start, end })
}

/// The circle through three points (centre, radius), if they aren't on one line.
fn circle_through(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    cadcraft_geom::Circle::from_3_points(a, b, c).filter(|f| f.radius > 0.0).map(|f| (f.center, f.radius))
}

const X: Vec3 = Vec3 { x: 1.0, y: 0.0, z: 0.0 };
const Y: Vec3 = Vec3 { x: 0.0, y: 1.0, z: 0.0 };

/// The circle through three points in space: its centre, radius and the unit vectors `e1`, `e2`
/// of its plane. For points of equal `z` they are X and Y, so angles are the chunk's.
struct Fit3 {
    center: Vec3,
    radius: f64,
    e1: Vec3,
    e2: Vec3,
}

impl Fit3 {
    /// The fit, if the points aren't on one line (relative to the triangle's size, so it holds at
    /// any scale).
    fn through(p1: Vec3, p2: Vec3, p3: Vec3) -> Option<Fit3> {
        let (u, v) = (p2 - p1, p3 - p1);
        let w = u.cross(v);
        let w2 = w.dot(w);
        if !(w2.is_finite() && w.len() > 1e-12 * (u.len() * v.len()) && w2 > 0.0) {
            return None;
        }
        // The centre in point-relative coordinates: ((|u|² v − |v|² u) × w) / (2 |w|²).
        let center = p1 + (v * u.dot(u) - u * v.dot(v)).cross(w) * (0.5 / w2);
        let radius = (p1 - center).len();
        // The plane's normal towards +Z (which way the arc turns comes from the middle point), and
        // X (or Y) laid into the plane.
        let up = if w.z < 0.0 { -1.0 } else { 1.0 };
        let n = w * (up / w.len());
        let along = |a: Vec3| a - n * a.dot(n);
        let e1 = if along(X).len() > 1e-6 { along(X).normalized() } else { along(Y).normalized() };
        let e2 = n.cross(e1);
        (center.is_finite() && radius.is_finite() && radius > 0.0 && e2.is_finite()).then_some(Fit3 { center, radius, e1, e2 })
    }

    /// The angle of `p` around the centre, from `e1` towards `e2`.
    fn angle(&self, p: Vec3) -> f64 {
        let d = p - self.center;
        d.dot(self.e2).atan2(d.dot(self.e1))
    }
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

    /// ATTRIBUTE_FILL is a trait like the colour: 1 (kAcGiFillAlways) fills every polygon and
    /// circle until a 2 (kAcGiFillNever) ends it, whatever is drawn in between. AutoCAD
    /// Mechanical writes 2 before its balloons' circles and 1 before their arrowheads and dots.
    #[test]
    fn fill_is_a_trait_until_turned_off() {
        let mut tri = 3u32.to_le_bytes().to_vec();
        for (x, y) in [(0.0, 0.0), (4.0, 0.0), (2.0, 3.0)] {
            tri.extend(pt(x, y));
        }
        let mut line = 2u32.to_le_bytes().to_vec();
        line.extend(pt(0.0, 0.0));
        line.extend(pt(1.0, 0.0));
        let mut dot = pt(5.0, 5.0);
        dot.extend(0.5f64.to_le_bytes());
        dot.extend(pt(0.0, 0.0));
        let (on, off) = (chunk(20, &1u32.to_le_bytes()), chunk(20, &2u32.to_le_bytes()));
        let es = proxy_entities(
            &graphics(&[on, chunk(6, &line), chunk(7, &tri), chunk(2, &dot), chunk(7, &tri), off, chunk(7, &tri), chunk(2, &dot)]),
            &tables(),
            Handle(1),
        );
        let kinds: Vec<&str> = es.iter().map(|e| e.kind.type_name()).collect();
        // A filled shape is a solid hatch with its outline on top.
        assert_eq!(kinds, ["Line", "Hatch", "Polyline", "Hatch", "Circle", "Hatch", "Polyline", "Polyline", "Circle"]);
        assert!(matches!(&es[1].kind, EntityKind::Hatch(h) if h.solid && h.loops[0].vertices.len() == 3));
        assert!(matches!(&es[2].kind, EntityKind::LwPolyline(p) if p.closed));
        // The filled circle: two half-circle arcs around its centre.
        let disc: Vec<(Vec2, f64)> = match &es[3].kind {
            EntityKind::Hatch(h) => h.loops[0].vertices.iter().map(|v| (v.p, v.bulge)).collect(),
            other => panic!("{other:?}"),
        };
        assert_eq!(disc, [(Vec2::new(5.5, 5.0), 1.0), (Vec2::new(4.5, 5.0), 1.0)]);
        assert!(matches!(&es[6].kind, EntityKind::LwPolyline(p) if p.closed));
    }

    /// A circle or arc under a matrix that scales its plane unevenly is an ellipse, measured
    /// and snapped as one; one that scales evenly stays a circle or arc.
    #[test]
    fn circles_and_arcs_under_an_uneven_matrix_are_ellipses() {
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        let push = |sx: f64, sy: f64| {
            let mut m = [0.0f64; 16];
            (m[0], m[5], m[10], m[15], m[3]) = (sx, sy, 1.0, 1.0, 10.0);
            chunk(29, &m.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())
        };
        let mut circle = pt(0.0, 0.0);
        circle.extend(1.0f64.to_le_bytes());
        circle.extend(pt(0.0, 0.0));
        let quarter = {
            let mut a = pt(0.0, 0.0);
            a.extend(1.0f64.to_le_bytes());
            a.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
            a.extend(pt(1.0, 0.0));
            a.extend(FRAC_PI_2.to_le_bytes());
            a.extend(0u32.to_le_bytes());
            a
        };
        // Three points of the same quarter arc, as CIRCULAR_ARC_3P gives them.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let quarter_3p: Vec<u8> = [pt(1.0, 0.0), pt(h, h), pt(0.0, 1.0)].concat();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let ell = |e: &Entity| match &e.kind {
            EntityKind::Ellipse(e) => (e.center, e.major, e.ratio, e.start, e.end),
            other => panic!("{other:?}"),
        };
        let es = proxy_entities(&graphics(&[push(2.0, 1.0), chunk(2, &circle), chunk(4, &quarter), chunk(5, &quarter_3p)]), &tables(), Handle(1));
        let (c, major, ratio, start, end) = ell(&es[0]);
        assert!(c == Vec3::new(10.0, 0.0, 0.0) && near(major.x.abs(), 2.0) && near(major.y, 0.0) && near(ratio, 0.5) && (start, end) == (0.0, TAU));
        for e in &es[1..] {
            let (_, major, ratio, start, end) = ell(e);
            // The major axis may point either way along x; the arc runs from (12, 0) to (10, 1).
            let at = |t: f64| Vec2::new(10.0, 0.0) + major.xy() * t.cos() + major.xy().perp() * (ratio * t.sin());
            assert!((at(start) - Vec2::new(12.0, 0.0)).len() < 1e-9 && (at(end) - Vec2::new(10.0, 1.0)).len() < 1e-9, "{start} {end}");
            assert!(near((end - start).rem_euclid(TAU), FRAC_PI_2));
        }
        // Mirrored along x and stretched: the quarter arc runs from (10, 1) to (8, 0).
        let es = proxy_entities(&graphics(&[push(-2.0, 1.0), chunk(4, &quarter)]), &tables(), Handle(1));
        let (_, major, ratio, start, end) = ell(&es[0]);
        let at = |t: f64| Vec2::new(10.0, 0.0) + major.xy() * t.cos() + major.xy().perp() * (ratio * t.sin());
        assert!((at(start) - Vec2::new(10.0, 1.0)).len() < 1e-9 && (at(end) - Vec2::new(8.0, 0.0)).len() < 1e-9, "{start} {end}");
        // A filled circle under it: a solid hatch along the ellipse, under the ellipse.
        let fill = chunk(20, &1u32.to_le_bytes());
        let es = proxy_entities(&graphics(&[push(2.0, 1.0), fill, chunk(2, &circle)]), &tables(), Handle(1));
        match (&es[0].kind, &es[1].kind) {
            (EntityKind::Hatch(h), EntityKind::Ellipse(_)) => {
                let xs = h.loops[0].vertices.iter().map(|v| v.p.x);
                let (lo, hi) = xs.fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
                assert!(near(lo, 8.0) && near(hi, 12.0), "{lo} {hi}");
            }
            other => panic!("{other:?}"),
        }
        // Picked and snapped by the ellipse only, not the hatch's sides.
        let u = Unknown { dxf_type: "ACAD_PROXY_ENTITY".into(), tags: Vec::new(), graphics: es };
        let prims = EntityKind::Unknown(u).prims();
        assert!(matches!(prims.as_slice(), [crate::entity::Prim::Ellipse(_)]), "{prims:?}");
        // A tiny scale under a far translation keeps the circle: radius 1e8 at scale 1e-8 is 1.
        let mut far = [0.0f64; 16];
        (far[0], far[5], far[10], far[15], far[3], far[7]) = (1e-8, 1e-8, 1.0, 1.0, 1e9, 1e9);
        let far = chunk(29, &far.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        let mut big = pt(0.0, 0.0);
        big.extend(1e8f64.to_le_bytes());
        big.extend(pt(0.0, 0.0));
        let es = proxy_entities(&graphics(&[far, chunk(2, &big)]), &tables(), Handle(1));
        assert!(matches!(&es[..], [e] if matches!(&e.kind, EntityKind::Circle(c) if (c.radius - 1.0).abs() < 1e-6)), "{es:?}");
        // Even scaling, mirrored or not, keeps circles and arcs.
        let es = proxy_entities(&graphics(&[push(-3.0, 3.0), chunk(2, &circle), chunk(4, &quarter), chunk(5, &quarter_3p)]), &tables(), Handle(1));
        assert!(matches!(&es[0].kind, EntityKind::Circle(c) if near(c.radius, 3.0)));
        for e in &es[1..] {
            let angle = |a: f64, b: f64| near((a - b).rem_euclid(TAU).min((b - a).rem_euclid(TAU)), 0.0);
            assert!(matches!(&e.kind, EntityKind::Arc(a) if near(a.radius, 3.0) && angle(a.start, FRAC_PI_2) && angle(a.end, PI)), "{e:?}");
        }
    }

    /// Three-point chunks are fitted in the plane of their points, then mapped: a circle across XY
    /// turned into it, points far smaller than the scale that enlarges them, and circles and arcs
    /// seen edge-on.
    #[test]
    fn three_point_chunks_fit_in_their_own_plane() {
        use std::f64::consts::TAU;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let p3 = |p: [f64; 3]| p.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let mat = |m: [f64; 16]| chunk(29, &m.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        let turn = mat([0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let across = [p3([0.0, 1.0, 0.0]), p3([0.0, 0.0, 1.0]), p3([0.0, -1.0, 0.0])].concat();
        let tiny = mat([1e7, 0.0, 0.0, 0.0, 0.0, 1e7, 0.0, 0.0, 0.0, 0.0, 1e7, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let small = [p3([1e-7, 0.0, 0.0]), p3([0.0, 1e-7, 0.0]), p3([-1e-7, 0.0, 0.0])].concat();
        // The arc's half is the one through the middle point.
        for (m, pts, mid) in [(turn, across.clone(), Vec2::new(1.0, 0.0)), (tiny, small, Vec2::new(0.0, 1.0))] {
            let es = proxy_entities(&graphics(&[m, chunk(3, &pts), chunk(5, &pts)]), &tables(), Handle(1));
            assert!(matches!(&es[0].kind, EntityKind::Circle(c) if c.center.xy().len() < 1e-9 && near(c.radius, 1.0)), "{es:?}");
            match &es[1].kind {
                EntityKind::Arc(a) => {
                    let m = a.start + (a.end - a.start).rem_euclid(TAU) / 2.0;
                    assert!(near(a.radius, 1.0) && (Vec2::new(m.cos(), m.sin()) - mid).len() < 1e-9, "{a:?}");
                }
                other => panic!("{other:?}"),
            }
        }
        // Seen edge-on (in a vertical plane, no matrix) a circle or arc is a line over all it
        // covers in plan: for the arc through (0, 1) from (0, 0.6) to (0, 0.6), beyond its ends.
        let edge = |pts: Vec<u8>, kind: u32| match proxy_entities(&graphics(&[chunk(kind, &pts)]), &tables(), Handle(1)).as_slice() {
            [Entity { kind: EntityKind::Line(l), .. }] => {
                let (lo, hi) = (l.a.y.min(l.b.y), l.a.y.max(l.b.y));
                assert!(near(l.a.x, 0.0) && near(l.b.x, 0.0), "{l:?}");
                (lo, hi)
            }
            other => panic!("{other:?}"),
        };
        let (lo, hi) = edge(across.clone(), 3);
        assert!(near(lo, -1.0) && near(hi, 1.0));
        let (lo, hi) = edge(across, 5);
        assert!(near(lo, -1.0) && near(hi, 1.0));
        let (lo, hi) = edge([p3([0.0, 0.6, 0.8]), p3([0.0, 1.0, 0.0]), p3([0.0, 0.6, -0.8])].concat(), 5);
        assert!(near(lo, 0.6) && near(hi, 1.0), "{lo} {hi}");
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

    /// A finite matrix can still move a point past the largest float: a circle or a text that
    /// would land there is left out, as a polyline is, so no infinity reaches the extents.
    #[test]
    fn what_a_matrix_moves_out_of_range_is_left_out() {
        let mut m = [0.0f64; 16];
        (m[0], m[5], m[10], m[15], m[3]) = (1.0, 1.0, 1.0, 1.0, 1.7e308);
        let push: Vec<u8> = m.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut circle = pt(1e308, 0.0);
        circle.extend(1.0f64.to_le_bytes());
        circle.extend(pt(0.0, 0.0));
        let mut text = pt(1e308, 0.0);
        text.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
        text.extend(pt(1.0, 0.0));
        text.extend(b"far\0");
        text.extend(0i32.to_le_bytes());
        text.extend(1i32.to_le_bytes());
        for v in [2.5f64, 1.0, 0.0, 0.0] {
            text.extend(v.to_le_bytes());
        }
        text.extend([0u8; 20]);
        text.extend(b"txt\0");
        let es = proxy_entities(&graphics(&[chunk(29, &push), chunk(2, &circle), chunk(11, &text)]), &tables(), Handle(1));
        assert!(es.is_empty(), "{es:?}");
        // The same text without the matrix is drawn: the chunk itself is sound.
        assert_eq!(proxy_entities(&graphics(&[chunk(11, &text)]), &tables(), Handle(1)).len(), 1);
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
        let bad = Unknown { tags: vec![tag(92, "4"), tag(310, "01zz")], ..u.clone() };
        assert!(bad.proxy_graphics_bytes().is_empty());
        // The size group is the first after the AcDbProxyEntity marker: a 160 or 92 before it
        // belongs to the entity's common data.
        let marked = Unknown {
            tags: vec![
                tag(100, "AcDbEntity"),
                tag(160, "2"),
                tag(310, "EEEE"),
                tag(100, "AcDbProxyEntity"),
                tag(90, "498"),
                tag(92, "3"),
                tag(310, "010203"),
            ],
            ..u.clone()
        };
        assert_eq!(marked.proxy_graphics_bytes(), [1, 2, 3]);
        // A size far past the data gives the data there is.
        let short = Unknown { tags: vec![tag(92, "16000000"), tag(310, "0102")], ..u };
        assert_eq!(short.proxy_graphics_bytes(), [1, 2]);
    }

    /// Another entity saved with a picture of itself (a LARGE_RADIAL_DIMENSION) holds it in its
    /// AcDbEntity part; a 92 among its own groups after that isn't taken for it. Its dimension
    /// lines are POLYLINE_WITH_NORMAL chunks (type 32).
    #[test]
    fn a_kept_entity_draws_the_picture_in_its_acdbentity_part() {
        let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
        let mut line = 2u32.to_le_bytes().to_vec();
        line.extend(pt(250.0, 150.0));
        line.extend(pt(290.0, 190.0));
        line.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
        let g = graphics(&[chunk(32, &line)]);
        let hex: String = g.iter().map(|b| format!("{b:02X}")).collect();
        let size = g.len().to_string();
        let u = Unknown {
            dxf_type: "LARGE_RADIAL_DIMENSION".into(),
            tags: vec![
                tag(100, "AcDbEntity"),
                tag(8, "0"),
                tag(92, &size),
                tag(310, &hex),
                tag(100, "AcDbDimension"),
                tag(2, "*D1"),
                tag(100, "AcDbRadialDimensionLarge"),
                tag(40, "0.785398"),
            ],
            graphics: Vec::new(),
        };
        assert_eq!(u.proxy_graphics_bytes(), g);
        let es = proxy_entities(&u.proxy_graphics_bytes(), &tables(), Handle(1));
        assert!(
            matches!(&es[..], [e] if matches!(&e.kind, EntityKind::Line(l) if l.a == Vec3::new(250.0, 150.0, 0.0) && l.b == Vec3::new(290.0, 190.0, 0.0)))
        );
        // Without graphics in the AcDbEntity part, a 92 in the entity's own data is not one.
        let own = Unknown { tags: vec![tag(100, "AcDbEntity"), tag(8, "0"), tag(100, "AcDbSomething"), tag(92, "4"), tag(310, "01020304")], ..u };
        assert!(own.proxy_graphics_bytes().is_empty());
    }

    /// Saving renames the layer indices by name into the written table, reading only the 310
    /// groups the declared size needs: a damaged group after them doesn't stop it.
    #[test]
    fn layer_indices_follow_the_written_table() {
        let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
        let mut circle = pt(0.0, 0.0);
        circle.extend(1.0f64.to_le_bytes());
        circle.extend(pt(0.0, 0.0));
        let g = graphics(&[chunk(16, &2u32.to_le_bytes()), chunk(2, &circle)]);
        let hex: String = g.iter().map(|b| format!("{b:02X}")).collect();
        let read: Vec<String> = ["0", "A", "B", "C"].map(String::from).to_vec();
        let tables = ProxyTables { layers: read.clone(), ..ProxyTables::default() };
        let u = Unknown {
            dxf_type: "ACAD_PROXY_ENTITY".into(),
            tags: vec![tag(100, "AcDbProxyEntity"), tag(92, &g.len().to_string()), tag(310, &hex), tag(310, "GG")],
            graphics: proxy_entities(&g, &tables, Handle(1)),
        };
        assert_eq!(u.graphics[0].common.layer, "B");
        let written: Vec<String> = ["0", "B", "C"].map(String::from).to_vec();
        let tags = u.with_proxy_layers(&read, &written).expect("remapped");
        let again = Unknown { tags, ..u.clone() };
        assert_eq!(
            proxy_entities(&again.proxy_graphics_bytes(), &ProxyTables { layers: written, ..ProxyTables::default() }, Handle(1))[0].common.layer,
            "B"
        );
        assert_eq!(again.tags[3].value, "GG", "the group after the graphics is kept as read");
    }

    /// An index whose graphics stay on the layer before (99 here, past the table) doesn't make
    /// the index before it (3) look contradictory when that one comes again; an index above
    /// 65,535 isn't named (it resolves through the file's table), and the others still are.
    #[test]
    fn indices_are_named_around_kept_layers_and_huge_indices() {
        let mut circle = pt(0.0, 0.0);
        circle.extend(1.0f64.to_le_bytes());
        circle.extend(pt(0.0, 0.0));
        let read: Vec<String> = ["0", "A", "C", "B"].map(String::from).to_vec();
        let tables = ProxyTables { layers: read, ..ProxyTables::default() };
        let named = |indices: &[u32]| {
            let mut chunks = Vec::new();
            for &i in indices {
                chunks.push(chunk(16, &i.to_le_bytes()));
                chunks.push(chunk(2, &circle));
            }
            let g = graphics(&chunks);
            let u = Unknown { dxf_type: "ACAD_PROXY_ENTITY".into(), tags: Vec::new(), graphics: proxy_entities(&g, &tables, Handle(1)) };
            let mut learned: Vec<(u32, Option<String>)> = u.layers_by_index(&g, indices).into_iter().collect();
            learned.sort();
            learned
        };
        assert_eq!(named(&[0, 3, 99, 3]), [(0, Some("0".into())), (3, Some("B".into())), (99, None)]);
        assert_eq!(named(&[2, 70_000]), [(2, Some("C".into()))]);
        // Layer 4 (past the table, so it keeps layer 0), a circle, then layers 1 and 4 at once (4
        // keeps A): both circles follow index 4 in the marker decoding, on different layers.
        let g = graphics(&[
            chunk(16, &4u32.to_le_bytes()),
            chunk(2, &circle),
            chunk(16, &1u32.to_le_bytes()),
            chunk(16, &4u32.to_le_bytes()),
            chunk(2, &circle),
        ]);
        let tag = |code: i32, value: &str| RawTag { code, value: value.into() };
        let hex: String = g.iter().map(|b| format!("{b:02X}")).collect();
        let u = Unknown {
            dxf_type: "ACAD_PROXY_ENTITY".into(),
            tags: vec![tag(100, "AcDbProxyEntity"), tag(92, &g.len().to_string()), tag(310, &hex)],
            graphics: proxy_entities(&g, &tables, Handle(1)),
        };
        assert_eq!(u.graphics.iter().map(|e| e.common.layer.as_str()).collect::<Vec<_>>(), ["0", "A"]);
        assert!(!u.layers_by_index(&g, &[4, 1, 4]).contains_key(&4), "index 4 isn't named");
        // Saved with the same table, nothing changes, so both circles stay where they are.
        assert!(u.with_proxy_layers(&tables.layers, &tables.layers).is_none());
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
