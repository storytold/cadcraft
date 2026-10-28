//! SVG → Drawing: the static W3C SVG subset that drawing tools emit.
//!
//! Reads shapes (`rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`), `path`
//! outlines (`M L H V C S Q T A Z`, absolute and relative), `g` grouping with `transform`
//! attributes, `viewBox` and unit-bearing `width`/`height`, and Inkscape layers
//! (`inkscape:groupmode="layer"`). Geometry becomes centre-line entities in millimetres with
//! the page's top-left corner at the origin and Y pointing up — where a laser or pen job
//! wants it.
//!
//! Not read (each is a follow-up): CSS class rules (`<style>`), `use` references, fills,
//! gradients and hatch patterns, text, embedded raster images, rounded rectangle corners
//! (`rx`/`ry`), and elliptical arcs under non-uniform transforms (those are flattened into
//! line segments instead of losing the shape). What the drawing shows of those (all but fills,
//! gradients and patterns) is named in a read note, so it is never left out silently.

use std::collections::{BTreeMap, HashSet};

use cadcraft_color::Color;
use cadcraft_doc::{Circle, Common, Drawing, Ellipse, EntityKind, Layer, Line, LwPolyline, Space};
use cadcraft_geom::{Mat3, PolyVertex, Spline, Vec2, Vec3};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::{IoError, Result};

/// A decompressed SVGZ payload larger than this is refused (decompression bomb guard).
const MAX_DECOMPRESSED: usize = 64 << 20;
/// Shape elements per file (rect, circle, path, …; each counted once).
const MAX_ELEMENTS: usize = 500_000;
/// Path data segments per `d` attribute.
const MAX_PATH_SEGMENTS: usize = 100_000;
/// Vertices in one polyline, and control points in one spline.
const MAX_CURVE_POINTS: usize = 100_001;
/// Layers created from Inkscape layer groups.
const MAX_LAYERS: usize = 256;
/// Nested `<g>`/`<svg>` transforms.
const MAX_TRANSFORM_DEPTH: usize = 64;
/// Flattened chords per elliptical arc under a non-uniform transform.
const MAX_ARC_CHORDS: usize = 10_000;
/// Generated vertices retained across the whole document (all paths and shapes), so repeated
/// arc/curve flattening cannot amplify a compact input into unbounded geometry.
const MAX_TOTAL_VERTS: usize = 8_000_000;

/// Elements whose whole subtree is skipped (paint servers, text, raster images, metadata).
const SKIP: &[&[u8]] = &[
    b"defs",
    b"symbol",
    b"clipPath",
    b"mask",
    b"pattern",
    b"marker",
    b"linearGradient",
    b"radialGradient",
    b"filter",
    b"style",
    b"text",
    b"image",
    b"title",
    b"desc",
    b"metadata",
];

/// Elements that hold children and push a transform stack frame.
const CONTAINERS: &[&[u8]] = &[b"g", b"svg", b"a", b"switch"];

/// Read an SVG document, or a gzip-compressed one (`.svgz`, Inkscape's "Compressed SVG").
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    let unpacked;
    let plain: &[u8] = if bytes.starts_with(&[0x1f, 0x8b]) {
        unpacked = gunzip(bytes)?;
        &unpacked
    } else {
        bytes
    };
    let text = std::str::from_utf8(plain).map_err(|_| IoError::Format("SVG files must be UTF-8".into()))?;
    let mut parser = Parser::new();
    parser.parse(text)?;
    Ok(parser.drawing)
}

/// Decompress a gzip member (RFC 1952) with a hard output cap.
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let damaged = || IoError::Format("this SVGZ file's gzip stream is damaged".into());
    if bytes.get(2) != Some(&8) {
        return Err(damaged());
    }
    let flg = bytes.get(3).copied().unwrap_or(0);
    let mut pos = 10usize;
    if flg & 0x04 != 0 {
        // FEXTRA
        let extra = bytes.get(pos..pos + 2).ok_or_else(damaged)?;
        pos += 2 + usize::from(u16::from_le_bytes([extra[0], extra[1]]));
    }
    if flg & 0x08 != 0 {
        pos = skip_cstring(bytes, pos).ok_or_else(damaged)?; // FNAME
    }
    if flg & 0x10 != 0 {
        pos = skip_cstring(bytes, pos).ok_or_else(damaged)?; // FCOMMENT
    }
    if flg & 0x02 != 0 {
        pos = pos.saturating_add(2); // FHCRC
    }
    let end = bytes.len().checked_sub(8).filter(|e| *e >= pos).ok_or_else(damaged)?;
    miniz_oxide::inflate::decompress_to_vec_with_limit(&bytes[pos..end], MAX_DECOMPRESSED).map_err(|_| damaged())
}

/// Skip a NUL-terminated field; `None` when the terminator is missing.
fn skip_cstring(bytes: &[u8], from: usize) -> Option<usize> {
    let rest = bytes.get(from..)?;
    let nul = rest.iter().position(|b| *b == 0)?;
    Some(from + nul + 1)
}

fn parse_err<E: std::fmt::Display>(e: E) -> IoError {
    IoError::Format(format!("SVG: {e}"))
}

fn v3(p: Vec2) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0)
}

