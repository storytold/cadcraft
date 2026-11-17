//! SVG → Drawing: the static W3C SVG subset that drawing tools emit.
//!
//! Reads shapes (`rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`), `path`
//! outlines (`M L H V C S Q T A Z`, absolute and relative), `g` grouping, `transform` on groups
//! and shapes, `viewBox` and unit-bearing `width`/`height`, and Inkscape layers
//! (`inkscape:groupmode="layer"`). Geometry becomes centre-line entities in millimetres with
//! the page's top-left corner at the origin and Y pointing up — where a laser or pen job
//! wants it.
//!
//! Not read (each is a follow-up): CSS class rules (`<style>`), `use` references, fills,
//! gradients and hatch patterns, text, embedded raster images, rounded rectangle corners
//! (`rx`/`ry`), and elliptical arcs under non-uniform transforms (those are flattened into
//! line segments instead of losing the shape). What the drawing shows of those (all but fills,
//! gradients and patterns) is named in a read note, so it is never left out silently.

use std::collections::BTreeMap;

use cadcraft_color::Color;
use cadcraft_doc::{Circle, Common, Drawing, Ellipse, EntityKind, Layer, Line, LwPolyline, Space};
use cadcraft_geom::{Mat3, PolyVertex, Spline, Vec2, Vec3};

use crate::{IoError, Result};

/// A decompressed SVGZ payload larger than this is refused (decompression bomb guard).
const MAX_DECOMPRESSED: usize = 64 << 20;
/// Shape elements per file (rect, circle, path, …; each counted once).
const MAX_ELEMENTS: usize = 500_000;
/// Vertices in one polyline, and control points in one spline.
const MAX_CURVE_POINTS: usize = 100_001;
/// Flattened chords per elliptical arc under a non-uniform transform.
const MAX_ARC_CHORDS: usize = 10_000;
/// Generated vertices retained across the whole document (all paths and shapes), so repeated
/// arc/curve flattening cannot amplify a compact input into unbounded geometry.
const MAX_TOTAL_VERTS: usize = 8_000_000;

