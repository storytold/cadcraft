//! `vlax-curve-*`: lengths, points, parameters, derivatives and areas along a curve entity.
//!
//! Each kind keeps AutoLISP's parameter: a line's is the distance from its start, an arc's or a
//! circle's the angle, a polyline's the vertex index (a fraction of the way along the segment
//! after it), an ellipse's its own parameter, a spline's its knot parameter. Everything is
//! exact: ellipses and splines are evaluated on the curve itself, and their lengths and areas
//! integrated along it (the same arc lengths LENGTHEN uses); a fine flattening only finds where
//! to start looking for a closest point.

use cadcraft_doc::{Entity, EntityKind};
use cadcraft_geom::{Arc as GArc, Ellipse as GEllipse, Spline as GSpline, Vec2, bulge_to_arc};

use super::LispError;
use super::builtins::args_err;
use super::eval::{Ctx, Flow};
use super::value::Value;

type P3 = [f64; 3];

/// One stretch of the curve: a line or an arc, over parameters `p0..p1`, starting `d0` along.
#[derive(Clone, Debug)]
struct Piece {
    p0: f64,
    p1: f64,
    d0: f64,
    seg: Seg,
}

#[derive(Clone, Debug)]
enum Seg {
    Line(P3, P3),
    /// From angle `a0`, turning `turn` radians (negative: clockwise), at height `z`.
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        turn: f64,
        z: f64,
    },
}

impl Seg {
    fn len(&self) -> f64 {
        match self {
            Seg::Line(a, b) => dist3(a, b),
            Seg::Arc { r, turn, .. } => r * turn.abs(),
        }
    }
    fn at(&self, t: f64) -> P3 {
        match self {
            Seg::Line(a, b) => lerp3(a, b, t),
            Seg::Arc { c, r, a0, turn, z } => {
                let q = Vec2::polar(*c, *r, a0 + turn * t);
                [q.x, q.y, *z]
            }
        }
    }
    /// d/dt and d²/dt² at `t` (t from 0 to 1 along the piece).
    fn derivs(&self, t: f64) -> (P3, P3) {
        match self {
            Seg::Line(a, b) => ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [0.0; 3]),
            Seg::Arc { r, a0, turn, .. } => {
                let th = a0 + turn * t;
                let (s, c) = th.sin_cos();
                ([-r * turn * s, r * turn * c, 0.0], [-r * turn * turn * c, -r * turn * turn * s, 0.0])
            }
        }
    }
    /// The nearest point's `t` (0..1).
    fn closest(&self, p: &P3) -> f64 {
        match self {
            Seg::Line(a, b) => {
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let l2 = dot3(&d, &d);
                if l2 <= 1e-30 { 0.0 } else { (dot3(&[p[0] - a[0], p[1] - a[1], p[2] - a[2]], &d) / l2).clamp(0.0, 1.0) }
            }
            Seg::Arc { c, a0, turn, .. } => {
                if turn.abs() <= 1e-15 {
                    return 0.0;
                }
                let q = Vec2::new(p[0], p[1]);
                if q.dist(*c) <= 1e-15 {
                    return 0.0;
                }
                // Angle from the start in the direction of travel, 0..2π.
                let rel = (q.angle_to_from(*c) - a0) * turn.signum();
                let rel = rel.rem_euclid(std::f64::consts::TAU);
                let span = turn.abs();
                if rel <= span {
                    return rel / span;
                }
                // Outside the arc: the nearer end.
                let (s, e) = (self.at(0.0), self.at(1.0));
                if dist3(&s, p) <= dist3(&e, p) { 0.0 } else { 1.0 }
            }
        }
    }
    /// The nearest point on the line or the whole circle the piece lies on.
    fn closest_extended(&self, p: &P3) -> P3 {
        match self {
            Seg::Line(a, b) => {
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let l2 = dot3(&d, &d);
                let t = if l2 <= 1e-30 { 0.0 } else { dot3(&[p[0] - a[0], p[1] - a[1], p[2] - a[2]], &d) / l2 };
                lerp3(a, b, t)
            }
            Seg::Arc { c, r, z, .. } => {
                let q = Vec2::new(p[0], p[1]) - *c;
                let u = if q.len() <= 1e-15 { Vec2::new(1.0, 0.0) } else { q.normalized() };
                [c.x + u.x * r, c.y + u.y * r, *z]
            }
        }
    }
    /// ∫ (x dy − y dx) / 2 along the piece: its share of the enclosed area.
    fn area_term(&self) -> f64 {
        match self {
            Seg::Line(a, b) => (a[0] * b[1] - b[0] * a[1]) / 2.0,
            Seg::Arc { c, r, turn, .. } => {
                let (s, e) = (self.at(0.0), self.at(1.0));
                (c.x * (e[1] - s[1]) - c.y * (e[0] - s[0]) + r * r * turn) / 2.0
            }
        }
    }
}