struct Parser {
    drawing: Drawing,
    /// Root viewBox/unit mapping composed with every open container transform.
    stack: Vec<Mat3>,
    /// Current Inkscape layer name.
    layer: String,
    layers: HashSet<String>,
    layer_color: u8,
    elements: usize,
    /// Generated vertices still allowed across the whole document. Arc/curve flattening can turn
    /// a compact path into many samples, and each flushed run is retained, so without a shared
    /// budget repeated arcs amplify a small input into gigabytes of geometry (CWE-400).
    verts_left: usize,
    /// > 0 while inside a skipped subtree; counts the starts seen there.
    skip: usize,
    /// What was left out, by kind ([`LeftOut`]).
    left_out: BTreeMap<LeftOut, usize>,
}

/// Drawn content the reader leaves out, named in the drawing's read note.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LeftOut {
    Text,
    Image,
    Use,
    Style,
    RoundedCorners,
}

impl LeftOut {
    /// The kind of a skipped element ([`SKIP`]) that the drawing shows.
    fn of(name: &[u8]) -> Option<Self> {
        match name {
            b"text" => Some(Self::Text),
            b"image" => Some(Self::Image),
            _ => None,
        }
    }

    fn describe(self, n: usize) -> String {
        let (one, many) = match self {
            Self::Text => ("text", "texts"),
            Self::Image => ("raster image", "raster images"),
            Self::Use => ("`<use>` copy", "`<use>` copies"),
            Self::Style => ("`<style>` sheet (its class rules aren't applied)", "`<style>` sheets (their class rules aren't applied)"),
            Self::RoundedCorners => ("rectangle with rounded corners (drawn square)", "rectangles with rounded corners (drawn square)"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }
}

impl Parser {
    /// Initialize a metric SVG drawing with one document-wide generated-vertex allowance.
    fn new() -> Self {
        Parser {
            drawing: Drawing::new_metric(),
            stack: vec![Mat3::IDENTITY],
            layer: "0".into(),
            layers: HashSet::from(["0".to_string()]),
            layer_color: 1,
            elements: 0,
            verts_left: MAX_TOTAL_VERTS,
            skip: 0,
            left_out: BTreeMap::new(),
        }
    }

    fn left_out(&mut self, kind: LeftOut) {
        *self.left_out.entry(kind).or_default() += 1;
    }

    /// The read note on what was left out, if anything was.
    fn note(&self) -> Option<String> {
        if self.left_out.is_empty() {
            return None;
        }
        let list: Vec<String> = self.left_out.iter().map(|(k, n)| k.describe(*n)).collect();
        Some(format!("SVG: CADCraft doesn't read these yet, so they were left out: {}", list.join(", ")))
    }

    fn parse(&mut self, text: &str) -> Result<()> {
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(true);
        loop {
            match reader.read_event().map_err(parse_err)? {
                Event::Start(e) => {
                    let name = e.name();
                    self.on_start(name.into_inner().as_bytes(), &e)?;
                }
                Event::Empty(e) => {
                    let name = e.name();
                    self.on_element(name.into_inner().as_bytes(), &e)?;
                }
                Event::End(e) => {
                    let name = e.name();
                    self.on_end(name.into_inner().as_bytes());
                }
                Event::Eof => break,
                _ => {}
            }
        }
        self.drawing.read_notes.extend(self.note());
        Ok(())
    }

    /// A start tag: open a transform frame, descend into a skipped subtree, or treat a
    /// shape written as start+end the same as a self-closed one.
    fn on_start(&mut self, name: &[u8], e: &BytesStart) -> Result<()> {
        // A style sheet applies wherever it is (Inkscape keeps it in `<defs>`).
        if name == b"style" {
            self.left_out(LeftOut::Style);
        }
        if self.skip > 0 {
            self.skip += 1;
            return Ok(());
        }
        if hidden(e) {
            self.skip = 1;
            return Ok(());
        }
        if SKIP.contains(&name) {
            if let Some(kind) = LeftOut::of(name) {
                self.left_out(kind);
            }
            self.skip = 1;
            return Ok(());
        }
        if CONTAINERS.contains(&name) { self.enter_group(name, e) } else { self.on_element(name, e) }
    }

    fn on_element(&mut self, name: &[u8], e: &BytesStart) -> Result<()> {
        if self.skip > 0 {
            return Ok(());
        }
        if SKIP.contains(&name) || hidden(e) {
            if !hidden(e)
                && let Some(kind) = LeftOut::of(name)
            {
                self.left_out(kind);
            }
            return Ok(());
        }
        match name {
            b"use" => {
                self.left_out(LeftOut::Use);
                Ok(())
            }
            b"rect" => self.rect(e),
            b"circle" => self.circle(e),
            b"ellipse" => self.ellipse(e),
            b"line" => self.line(e),
            b"polyline" => self.poly_points(e, false),
            b"polygon" => self.poly_points(e, true),
            b"path" => self.path(e),
            _ => Ok(()),
        }
    }

    fn on_end(&mut self, name: &[u8]) {
        if self.skip > 0 {
            self.skip -= 1;
        } else if CONTAINERS.contains(&name) && self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    fn enter_group(&mut self, name: &[u8], e: &BytesStart) -> Result<()> {
        if name == b"svg" && self.stack.len() == 1 {
            self.stack[0] = viewport(e)?;
        }
        if name == b"g" && attr(e, b"inkscape:groupmode").as_deref() == Some("layer") {
            let raw = attr(e, b"inkscape:label").or_else(|| attr(e, b"id"));
            if let Some(raw) = raw {
                self.set_layer(&raw);
            }
        }
        if self.stack.len() >= MAX_TRANSFORM_DEPTH {
            return Err(IoError::Format(format!("SVG: more than {MAX_TRANSFORM_DEPTH} nested groups")));
        }
        let current = *self.stack.last().unwrap_or(&Mat3::IDENTITY);
        let next = match attr(e, b"transform").as_deref().and_then(parse_transform) {
            Some(local) => current.then_before(local),
            None => current,
        };
        self.stack.push(next);
        Ok(())
    }

    fn set_layer(&mut self, raw: &str) {
        let name = sanitize_layer(raw);
        if !self.layers.contains(&name) {
            if self.layers.len() >= MAX_LAYERS {
                return;
            }
            let color = Color::Index(self.layer_color);
            self.layer_color = if self.layer_color >= 7 { 1 } else { self.layer_color + 1 };
            self.drawing.layers.push(Layer { name: name.clone(), color, ..Layer::default() });
            self.layers.insert(name.clone());
        }
        self.layer = name;
    }

    /// The composed mapping for the current nesting: SVG user units → drawing units (mm).
    fn cur(&self) -> Mat3 {
        *self.stack.last().unwrap_or(&Mat3::IDENTITY)
    }

    fn map(&self, p: Vec2) -> Vec2 {
        self.cur().apply(p)
    }

    fn map_vec(&self, v: Vec2) -> Vec2 {
        self.cur().apply_vec(v)
    }

    /// Charge an entity against element and retained-vertex limits before adding it to model space.
    fn add(&mut self, kind: EntityKind) -> Result<()> {
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(IoError::Format(format!("SVG: more than {MAX_ELEMENTS} elements")));
        }
        // Charge the vertices this entity retains against the shared document budget, so repeated
        // flattened arcs/curves cannot amplify a compact path into unbounded geometry (CWE-400).
        let verts = match &kind {
            EntityKind::LwPolyline(p) => p.vertices.len(),
            EntityKind::Spline(s) => s.control.len(),
            _ => 1,
        };
        self.verts_left = self.verts_left.checked_sub(verts).ok_or_else(|| IoError::Format("SVG: too much generated geometry".into()))?;
        let common = Common { layer: self.layer.clone(), ..Common::default() };
        self.drawing.add(&Space::Model, common, kind).map(|_| ()).map_err(parse_err)
    }

    // --- shapes -------------------------------------------------------------

    fn rect(&mut self, e: &BytesStart) -> Result<()> {
        let x = num(e, b"x", 0.0);
        let y = num(e, b"y", 0.0);
        let w = num(e, b"width", 0.0);
        let h = num(e, b"height", 0.0);
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        if num(e, b"rx", 0.0) > 0.0 || num(e, b"ry", 0.0) > 0.0 {
            self.left_out(LeftOut::RoundedCorners);
        }
        let pts = [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)];
        self.add_polyline(&pts.map(|p| self.map(p)), true)
    }

    fn circle(&mut self, e: &BytesStart) -> Result<()> {
        let r = num(e, b"r", 0.0);
        if r <= 0.0 {
            return Ok(());
        }
        self.add_rounded(Vec2::new(num(e, b"cx", 0.0), num(e, b"cy", 0.0)), r, r)
    }

    fn ellipse(&mut self, e: &BytesStart) -> Result<()> {
        let rx = num(e, b"rx", 0.0);
        let ry = num(e, b"ry", 0.0);
        if rx <= 0.0 || ry <= 0.0 {
            return Ok(());
        }
        self.add_rounded(Vec2::new(num(e, b"cx", 0.0), num(e, b"cy", 0.0)), rx, ry)
    }

    /// A circle or ellipse through a transform. A circle stays a `CIRCLE` under rotations,
    /// uniform scales and reflections; anything else becomes an `ELLIPSE` holding the axes
    /// of the transformed shape — a linear map sends the unit circle to an ellipse whose
    /// semi-axes are the square roots of the eigenvalues of `A·Aᵀ`.
    fn add_rounded(&mut self, center_user: Vec2, rx: f64, ry: f64) -> Result<()> {
        let center = self.map(center_user);
        let bx = self.map_vec(Vec2::new(rx, 0.0));
        let by = self.map_vec(Vec2::new(0.0, ry));
        let len = |v: Vec2| v.x.hypot(v.y);
        let (lbx, lby, dot) = (len(bx), len(by), bx.x * by.x + bx.y * by.y);
        if lbx.is_finite() && lby.is_finite() && (lbx - lby).abs() <= 1e-9 * (lbx + lby) && dot.abs() <= 1e-9 * lbx * lby {
            return self.add(EntityKind::Circle(Circle { center: v3(center), radius: lbx }));
        }
        let xx = bx.x * bx.x + bx.y * bx.y;
        let yy = by.x * by.x + by.y * by.y;
        let xy = bx.x * by.x + bx.y * by.y;
        let trace = xx + yy;
        let disc = (trace * trace - 4.0 * (xx * yy - xy * xy)).max(0.0).sqrt();
        let l_max = 0.5 * (trace + disc);
        let l_min = 0.5 * (trace - disc);
        let s_max = l_max.max(0.0).sqrt();
        if s_max <= f64::EPSILON {
            return Ok(());
        }
        let axis = if xy.abs() <= 1e-12 * trace.max(1.0) {
            if yy > xx { Vec2::new(0.0, 1.0) } else { Vec2::new(1.0, 0.0) }
        } else {
            let v = Vec2::new(xy, l_max - xx);
            let l = len(v);
            if l <= f64::EPSILON {
                return Ok(());
            }
            v / l
        };
        let ratio = (l_min.max(0.0).sqrt() / s_max).clamp(1e-9, 1.0);
        let major = center + axis * s_max;
        self.add(EntityKind::Ellipse(Ellipse { center: v3(center), major: v3(major), ratio, start: 0.0, end: std::f64::consts::TAU }))
    }

    fn line(&mut self, e: &BytesStart) -> Result<()> {
        let a = Vec2::new(num(e, b"x1", 0.0), num(e, b"y1", 0.0));
        let b = Vec2::new(num(e, b"x2", 0.0), num(e, b"y2", 0.0));
        self.add(EntityKind::Line(Line { a: v3(self.map(a)), b: v3(self.map(b)) }))
    }

    fn poly_points(&mut self, e: &BytesStart, closed: bool) -> Result<()> {
        let Some(raw) = attr(e, b"points") else {
            return Ok(());
        };
        let nums: Vec<f64> = Numbers::new(&raw).collect();
        let mut pts: Vec<Vec2> = nums.as_chunks::<2>().0.iter().map(|[x, y]| self.map(Vec2::new(*x, *y))).collect();
        if pts.len() > MAX_CURVE_POINTS {
            pts.truncate(MAX_CURVE_POINTS);
        }
        self.add_polyline(&pts, closed)
    }

    fn add_polyline(&mut self, pts: &[Vec2], closed: bool) -> Result<()> {
        if pts.len() < 2 {
            return Ok(());
        }
        let vertices = pts.iter().map(|p| PolyVertex::new(*p)).collect();
        self.add(EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: 0.0, plinegen: false }))
    }

    // --- path data ----------------------------------------------------------

    fn path(&mut self, e: &BytesStart) -> Result<()> {
        let Some(d) = attr(e, b"d") else {
            return Ok(());
        };
        for sp in parse_path(&d)? {
            self.emit_subpath(&sp)?;
        }
        Ok(())
    }

    /// Emit one subpath: runs of lines and arcs become an `LWPOLYLINE` (an arc becomes a
    /// bulge vertex, exactly like DXF's polyline bulge), runs of Bézier curves become a
    /// `SPLINE`. Under a non-uniform transform a circular arc is no longer circular, so it
    /// is flattened into line vertices instead.
    fn emit_subpath(&mut self, sp: &Subpath) -> Result<()> {
        let m = self.cur();
        let det = m.det();
        let ex = m.apply_vec(Vec2::new(1.0, 0.0));
        let ey = m.apply_vec(Vec2::new(0.0, 1.0));
        let mag = (ex.x * ex.x + ex.y * ex.y + ey.x * ey.x + ey.y * ey.y).max(1.0);
        let uniform =
            (ex.x * ex.x + ex.y * ex.y - (ey.x * ey.x + ey.y * ey.y)).abs() <= 1e-9 * mag && (ex.x * ey.x + ex.y * ey.y).abs() <= 1e-9 * mag;
        let scale = det.abs().sqrt();

        let mut run: Vec<PolyVertex> = Vec::new();
        let mut ctrl: Vec<Vec2> = Vec::new();
        let mut cur_user = sp.start;
        let mut cur = m.apply(sp.start);

        for seg in &sp.segs {
            match seg {
                Seg::Line(p) => {
                    if run.is_empty() {
                        run.push(PolyVertex::new(cur));
                    }
                    cur = m.apply(*p);
                    run.push(PolyVertex::new(cur));
                    cur_user = *p;
                }
                Seg::Cubic(c1, c2, p) => {
                    self.flush_run(&mut run, false)?;
                    if ctrl.is_empty() {
                        ctrl.push(cur);
                    }
                    ctrl.push(m.apply(*c1));
                    ctrl.push(m.apply(*c2));
                    cur = m.apply(*p);
                    ctrl.push(cur);
                    cur_user = *p;
                    if ctrl.len() > MAX_CURVE_POINTS {
                        return Err(IoError::Format("SVG: one path has too many curve points".into()));
                    }
                }
                Seg::Arc { rx, ry, rot, large, sweep, end } => {
                    if uniform && scale > 0.0 {
                        let r = rx * scale;
                        let flipped = det < 0.0;
                        if r > 0.0
                            && let Some((_, delta)) = arc_center_circular(cur, m.apply(*end), r, *large, *sweep != flipped)
                        {
                            if run.is_empty() {
                                run.push(PolyVertex::new(cur));
                            }
                            if let Some(last) = run.last_mut() {
                                last.bulge = (delta / 4.0).tan();
                            }
                            run.push(PolyVertex::new(m.apply(*end)));
                            cur = m.apply(*end);
                            cur_user = *end;
                        }
                    } else if *rx > 0.0 && *ry > 0.0 {
                        self.flush_run(&mut run, false)?;
                        self.flush_ctrl(&mut ctrl)?;
                        for p in flatten_arc(&m, cur_user, *end, (*rx, *ry), *rot, *large, *sweep)? {
                            if run.is_empty() {
                                run.push(PolyVertex::new(cur));
                            }
                            run.push(PolyVertex::new(p));
                        }
                        cur = m.apply(*end);
                        cur_user = *end;
                    }
                }
            }
            if run.len() > MAX_CURVE_POINTS {
                return Err(IoError::Format("SVG: one path has too many vertices".into()));
            }
        }
        self.flush_run(&mut run, sp.closed)?;
        self.flush_ctrl(&mut ctrl)?;
        Ok(())
    }

    fn flush_run(&mut self, run: &mut Vec<PolyVertex>, closed: bool) -> Result<()> {
        if run.len() >= 2 {
            let vertices = std::mem::take(run);
            self.add(EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: 0.0, plinegen: false }))?;
        } else {
            run.clear();
        }
        Ok(())
    }

    fn flush_ctrl(&mut self, ctrl: &mut Vec<Vec2>) -> Result<()> {
        if ctrl.len() >= 4 {
            let spline = Spline::from_control(std::mem::take(ctrl), 3);
            self.add(EntityKind::Spline(spline))?;
        } else {
            ctrl.clear();
        }
        Ok(())
    }
}