/// Read an SVG document, or a gzip-compressed one (`.svgz`, Inkscape's "Compressed SVG").
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    let unpacked;
    let plain: &[u8] = if bytes.starts_with(&[0x1f, 0x8b]) {
        unpacked =
            craft_svg::svgz::inflate(bytes, MAX_DECOMPRESSED).map_err(|_| IoError::Format("this SVGZ file's gzip stream is damaged".into()))?;
        &unpacked
    } else {
        bytes
    };
    let text = std::str::from_utf8(plain).map_err(|_| IoError::Format("SVG files must be UTF-8".into()))?;
    let mut parser = Parser::new();
    parser.parse(text)?;
    Ok(parser.drawing)
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
    elements: usize,
    /// Generated vertices still allowed across the whole document. Arc/curve flattening can turn
    /// a compact path into many samples, and each flushed run is retained, so without a shared
    /// budget repeated arcs amplify a small input into gigabytes of geometry (CWE-400).
    verts_left: usize,
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
            elements: 0,
            verts_left: MAX_TOTAL_VERTS,
            left_out: BTreeMap::new(),
        }
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
        use craft_svg::neutral_document::{self, Event, Skipped};
        let mut failure = None;
        let mut layer_color = 1u8;
        let result = neutral_document::visit(text, &mut |event| {
            let r = match event {
                Event::Layer(name) => {
                    let color = Color::Index(layer_color);
                    layer_color = if layer_color >= 7 { 1 } else { layer_color + 1 };
                    self.drawing.layers.push(Layer { name, color, ..Layer::default() });
                    Ok(())
                }
                Event::Shape(shape) => self.map_shape(shape),
            };
            r.map_err(|e| {
                failure = Some(e);
                neutral_document::Error("CAD geometry mapping failed".into())
            })
        });
        if let Some(e) = failure {
            return Err(e);
        }
        let doc = result.map_err(|e| IoError::Format(e.to_string()))?;
        for (kind, count) in doc.skipped {
            let kind = match kind {
                Skipped::Text => LeftOut::Text,
                Skipped::Image => LeftOut::Image,
                Skipped::Use => LeftOut::Use,
                Skipped::Style => LeftOut::Style,
                Skipped::RoundedCorners => LeftOut::RoundedCorners,
            };
            self.left_out.insert(kind, count);
        }
        self.drawing.read_notes.extend(self.note());
        Ok(())
    }

    fn map_shape(&mut self, shape: craft_svg::neutral_document::Shape) -> Result<()> {
        use craft_svg::neutral_document::Primitive;
        let point = |p: craft_svg::path_data::Point| Vec2::new(p.x, p.y);
        let [a, b, c, d, e, f] = shape.transform;
        if let Some(current) = self.stack.first_mut() {
            *current = Mat3 { a, b, c, d, e, f };
        }
        self.layer = shape.layer;
        match shape.primitive {
            Primitive::Rect { x, y, width, height } => {
                let pts = [Vec2::new(x, y), Vec2::new(x + width, y), Vec2::new(x + width, y + height), Vec2::new(x, y + height)];
                self.add_polyline(&pts.map(|p| self.map(p)), true)?;
            }
            Primitive::Circle { center, radius } => self.add_rounded(point(center), radius, radius)?,
            Primitive::Ellipse { center, rx, ry } => self.add_rounded(point(center), rx, ry)?,
            Primitive::Line { a, b } => self.add(EntityKind::Line(Line { a: v3(self.map(point(a))), b: v3(self.map(point(b))) }))?,
            Primitive::Polyline { points, closed } => {
                self.add_polyline(&points.into_iter().map(|p| self.map(point(p))).collect::<Vec<_>>(), closed)?
            }
            Primitive::Path(paths) => {
                for path in paths {
                    let path = Subpath {
                        start: point(path.start),
                        closed: path.closed,
                        segs: path
                            .segs
                            .into_iter()
                            .map(|seg| match seg {
                                craft_svg::path_data::Segment::Line(p) => Seg::Line(point(p)),
                                craft_svg::path_data::Segment::Cubic(a, b, c) => Seg::Cubic(point(a), point(b), point(c)),
                                craft_svg::path_data::Segment::Arc { rx, ry, rot, large, sweep, end } => {
                                    Seg::Arc { rx, ry, rot, large, sweep, end: point(end) }
                                }
                            })
                            .collect(),
                    };
                    self.emit_subpath(&path)?;
                }
            }
        }
        Ok(())
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

    fn add_polyline(&mut self, pts: &[Vec2], closed: bool) -> Result<()> {
        if pts.len() < 2 {
            return Ok(());
        }
        let vertices = pts.iter().map(|p| PolyVertex::new(*p)).collect();
        self.add(EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: 0.0, plinegen: false }))
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

fn arc_center_circular(p0: Vec2, p1: Vec2, r: f64, large: bool, sweep: bool) -> Option<(Vec2, f64)> {
    let point = |p: Vec2| craft_svg::path_data::Point::new(p.x, p.y);
    craft_svg::path_data::arc_center_circular(point(p0), point(p1), r, large, sweep).map(|(p, angle)| (Vec2::new(p.x, p.y), angle))
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

fn elliptical_params(p0: Vec2, p1: Vec2, rx: f64, ry: f64, phi: f64, large: bool, sweep: bool) -> Option<(Vec2, f64, f64)> {
    let point = |p: Vec2| craft_svg::path_data::Point::new(p.x, p.y);
    craft_svg::path_data::elliptical_params(point(p0), point(p1), rx, ry, phi, large, sweep)
        .map(|(p, start, sweep)| (Vec2::new(p.x, p.y), start, sweep))
}

/// Convert shared SVG path segments to CADCraft's geometry types.
#[cfg(test)]
fn parse_path(d: &str) -> Result<Vec<Subpath>> {
    use craft_svg::path_data::{Point, Segment};
    let point = |p: Point| Vec2::new(p.x, p.y);
    let paths = craft_svg::path_data::parse_path(d).map_err(|e| IoError::Format(e.to_string()))?;
    Ok(paths
        .into_iter()
        .map(|sp| Subpath {
            start: point(sp.start),
            closed: sp.closed,
            segs: sp
                .segs
                .into_iter()
                .map(|seg| match seg {
                    Segment::Line(p) => Seg::Line(point(p)),
                    Segment::Cubic(c1, c2, p) => Seg::Cubic(point(c1), point(c2), point(p)),
                    Segment::Arc { rx, ry, rot, large, sweep, end } => Seg::Arc { rx, ry, rot, large, sweep, end: point(end) },
                })
                .collect(),
        })
        .collect())
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