trait AngleFrom {
    fn angle_to_from(self, c: Vec2) -> f64;
}
impl AngleFrom for Vec2 {
    fn angle_to_from(self, c: Vec2) -> f64 {
        (self.y - c.y).atan2(self.x - c.x)
    }
}

fn dot3(a: &P3, b: &P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn dist3(a: &P3, b: &P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn lerp3(a: &P3, b: &P3, t: f64) -> P3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn scale3(a: &P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// What evaluates the curve exactly at a parameter, where the pieces are only a flattening.
#[derive(Clone, Debug)]
enum Exact {
    None,
    Ellipse(GEllipse, f64),
    Spline(GSpline),
}

/// A curve entity, ready to measure.
#[derive(Clone, Debug)]
pub struct Curve {
    pieces: Vec<Piece>,
    exact: Exact,
    start: f64,
    end: f64,
    /// The whole length: the pieces' for lines, arcs and polylines, the exact curve's otherwise.
    len: f64,
    closed: bool,
    periodic: bool,
    planar: bool,
}

/// Most points a flattened ellipse or spline is searched along.
const MAX_SAMPLES: usize = 16_384;

impl Curve {
    /// The curve an entity is, or `None` for one that isn't (text, blocks, hatches…).
    pub fn of(e: &Entity) -> Option<Curve> {
        let tau = std::f64::consts::TAU;
        let c = match &e.kind {
            EntityKind::Line(l) => {
                let (a, b) = ([l.a.x, l.a.y, l.a.z], [l.b.x, l.b.y, l.b.z]);
                let len = dist3(&a, &b);
                Curve::from_pieces(vec![(0.0, len, Seg::Line(a, b))], Exact::None, false, false, true)
            }
            EntityKind::Arc(a) => {
                let sweep = GArc::new(a.center.xy(), a.radius, a.start, a.end).sweep();
                let seg = Seg::Arc { c: a.center.xy(), r: a.radius, a0: a.start, turn: sweep, z: a.center.z };
                Curve::from_pieces(vec![(a.start, a.start + sweep, seg)], Exact::None, false, false, true)
            }
            EntityKind::Circle(c) => {
                let seg = Seg::Arc { c: c.center.xy(), r: c.radius, a0: 0.0, turn: tau, z: c.center.z };
                Curve::from_pieces(vec![(0.0, tau, seg)], Exact::None, true, true, true)
            }
            EntityKind::LwPolyline(p) => {
                let z = p.elevation;
                // One piece per vertex, zero-length ones included: parameters count every vertex.
                let n = if p.closed { p.vertices.len() } else { p.vertices.len().saturating_sub(1) };
                let mut pieces = Vec::new();
                for i in 0..n {
                    let Some(a) = p.vertices.get(i) else { break };
                    let Some(b) = p.vertices.get((i + 1) % p.vertices.len().max(1)) else { break };
                    let seg = match bulge_to_arc(a.p, b.p, a.bulge) {
                        Some((arc, ccw)) => {
                            let turn = if ccw { arc.sweep() } else { -arc.sweep() };
                            let a0 = if ccw { arc.start } else { arc.start + arc.sweep() };
                            Seg::Arc { c: arc.center, r: arc.radius, a0, turn, z }
                        }
                        None => Seg::Line([a.p.x, a.p.y, z], [b.p.x, b.p.y, z]),
                    };
                    pieces.push((i as f64, (i + 1) as f64, seg));
                }
                if pieces.is_empty() {
                    let v = p.vertices.first()?;
                    pieces.push((0.0, 0.0, Seg::Line([v.p.x, v.p.y, z], [v.p.x, v.p.y, z])));
                }
                Curve::from_pieces(pieces, Exact::None, p.closed, false, true)
            }
            EntityKind::Polyline3d(p) => {
                let pts: Vec<P3> = p.points.iter().map(|q| [q.x, q.y, q.z]).collect();
                let n = if p.closed { pts.len() } else { pts.len().saturating_sub(1) };
                let mut pieces = Vec::new();
                for i in 0..n {
                    let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % pts.len().max(1))) else { break };
                    pieces.push((i as f64, (i + 1) as f64, Seg::Line(*a, *b)));
                }
                if pieces.is_empty() {
                    let a = *pts.first()?;
                    pieces.push((0.0, 0.0, Seg::Line(a, a)));
                }
                let planar = coplanar(&pts);
                Curve::from_pieces(pieces, Exact::None, p.closed, false, planar)
            }
            EntityKind::Ellipse(el) => {
                let ge = GEllipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
                let full = ge.is_full();
                let (t0, sweep) = if full { (0.0, tau) } else { (el.start, ge.sweep()) };
                let n = ((sweep / tau * 4096.0).ceil() as usize).clamp(64, MAX_SAMPLES);
                let z = el.center.z;
                let pts: Vec<(f64, P3)> = (0..=n)
                    .map(|i| {
                        let t = t0 + sweep * i as f64 / n as f64;
                        let q = ge.at_param(t);
                        (t, [q.x, q.y, z])
                    })
                    .collect();
                Curve::from_pieces(flattened(&pts), Exact::Ellipse(ge, z), full, full, true)
                    .map(|c| Curve { len: ge.arc_length_from(t0, sweep), ..c })
            }
            EntityKind::Spline(sp) => {
                if !sp.is_valid() {
                    return None;
                }
                let (lo, hi) = sp.domain();
                if !(lo.is_finite() && hi.is_finite() && hi >= lo) {
                    return None;
                }
                let n = (sp.control.len().saturating_mul(256)).clamp(1024, MAX_SAMPLES);
                let pts: Vec<(f64, P3)> = (0..=n)
                    .map(|i| {
                        let t = lo + (hi - lo) * i as f64 / n as f64;
                        let q = sp.eval(t);
                        (t, [q.x, q.y, 0.0])
                    })
                    .collect();
                Curve::from_pieces(flattened(&pts), Exact::Spline(sp.clone()), sp.closed, sp.closed && sp.is_periodic(), true)
                    .map(|c| Curve { len: sp.length(), ..c })
            }
            _ => return None,
        };
        c.filter(|c| c.len.is_finite() && c.pieces.iter().all(|p| p.p0.is_finite() && p.p1.is_finite() && p.seg.len().is_finite()))
    }

    fn from_pieces(raw: Vec<(f64, f64, Seg)>, exact: Exact, closed: bool, periodic: bool, planar: bool) -> Option<Curve> {
        let start = raw.first()?.0;
        let end = raw.last()?.1;
        let mut d = 0.0;
        let pieces = raw
            .into_iter()
            .map(|(p0, p1, seg)| {
                let piece = Piece { p0, p1, d0: d, seg };
                d += piece.seg.len();
                piece
            })
            .collect();
        Some(Curve { pieces, exact, start, end, len: d, closed, periodic, planar })
    }

    pub fn start_param(&self) -> f64 {
        self.start
    }
    pub fn end_param(&self) -> f64 {
        self.end
    }
    pub fn length(&self) -> f64 {
        self.len
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn is_periodic(&self) -> bool {
        self.periodic
    }
    pub fn is_planar(&self) -> bool {
        self.planar
    }

    fn tol(&self) -> f64 {
        1e-9 * (1.0 + self.length())
    }

    /// The piece holding parameter `t` and how far along it (0..1); `None` off the curve.
    fn at_param(&self, t: f64) -> Option<(&Piece, f64)> {
        let eps = 1e-9 * (1.0 + (self.end - self.start).abs());
        if !t.is_finite() || t < self.start - eps || t > self.end + eps {
            return None;
        }
        let t = t.clamp(self.start, self.end);
        let p = self.pieces.iter().find(|p| t <= p.p1 + 1e-12).or(self.pieces.last())?;
        let span = p.p1 - p.p0;
        Some((p, if span.abs() <= 1e-15 { 0.0 } else { ((t - p.p0) / span).clamp(0.0, 1.0) }))
    }

    /// `d` kept within the curve's length; `None` past either end.
    fn in_length(&self, d: f64) -> Option<f64> {
        let eps = self.tol();
        (d.is_finite() && d >= -eps && d <= self.len + eps).then(|| d.clamp(0.0, self.len))
    }

    /// The piece `d` along the pieces and how far along it (0..1); `None` past either end.
    fn at_dist(&self, d: f64) -> Option<(&Piece, f64)> {
        let d = self.in_length(d)?;
        let p = self.pieces.iter().find(|p| d <= p.d0 + p.seg.len() + 1e-12).or(self.pieces.last())?;
        let l = p.seg.len();
        Some((p, if l <= 1e-15 { 0.0 } else { ((d - p.d0) / l).clamp(0.0, 1.0) }))
    }

    fn param_of(p: &Piece, f: f64) -> f64 {
        p.p0 + (p.p1 - p.p0) * f
    }

    pub fn point_at_param(&self, t: f64) -> Option<P3> {
        let (p, f) = self.at_param(t)?;
        Some(match &self.exact {
            Exact::None => p.seg.at(f),
            Exact::Ellipse(e, z) => {
                let q = e.at_param(t);
                [q.x, q.y, *z]
            }
            Exact::Spline(sp) => {
                let q = sp.eval(t);
                [q.x, q.y, 0.0]
            }
        })
    }

    pub fn dist_at_param(&self, t: f64) -> Option<f64> {
        let (p, f) = self.at_param(t)?;
        let t = t.clamp(self.start, self.end);
        Some(match &self.exact {
            Exact::None => p.d0 + p.seg.len() * f,
            Exact::Ellipse(e, _) => e.arc_length_from(self.start, t - self.start),
            Exact::Spline(sp) => sp.length_between(self.start, t),
        })
    }

    pub fn param_at_dist(&self, d: f64) -> Option<f64> {
        if let Exact::None = self.exact {
            let (p, f) = self.at_dist(d)?;
            return Some(Curve::param_of(p, f));
        }
        let d = self.in_length(d)?;
        if d >= self.len {
            return Some(self.end);
        }
        if let Some(t) = self.newton_param_at(d) {
            return Some(t);
        }
        Some(match &self.exact {
            Exact::Ellipse(e, _) => e.param_at_length(self.start, d).unwrap_or(self.end).clamp(self.start, self.end),
            Exact::Spline(sp) => sp.param_at_length(self.start, d),
            Exact::None => self.start,
        })
    }

    /// Exact arc length from parameter `a` to `b` (negative when `b` comes first).
    fn exact_len_between(&self, a: f64, b: f64) -> f64 {
        let (lo, hi, sign) = if a <= b { (a, b, 1.0) } else { (b, a, -1.0) };
        sign * match &self.exact {
            Exact::Ellipse(e, _) => e.arc_length_from(lo, hi - lo),
            Exact::Spline(sp) => sp.length_between(lo, hi),
            Exact::None => 0.0,
        }
    }

    /// The speed |dC/dt| of an ellipse or a spline.
    fn exact_speed(&self, t: f64) -> f64 {
        match &self.exact {
            Exact::Ellipse(e, _) => e.derivative(t).len(),
            Exact::Spline(sp) => sp.derivative(t).len(),
            Exact::None => 0.0,
        }
    }

    /// The parameter `d` along an ellipse or a spline: the flattening's guess, then Newton steps
    /// on the exact arc length (whose derivative is the speed), measured from the guessed piece's
    /// start so that each step integrates only a short stretch. `None` when it doesn't settle
    /// (a cusp, where the speed vanishes), for the slower bisection to answer.
    fn newton_param_at(&self, d: f64) -> Option<f64> {
        let chord = self.pieces.last().map_or(0.0, |p| p.d0 + p.seg.len());
        let g = if self.len > 0.0 { (d * chord / self.len).clamp(0.0, chord) } else { 0.0 };
        let p = self.pieces.iter().find(|p| g <= p.d0 + p.seg.len() + 1e-12).or(self.pieces.last())?;
        let l = p.seg.len();
        let mut t = Curve::param_of(p, if l <= 1e-15 { 0.0 } else { ((g - p.d0) / l).clamp(0.0, 1.0) });
        let (base_t, base) = (p.p0, self.exact_len_between(self.start, p.p0));
        let tol = 1e-12 * (1.0 + self.len);
        for _ in 0..16 {
            let err = base + self.exact_len_between(base_t, t) - d;
            if !err.is_finite() {
                return None;
            }
            if err.abs() <= tol {
                return Some(t);
            }
            let speed = self.exact_speed(t);
            if !(speed.is_finite() && speed > 1e-12 * (1.0 + self.len)) {
                return None;
            }
            let next = (t - err / speed).clamp(self.start, self.end);
            if next == t {
                // Held at an end short of `d`: leave it to the bisection.
                return None;
            }
            t = next;
        }
        None
    }

    pub fn point_at_dist(&self, d: f64) -> Option<P3> {
        let t = self.param_at_dist(d)?;
        self.point_at_param(t)
    }

    /// First and second derivatives with respect to the parameter.
    pub fn derivs(&self, t: f64) -> Option<(P3, P3)> {
        let (p, f) = self.at_param(t)?;
        Some(match &self.exact {
            Exact::None => {
                let span = p.p1 - p.p0;
                if span.abs() <= 1e-15 {
                    return Some(([0.0; 3], [0.0; 3]));
                }
                let (d1, d2) = p.seg.derivs(f);
                (scale3(&d1, 1.0 / span), scale3(&d2, 1.0 / (span * span)))
            }
            Exact::Ellipse(e, _) => {
                let (a, b) = (e.major, e.minor());
                let (s, c) = t.sin_cos();
                ([-a.x * s + b.x * c, -a.y * s + b.y * c, 0.0], [-a.x * c - b.x * s, -a.y * c - b.y * s, 0.0])
            }
            Exact::Spline(sp) => {
                // The exact first derivative; the second by central differences on it, one-sided
                // at the ends.
                let t = t.clamp(self.start, self.end);
                let d1 = sp.derivative(t);
                let h = 1e-5 * (self.end - self.start).abs().max(1e-9);
                let (lo, hi) = ((t - h).max(self.start), (t + h).min(self.end));
                let w = hi - lo;
                if w <= 0.0 {
                    return Some(([d1.x, d1.y, 0.0], [0.0; 3]));
                }
                let d2 = (sp.derivative(hi) - sp.derivative(lo)) * (1.0 / w);
                ([d1.x, d1.y, 0.0], [d2.x, d2.y, 0.0])
            }
        })
    }

    /// The parameter of the point nearest `q`, and that point.
    pub fn closest(&self, q: &P3) -> Option<(f64, P3)> {
        let mut best: Option<(f64, f64)> = None;
        for p in &self.pieces {
            let f = p.seg.closest(q);
            let d = dist3(&p.seg.at(f), q);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, Curve::param_of(p, f)));
            }
        }
        let (_, mut t) = best?;
        // Refine on the exact curve (the pieces are a flattening there).
        match &self.exact {
            Exact::None => {}
            Exact::Ellipse(e, _) => {
                if self.closed {
                    t = e.param_of(Vec2::new(q[0], q[1])).rem_euclid(std::f64::consts::TAU);
                }
                t = newton(self, q, t);
            }
            Exact::Spline(_) => t = newton(self, q, t),
        }
        Some((t, self.point_at_param(t)?))
    }

    /// With `extend`, an open curve's ends are carried on along their line or circle.
    pub fn closest_point(&self, q: &P3, extend: bool) -> Option<P3> {
        let (_, mut best) = self.closest(q)?;
        if extend && !self.closed {
            let ends = [self.pieces.first(), self.pieces.last()];
            for p in ends.into_iter().flatten() {
                let e = match (&self.exact, &p.seg) {
                    (Exact::None, seg) => seg.closest_extended(q),
                    // An open ellipse goes on round the whole ellipse.
                    (Exact::Ellipse(e, z), _) => {
                        let full = GEllipse { start: 0.0, end: std::f64::consts::TAU, ..*e };
                        let v = full.closest(Vec2::new(q[0], q[1]));
                        [v.x, v.y, *z]
                    }
                    (Exact::Spline(_), _) => continue,
                };
                if dist3(&e, q) < dist3(&best, q) {
                    best = e;
                }
            }
        }
        Some(best)
    }

    /// The parameter of a point on the curve; `None` for a point off it.
    pub fn param_at_point(&self, q: &P3) -> Option<f64> {
        let (t, p) = self.closest(q)?;
        (dist3(&p, q) <= 1e-6 * (1.0 + self.length())).then_some(t)
    }

    /// The area the curve encloses (an open one closed by the chord between its ends).
    pub fn area(&self) -> f64 {
        let mut a: f64 = match &self.exact {
            Exact::Ellipse(e, _) => {
                let (t0, t1) = (self.start, self.end);
                let (p0, p1) = (e.at_param(t0), e.at_param(t1));
                let (c, ma, mi) = (e.center, e.major, e.minor());
                (c.x * (p1.y - p0.y) - c.y * (p1.x - p0.x) + (ma.x * mi.y - ma.y * mi.x) * (t1 - t0)) / 2.0
            }
            // Integrated on the exact curve, seen from its start point, so the closing chord adds
            // nothing.
            Exact::Spline(sp) => return sp.area_term_between(self.start, self.end, sp.eval(self.start)).abs(),
            Exact::None => self.pieces.iter().map(|p| p.seg.area_term()).sum(),
        };
        if !self.closed
            && let (Some(s), Some(e)) = (self.point_at_param(self.start), self.point_at_param(self.end))
        {
            a += (e[0] * s[1] - s[0] * e[1]) / 2.0;
        }
        a.abs()
    }
}