/// The element is not rendered (`display="none"`, or a style that says so).
fn hidden(e: &BytesStart) -> bool {
    if attr(e, b"display").as_deref() == Some("none") {
        return true;
    }
    attr(e, b"style").is_some_and(|s| s.replace(' ', "").contains("display:none"))
}

fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.into_inner().as_bytes() == name {
            return a.normalized_value(XmlVersion::Explicit1_0).ok().map(|v| v.into_owned());
        }
    }
    None
}

fn num(e: &BytesStart, name: &[u8], fallback: f64) -> f64 {
    attr(e, name).as_deref().and_then(parse_length_mm).unwrap_or(fallback)
}

/// A bare number, or a number with an absolute CSS/SVG unit: millimetres here, everything
/// else (px, the default; pt, pc, in, cm) through the CSS reference pixel (96 per inch).
fn parse_length_mm(s: &str) -> Option<f64> {
    let s = s.trim();
    let cut = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')).unwrap_or(s.len());
    let (n, unit) = s.split_at(cut);
    let value: f64 = n.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    match unit.trim() {
        "" | "px" => Some(value),
        "mm" => Some(value),
        "cm" => Some(value * 10.0),
        "in" => Some(value * 25.4),
        "pt" => Some(value * 25.4 / 72.0),
        "pc" => Some(value * 25.4 / 6.0),
        _ => Some(value * 25.4 / 96.0),
    }
}

