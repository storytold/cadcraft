//! Exact conic geometry: implicit conics, pencils of conics and exact intersections and
//! closest points for ellipses (geometry-parity.md, "What to do first" item 1).
//!
//! A conic is `a·x² + b·xy + c·y² + d·x + e·y + f = 0`, or in homogeneous form the symmetric
//! matrix `[[a, b/2, d/2], [b/2, c, e/2], [d/2, e/2, f]]`.
//!
//! Two conics are intersected through their **pencil** `Q1 + λ·Q2`: every conic of the pencil
//! passes through the common points, and the values of λ that make `det(Q1 + λ·Q2) = 0` (a
//! cubic, so at least one is real) give degenerate members, pairs of lines through those points.
//! Splitting a degenerate member into its two lines turns the quartic problem into line–conic
//! intersections, which are quadratics.
//!
//! Numerics: every ellipse problem is first mapped by the affine map that takes one ellipse to the
//! unit circle (centre to the origin, axes to unit length). That keeps coefficients of order one
//! whatever the drawing's coordinates (1e6–1e7 survey drawings included), and in that frame the
//! ellipse parameter is just the polar angle. Candidate points are polished by Newton's method on
//! the two original equations and kept only when both residuals are small.
//!
//! Nothing here panics: non-finite or degenerate input returns no points.

use crate::{Circle, EPS, Ellipse, Line, Vec2, angle_in_sweep, line_circle, norm_angle};

/// A general conic `a·x² + b·xy + c·y² + d·x + e·y + f = 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conic {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

type M3 = [[f64; 3]; 3];

/// Homogeneous line `g0·x + g1·y + g2 = 0`.
type HLine = [f64; 3];

