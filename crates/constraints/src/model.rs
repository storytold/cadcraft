//! Maps constrained entities to a flat parameter vector and back.
//!
//! - Line: `ax ay bx by`
//! - Circle: `cx cy r`
//! - Arc: `cx cy r start end` (angles in radians)
//! - LwPolyline: `x0 y0 x1 y1 …` (bulges are kept; segments are treated as straight lines)
//! - Point: `x y`
//! - Ellipse: `cx cy mx my ratio` plus `start end` (parameters, radians) for an elliptical arc;
//!   `(mx, my)` is the major axis vector
//! - Spline: its defining points `x0 y0 x1 y1 …`: the fit points of an interpolating spline
//!   (rebuilt through them on write-back), else the control points
//! - Block reference: `x y rotation`; the block's characteristic points ride along rigidly
//!
//! Constraints may append auxiliary unknowns after the entity parameters (contact parameters of
//! a tangency between curves); they are never written back.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cadcraft_doc::{Block, Drawing, EntityKind, GeomRef, Handle, Insert, Sub};
use cadcraft_geom::{Spline as GSpline, Vec2, ccw_sweep};

use crate::curves::{Ell, Frame, clamped, spline_end};

/// Most parameters one solve may have.
pub const MAX_PARAMS: usize = 4000;
/// Most defining points of a spline taking part in constraints.
pub const MAX_SPLINE_POINTS: usize = 500;
/// Most characteristic points of a block reference used as rigid constraint points.
pub const MAX_RIGID_POINTS: usize = 256;
/// Block members examined when sampling rigid points, counting those that contribute none, so a
/// block padded with point-less entities cannot make sampling (repeated per Insert) unbounded.
const MAX_RIGID_MEMBERS: usize = 100_000;
/// Shared across every model built for one analysis request.
pub(crate) const MAX_RIGID_WORK: usize = 1_000_000;
/// How far past 1 a solved ellipse ratio may land by rounding and still be stored as 1.
const RATIO_TOL: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Line,
    Circle,
    Arc,
    Poly {
        n: usize,
        closed: bool,
    },
    Point,
    /// `arc`: elliptical arc (has start/end parameters).
    Ellipse {
        arc: bool,
    },
    /// `n` defining points; `fit`: they are fit points; `ends`: the curve has a start and an end
    /// (open and clamped) that are its first and last defining points.
    Spline {
        n: usize,
        fit: bool,
        ends: bool,
    },
    /// Block reference with `n` rigid points.
    Insert {
        n: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub handle: Handle,
    pub shape: Shape,
    pub off: usize,
    pub len: usize,
}

/// The spline last rebuilt for each handle, with the defining points it was rebuilt from.
type SplineCache = HashMap<Handle, (Vec<Vec2>, Rc<GSpline>)>;

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub slots: Vec<Slot>,
    pub x0: Vec<f64>,
    index: HashMap<Handle, usize>,
    /// Spline templates (degree, knots, weights, fit options) the defining points plug into.
    splines: HashMap<Handle, GSpline>,
    /// Block reference rigid points relative to the insertion point at rotation 0.
    rigid: HashMap<Handle, Vec<Vec2>>,
    /// Rebuilt splines, shared with every model of the same [`Prep`]: residuals that ask for the
    /// same spline at the same defining points (several constraints on one spline, or derivatives
    /// taken along other objects' parameters) reuse one rebuild instead of a dense solve each.
    rebuilt: Rc<RefCell<SplineCache>>,
}

/// Geometry preparation shared by every model built for one operation (a solve, a DOF report, a
/// purge): rigid samples per block, a case-insensitive block-name index and rebuilt splines. A
/// block referenced by many Inserts is sampled once, an unknown block name costs one hashed
/// lookup, and a spline referenced by many constraints is rebuilt once (CWE-407).
#[derive(Debug)]
pub(crate) struct Prep {
    /// Block members that may still be examined across the operation.
    members_left: usize,
    /// Block-local rigid samples by the name an Insert gives (`None`: no such block).
    blocks: HashMap<String, Option<Rc<[Vec2]>>>,
    /// Lower-cased block name to the stored name it resolves to, built on the first exact miss.
    folded: Option<HashMap<String, String>>,
    rebuilt: Rc<RefCell<SplineCache>>,
}

impl Prep {
    /// Preparation that may examine at most `members` block members in all.
    pub(crate) fn new(members: usize) -> Prep {
        Prep { members_left: members, blocks: HashMap::new(), folded: None, rebuilt: Rc::default() }
    }

