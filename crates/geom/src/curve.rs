use serde::{Deserialize, Serialize};

use crate::{Bounds2, EPS, PI, TAU, Vec2, angle_in_sweep, arc_segments, ccw_sweep, norm_angle};

/// A straight segment.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub a: Vec2,
    pub b: Vec2,
}

impl Line {
    pub fn new(a: Vec2, b: Vec2) -> Self {
        Line { a, b }
    }
    pub fn len(&self) -> f64 {
        self.a.dist(self.b)
    }
    pub fn dir(&self) -> Vec2 {
        (self.b - self.a).normalized()
    }
    pub fn mid(&self) -> Vec2 {
        self.a.mid(self.b)
    }
    /// Parameter (0..1 on the segment) of the projection of `p`.
    pub fn param_of(&self, p: Vec2) -> f64 {
        let d = self.b - self.a;
        let l2 = d.len2();
        if l2 <= EPS * EPS { 0.0 } else { (p - self.a).dot(d) / l2 }
    }
    pub fn at(&self, t: f64) -> Vec2 {
        self.a.lerp(self.b, t)
    }
    /// Closest point on the segment.
    pub fn closest(&self, p: Vec2) -> Vec2 {
        self.at(self.param_of(p).clamp(0.0, 1.0))
    }
    /// Foot of the perpendicular on the infinite line.
    pub fn project(&self, p: Vec2) -> Vec2 {
        self.at(self.param_of(p))
    }
    pub fn dist(&self, p: Vec2) -> f64 {
        self.closest(p).dist(p)
    }
    /// Signed side: > 0 left of a→b.
    pub fn side(&self, p: Vec2) -> f64 {
        (self.b - self.a).cross(p - self.a)
    }
    pub fn offset(&self, d: f64) -> Line {
        let n = (self.b - self.a).normalized().perp() * d;
        Line::new(self.a + n, self.b + n)
    }
}

/// A circular arc, CCW from `start` to `end` angle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    pub center: Vec2,
    pub radius: f64,
    pub start: f64,
    pub end: f64,
}

impl Arc {
    pub fn new(center: Vec2, radius: f64, start: f64, end: f64) -> Self {
        Arc { center, radius: radius.abs(), start: norm_angle(start), end: norm_angle(end) }
    }
    pub fn sweep(&self) -> f64 {
        ccw_sweep(self.start, self.end)
    }
    pub fn start_point(&self) -> Vec2 {
        Vec2::polar(self.center, self.radius, self.start)
    }
    pub fn end_point(&self) -> Vec2 {
        Vec2::polar(self.center, self.radius, self.end)
    }
    pub fn mid_point(&self) -> Vec2 {
        Vec2::polar(self.center, self.radius, self.start + self.sweep() / 2.0)
    }
    pub fn len(&self) -> f64 {
        self.radius * self.sweep()
    }
    pub fn contains_angle(&self, a: f64) -> bool {
        angle_in_sweep(a, self.start, self.end)
    }
    /// Point at parameter `t` in 0..1 along the sweep.
    pub fn at(&self, t: f64) -> Vec2 {
        Vec2::polar(self.center, self.radius, self.start + self.sweep() * t)
    }
    /// Arc through three points, or `None` when collinear.
    pub fn from_3_points(p1: Vec2, p2: Vec2, p3: Vec2) -> Option<Arc> {
        let c = Circle::from_3_points(p1, p2, p3)?;
        let a1 = c.center.angle_to(p1);
        let a2 = c.center.angle_to(p2);
        let a3 = c.center.angle_to(p3);
        // CCW from p1 to p3 must pass p2; otherwise go the other way.
        if angle_in_sweep(a2, a1, a3) { Some(Arc::new(c.center, c.radius, a1, a3)) } else { Some(Arc::new(c.center, c.radius, a3, a1)) }
    }
    /// Arc from start point, center and end point (CCW).
    pub fn from_start_center_end(s: Vec2, c: Vec2, e: Vec2) -> Arc {
        Arc::new(c, s.dist(c), c.angle_to(s), c.angle_to(e))
    }
    pub fn closest(&self, p: Vec2) -> Vec2 {
        let a = self.center.angle_to(p);
        if self.contains_angle(a) && p.dist(self.center) > EPS {
            Vec2::polar(self.center, self.radius, a)
        } else {
            let s = self.start_point();
            let e = self.end_point();
            if s.dist(p) <= e.dist(p) { s } else { e }
        }
    }
    pub fn bounds(&self) -> Bounds2 {
        let mut b = Bounds2::from_points([self.start_point(), self.end_point()]);
        for k in 0..4 {
            let a = k as f64 * PI / 2.0;
            if self.contains_angle(a) {
                b.add(Vec2::polar(self.center, self.radius, a));
            }
        }
        b
    }
    pub fn tessellate(&self, tol: f64, out: &mut Vec<Vec2>) {
        let sweep = self.sweep();
        let n = arc_segments(self.radius, sweep, tol);
        for i in 0..=n {
            out.push(Vec2::polar(self.center, self.radius, self.start + sweep * i as f64 / n as f64));
        }
    }
}