/// The root mapping: `viewBox` and unit-bearing `width`/`height` become millimetres per user
/// unit, and the page's Y axis is flipped so the drawing keeps its appearance with Y up.
fn viewport(e: &BytesStart) -> Result<Mat3> {
    let vb = attr(e, b"viewBox").and_then(|raw| {
        let nums: Vec<f64> = Numbers::new(&raw).collect();
        match nums.as_slice() {
            [x, y, w, h] if w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0 => Some((*x, *y, *w, *h)),
            _ => None,
        }
    });
    // A bare or `px` root size is in CSS pixels, not millimetres.
    let root_mm = |name: &[u8]| {
        let raw = attr(e, name)?;
        let mm = parse_length_mm(&raw)?;
        let unit = raw.trim().trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')).trim();
        Some(if matches!(unit, "" | "px") { mm * 25.4 / 96.0 } else { mm })
    };
    let w_mm = root_mm(b"width");
    let h_mm = root_mm(b"height");
    let unit = match (vb, w_mm, h_mm) {
        (Some((_, _, vw, _)), Some(w), _) if w > 0.0 => w / vw,
        (Some((_, _, _, vh)), _, Some(h)) if h > 0.0 => h / vh,
        (Some(_), _, _) => 25.4 / 96.0,
        (None, _, _) => 25.4 / 96.0,
    };
    let (vx, vy, vh) = match vb {
        Some((x, y, _, h)) => (x, y, h),
        None => (0.0, 0.0, 0.0),
    };
    Ok(Mat3 { a: unit, b: 0.0, c: 0.0, d: -unit, e: -vx * unit, f: (vy + vh) * unit })
}