impl Conic {
    /// Conic from its six coefficients `a·x² + b·xy + c·y² + d·x + e·y + f`.
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Conic { a, b, c, d, e, f }
    }

    /// The degenerate conic made of the two lines through `p1, p2` and `q1, q2`.
    pub fn from_line_pair(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> Self {
        let l = line_through(p1, p2);
        let m = line_through(q1, q2);
        Conic {
            a: l[0] * m[0],
            b: l[0] * m[1] + l[1] * m[0],
            c: l[1] * m[1],
            d: l[0] * m[2] + l[2] * m[0],
            e: l[1] * m[2] + l[2] * m[1],
            f: l[2] * m[2],
        }
    }

    /// The implicit equation of a (full) ellipse. `None` for a degenerate or non-finite ellipse.
    pub fn from_ellipse(el: &Ellipse) -> Option<Self> {
        let frame = Frame::of(el)?;
        // A point w is on the ellipse when |M⁻¹(w − c)|² = 1, with M⁻¹ rows u/|u|², v/|v|².
        let (u, v) = (frame.inv_u, frame.inv_v);
        let k = frame.center;
        let a = u.x * u.x + v.x * v.x;
        let b = 2.0 * (u.x * u.y + v.x * v.y);
        let c = u.y * u.y + v.y * v.y;
        let (ku, kv) = (u.dot(k), v.dot(k));
        let d = -2.0 * (u.x * ku + v.x * kv);
        let e = -2.0 * (u.y * ku + v.y * kv);
        let f = ku * ku + kv * kv - 1.0;
        let q = Conic { a, b, c, d, e, f };
        q.is_finite().then_some(q)
    }

    /// The implicit equation of a circle.
    pub fn from_circle(c: &Circle) -> Option<Self> {
        let (x, y, r) = (c.center.x, c.center.y, c.radius);
        let q = Conic { a: 1.0, b: 0.0, c: 1.0, d: -2.0 * x, e: -2.0 * y, f: x * x + y * y - r * r };
        (q.is_finite() && r > 0.0).then_some(q)
    }

    /// True when all six coefficients are finite.
    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().all(|v| v.is_finite())
    }

    /// Value of the equation at `p` (zero on the curve).
    pub fn eval(&self, p: Vec2) -> f64 {
        self.a * p.x * p.x + self.b * p.x * p.y + self.c * p.y * p.y + self.d * p.x + self.e * p.y + self.f
    }

    /// `self + lambda · other`: a member of the pencil of the two conics.
    pub fn add_scaled(&self, other: &Conic, lambda: f64) -> Conic {
        Conic {
            a: self.a + lambda * other.a,
            b: self.b + lambda * other.b,
            c: self.c + lambda * other.c,
            d: self.d + lambda * other.d,
            e: self.e + lambda * other.e,
            f: self.f + lambda * other.f,
        }
    }

    /// `b² − 4ac`: negative for an ellipse, zero for a parabola, positive for a hyperbola.
    pub fn discriminant(&self) -> f64 {
        self.b * self.b - 4.0 * self.a * self.c
    }

    /// Determinant of the homogeneous matrix: zero for a degenerate conic (lines or a point).
    pub fn det(&self) -> f64 {
        det3(&self.matrix())
    }

    /// The same conic scaled so its largest coefficient has magnitude 1 (unchanged if all are 0).
    pub fn normalized(&self) -> Conic {
        let m = self.max_abs();
        if m > 0.0 && m.is_finite() {
            Conic { a: self.a / m, b: self.b / m, c: self.c / m, d: self.d / m, e: self.e / m, f: self.f / m }
        } else {
            *self
        }
    }

    /// Largest coefficient magnitude (the normalisation scale).
    fn max_abs(&self) -> f64 {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().fold(0.0_f64, |m, v| m.max(v.abs()))
    }

    /// Symmetric homogeneous 3×3 matrix of the conic.
    fn matrix(&self) -> M3 {
        [[self.a, self.b * 0.5, self.d * 0.5], [self.b * 0.5, self.c, self.e * 0.5], [self.d * 0.5, self.e * 0.5, self.f]]
    }

    /// The ellipse this conic describes, as a full [`Ellipse`]; `None` when the conic is not a
    /// real, non-degenerate ellipse (or is a circle of zero radius).
    pub fn to_ellipse(&self) -> Option<Ellipse> {
        let q = self.normalized();
        let n = q.a.abs().max(q.b.abs()).max(q.c.abs());
        if !(q.is_finite() && n > 0.0) {
            return None;
        }
        // Relative thresholds: an ellipse needs b² − 4ac < 0 and a non-zero determinant.
        let den = 4.0 * q.a * q.c - q.b * q.b;
        if den <= n * n * 1e-12 || q.det().abs() <= n * n * n * 1e-14 {
            return None;
        }
        let center = Vec2::new((q.b * q.e - 2.0 * q.c * q.d) / den, (q.b * q.d - 2.0 * q.a * q.e) / den);
        // Rotate by θ to remove the xy term: a' and c' are the eigenvalues of the quadratic part.
        let theta = 0.5 * q.b.atan2(q.a - q.c);
        let (s, c) = theta.sin_cos();
        let a2 = q.a * c * c + q.b * c * s + q.c * s * s;
        let c2 = q.a * s * s - q.b * c * s + q.c * c * c;
        let f2 = q.eval(center);
        if a2 == 0.0 || c2 == 0.0 {
            return None;
        }
        let (asq, bsq) = (-f2 / a2, -f2 / c2);
        if !(asq > 0.0 && bsq > 0.0 && asq.is_finite() && bsq.is_finite()) {
            return None;
        }
        let (major_len, minor_len, angle) =
            if asq >= bsq { (asq.sqrt(), bsq.sqrt(), theta) } else { (bsq.sqrt(), asq.sqrt(), theta + crate::PI * 0.5) };
        let major = Vec2::new(angle.cos(), angle.sin()) * major_len;
        (center.is_finite() && major.is_finite()).then(|| Ellipse::full(center, major, minor_len / major_len))
    }
}

/// The ellipse's affine frame: world `w = center + u·X + v·Y` with `(X, Y)` on the unit circle.
struct Frame {
    center: Vec2,
    u: Vec2,
    v: Vec2,
    /// Rows of the inverse map: `X = inv_u · (w − center)`, `Y = inv_v · (w − center)`.
    inv_u: Vec2,
    inv_v: Vec2,
}

impl Frame {
    /// Frame of an ellipse; `None` when its axes are degenerate or not finite.
    fn of(el: &Ellipse) -> Option<Frame> {
        let (u, v) = (el.major, el.minor());
        let (lu, lv) = (u.len2(), v.len2());
        let ok = el.center.is_finite() && u.is_finite() && v.is_finite() && el.ratio.is_finite();
        if !ok || lu.sqrt() < EPS || lv.sqrt() < EPS || !(lu.is_normal() && lv.is_normal()) {
            return None;
        }
        Some(Frame { center: el.center, u, v, inv_u: u / lu, inv_v: v / lv })
    }