    /// The block an Insert names, as [`Drawing::block`] resolves it (exact name, else the first
    /// stored name equal ignoring ASCII case), without scanning every block on each miss.
    fn resolve<'d>(&mut self, d: &'d Drawing, name: &str) -> Option<&'d Block> {
        if let Some(b) = d.blocks.get(name) {
            return Some(b);
        }
        let folded = self.folded.get_or_insert_with(|| {
            let mut m = HashMap::with_capacity(d.blocks.len());
            for k in d.blocks.keys() {
                m.entry(k.to_ascii_lowercase()).or_insert_with(|| k.clone());
            }
            m
        });
        d.blocks.get(folded.get(&name.to_ascii_lowercase())?).map(|b| &**b)
    }

    /// Finite characteristic points of a block in its own coordinates, sampled once per name.
    fn block_points(&mut self, d: &Drawing, name: &str) -> Option<Rc<[Vec2]>> {
        if let Some(s) = self.blocks.get(name) {
            return s.clone();
        }
        let sampled = self.resolve(d, name).map(|b| sample_block(b, &mut self.members_left));
        self.blocks.insert(name.to_string(), sampled.clone());
        sampled
    }

    /// World positions of an Insert's rigid points, at most [`MAX_RIGID_POINTS`].
    fn rigid_points(&mut self, d: &Drawing, ins: &Insert) -> Vec<Vec2> {
        let Some(local) = self.block_points(d, &ins.block) else { return Vec::new() };
        let Some(block) = self.resolve(d, &ins.block) else { return Vec::new() };
        let m = ins.transform(block.base.xy());
        local.iter().map(|p| m.apply(*p)).filter(|p| p.is_finite()).take(MAX_RIGID_POINTS).collect()
    }
}

impl Model {
    /// Work for a spline that an endpoint residual actually reconstructs. Fit interpolation
    /// uses dense Gaussian elimination (cubic); control splines clone and evaluate their template.
    pub(crate) fn spline_reconstruct_cost(&self, handle: Handle) -> f64 {
        let Some(Shape::Spline { n, fit, .. }) = self.slot(handle).map(|s| s.shape) else { return 0.0 };
        let n = n as f64;
        if fit { n * n * n } else { n * n }
    }
}

/// Can this entity take part in constraints?
pub fn supported(kind: &EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Line(_)
            | EntityKind::Circle(_)
            | EntityKind::Arc(_)
            | EntityKind::LwPolyline(_)
            | EntityKind::Point(_)
            | EntityKind::Ellipse(_)
            | EntityKind::Spline(_)
            | EntityKind::Insert(_)
    )
}

/// World positions of a block reference's rigid points: the characteristic points (end points,
/// centres, vertices, nested insertion points) of the block's own objects, at most
/// [`MAX_RIGID_POINTS`]. Nested blocks are not entered.
pub fn rigid_points(d: &Drawing, ins: &Insert) -> Vec<Vec2> {
    Prep::new(MAX_RIGID_MEMBERS).rigid_points(d, ins)
}

/// Sample a block's finite characteristic points in block coordinates, within the per-block
/// member limit and the caller-shared `members_left`. Members that contribute no points still
/// consume traversal work; nested blocks are not entered.
fn sample_block(block: &Block, members_left: &mut usize) -> Rc<[Vec2]> {
    let mut out = Vec::new();
    // Bound the members examined, not only the points collected: entities that contribute no
    // points (Text, Ray, …) still cost traversal (CWE-407).
    for (examined, e) in block.entities.iter().enumerate() {
        if out.len() >= MAX_RIGID_POINTS || examined >= MAX_RIGID_MEMBERS || *members_left == 0 {
            break;
        }
        *members_left -= 1;
        let pts: Vec<Vec2> = match &e.kind {
            EntityKind::Line(l) => vec![l.a.xy(), l.b.xy()],
            EntityKind::Arc(a) => {
                let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
                vec![g.start_point(), g.end_point(), g.center]
            }
            EntityKind::Circle(c) => vec![c.center.xy()],
            EntityKind::Point(p) => vec![p.p.xy()],
            EntityKind::Ellipse(el) => vec![el.center.xy()],
            EntityKind::Insert(i) => vec![i.insert.xy()],
            EntityKind::LwPolyline(p) => p.vertices.iter().take(MAX_RIGID_POINTS).map(|v| v.p).collect(),
            _ => Vec::new(),
        };
        out.extend(pts.into_iter().filter(|p| p.is_finite()));
    }
    out.truncate(MAX_RIGID_POINTS);
    out.into()
}

/// Constraint points of an object as the solver sees them (`Sub`, position): end points,
/// centres, axis ends, defining points, rigid points. Points at the same place are listed once.
pub fn ref_points(d: &Drawing, h: Handle) -> Vec<(Sub, Vec2)> {
    let m = Model::build(d, &[h]);
    let Some(s) = m.slots.first() else { return Vec::new() };
    let n = match s.shape {
        Shape::Ellipse { .. } => 4,
        Shape::Spline { n, .. } | Shape::Insert { n } | Shape::Poly { n, .. } => n,
        _ => 0,
    };
    let mut out: Vec<(Sub, Vec2)> = Vec::new();
    for sub in [Sub::Whole, Sub::Center, Sub::Start, Sub::End, Sub::Mid].into_iter().chain((0..n).map(|i| Sub::Vertex(i as u32))) {
        if let Some(p) = m.point(&m.x0, &GeomRef::new(h, sub))
            && !out.iter().any(|(_, q)| *q == p)
        {
            out.push((sub, p));
        }
    }
    out
}