/// `transform="…"`: a list of functions, applied left to right.
fn parse_transform(s: &str) -> Option<Mat3> {
    let mut out: Option<Mat3> = None;
    let mut rest = s.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim();
        let close = rest[open..].find(')')? + open;
        let args: Vec<f64> = Numbers::new(&rest[open + 1..close]).collect();
        let m = match (name, args.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => Mat3 { a: *a, b: *b, c: *c, d: *d, e: *e, f: *f },
            ("translate", [x]) => Mat3::translate(Vec2::new(*x, 0.0)),
            ("translate", [x, y]) => Mat3::translate(Vec2::new(*x, *y)),
            ("scale", [s]) => Mat3::scale(*s, *s),
            ("scale", [sx, sy]) => Mat3::scale(*sx, *sy),
            ("rotate", [a]) => Mat3::rotate(a.to_radians()),
            ("rotate", [a, cx, cy]) => Mat3::rotate_about(Vec2::new(*cx, *cy), a.to_radians()),
            ("skewX", [a]) => Mat3 { a: 1.0, b: 0.0, c: a.to_radians().tan(), d: 1.0, e: 0.0, f: 0.0 },
            ("skewY", [a]) => Mat3 { a: 1.0, b: a.to_radians().tan(), c: 0.0, d: 1.0, e: 0.0, f: 0.0 },
            _ => return None,
        };
        out = Some(match out {
            Some(prev) => prev.then_before(m),
            None => m,
        });
        rest = rest[close + 1..].trim_start_matches(|c: char| c == ',' || c.is_whitespace());
    }
    out
}