    /// World point to unit-circle coordinates.
    fn to_local(&self, w: Vec2) -> Vec2 {
        let d = w - self.center;
        Vec2::new(self.inv_u.dot(d), self.inv_v.dot(d))
    }

    /// Unit-circle coordinates to world point.
    fn to_world(&self, p: Vec2) -> Vec2 {
        self.center + self.u * p.x + self.v * p.y
    }

    /// The conic of another ellipse, expressed in this frame's coordinates.
    fn conic_of(&self, other: &Frame) -> Conic {
        // other: |N·p + t|² = 1 with N = other⁻¹ · [u v] and t = other⁻¹ (centre − other.centre).
        let t = other.to_local(self.center);
        let n00 = other.inv_u.dot(self.u);
        let n01 = other.inv_u.dot(self.v);
        let n10 = other.inv_v.dot(self.u);
        let n11 = other.inv_v.dot(self.v);
        Conic {
            a: n00 * n00 + n10 * n10,
            b: 2.0 * (n00 * n01 + n10 * n11),
            c: n01 * n01 + n11 * n11,
            d: 2.0 * (n00 * t.x + n10 * t.y),
            e: 2.0 * (n01 * t.x + n11 * t.y),
            f: t.x * t.x + t.y * t.y - 1.0,
        }
    }
}

/// The ellipse parameter of a point given in an ellipse's unit-circle frame.
fn param_in_frame(p: Vec2) -> f64 {
    norm_angle(p.y.atan2(p.x))
}

/// Intersections of the infinite line through `l` with a full ellipse, as
/// `(point, line parameter, ellipse parameter)`. Exact: the affine map that takes the ellipse to
/// the unit circle keeps line parameters, so this is a circle problem.
pub fn line_ellipse(l: &Line, el: &Ellipse) -> Vec<(Vec2, f64, f64)> {
    let Some(frame) = Frame::of(el) else { return Vec::new() };
    let local = Line::new(frame.to_local(l.a), frame.to_local(l.b));
    line_circle(&local, &Circle::new(Vec2::ZERO, 1.0))
        .into_iter()
        .filter_map(|(p, t)| {
            let tp = param_in_frame(p);
            let w = el.center + el.major * tp.cos() + el.minor() * tp.sin();
            w.is_finite().then_some((w, t, tp))
        })
        .collect()
}

/// Intersections of a circle with a full ellipse, as `(point, circle angle, ellipse parameter)`.
pub fn circle_ellipse(c: &Circle, el: &Ellipse) -> Vec<(Vec2, f64, f64)> {
    let circle = Ellipse::full(c.center, Vec2::new(c.radius, 0.0), 1.0);
    ellipse_ellipse(&circle, el)
}

/// Intersections of two full ellipses, as `(point, parameter on e1, parameter on e2)`; at most 4.
/// Coincident ellipses (infinitely many common points) return no points, like [`crate::circle_circle`].
pub fn ellipse_ellipse(e1: &Ellipse, e2: &Ellipse) -> Vec<(Vec2, f64, f64)> {
    let (Some(f1), Some(f2)) = (Frame::of(e1), Frame::of(e2)) else { return Vec::new() };
    // Map the rounder ellipse to the unit circle: the other one is then stretched the least.
    let swap = e2.ratio.abs().min(1.0 / e2.ratio.abs()) > e1.ratio.abs().min(1.0 / e1.ratio.abs());
    let (base, other) = if swap { (&f2, &f1) } else { (&f1, &f2) };
    let q = base.conic_of(other);
    unit_circle_conic(&q)
        .into_iter()
        .filter_map(|p| {
            let t_base = param_in_frame(p);
            let w = base.to_world(Vec2::new(t_base.cos(), t_base.sin()));
            let t_other = param_in_frame(other.to_local(w));
            let (t1, t2) = if swap { (t_other, t_base) } else { (t_base, t_other) };
            w.is_finite().then_some((w, t1, t2))
        })
        .collect()
}