fn p2(x: &[f64], i: usize) -> Option<Vec2> {
    Some(Vec2::new(*x.get(i)?, *x.get(i + 1)?))
}

/// Does a spline interpolate its fit points (so they can be its parameters)?
fn fit_mode(s: &GSpline) -> bool {
    s.fit.len() >= 2 && s.fit_opts.tolerance == 0.0
}

impl Model {
    /// Build a model for the given entities. Unsupported or missing entities are skipped.
    pub fn build(d: &Drawing, handles: &[Handle]) -> Model {
        Self::build_with(d, handles, &mut Prep::new(MAX_RIGID_WORK))
    }

    /// Add the parameters of one supported entity to `m`, and return its shape; `None` when it is
    /// left out (too large, degenerate or unsupported).
    fn add_params(m: &mut Model, d: &Drawing, h: Handle, kind: &EntityKind, off: usize, prep: &mut Prep) -> Option<Shape> {
        let shape = match kind {
            EntityKind::Line(l) => {
                m.x0.extend([l.a.x, l.a.y, l.b.x, l.b.y]);
                Shape::Line
            }
            EntityKind::Circle(c) => {
                m.x0.extend([c.center.x, c.center.y, c.radius]);
                Shape::Circle
            }
            EntityKind::Arc(a) => {
                m.x0.extend([a.center.x, a.center.y, a.radius, a.start, a.end]);
                Shape::Arc
            }
            EntityKind::LwPolyline(p) => {
                if p.vertices.len() < 2 || off + p.vertices.len() * 2 > MAX_PARAMS {
                    return None;
                }
                for v in &p.vertices {
                    m.x0.extend([v.p.x, v.p.y]);
                }
                Shape::Poly { n: p.vertices.len(), closed: p.closed }
            }
            EntityKind::Point(p) => {
                m.x0.extend([p.p.x, p.p.y]);
                Shape::Point
            }
            EntityKind::Ellipse(el) => {
                let ell = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
                let arc = !ell.is_full();
                m.x0.extend([el.center.x, el.center.y, el.major.x, el.major.y, el.ratio]);
                if arc {
                    m.x0.extend([el.start, el.end]);
                }
                Shape::Ellipse { arc }
            }
            EntityKind::Spline(sp) => {
                let fit = fit_mode(sp);
                let pts = if fit { &sp.fit } else { &sp.control };
                let n = pts.len();
                if !(2..=MAX_SPLINE_POINTS).contains(&n) || off + n * 2 > MAX_PARAMS || (!fit && !sp.is_valid()) {
                    return None;
                }
                for p in pts {
                    m.x0.extend([p.x, p.y]);
                }
                // Fit points interpolate exactly, so the curve's ends are the first and last
                // fit point; a control polygon's ends are its first and last points when the
                // knots are clamped.
                let ends = !sp.closed && (fit || (clamped(sp, false) && clamped(sp, true)));
                // Keep only what rebuilding reads, so each rebuild copies no unused data: missing
                // weights default to 1 in eval and only excess weights are unused; a control
                // spline's fit points are dropped when its control points move (write_back), and
                // a fit spline is rebuilt from its parameters, not from its old control net.
                let mut kept = if fit {
                    GSpline { fit_opts: sp.fit_opts, closed: sp.closed, ..GSpline::default() }
                } else {
                    GSpline { fit: Vec::new(), ..sp.clone() }
                };
                kept.weights.truncate(kept.control.len());
                m.splines.insert(h, kept);
                Shape::Spline { n, fit, ends }
            }
            EntityKind::Insert(ins) => {
                let base = Vec2::new(ins.insert.x, ins.insert.y);
                let pts: Vec<Vec2> = prep.rigid_points(d, ins).into_iter().map(|p| (p - base).rotate(-ins.rotation)).collect();
                m.x0.extend([ins.insert.x, ins.insert.y, ins.rotation]);
                let n = pts.len();
                m.rigid.insert(h, pts);
                Shape::Insert { n }
            }
            _ => return None,
        };
        Some(shape)
    }

    /// Build solver parameters for supported entities, sharing block sampling, its member budget
    /// and spline rebuilds with every other model built from `prep`.
    pub(crate) fn build_with(d: &Drawing, handles: &[Handle], prep: &mut Prep) -> Model {
        let mut m = Model { rebuilt: Rc::clone(&prep.rebuilt), ..Model::default() };
        for &h in handles {
            if m.index.contains_key(&h) {
                continue;
            }
            let Some(e) = d.entity(h) else { continue };
            let off = m.x0.len();
            let Some(shape) = Self::add_params(&mut m, d, h, &e.kind, off, prep) else { continue };
            if m.x0.len() > MAX_PARAMS || m.x0.get(off..).unwrap_or(&[]).iter().any(|v| !v.is_finite()) {
                m.x0.truncate(off);
                continue;
            }
            let len = m.x0.len() - off;
            m.index.insert(h, m.slots.len());
            m.slots.push(Slot { handle: h, shape, off, len });
        }
        m
    }

    pub fn slot(&self, h: Handle) -> Option<&Slot> {
        self.slots.get(*self.index.get(&h)?)
    }