fn sanitize_layer(raw: &str) -> String {
    let cleaned: String =
        raw.trim().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '$') { c } else { '_' }).take(128).collect();
    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() { "SVG".into() } else { cleaned }
}

/// A scanner over an SVG number list: separators are whitespace and commas, and a sign
/// starts a new number, so `10-20` is two numbers and `1e-5 2` is two numbers.
struct Numbers<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Numbers<'a> {
    fn new(s: &'a str) -> Self {
        Numbers { s: s.as_bytes(), i: 0 }
    }

    fn skip_seps(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\r' | b'\n' | b',')) {
            self.i += 1;
        }
    }

    fn next_num(&mut self) -> Option<f64> {
        self.skip_seps();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let mut digits = 0;
        let mut dot = false;
        let mut exp = false;
        while let Some(c) = self.s.get(self.i) {
            match c {
                b'0'..=b'9' => {
                    digits += 1;
                    self.i += 1;
                }
                b'.' if !dot && !exp => {
                    dot = true;
                    self.i += 1;
                }
                b'e' | b'E' if !exp && digits > 0 => {
                    exp = true;
                    self.i += 1;
                    if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
        if digits == 0 {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse::<f64>().ok().filter(|v| v.is_finite())
    }
}

impl Iterator for Numbers<'_> {
    type Item = f64;
    fn next(&mut self) -> Option<f64> {
        self.next_num()
    }
}

/// One `M …` run of a `d` attribute.
struct Subpath {
    start: Vec2,
    segs: Vec<Seg>,
    closed: bool,
}

enum Seg {
    Line(Vec2),
    Cubic(Vec2, Vec2, Vec2),
    Arc { rx: f64, ry: f64, rot: f64, large: bool, sweep: bool, end: Vec2 },
}

/// Tokens of a `d` attribute: command letters and numbers, in order.
enum Tok {
    Cmd(u8),
    Num(f64),
}

struct Tokenizer<'a> {
    s: &'a [u8],
    i: usize,
    /// Start of the most recently returned token.
    tok_start: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(s: &'a str) -> Self {
        Tokenizer { s: s.as_bytes(), i: 0, tok_start: 0 }
    }

    /// Read an arc flag: a single `0` or `1` character, which needs no separator after it.
    fn next_flag(&mut self) -> Option<f64> {
        let mut nums = Numbers { s: self.s, i: self.i };
        nums.skip_seps();
        let v = match self.s.get(nums.i)? {
            b'0' => 0.0,
            b'1' => 1.0,
            _ => return None,
        };
        self.i = nums.i + 1;
        Some(v)
    }

    fn next_tok(&mut self) -> Option<Tok> {
        let mut nums = Numbers { s: self.s, i: self.i };
        nums.skip_seps();
        self.i = nums.i;
        self.tok_start = self.i;
        let c = self.s.get(self.i).copied()?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            return Some(Tok::Cmd(c));
        }
        let tok = nums.next_num().map(Tok::Num);
        self.i = nums.i;
        tok
    }
}