/// Lines between consecutive (parameter, point) samples.
fn flattened(pts: &[(f64, P3)]) -> Vec<(f64, f64, Seg)> {
    pts.windows(2).filter_map(|w| Some((w.first()?.0, w.get(1)?.0, Seg::Line(w.first()?.1, w.get(1)?.1)))).collect()
}

/// A few Newton steps on (C(t) − q)·C'(t) = 0, kept within the curve's parameters.
fn newton(c: &Curve, q: &P3, mut t: f64) -> f64 {
    for _ in 0..8 {
        let (Some(p), Some((d1, d2))) = (c.point_at_param(t), c.derivs(t)) else { break };
        let r = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
        let f = dot3(&r, &d1);
        let df = dot3(&d1, &d1) + dot3(&r, &d2);
        if df.abs() <= 1e-30 || !f.is_finite() || !df.is_finite() {
            break;
        }
        let next = (t - f / df).clamp(c.start, c.end);
        if (next - t).abs() <= 1e-14 * (1.0 + t.abs()) {
            t = next;
            break;
        }
        // Keep a step only when it brings the point nearer.
        match c.point_at_param(next) {
            Some(pn) if dist3(&pn, q) <= dist3(&p, q) => t = next,
            _ => break,
        }
    }
    t
}

/// Whether 3D points lie in one plane.
fn coplanar(pts: &[P3]) -> bool {
    let Some(o) = pts.first() else { return true };
    let sub = |a: &P3| [a[0] - o[0], a[1] - o[1], a[2] - o[2]];
    let cross = |a: &P3, b: &P3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let scale = pts.iter().map(|p| dist3(p, o)).fold(0.0, f64::max).max(1e-12);
    let mut normal: Option<P3> = None;
    for (i, a) in pts.iter().enumerate().skip(1) {
        for b in pts.iter().skip(i + 1) {
            let n = cross(&sub(a), &sub(b));
            if dot3(&n, &n).sqrt() > 1e-9 * scale * scale {
                normal = Some(n);
                break;
            }
        }
        if normal.is_some() {
            break;
        }
    }
    let Some(n) = normal else { return true };
    let nl = dot3(&n, &n).sqrt();
    pts.iter().all(|p| (dot3(&sub(p), &n) / nl).abs() <= 1e-9 * scale)
}