/// A full circle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub center: Vec2,
    pub radius: f64,
}

impl Circle {
    pub fn new(center: Vec2, radius: f64) -> Self {
        Circle { center, radius: radius.abs() }
    }
    pub fn from_3_points(p1: Vec2, p2: Vec2, p3: Vec2) -> Option<Circle> {
        let d = 2.0 * (p1.x * (p2.y - p3.y) + p2.x * (p3.y - p1.y) + p3.x * (p1.y - p2.y));
        if d.abs() < 1e-12 {
            return None;
        }
        let s1 = p1.len2();
        let s2 = p2.len2();
        let s3 = p3.len2();
        let ux = (s1 * (p2.y - p3.y) + s2 * (p3.y - p1.y) + s3 * (p1.y - p2.y)) / d;
        let uy = (s1 * (p3.x - p2.x) + s2 * (p1.x - p3.x) + s3 * (p2.x - p1.x)) / d;
        let c = Vec2::new(ux, uy);
        Some(Circle::new(c, c.dist(p1)))
    }
    pub fn from_2_points(p1: Vec2, p2: Vec2) -> Circle {
        Circle::new(p1.mid(p2), p1.dist(p2) / 2.0)
    }
    pub fn closest(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        if d.len() < EPS { self.center + Vec2::X * self.radius } else { self.center + d.normalized() * self.radius }
    }
    pub fn bounds(&self) -> Bounds2 {
        Bounds2::new(self.center - Vec2::new(self.radius, self.radius), self.center + Vec2::new(self.radius, self.radius))
    }
    pub fn tessellate(&self, tol: f64, out: &mut Vec<Vec2>) {
        Arc { center: self.center, radius: self.radius, start: 0.0, end: TAU }.tessellate(tol, out);
    }
    /// Tangent points from external point `p`.
    pub fn tangent_points(&self, p: Vec2) -> Vec<Vec2> {
        let d = p.dist(self.center);
        if d <= self.radius + EPS {
            return Vec::new();
        }
        let base = self.center.angle_to(p);
        let off = (self.radius / d).clamp(-1.0, 1.0).acos();
        vec![Vec2::polar(self.center, self.radius, base + off), Vec2::polar(self.center, self.radius, base - off)]
    }
    /// Lines tangent to both `self` and `other`, as (point on `self`, point on `other`) pairs:
    /// up to two outer (belt) tangents and two inner (crossed) tangents. Empty for concentric
    /// circles or when one circle lies inside the other; tangents that touch both circles at the
    /// same point (circles touching each other) are left out.
    pub fn common_tangents(&self, other: &Circle) -> Vec<(Vec2, Vec2)> {
        let (r1, r2) = (self.radius, other.radius);
        let v = other.center - self.center;
        let d = v.len();
        if !(d.is_finite() && r1.is_finite() && r2.is_finite()) || d <= EPS {
            return Vec::new();
        }
        let u = v / d;
        let mut out = Vec::new();
        // `s` = +1: outer tangents (both circles on the same side); -1: inner tangents.
        for s in [1.0, -1.0] {
            let c = (r1 - s * r2) / d;
            if c.abs() > 1.0 - EPS {
                continue;
            }
            let h = (1.0 - c * c).max(0.0).sqrt();
            for k in [1.0, -1.0] {
                // Unit normal of the tangent line, pointing from each center towards its tangent point.
                let n = u * c + u.perp() * (h * k);
                let (p, q) = (self.center + n * r1, other.center + n * (s * r2));
                if p.is_finite() && q.is_finite() && p.dist(q) > EPS {
                    out.push((p, q));
                }
            }
        }
        out
    }
}