/// Split `d` into subpaths, resolving relative commands and the S/T control reflections.
fn parse_path(d: &str) -> Result<Vec<Subpath>> {
    let mut tk = Tokenizer::new(d);
    let mut lookahead: Option<Tok> = None;
    let mut cmd = 0u8;
    let mut cur = Vec2::ZERO;
    let mut start = Vec2::ZERO;
    let mut segs: Vec<Seg> = Vec::new();
    let mut out: Vec<Subpath> = Vec::new();
    let closed = false;
    let mut prev_cubic: Option<Vec2> = None;
    let mut prev_quad: Option<Vec2> = None;
    let mut segments = 0usize;

    let next_num = |tk: &mut Tokenizer, look: &mut Option<Tok>| -> Result<f64> {
        match look.take() {
            Some(Tok::Num(v)) => Ok(v),
            Some(other @ Tok::Cmd(_)) => {
                *look = Some(other);
                Err(IoError::Format("SVG: path command is missing coordinates".into()))
            }
            None => match tk.next_tok() {
                Some(Tok::Num(v)) => Ok(v),
                Some(other @ Tok::Cmd(_)) => {
                    *look = Some(other);
                    Err(IoError::Format("SVG: path command is missing coordinates".into()))
                }
                None => Err(IoError::Format("SVG: path command is missing coordinates".into())),
            },
        }
    };

    while let Some(tok) = lookahead.take().or_else(|| tk.next_tok()) {
        let rel = match tok {
            Tok::Cmd(c) => {
                if c.eq_ignore_ascii_case(&b'z') {
                    if !segs.is_empty() {
                        out.push(Subpath { start, segs, closed: true });
                        segs = Vec::new();
                    }
                    cur = start;
                    cmd = 0;
                    continue;
                }
                cmd = c;
                c.is_ascii_lowercase()
            }
            tok @ Tok::Num(_) => {
                if cmd == 0 {
                    return Err(IoError::Format("SVG: path data does not start with a command".into()));
                }
                lookahead = Some(tok);
                cmd.is_ascii_lowercase()
            }
        };
        let abs = cmd.to_ascii_uppercase();
        let count = match abs {
            b'M' | b'L' => 2,
            b'H' | b'V' | b'T' => 1,
            b'C' => 6,
            b'S' | b'Q' => 4,
            b'A' => 7,
            _ => return Err(IoError::Format(format!("SVG: unknown path command `{}`", cmd as char))),
        };
        let mut args = [0.0f64; 7];
        for (idx, slot) in args.iter_mut().enumerate().take(count) {
            if abs == b'A' && (idx == 3 || idx == 4) {
                // Flags are single characters; re-scan a pending number token from its start.
                if matches!(lookahead, Some(Tok::Num(_))) {
                    lookahead = None;
                    tk.i = tk.tok_start;
                }
                if lookahead.is_none() {
                    *slot = tk.next_flag().ok_or_else(|| IoError::Format("SVG: path command is missing coordinates".into()))?;
                    continue;
                }
            }
            *slot = next_num(&mut tk, &mut lookahead)?;
        }
        let point = |x: f64, y: f64| if rel { cur + Vec2::new(x, y) } else { Vec2::new(x, y) };
        match abs {
            b'M' => {
                if !segs.is_empty() {
                    out.push(Subpath { start, segs, closed: false });
                    segs = Vec::new();
                }
                cur = point(args[0], args[1]);
                start = cur;
                cmd = if rel { b'l' } else { b'L' };
                prev_cubic = None;
                prev_quad = None;
            }
            b'L' => {
                cur = point(args[0], args[1]);
                segs.push(Seg::Line(cur));
                prev_cubic = None;
                prev_quad = None;
            }
            b'H' => {
                cur = Vec2::new(if rel { cur.x + args[0] } else { args[0] }, cur.y);
                segs.push(Seg::Line(cur));
                prev_cubic = None;
                prev_quad = None;
            }
            b'V' => {
                cur = Vec2::new(cur.x, if rel { cur.y + args[0] } else { args[0] });
                segs.push(Seg::Line(cur));
                prev_cubic = None;
                prev_quad = None;
            }
            b'C' => {
                let (c1, c2, p) = (point(args[0], args[1]), point(args[2], args[3]), point(args[4], args[5]));
                segs.push(Seg::Cubic(c1, c2, p));
                prev_cubic = Some(c2);
                prev_quad = None;
                cur = p;
            }
            b'S' => {
                let c1 = prev_cubic.map_or(cur, |c2| cur * 2.0 - c2);
                let (c2, p) = (point(args[0], args[1]), point(args[2], args[3]));
                segs.push(Seg::Cubic(c1, c2, p));
                prev_cubic = Some(c2);
                prev_quad = None;
                cur = p;
            }
            b'Q' => {
                let (q, p) = (point(args[0], args[1]), point(args[2], args[3]));
                segs.push(Seg::Cubic(cur + (q - cur) * (2.0 / 3.0), p + (q - p) * (2.0 / 3.0), p));
                prev_quad = Some(q);
                prev_cubic = None;
                cur = p;
            }
            b'T' => {
                let reflect = prev_quad.map_or(cur, |q| cur * 2.0 - q);
                let p = point(args[0], args[1]);
                segs.push(Seg::Cubic(cur + (reflect - cur) * (2.0 / 3.0), p + (reflect - p) * (2.0 / 3.0), p));
                prev_quad = Some(reflect);
                prev_cubic = None;
                cur = p;
            }
            b'A' => {
                let end = point(args[5], args[6]);
                segs.push(Seg::Arc { rx: args[0].abs(), ry: args[1].abs(), rot: args[2], large: args[3] != 0.0, sweep: args[4] != 0.0, end });
                cur = end;
                prev_cubic = None;
                prev_quad = None;
            }
            _ => return Err(IoError::Format("SVG: unsupported path command".into())),
        }
        segments += 1;
        if segments > MAX_PATH_SEGMENTS {
            return Err(IoError::Format("SVG: path has too many segments".into()));
        }
        if segs.len() > MAX_CURVE_POINTS {
            return Err(IoError::Format("SVG: path has too many segments".into()));
        }
    }
    if !segs.is_empty() {
        out.push(Subpath { start, segs, closed });
    }
    Ok(out)
}