    /// A point-like reference.
    pub fn point(&self, x: &[f64], r: &GeomRef) -> Option<Vec2> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        match (s.shape, r.sub) {
            (Shape::Line, Sub::Start) => p2(x, o),
            (Shape::Line, Sub::End) => p2(x, o + 2),
            (Shape::Line, Sub::Mid) => Some(p2(x, o)?.mid(p2(x, o + 2)?)),
            (Shape::Circle | Shape::Arc, Sub::Center) => p2(x, o),
            (Shape::Arc, Sub::Start | Sub::End | Sub::Mid) => {
                let c = p2(x, o)?;
                let rad = *x.get(o + 2)?;
                let (a0, a1) = (*x.get(o + 3)?, *x.get(o + 4)?);
                let a = match r.sub {
                    Sub::Start => a0,
                    Sub::End => a1,
                    _ => a0 + ccw_sweep(a0, a1) / 2.0,
                };
                Some(Vec2::polar(c, rad, a))
            }
            (Shape::Poly { n, .. }, Sub::Vertex(i)) => {
                let i = i as usize;
                if i < n { p2(x, o + 2 * i) } else { None }
            }
            (Shape::Poly { .. }, Sub::Start) => p2(x, o),
            (Shape::Poly { n, .. }, Sub::End) => p2(x, o + 2 * n.checked_sub(1)?),
            (Shape::Poly { .. }, Sub::Mid) => None,
            (Shape::Point, Sub::Whole | Sub::Start | Sub::Center) => p2(x, o),
            (Shape::Ellipse { .. }, Sub::Center) => p2(x, o),
            (Shape::Ellipse { .. }, Sub::Vertex(i)) => {
                // Axis end points: +major, +minor, −major, −minor.
                let e = self.ell_at(x, o, false)?;
                Some(match i {
                    0 => e.c + e.mj,
                    1 => e.c + e.minor(),
                    2 => e.c - e.mj,
                    3 => e.c - e.minor(),
                    _ => return None,
                })
            }
            (Shape::Ellipse { arc: true }, Sub::Start | Sub::End | Sub::Mid) => {
                let e = self.ell_at(x, o, true)?;
                Some(e.at(match r.sub {
                    Sub::Start => e.t0,
                    Sub::End => e.t1,
                    _ => e.t0 + ccw_sweep(e.t0, e.t1) / 2.0,
                }))
            }
            (Shape::Spline { n, .. }, Sub::Vertex(i)) => {
                let i = i as usize;
                if i < n { p2(x, o + 2 * i) } else { None }
            }
            (Shape::Spline { ends: true, .. }, Sub::Start) => p2(x, o),
            (Shape::Spline { n, ends: true, .. }, Sub::End) => p2(x, o + 2 * n.checked_sub(1)?),
            (Shape::Insert { .. }, Sub::Whole | Sub::Start | Sub::Center) => p2(x, o),
            (Shape::Insert { n }, Sub::Vertex(i)) => {
                let i = i as usize;
                if i >= n {
                    return None;
                }
                let q = *self.rigid.get(&s.handle)?.get(i)?;
                Some(p2(x, o)? + q.rotate(*x.get(o + 2)?))
            }
            _ => None,
        }
    }

    fn ell_at(&self, x: &[f64], o: usize, arc: bool) -> Option<Ell> {
        let (t0, t1) = if arc { (*x.get(o + 5)?, *x.get(o + 6)?) } else { (0.0, std::f64::consts::TAU) };
        Some(Ell { c: p2(x, o)?, mj: p2(x, o + 2)?, ratio: *x.get(o + 4)?, t0, t1, arc })
    }

    /// An ellipse-like reference (ellipse or elliptical arc).
    pub fn ellipse(&self, x: &[f64], r: &GeomRef) -> Option<Ell> {
        let s = self.slot(r.handle)?;
        match (s.shape, r.sub) {
            (Shape::Ellipse { arc }, Sub::Whole) => self.ell_at(x, s.off, arc),
            _ => None,
        }
    }

    /// Is this reference (any part of) a spline?
    pub fn is_spline(&self, r: &GeomRef) -> bool {
        self.slot(r.handle).is_some_and(|s| matches!(s.shape, Shape::Spline { .. }))
    }

    /// The spline of a reference with the defining points taken from `x`. The last rebuild of
    /// each spline is kept, so asking again at the same points costs a comparison, not a rebuild.
    fn spline(&self, x: &[f64], s: &Slot) -> Option<Rc<GSpline>> {
        let Shape::Spline { n, fit, .. } = s.shape else { return None };
        let t = self.splines.get(&s.handle)?;
        let pts = (0..n).map(|i| p2(x, s.off + 2 * i)).collect::<Option<Vec<_>>>()?;
        if let Some((at, sp)) = self.rebuilt.borrow().get(&s.handle)
            && *at == pts
        {
            return Some(Rc::clone(sp));
        }
        let sp = Rc::new(if fit { t.refit(&pts, t.closed) } else { GSpline { control: pts.clone(), ..t.clone() } });
        self.rebuilt.borrow_mut().insert(s.handle, (pts, Rc::clone(&sp)));
        Some(sp)
    }

    /// The start (`at_end` false) or end of a curve reference, in the curve's own direction:
    /// lines and polyline segments, arcs, elliptical arcs and open splines. A polyline's
    /// `start`/`end` are its first and last segments' outer ends.
    pub fn end_frame(&self, x: &[f64], r: &GeomRef, at_end: bool) -> Option<Frame> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        let seg = match (s.shape, r.sub) {
            (Shape::Poly { .. }, Sub::Start) => Some(GeomRef::new(r.handle, Sub::Segment(0))),
            (Shape::Poly { n, closed: false }, Sub::End) => Some(GeomRef::new(r.handle, Sub::Segment(n.checked_sub(2)? as u32))),
            (Shape::Line, Sub::Start | Sub::End) => Some(GeomRef::whole(r.handle)),
            _ => None,
        };
        if let Some((a, b)) = self.line(x, &seg.unwrap_or(*r)) {
            let d = b - a;
            let l = d.len();
            if crate::curves::not_above(l, 1e-300) {
                return None;
            }
            return Some(Frame { p: if at_end { b } else { a }, t: d * (1.0 / l), k: 0.0 });
        }
        match (s.shape, r.sub) {
            (Shape::Arc, Sub::Whole | Sub::Start | Sub::End) => {
                let (c, rad) = (p2(x, o)?, *x.get(o + 2)?);
                let a = *x.get(if at_end { o + 4 } else { o + 3 })?;
                if crate::curves::not_above(rad.abs(), 1e-300) {
                    return None;
                }
                let (p, d1) = crate::curves::circle_at(c, rad, a);
                Some(Frame { p, t: d1.normalized(), k: 1.0 / rad })
            }
            (Shape::Ellipse { arc: true }, Sub::Whole | Sub::Start | Sub::End) => {
                let e = self.ell_at(x, o, true)?;
                e.frame(if at_end { e.t1 } else { e.t0 })
            }
            (Shape::Spline { ends: true, .. }, Sub::Whole | Sub::Start | Sub::End) => spline_end(&*self.spline(x, s)?, at_end),
            _ => None,
        }
    }

    /// Squared distance of a block reference's farthest rigid point from its insertion point.
    pub fn rigid_extent2(&self, h: Handle) -> f64 {
        self.rigid.get(&h).map_or(0.0, |v| v.iter().map(|q| q.len2()).fold(0.0, f64::max))
    }

    /// Where to draw a glyph for a reference that is not a point, line or circle.
    pub fn anchor(&self, x: &[f64], r: &GeomRef) -> Option<Vec2> {
        let s = self.slot(r.handle)?;
        match s.shape {
            Shape::Ellipse { .. } => self.ell_at(x, s.off, false).map(|e| e.c + e.minor()),
            Shape::Spline { n, .. } => p2(x, s.off + 2 * (n / 2)),
            Shape::Insert { .. } => p2(x, s.off),
            _ => None,
        }
    }

    /// Vertex indices of a polyline segment.
    fn seg(n: usize, closed: bool, i: usize) -> Option<(usize, usize)> {
        if i + 1 < n {
            Some((i, i + 1))
        } else if closed && i + 1 == n && n > 2 {
            Some((i, 0))
        } else {
            None
        }
    }

    /// A line-like reference (line or polyline segment).
    pub fn line(&self, x: &[f64], r: &GeomRef) -> Option<(Vec2, Vec2)> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        match (s.shape, r.sub) {
            (Shape::Line, Sub::Whole) => Some((p2(x, o)?, p2(x, o + 2)?)),
            (Shape::Poly { n, closed }, Sub::Segment(i)) => {
                let (a, b) = Self::seg(n, closed, i as usize)?;
                Some((p2(x, o + 2 * a)?, p2(x, o + 2 * b)?))
            }
            (Shape::Poly { n: 2, .. }, Sub::Whole) => Some((p2(x, o)?, p2(x, o + 2)?)),
            _ => None,
        }
    }

    /// A circle-like reference (circle or arc).
    pub fn circle(&self, x: &[f64], r: &GeomRef) -> Option<(Vec2, f64)> {
        let s = self.slot(r.handle)?;
        match (s.shape, r.sub) {
            (Shape::Circle | Shape::Arc, Sub::Whole) => Some((p2(x, s.off)?, *x.get(s.off + 2)?)),
            _ => None,
        }
    }

    /// Arc sweep (radians) of an arc reference.
    pub fn arc_sweep(&self, x: &[f64], r: &GeomRef) -> Option<f64> {
        let s = self.slot(r.handle)?;
        match (s.shape, r.sub) {
            (Shape::Arc, Sub::Whole) => Some(ccw_sweep(*x.get(s.off + 3)?, *x.get(s.off + 4)?)),
            _ => None,
        }
    }

    /// The points of an object a Fix locks directly (ends, centre, vertices, insertion point).
    pub fn point_refs(&self, h: Handle) -> Vec<GeomRef> {
        let Some(s) = self.slot(h) else { return Vec::new() };
        let at = |sub: Sub| GeomRef { handle: h, sub };
        match s.shape {
            Shape::Line => vec![at(Sub::Start), at(Sub::End)],
            Shape::Point => vec![at(Sub::Whole)],
            Shape::Circle | Shape::Arc | Shape::Ellipse { .. } | Shape::Insert { .. } => vec![at(Sub::Center)],
            Shape::Poly { n, .. } | Shape::Spline { n, .. } => (0..n).filter_map(|i| u32::try_from(i).ok()).map(|i| at(Sub::Vertex(i))).collect(),
        }
    }

    /// Parameters a Fix on this reference locks directly. `None` when the reference is a derived
    /// point (fixed through a residual instead).
    pub fn lock_indices(&self, r: &GeomRef) -> Option<Vec<usize>> {
        let s = self.slot(r.handle)?;
        let o = s.off;
        let v = match (s.shape, r.sub) {
            (_, Sub::Whole) => (o..o + s.len).collect(),
            (Shape::Line, Sub::Start) => vec![o, o + 1],
            (Shape::Line, Sub::End) => vec![o + 2, o + 3],
            (Shape::Circle | Shape::Arc, Sub::Center) => vec![o, o + 1],
            (Shape::Poly { n, .. }, Sub::Vertex(i)) if (i as usize) < n => vec![o + 2 * i as usize, o + 2 * i as usize + 1],
            (Shape::Poly { .. }, Sub::Start) => vec![o, o + 1],
            (Shape::Poly { n, .. }, Sub::End) if n > 0 => vec![o + 2 * (n - 1), o + 2 * (n - 1) + 1],
            (Shape::Poly { n, closed }, Sub::Segment(i)) => {
                let (a, b) = Self::seg(n, closed, i as usize)?;
                vec![o + 2 * a, o + 2 * a + 1, o + 2 * b, o + 2 * b + 1]
            }
            (Shape::Ellipse { .. } | Shape::Insert { .. }, Sub::Center) => vec![o, o + 1],
            (Shape::Insert { .. }, Sub::Start) => vec![o, o + 1],
            (Shape::Spline { n, .. }, Sub::Vertex(i)) if (i as usize) < n => vec![o + 2 * i as usize, o + 2 * i as usize + 1],
            (Shape::Spline { ends: true, .. }, Sub::Start) => vec![o, o + 1],
            (Shape::Spline { n, ends: true, .. }, Sub::End) if n > 0 => vec![o + 2 * (n - 1), o + 2 * (n - 1) + 1],
            _ => return None,
        };
        Some(v)
    }

    /// Can `write_back` store `x` as it is? An ellipse keeps 0 < ratio <= 1, its major axis the
    /// longer one: a solution past that is stored with its axes swapped or its minor axis
    /// mirrored (see [`Model::stored_form`]), which renumbers the axis ends.
    pub fn storable(&self, x: &[f64]) -> bool {
        self.slots.iter().all(|s| match s.shape {
            Shape::Ellipse { .. } => x.get(s.off + 4).is_some_and(|r| *r > 0.0 && *r <= 1.0 + RATIO_TOL),
            _ => true,
        })
    }

    /// `x` with every ellipse re-expressed as [`Model::write_back`] stores it (the same curve with
    /// the major axis the longer one, a full ellipse's negative ratio made positive), so the
    /// constraints can be checked against what would be stored. `None` when an elliptical arc has
    /// a negative ratio or an ellipse a zero one.
    pub fn stored_form(&self, x: &[f64]) -> Option<Vec<f64>> {
        let mut xc = x.to_vec();
        for s in &self.slots {
            let Shape::Ellipse { arc } = s.shape else { continue };
            let o = s.off;
            let mut ratio = *x.get(o + 4)?;
            if ratio < 0.0 && !arc {
                ratio = -ratio;
            }
            if ratio <= 0.0 {
                return None;
            }
            if ratio > 1.0 + RATIO_TOL {
                let major = p2(x, o + 2)?.perp() * ratio;
                *xc.get_mut(o + 2)? = major.x;
                *xc.get_mut(o + 3)? = major.y;
                ratio = 1.0 / ratio;
                if arc {
                    *xc.get_mut(o + 5)? -= std::f64::consts::FRAC_PI_2;
                    *xc.get_mut(o + 6)? -= std::f64::consts::FRAC_PI_2;
                }
            }
            *xc.get_mut(o + 4)? = ratio;
        }
        Some(xc)
    }

    /// The parameter `stored_form` gives the point at parameter `t` on ellipse `h` (unchanged for
    /// any other object): `-t` where a negative ratio was made positive, a quarter turn back where
    /// the axes were swapped.
    pub fn stored_t(&self, x: &[f64], h: Handle, t: f64) -> f64 {
        let Some(s) = self.slot(h) else { return t };
        let (Shape::Ellipse { .. }, Some(&ratio)) = (s.shape, x.get(s.off + 4)) else { return t };
        let (t, ratio) = if ratio < 0.0 { (-t, -ratio) } else { (t, ratio) };
        if ratio > 1.0 + RATIO_TOL { t - std::f64::consts::FRAC_PI_2 } else { t }
    }

    /// Write solved parameters back. Returns the handles that moved.
    pub fn write_back(&self, d: &mut Drawing, x: &[f64]) -> Vec<Handle> {
        let mut moved = Vec::new();
        for s in &self.slots {
            let (Some(new), Some(old)) = (x.get(s.off..s.off + s.len), self.x0.get(s.off..s.off + s.len)) else { continue };
            if new.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let scale = old.iter().fold(1.0f64, |m, v| m.max(v.abs()));
            if new.iter().zip(old).all(|(a, b)| (a - b).abs() <= 1e-13 * scale) {
                continue;
            }
            let shape = s.shape;
            // A control-point spline loses its fit data once its control points move.
            let spline = self.spline(x, s).map(|sp| {
                let mut sp = GSpline::clone(&sp);
                if matches!(shape, Shape::Spline { fit: false, .. }) {
                    sp.fit.clear();
                }
                sp
            });
            let ok = d.modify_entity(s.handle, |e| match (&mut e.kind, shape) {
                (EntityKind::Line(l), Shape::Line) => {
                    if let [ax, ay, bx, by] = new {
                        l.a.x = *ax;
                        l.a.y = *ay;
                        l.b.x = *bx;
                        l.b.y = *by;
                    }
                }
                (EntityKind::Circle(c), Shape::Circle) => {
                    if let [cx, cy, r] = new {
                        c.center.x = *cx;
                        c.center.y = *cy;
                        c.radius = r.abs().max(1e-12);
                    }
                }
                (EntityKind::Arc(a), Shape::Arc) => {
                    if let [cx, cy, r, s0, s1] = new {
                        // A negative radius puts every point opposite its angle: turning both
                        // ends half a turn keeps the same points with a positive radius.
                        let turn = if *r < 0.0 { std::f64::consts::PI } else { 0.0 };
                        a.center.x = *cx;
                        a.center.y = *cy;
                        a.radius = r.abs().max(1e-12);
                        a.start = cadcraft_geom::norm_angle(*s0 + turn);
                        a.end = cadcraft_geom::norm_angle(*s1 + turn);
                    }
                }
                (EntityKind::LwPolyline(p), Shape::Poly { n, .. }) if p.vertices.len() == n => {
                    for (i, v) in p.vertices.iter_mut().enumerate() {
                        if let (Some(px), Some(py)) = (new.get(2 * i), new.get(2 * i + 1)) {
                            v.p = Vec2::new(*px, *py);
                        }
                    }
                }
                (EntityKind::Point(p), Shape::Point) => {
                    if let [px, py] = new {
                        p.p.x = *px;
                        p.p.y = *py;
                    }
                }
                (EntityKind::Ellipse(el), Shape::Ellipse { arc }) => {
                    if let [cx, cy, mx, my, ratio, rest @ ..] = new {
                        let (mut major, mut ratio) = (Vec2::new(*mx, *my), ratio.abs());
                        let (mut t0, mut t1) = match rest {
                            [a, b] if arc => (*a, *b),
                            _ => (el.start, el.end),
                        };
                        // Keep the stored form: the major axis is the longer one. `storable` lets a
                        // solved ratio exceed 1 only by rounding: that is a circle, not a swap.
                        if ratio > 1.0 + RATIO_TOL {
                            major = major.perp() * ratio;
                            ratio = 1.0 / ratio;
                            if arc {
                                t0 -= std::f64::consts::FRAC_PI_2;
                                t1 -= std::f64::consts::FRAC_PI_2;
                            }
                        }
                        if major.len() > 1e-12 {
                            el.center.x = *cx;
                            el.center.y = *cy;
                            el.major.x = major.x;
                            el.major.y = major.y;
                            el.ratio = ratio.clamp(1e-9, 1.0);
                            if arc {
                                el.start = cadcraft_geom::norm_angle(t0);
                                el.end = cadcraft_geom::norm_angle(t1);
                            }
                        }
                    }
                }
                (EntityKind::Spline(sp), Shape::Spline { .. }) => {
                    if let Some(rebuilt) = spline.as_ref() {
                        *sp = rebuilt.clone();
                    }
                }
                (EntityKind::Insert(ins), Shape::Insert { .. }) => {
                    if let [ix, iy, rot] = new {
                        ins.insert.x = *ix;
                        ins.insert.y = *iy;
                        ins.rotation = cadcraft_geom::norm_angle(*rot);
                    }
                }
                _ => {}
            });
            if ok.is_ok() {
                moved.push(s.handle);
            }
        }
        moved
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use cadcraft_doc::{Block, Common, Entity, Line, Space};
    use cadcraft_geom::Vec3;
    use std::sync::Arc;

    /// Writing an unchanged model back leaves partial rational weights and the original spline geometry intact.
    #[test]
    fn write_back_keeps_partial_rational_weights() {
        let mut drawing = Drawing::default();
        let mut spline = GSpline::from_control(vec![Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 0.0)], 2);
        spline.weights = vec![2.0, 0.5];
        let h = drawing.add(&Space::Model, Common::default(), EntityKind::Spline(spline.clone())).unwrap();
        let model = Model::build(&drawing, &[h]);
        model.write_back(&mut drawing, &model.x0);
        let EntityKind::Spline(after) = &drawing.entity(h).unwrap().kind else { panic!("spline") };
        assert_eq!(after.weights, spline.weights);
        for i in 0..=10 {
            assert!(after.eval(i as f64 / 10.0).near(spline.eval(i as f64 / 10.0), 1e-12));
        }
    }

    fn add_block(d: &mut Drawing, name: &str, lines: usize) {
        let mut block = Block::new(name);
        for _ in 0..lines {
            block.entities.push(Entity {
                handle: d.new_handle(),
                common: Common::default(),
                kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) }),
            });
        }
        d.blocks.insert(name.into(), Arc::new(block));
    }

    fn insert(block: &str) -> Insert {
        Insert {
            block: block.into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
            clip: None,
        }
    }

    fn add_insert(d: &mut Drawing, block: &str) -> Handle {
        d.add(&Space::Model, Common::default(), EntityKind::Insert(insert(block))).unwrap()
    }

    /// The member budget is shared by every model of one preparation, and a block it already
    /// sampled is reused by later Inserts without examining its members again.
    #[test]
    fn rigid_sampling_budget_is_shared_and_blocks_sampled_once() {
        let mut d = Drawing::default();
        add_block(&mut d, "BAR", 3);
        add_block(&mut d, "BAZ", 3);
        let (bar, baz, bar2) = (add_insert(&mut d, "BAR"), add_insert(&mut d, "BAZ"), add_insert(&mut d, "BAR"));
        let mut prep = Prep::new(2);
        let first = Model::build_with(&d, &[bar], &mut prep);
        assert_eq!(first.rigid[&bar].len(), 4);
        assert_eq!(prep.members_left, 0);
        let other = Model::build_with(&d, &[baz], &mut prep);
        assert!(other.rigid[&baz].is_empty());
        let again = Model::build_with(&d, &[bar2], &mut prep);
        assert_eq!(again.rigid[&bar2].len(), 4);
    }

    /// Block names resolve ignoring ASCII case, as `Drawing::block` does, through one index.
    #[test]
    fn block_names_resolve_ignoring_case() {
        let mut d = Drawing::default();
        add_block(&mut d, "Bar", 2);
        let (lower, missing) = (add_insert(&mut d, "bAR"), add_insert(&mut d, "nope"));
        let mut prep = Prep::new(MAX_RIGID_WORK);
        let m = Model::build_with(&d, &[lower, missing], &mut prep);
        assert_eq!(m.rigid[&lower].len(), 4);
        assert!(m.rigid[&missing].is_empty());
        assert!(prep.folded.as_ref().is_some_and(|f| f.len() == 1));
        assert_eq!(rigid_points(&d, &insert("BAR")).len(), 4);
    }

    /// A spline asked for again at the same defining points is not rebuilt, also from another
    /// model of the same preparation; moved points rebuild it.
    #[test]
    fn spline_rebuilds_are_reused_at_the_same_points() {
        let mut d = Drawing::default();
        let pts: Vec<Vec2> = (0..6).map(|i| Vec2::new(i as f64, (i % 2) as f64)).collect();
        let h = d.add(&Space::Model, Common::default(), EntityKind::Spline(GSpline::fit_with(&pts, false, Default::default()))).unwrap();
        let mut prep = Prep::new(MAX_RIGID_WORK);
        let a = Model::build_with(&d, &[h], &mut prep);
        let b = Model::build_with(&d, &[h], &mut prep);
        let (sa, sb) = (a.slot(h).unwrap(), b.slot(h).unwrap());
        assert!(matches!(sa.shape, Shape::Spline { fit: true, .. }));
        let first = a.spline(&a.x0, sa).unwrap();
        assert!(Rc::ptr_eq(&first, &a.spline(&a.x0, sa).unwrap()));
        assert!(Rc::ptr_eq(&first, &b.spline(&b.x0, sb).unwrap()));
        let mut moved = a.x0.clone();
        moved[1] += 0.5;
        let other = a.spline(&moved, sa).unwrap();
        assert!(!Rc::ptr_eq(&first, &other));
        assert_eq!(other.fit[0], Vec2::new(0.0, 0.5));
    }

    /// A control spline's template keeps no fit points and no excess weights.
    #[test]
    fn control_template_drops_unused_data() {
        let mut d = Drawing::default();
        let mut sp = GSpline::from_control(vec![Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 0.0), Vec2::new(4.0, 1.0)], 3);
        sp.fit = vec![Vec2::ZERO; 10_000];
        sp.fit_opts.tolerance = 0.5;
        sp.weights = vec![1.0; 10_000];
        let h = d.add(&Space::Model, Common::default(), EntityKind::Spline(sp)).unwrap();
        let m = Model::build(&d, &[h]);
        let t = &m.splines[&h];
        assert!(t.fit.is_empty());
        assert_eq!(t.weights.len(), 4);
    }
}