// ---------------- the builtins ----------------

fn arity(a: &[Value], min: usize, max: usize) -> Result<(), LispError> {
    if a.len() < min {
        return Err(args_err(true));
    }
    if a.len() > max {
        return Err(args_err(false));
    }
    Ok(())
}

/// The curve an argument names (an entity name).
fn curve_arg(ctx: &mut Ctx<'_>, v: &Value) -> Result<Curve, LispError> {
    let Value::Ename(h) = v else { return Err(LispError::bad_arg("lentityp", v)) };
    let d = ctx.s.doc().map_err(|e| LispError::new(e.to_string()))?;
    let e = d.entity(*h).ok_or_else(|| LispError::bad_arg("lentityp", v))?;
    Curve::of(e).ok_or_else(|| LispError::bad_arg("curvep", v))
}

fn num(v: &Value) -> Result<f64, LispError> {
    v.as_number().ok_or_else(|| LispError::bad_arg("numberp", v))
}

fn point(v: &Value) -> Result<P3, LispError> {
    v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", v))
}

fn val_point(p: Option<P3>) -> Value {
    p.map(|p| Value::point(&p)).unwrap_or_default()
}
fn val_real(r: Option<f64>) -> Value {
    r.map(Value::Real).unwrap_or_default()
}
fn done(v: Value) -> Result<Flow, LispError> {
    Ok(Flow::Value(v))
}