/// An ellipse (or elliptical arc), DXF-style: center, major-axis endpoint vector, ratio, params.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub center: Vec2,
    pub major: Vec2,
    pub ratio: f64,
    pub start: f64,
    pub end: f64,
}

impl Ellipse {
    pub fn full(center: Vec2, major: Vec2, ratio: f64) -> Self {
        Ellipse { center, major, ratio, start: 0.0, end: TAU }
    }
    pub fn is_full(&self) -> bool {
        (ccw_sweep(self.start, self.end) - TAU).abs() < 1e-9
    }
    pub fn minor(&self) -> Vec2 {
        self.major.perp() * self.ratio
    }
    pub fn at_param(&self, t: f64) -> Vec2 {
        self.center + self.major * t.cos() + self.minor() * t.sin()
    }
    pub fn sweep(&self) -> f64 {
        ccw_sweep(self.start, self.end)
    }
    pub fn tessellate(&self, tol: f64, out: &mut Vec<Vec2>) {
        let r = self.major.len();
        let sweep = self.sweep();
        let n = arc_segments(r, sweep, tol).max(8);
        for i in 0..=n {
            out.push(self.at_param(self.start + sweep * i as f64 / n as f64));
        }
    }
    pub fn bounds(&self) -> Bounds2 {
        let mut pts = Vec::new();
        self.tessellate(self.major.len() * 1e-4, &mut pts);
        Bounds2::from_points(pts)
    }
    /// Parameter of the point on the ellipse nearest the direction of `p` (approximate).
    pub fn param_of(&self, p: Vec2) -> f64 {
        let ma = self.major.len();
        if ma < EPS {
            return 0.0;
        }
        let ux = self.major / ma;
        let local = p - self.center;
        let x = local.dot(ux);
        let y = local.dot(ux.perp());
        let mb = ma * self.ratio;
        norm_angle((y / mb.max(EPS)).atan2(x / ma))
    }
    pub fn closest(&self, p: Vec2) -> Vec2 {
        let mut pts = Vec::new();
        self.tessellate(self.major.len() * 1e-3, &mut pts);
        nearest_on_polyline(&pts, p).unwrap_or(self.center)
    }
}

/// The nearest point to `p` on a polyline (through tessellated points).
pub(crate) fn nearest_on_polyline(pts: &[Vec2], p: Vec2) -> Option<Vec2> {
    let mut best: Option<(f64, Vec2)> = None;
    for w in pts.windows(2) {
        if let [a, b] = w {
            let c = Line::new(*a, *b).closest(p);
            let d = c.dist(p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c));
            }
        }
    }
    best.map(|(_, c)| c).or_else(|| pts.first().copied())
}

/// A primitive segment: a line or an arc (polyline pieces, boundary loops, offsets).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Line(Line),
    /// An arc with direction: `ccw == false` runs from end to start in arc terms.
    Arc {
        arc: Arc,
        ccw: bool,
    },
}