/// Like [`ellipse_ellipse`] but only the points on both elliptical arcs (each ellipse's
/// `start..end` parameter sweep). Full ellipses keep every point.
pub fn ellipse_arcs(e1: &Ellipse, e2: &Ellipse) -> Vec<(Vec2, f64, f64)> {
    ellipse_ellipse(e1, e2).into_iter().filter(|(_, t1, t2)| on_ellipse_arc(e1, *t1) && on_ellipse_arc(e2, *t2)).collect()
}

/// True when parameter `t` lies on the elliptical arc (always true for a full ellipse).
pub fn on_ellipse_arc(el: &Ellipse, t: f64) -> bool {
    el.is_full() || angle_in_sweep(t, el.start, el.end)
}

/// Common points of the unit circle `x² + y² = 1` with a conic `q`, at most 4.
fn unit_circle_conic(q: &Conic) -> Vec<Vec2> {
    let circle = Conic::new(1.0, 0.0, 1.0, 0.0, 0.0, -1.0);
    let q = q.normalized();
    if !q.is_finite() || q.max_abs() == 0.0 {
        return Vec::new();
    }
    // The same conic (coincident curves): infinitely many common points.
    let same = |s: f64| (q.a - s).abs() + q.b.abs() + (q.c - s).abs() + q.d.abs() + q.e.abs() + (q.f + s).abs() < 1e-12;
    if same(1.0) || same(-1.0) {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let cm = circle.matrix();
    let qm = q.matrix();
    // det(C + λQ) = c0 + c1 λ + c2 λ² + c3 λ³; c0 = det C = −1, c3 = det Q.
    let (c0, c1, c2, c3) = det_pencil(&cm, &qm);
    if c3.abs() < 1e-13 {
        // Q itself (λ = ∞) is degenerate: split it directly.
        split_degenerate(&qm, &mut candidates);
    }
    for lambda in real_cubic_roots(c3, c2, c1, c0) {
        let m = add_m3(&cm, &qm, lambda);
        split_degenerate(&m, &mut candidates);
    }
    let mut points: Vec<Vec2> = Vec::new();
    for p in candidates {
        let Some(p) = polish(&circle, &q, p) else { continue };
        if points.iter().all(|o| o.dist(p) > 1e-7) {
            points.push(p);
        }
    }
    points.truncate(4);
    points
}

/// Lines of a degenerate conic matrix `m`, intersected with the unit circle; each point (and,
/// for complex conjugate lines, their real crossing) is pushed as a candidate.
fn split_degenerate(m: &M3, out: &mut Vec<Vec2>) {
    let scale = m.iter().flatten().fold(0.0_f64, |s, v| s.max(v.abs()));
    if !(scale > 0.0 && scale.is_finite()) {
        return;
    }
    let m = scale_m3(m, 1.0 / scale);
    let adj = adjugate(&m);
    let (i, bii) = (0..3).map(|i| (i, adj[i][i])).fold((0, 0.0_f64), |best, x| if x.1.abs() > best.1.abs() { x } else { best });
    if bii.abs() < 1e-12 {
        // Rank 1: a double line m = k·l·lᵀ; the row with the largest diagonal is ∝ l.
        let (j, _) = (0..3).map(|j| (j, m[j][j].abs())).fold((0, 0.0_f64), |best, x| if x.1 > best.1 { x } else { best });
        line_unit_circle(&m[j], out);
        return;
    }
    if bii > 0.0 {
        // adj = +p·pᵀ: two complex conjugate lines meeting at the real point p.
        let beta = bii.sqrt();
        let p = [adj[0][i] / beta, adj[1][i] / beta, adj[2][i] / beta];
        if p[2].abs() > 1e-300 {
            out.push(Vec2::new(p[0] / p[2], p[1] / p[2]));
        }
        return;
    }
    // adj = −p·pᵀ with p the lines' crossing; m + [p]ₓ' has rank 1 = 2·g·hᵀ (Richter-Gebert).
    let beta = (-bii).sqrt();
    let p = [adj[0][i] / beta, adj[1][i] / beta, adj[2][i] / beta];
    let c = [[m[0][0], m[0][1] + p[2], m[0][2] - p[1]], [m[1][0] - p[2], m[1][1], m[1][2] + p[0]], [m[2][0] + p[1], m[2][1] - p[0], m[2][2]]];
    let mut best = (0, 0, 0.0_f64);
    for (r, row) in c.iter().enumerate() {
        for (k, v) in row.iter().enumerate() {
            if v.abs() > best.2 {
                best = (r, k, v.abs());
            }
        }
    }
    let (r, k, _) = best;
    let g = c[r];
    let h = [c[0][k], c[1][k], c[2][k]];
    line_unit_circle(&g, out);
    line_unit_circle(&h, out);
}

/// Intersections of the homogeneous line `g` with the unit circle.
fn line_unit_circle(g: &HLine, out: &mut Vec<Vec2>) {
    let n2 = g[0] * g[0] + g[1] * g[1];
    if !(n2 > 1e-24 * (n2 + g[2] * g[2]) && n2.is_finite()) {
        return; // the line at infinity (or nothing)
    }
    let n = n2.sqrt();
    let (nx, ny, dist) = (g[0] / n, g[1] / n, -g[2] / n);
    let foot = Vec2::new(nx * dist, ny * dist);
    let delta = 1.0 - dist * dist;
    if delta < -1e-9 {
        return;
    }
    if delta <= 1e-12 {
        out.push(foot);
        return;
    }
    let step = Vec2::new(-ny, nx) * delta.sqrt();
    out.push(foot + step);
    out.push(foot - step);
}

/// Newton's method on {unit circle, q}; returns the point when both residuals are tiny.
fn polish(circle: &Conic, q: &Conic, mut p: Vec2) -> Option<Vec2> {
    if !p.is_finite() {
        return None;
    }
    for _ in 0..8 {
        let (f1, f2) = (circle.eval(p), q.eval(p));
        let (j11, j12) = (2.0 * p.x, 2.0 * p.y);
        let (j21, j22) = (2.0 * q.a * p.x + q.b * p.y + q.d, q.b * p.x + 2.0 * q.c * p.y + q.e);
        let det = j11 * j22 - j12 * j21;
        if det.abs() < 1e-10 {
            break; // tangency: the candidate from the split is already the best estimate
        }
        let step = Vec2::new((f1 * j22 - f2 * j12) / det, (j11 * f2 - j21 * f1) / det);
        let next = p - step;
        if !next.is_finite() || circle.eval(next).abs() + q.eval(next).abs() > f1.abs() + f2.abs() {
            break;
        }
        p = next;
        if step.len() < 1e-16 {
            break;
        }
    }
    // Project onto the circle exactly, then check the conic.
    let r = p.len();
    if !(r > 0.0 && r.is_finite()) {
        return None;
    }
    let p = p / r;
    (q.eval(p).abs() < 1e-8 && (r - 1.0).abs() < 1e-6).then_some(p)
}

// ---------- small exact helpers ----------

/// Homogeneous line through two points (cross product of their homogeneous coordinates).
fn line_through(p: Vec2, q: Vec2) -> HLine {
    [q.y - p.y, p.x - q.x, q.x * p.y - p.x * q.y]
}

/// Determinant of a 3×3 matrix (cofactor expansion along the first row).
fn det3(m: &M3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Adjugate (transposed cofactor matrix) of a 3×3 matrix.
fn adjugate(m: &M3) -> M3 {
    let c = |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    [[c(1, 2, 1, 2), -c(0, 2, 1, 2), c(0, 1, 1, 2)], [-c(1, 2, 0, 2), c(0, 2, 0, 2), -c(0, 1, 0, 2)], [c(1, 2, 0, 1), -c(0, 2, 0, 1), c(0, 1, 0, 1)]]
}

/// `a + lambda · b` for 3×3 matrices.
fn add_m3(a: &M3, b: &M3, lambda: f64) -> M3 {
    let mut m = *a;
    for (row, brow) in m.iter_mut().zip(b) {
        for (v, bv) in row.iter_mut().zip(brow) {
            *v += lambda * bv;
        }
    }
    m
}

/// `s · a` for a 3×3 matrix.
fn scale_m3(a: &M3, s: f64) -> M3 {
    let mut m = *a;
    m.iter_mut().flatten().for_each(|v| *v *= s);
    m
}

/// Replace column `k` of `a` with column `k` of `b` for each `k` set in `mask`.
fn mix_columns(a: &M3, b: &M3, mask: [bool; 3]) -> M3 {
    let mut m = *a;
    for (row, brow) in m.iter_mut().zip(b) {
        for k in 0..3 {
            if mask[k] {
                row[k] = brow[k];
            }
        }
    }
    m
}

/// Coefficients of `det(a + λ·b)` (the determinant is linear in each column).
fn det_pencil(a: &M3, b: &M3) -> (f64, f64, f64, f64) {
    let d = |mask| det3(&mix_columns(a, b, mask));
    let c0 = det3(a);
    let c1 = d([true, false, false]) + d([false, true, false]) + d([false, false, true]);
    let c2 = d([true, true, false]) + d([true, false, true]) + d([false, true, true]);
    let c3 = det3(b);
    (c0, c1, c2, c3)
}

/// Real roots of `c3·x³ + c2·x² + c1·x + c0`, each refined by Newton's method.
fn real_cubic_roots(c3: f64, c2: f64, c1: f64, c0: f64) -> Vec<f64> {
    let scale = c3.abs().max(c2.abs()).max(c1.abs()).max(c0.abs());
    if !(scale > 0.0 && scale.is_finite()) {
        return Vec::new();
    }
    let (c3, c2, c1, c0) = (c3 / scale, c2 / scale, c1 / scale, c0 / scale);
    let f = |x: f64| ((c3 * x + c2) * x + c1) * x + c0;
    let df = |x: f64| (3.0 * c3 * x + 2.0 * c2) * x + c1;
    let mut roots = Vec::new();
    if c3.abs() < 1e-13 {
        // Quadratic (or linear).
        if c2.abs() < 1e-13 {
            if c1.abs() > 1e-13 {
                roots.push(-c0 / c1);
            }
        } else {
            let disc = c1 * c1 - 4.0 * c2 * c0;
            if disc >= 0.0 {
                let s = disc.sqrt();
                let qv = -0.5 * (c1 + c1.signum() * s);
                if qv != 0.0 {
                    roots.push(qv / c2);
                    roots.push(c0 / qv);
                } else {
                    roots.push(0.0);
                }
            }
        }
    } else {
        // Monic depressed cubic t³ + p·t + q with x = t − a/3.
        let (a, b, c) = (c2 / c3, c1 / c3, c0 / c3);
        let p = b - a * a / 3.0;
        let q = 2.0 * a * a * a / 27.0 - a * b / 3.0 + c;
        let shift = -a / 3.0;
        let disc = q * q / 4.0 + p * p * p / 27.0;
        if disc > 0.0 {
            let s = disc.sqrt();
            let u = (-q / 2.0 + s).cbrt();
            let v = (-q / 2.0 - s).cbrt();
            roots.push(u + v + shift);
        } else if p.abs() < 1e-300 {
            roots.push(shift);
        } else {
            let r = (-p / 3.0).sqrt();
            let phi = (3.0 * q / (2.0 * p * r)).clamp(-1.0, 1.0).acos() / 3.0;
            for k in 0..3 {
                roots.push(2.0 * r * (phi - crate::TAU * k as f64 / 3.0).cos() + shift);
            }
        }
    }
    roots
        .into_iter()
        .filter(|x| x.is_finite())
        .map(|mut x| {
            for _ in 0..4 {
                let d = df(x);
                if d == 0.0 {
                    break;
                }
                let nx = x - f(x) / d;
                if !nx.is_finite() || f(nx).abs() > f(x).abs() {
                    break;
                }
                x = nx;
            }
            x
        })
        .collect()
}

// ---------- closest point ----------

/// Parameter of the point of the **full** ellipse nearest to `p` (exact, robust bisection after
/// D. Eberly, "Distance from a Point to an Ellipse", 2013). `None` for a degenerate ellipse.
pub fn ellipse_closest_param(el: &Ellipse, p: Vec2) -> Option<f64> {
    let a = el.major.len();
    let b = a * el.ratio.abs();
    if !(a > EPS && b > EPS && a.is_finite() && b.is_finite() && p.is_finite() && el.center.is_finite()) {
        return None;
    }
    let ux = el.major / a;
    let d = p - el.center;
    // Local coordinates along the major axis and the minor direction (major.perp()).
    let (x, y) = (d.dot(ux), d.dot(ux.perp()));
    // Eberly's routine wants e0 ≥ e1; a ratio above 1 swaps the roles.
    let (q0, q1) = if a >= b {
        closest_first_quadrant(a, b, x.abs(), y.abs())
    } else {
        let (s1, s0) = closest_first_quadrant(b, a, y.abs(), x.abs());
        (s0, s1)
    };
    let (qx, qy) = (q0.copysign(x), q1.copysign(y));
    let sgn = el.ratio.signum();
    let t = norm_angle((sgn * qy / b).atan2(qx / a));
    t.is_finite().then_some(t)
}

/// Closest point on the ellipse `(x/e0)² + (y/e1)² = 1`, `e0 ≥ e1 > 0`, to `(y0, y1)` with both ≥ 0.
fn closest_first_quadrant(e0: f64, e1: f64, y0: f64, y1: f64) -> (f64, f64) {
    if y1 > 0.0 {
        if y0 > 0.0 {
            let z0 = y0 / e0;
            let z1 = y1 / e1;
            let g = z0 * z0 + z1 * z1 - 1.0;
            if g != 0.0 {
                let r0 = (e0 / e1) * (e0 / e1);
                // Work with u = s + 1 so the second denominator never rounds to zero near the
                // major axis (s → −1 there): u is bracketed below by z1 > 0.
                let u = eberly_root_shifted(r0, z0, z1, g);
                return (r0 * y0 / (u + (r0 - 1.0)), y1 / u);
            }
            return (y0, y1);
        }
        return (0.0, e1);
    }
    let denom = e0 * e0 - e1 * e1;
    if y0 < denom / e0 {
        let x0 = e0 * e0 * y0 / denom;
        let xr = x0 / e0;
        return (x0, e1 * (1.0 - xr * xr).max(0.0).sqrt());
    }
    (e0, 0.0)
}

/// Root `u = s + 1` of Eberly's monotone function `(r0·z0/(u + r0 − 1))² + (z1/u)² − 1`, by bisection
/// on `[z1, 1]` (inside the ellipse) or `[z1, |(r0·z0, z1)|]` (outside). Solving for `u` instead of
/// `s` keeps full relative precision when the root is near `s = −1`, i.e. for points very close to
/// the major axis, where `s + 1` would otherwise cancel to zero.
fn eberly_root_shifted(r0: f64, z0: f64, z1: f64, g: f64) -> f64 {
    let n0 = r0 * z0;
    let shift = r0 - 1.0;
    let mut u0 = z1;
    let mut u1 = if g < 0.0 { 1.0 } else { n0.hypot(z1) };
    let mut u = u1;
    for _ in 0..1100 {
        u = 0.5 * (u0 + u1);
        if u == u0 || u == u1 {
            break;
        }
        let ratio0 = n0 / (u + shift);
        let ratio1 = z1 / u;
        let gs = ratio0 * ratio0 + ratio1 * ratio1 - 1.0;
        if gs > 0.0 {
            u0 = u;
        } else if gs < 0.0 {
            u1 = u;
        } else {
            break;
        }
    }
    u
}

/// Parameter of the point of the elliptical arc (or full ellipse) nearest to `p`, exact.
/// On an arc whose global nearest point is off the arc, the candidates are the other feet of the
/// normals from `p` (the arc's intersections with the Apollonius hyperbola) and the arc ends.
pub fn ellipse_arc_closest_param(el: &Ellipse, p: Vec2) -> Option<f64> {
    let t = ellipse_closest_param(el, p)?;
    if on_ellipse_arc(el, t) {
        return Some(t);
    }
    let frame = Frame::of(el)?;
    let a = el.major.len();
    let b = a * el.ratio.abs();
    let local = frame.to_local(p);
    let (px, py) = (local.x * a, local.y * b);
    // Feet of the normals in the unit-circle frame: (a² − b²)·XY + b·py·X − a·px·Y = 0.
    let apollonius = Conic::new(0.0, a * a - b * b, 0.0, b * py, -a * px, 0.0);
    let mut best: Option<(f64, f64)> = None;
    let mut consider = |t: f64| {
        let d = el.at_param(t).dist(p);
        if d.is_finite() && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, t));
        }
    };
    consider(el.start);
    consider(el.end);
    for q in unit_circle_conic(&apollonius) {
        let tq = param_in_frame(q);
        if on_ellipse_arc(el, tq) {
            consider(tq);
        }
    }
    best.map(|(_, t)| t)
}