pub fn get_start_param(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    done(Value::Real(curve_arg(ctx, &a[0])?.start_param()))
}
pub fn get_end_param(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    done(Value::Real(curve_arg(ctx, &a[0])?.end_param()))
}
pub fn get_start_point(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_point(c.point_at_param(c.start_param())))
}
pub fn get_end_point(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_point(c.point_at_param(c.end_param())))
}
pub fn get_dist_at_param(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_real(c.dist_at_param(num(&a[1])?)))
}
pub fn get_param_at_dist(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_real(c.param_at_dist(num(&a[1])?)))
}
pub fn get_point_at_param(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_point(c.point_at_param(num(&a[1])?)))
}
pub fn get_point_at_dist(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_point(c.point_at_dist(num(&a[1])?)))
}
pub fn get_param_at_point(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_real(c.param_at_point(&point(&a[1])?)))
}
pub fn get_dist_at_point(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    let t = c.param_at_point(&point(&a[1])?);
    done(val_real(t.and_then(|t| c.dist_at_param(t))))
}
pub fn get_closest_point_to(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 3)?;
    let c = curve_arg(ctx, &a[0])?;
    let extend = a.get(2).is_some_and(|v| !v.is_nil());
    done(val_point(c.closest_point(&point(&a[1])?, extend)))
}
/// `(vlax-curve-getClosestPointToProjection curve point normal [extend])`: the curves here lie
/// in planes square to Z, so for a Z normal this is the closest point to the point dropped onto
/// that plane.
pub fn get_closest_point_to_projection(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 3, 4)?;
    let c = curve_arg(ctx, &a[0])?;
    let q = point(&a[1])?;
    let n = point(&a[2])?;
    let nl = dot3(&n, &n).sqrt();
    if nl <= 1e-12 {
        return Err(LispError::bad_arg("vector", &a[2]));
    }
    let extend = a.get(3).is_some_and(|v| !v.is_nil());
    // Slide the point along the normal into the curve's plane (z of its start).
    let z0 = c.point_at_param(c.start_param()).map_or(0.0, |p| p[2]);
    let q = if n[2].abs() > 1e-12 * nl {
        let k = (z0 - q[2]) / n[2];
        [q[0] + n[0] * k, q[1] + n[1] * k, z0]
    } else {
        q
    };
    done(val_point(c.closest_point(&q, extend)))
}
pub fn get_first_deriv(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_point(c.derivs(num(&a[1])?).map(|d| d.0)))
}
pub fn get_second_deriv(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let c = curve_arg(ctx, &a[0])?;
    done(val_point(c.derivs(num(&a[1])?).map(|d| d.1)))
}
pub fn get_area(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    done(Value::Real(curve_arg(ctx, &a[0])?.area()))
}
pub fn is_closed(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    done(Value::bool(curve_arg(ctx, &a[0])?.is_closed()))
}
pub fn is_periodic(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    done(Value::bool(curve_arg(ctx, &a[0])?.is_periodic()))
}
pub fn is_planar(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    done(Value::bool(curve_arg(ctx, &a[0])?.is_planar()))
}