impl Segment {
    pub fn start(&self) -> Vec2 {
        match self {
            Segment::Line(l) => l.a,
            Segment::Arc { arc, ccw: true } => arc.start_point(),
            Segment::Arc { arc, ccw: false } => arc.end_point(),
        }
    }
    pub fn end(&self) -> Vec2 {
        match self {
            Segment::Line(l) => l.b,
            Segment::Arc { arc, ccw: true } => arc.end_point(),
            Segment::Arc { arc, ccw: false } => arc.start_point(),
        }
    }
    pub fn len(&self) -> f64 {
        match self {
            Segment::Line(l) => l.len(),
            Segment::Arc { arc, .. } => arc.len(),
        }
    }
    pub fn mid(&self) -> Vec2 {
        match self {
            Segment::Line(l) => l.mid(),
            Segment::Arc { arc, .. } => arc.mid_point(),
        }
    }
    pub fn closest(&self, p: Vec2) -> Vec2 {
        match self {
            Segment::Line(l) => l.closest(p),
            Segment::Arc { arc, .. } => arc.closest(p),
        }
    }
    pub fn bounds(&self) -> Bounds2 {
        match self {
            Segment::Line(l) => Bounds2::new(l.a, l.b),
            Segment::Arc { arc, .. } => arc.bounds(),
        }
    }
    /// Points along the segment in its direction, including both ends.
    pub fn tessellate(&self, tol: f64, out: &mut Vec<Vec2>) {
        match self {
            Segment::Line(l) => {
                out.push(l.a);
                out.push(l.b);
            }
            Segment::Arc { arc, ccw } => {
                let start = out.len();
                arc.tessellate(tol, out);
                if !ccw && let Some(s) = out.get_mut(start..) {
                    s.reverse()
                }
            }
        }
    }
    /// Point at parameter `t` (0..1) in segment direction.
    pub fn at(&self, t: f64) -> Vec2 {
        match self {
            Segment::Line(l) => l.at(t),
            Segment::Arc { arc, ccw: true } => arc.at(t),
            Segment::Arc { arc, ccw: false } => arc.at(1.0 - t),
        }
    }
    pub fn reversed(&self) -> Segment {
        match *self {
            Segment::Line(l) => Segment::Line(Line::new(l.b, l.a)),
            Segment::Arc { arc, ccw } => Segment::Arc { arc, ccw: !ccw },
        }
    }
    /// Tangent direction (unit) at parameter t.
    pub fn tangent(&self, t: f64) -> Vec2 {
        match self {
            Segment::Line(l) => l.dir(),
            Segment::Arc { arc, ccw } => {
                let p = self.at(t);
                let r = (p - arc.center).perp().normalized();
                if *ccw { r } else { -r }
            }
        }
    }
    /// Offset by `d` to the left of travel direction. `None` if an arc collapses.
    pub fn offset(&self, d: f64) -> Option<Segment> {
        match *self {
            Segment::Line(l) => Some(Segment::Line(l.offset(d))),
            Segment::Arc { arc, ccw } => {
                // Left of a CCW arc is toward the center.
                let r = if ccw { arc.radius - d } else { arc.radius + d };
                if r <= EPS {
                    return None;
                }
                Some(Segment::Arc { arc: Arc { radius: r, ..arc }, ccw })
            }
        }
    }
}

/// A polyline vertex: position, bulge to the next vertex (tan(θ/4)), and optional widths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PolyVertex {
    pub p: Vec2,
    #[serde(default)]
    pub bulge: f64,
    #[serde(default)]
    pub start_width: f64,
    #[serde(default)]
    pub end_width: f64,
}

impl PolyVertex {
    pub fn new(p: Vec2) -> Self {
        PolyVertex { p, ..Default::default() }
    }
    pub fn with_bulge(p: Vec2, bulge: f64) -> Self {
        PolyVertex { p, bulge, ..Default::default() }
    }
}