/// The centre and signed sweep of a circular arc from `p0` to `p1` in a Y-up frame, per the
/// SVG endpoint parameterisation (F.6.5). Radii that cannot span the chord are scaled up,
/// as the specification requires.
fn arc_center_circular(p0: Vec2, p1: Vec2, r: f64, large: bool, sweep: bool) -> Option<(Vec2, f64)> {
    if r <= 0.0 || !r.is_finite() {
        return None;
    }
    let d = p1 - p0;
    let dist = d.x.hypot(d.y);
    let r = if dist > 2.0 * r { dist / 2.0 } else { r };
    if dist <= f64::EPSILON {
        return None;
    }
    let mid = (p0 + p1) * 0.5;
    let h = (r * r - (dist * dist) / 4.0).max(0.0).sqrt();
    let n = Vec2::new(-d.y / dist, d.x / dist);
    let sign = if large != sweep { 1.0 } else { -1.0 };
    let center = mid + n * (h * sign);
    let a0 = (p0.y - center.y).atan2(p0.x - center.x);
    let a1 = (p1.y - center.y).atan2(p1.x - center.x);
    let ccw = (a1 - a0).rem_euclid(std::f64::consts::TAU);
    Some((center, pick_sweep(ccw, large, sweep)))
}

/// The signed sweep (CCW positive) for a chord sweep `ccw` in `[0, 2π)`: the centre is
/// already chosen so that the magnitude matches `large`, so only the direction is left.
fn pick_sweep(ccw: f64, large: bool, sweep: bool) -> f64 {
    let tau = std::f64::consts::TAU;
    let eps = f64::EPSILON;
    if sweep {
        if ccw <= eps { if large { tau } else { eps } } else { ccw }
    } else if ccw >= tau - eps {
        if large { -tau } else { -eps }
    } else {
        ccw - tau
    }
}

/// Flatten a general elliptical arc (any `rx`/`ry`/rotation) from `p0` to `p1` in user
/// space, then map every sample through `m`.
fn flatten_arc(m: &Mat3, p0: Vec2, p1: Vec2, (rx, ry): (f64, f64), rot_deg: f64, large: bool, sweep: bool) -> Result<Vec<Vec2>> {
    let (c, theta, delta) = elliptical_params(p0, p1, rx, ry, rot_deg.to_radians(), large, sweep)
        .ok_or_else(|| IoError::Format("SVG: elliptical arc cannot be converted".into()))?;
    let axes = Mat3::rotate(rot_deg.to_radians());
    let rmax = rx.max(ry);
    let tol = 0.05_f64;
    let steps = if rmax > tol {
        let step = 2.0 * (1.0 - (tol / rmax).min(1.0)).acos().max(1e-6);
        ((delta.abs() / step).ceil() as usize).clamp(8, MAX_ARC_CHORDS)
    } else {
        8
    };
    let mut out = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let t = theta + delta * (k as f64) / (steps as f64);
        let local = Vec2::new(rx * t.cos(), ry * t.sin());
        out.push(m.apply(c + axes.apply_vec(local)));
    }
    Ok(out)
}

/// Endpoint → centre parameterisation for a rotated elliptical arc (SVG F.6.5), in the
/// frame the points are given in. Returns the centre, the start parameter and the signed
/// sweep.
fn elliptical_params(p0: Vec2, p1: Vec2, rx: f64, ry: f64, phi: f64, large: bool, sweep: bool) -> Option<(Vec2, f64, f64)> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let (sphi, cphi) = phi.sin_cos();
    let dx = (p0.x - p1.x) / 2.0;
    let dy = (p0.y - p1.y) / 2.0;
    let x1 = cphi * dx + sphi * dy;
    let y1 = -sphi * dx + cphi * dy;
    let (rx, ry) = {
        let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            (rx * s, ry * s)
        } else {
            (rx, ry)
        }
    };
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let coef = if sweep != large { 1.0 } else { -1.0 } * (num.max(0.0) / den.max(f64::MIN_POSITIVE)).sqrt();
    let cxp = coef * rx * y1 / ry;
    let cyp = -coef * ry * x1 / rx;
    let center = Vec2::new(cphi * cxp - sphi * cyp + (p0.x + p1.x) / 2.0, sphi * cxp + cphi * cyp + (p0.y + p1.y) / 2.0);
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let dot = ux * vx + uy * vy;
        let det = ux * vy - uy * vx;
        let denom = (ux.hypot(uy) * vx.hypot(vy)).max(f64::MIN_POSITIVE);
        (dot / denom).clamp(-1.0, 1.0).acos().copysign(det)
    };
    let (ux, uy) = ((x1 - cxp) / rx, (y1 - cyp) / ry);
    let (vx, vy) = ((-x1 - cxp) / rx, (-y1 - cyp) / ry);
    let theta = angle(1.0, 0.0, ux, uy);
    let ccw = angle(ux, uy, vx, vy).rem_euclid(std::f64::consts::TAU);
    Some((center, theta, pick_sweep(ccw, large, sweep)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc_of(d: &str) -> (bool, bool, Vec2) {
        let subs = parse_path(d).unwrap_or_else(|e| panic!("{d}: {e:?}"));
        match &subs[0].segs[0] {
            Seg::Arc { large, sweep, end, .. } => (*large, *sweep, *end),
            _ => panic!("not an arc"),
        }
    }

    #[test]
    fn compact_arc_flags_read_as_single_characters() {
        let spaced = arc_of("M10 10 a20 20 0 0 1 20 20");
        assert_eq!(arc_of("M10 10 a20 20 0 01 20 20"), spaced);
        assert_eq!(arc_of("M10 10 a20 20 0 0120 20"), spaced);
        assert_eq!(arc_of("M10 10 a20 20 0 1 0 20 20"), (true, false, spaced.2));
        assert!(arc_of("M10 10 a20 20 0 10-20 20").0);
    }
}