/// Convert a bulge segment to an arc. `None` for a straight (zero-bulge or degenerate) segment.
pub fn bulge_to_arc(a: Vec2, b: Vec2, bulge: f64) -> Option<(Arc, bool)> {
    if bulge.abs() < 1e-12 || !bulge.is_finite() || a.near(b, EPS) {
        return None;
    }
    let theta = 4.0 * bulge.atan(); // signed included angle
    let chord = a.dist(b);
    let r = chord / (2.0 * (theta / 2.0).sin().abs());
    let mid = a.mid(b);
    let sagitta_dir = (b - a).normalized().perp();
    // distance from chord midpoint to center
    let h = r * (theta / 2.0).cos().abs();
    // For bulge > 0 (CCW), center is to the left of a→b when |θ| < π.
    let sign = if bulge > 0.0 { 1.0 } else { -1.0 };
    let center = if theta.abs() < PI { mid + sagitta_dir * (h * sign) } else { mid - sagitta_dir * (h * sign) };
    let sa = center.angle_to(a);
    let ea = center.angle_to(b);
    if bulge > 0.0 { Some((Arc::new(center, r, sa, ea), true)) } else { Some((Arc::new(center, r, ea, sa), false)) }
}

/// Bulge for an arc traversed from `a` to `b` with the given sweep (signed: + = CCW).
pub fn arc_to_bulge(signed_sweep: f64) -> f64 {
    (signed_sweep / 4.0).tan()
}

/// A 2D polyline with bulges (LWPOLYLINE).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polyline {
    pub vertices: Vec<PolyVertex>,
    pub closed: bool,
}

impl Polyline {
    pub fn from_points(pts: &[Vec2], closed: bool) -> Self {
        Polyline { vertices: pts.iter().map(|p| PolyVertex::new(*p)).collect(), closed }
    }
    /// The segments, including the closing segment when closed.
    pub fn segments(&self) -> Vec<Segment> {
        let n = self.vertices.len();
        let mut out = Vec::with_capacity(n);
        let count = if self.closed { n } else { n.saturating_sub(1) };
        for i in 0..count {
            let (Some(v), Some(w)) = (self.vertices.get(i), self.vertices.get((i + 1) % n.max(1))) else { continue };
            if v.p.near(w.p, EPS) && v.bulge.abs() < 1e-12 {
                continue;
            }
            match bulge_to_arc(v.p, w.p, v.bulge) {
                Some((arc, ccw)) => out.push(Segment::Arc { arc, ccw }),
                None => out.push(Segment::Line(Line::new(v.p, w.p))),
            }
        }
        out
    }
    pub fn len(&self) -> f64 {
        self.segments().iter().map(Segment::len).sum()
    }
    pub fn bounds(&self) -> Bounds2 {
        let mut b = Bounds2::from_points(self.vertices.iter().map(|v| v.p));
        for s in self.segments() {
            b = b.union(&s.bounds());
        }
        b
    }
    pub fn tessellate(&self, tol: f64) -> Vec<Vec2> {
        let mut out: Vec<Vec2> = Vec::new();
        for s in self.segments() {
            let mut pts = Vec::new();
            s.tessellate(tol, &mut pts);
            let skip = usize::from(out.last().is_some_and(|l| pts.first().is_some_and(|f| l.near(*f, 1e-9))));
            out.extend(pts.into_iter().skip(skip));
        }
        if out.is_empty() {
            out.extend(self.vertices.first().map(|v| v.p));
        }
        out
    }
    /// Signed area (CCW positive) of a closed polyline including bulge areas.
    pub fn area(&self) -> f64 {
        let pts = self.tessellate(1e-4);
        shoelace(&pts)
    }
    pub fn closest(&self, p: Vec2) -> Option<Vec2> {
        let mut best: Option<(f64, Vec2)> = None;
        for s in self.segments() {
            let c = s.closest(p);
            let d = c.dist(p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c));
            }
        }
        best.map(|(_, c)| c).or_else(|| self.vertices.first().map(|v| v.p))
    }
    /// True if `p` is inside a closed polyline (even-odd on the tessellation).
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(&self.tessellate(1e-3), p)
    }
}

/// Signed polygon area (CCW positive).
pub fn shoelace(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) {
            s += a.cross(*b);
        }
    }
    s / 2.0
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(poly: &[Vec2], p: Vec2) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        if let (Some(a), Some(b)) = (poly.get(i), poly.get(j))
            && (a.y > p.y) != (b.y > p.y)
        {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
