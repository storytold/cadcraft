//! Curve math for the residuals: ellipse points and derivatives, B-spline end derivatives and
//! curve end frames (point, tangent, curvature) for tangent and smooth (G2) joins.
//!
//! Textbook formulas only: the derivative of a clamped B-spline at its ends follows from the
//! derivative control points (Piegl & Tiller, *The NURBS Book*, ch. 3–4), rational curves through
//! the quotient rule on homogeneous coordinates.

use cadcraft_geom::{Spline as GSpline, Vec2};

/// `v <= lim`, or NaN.
pub(crate) fn not_above(v: f64, lim: f64) -> bool {
    v.is_nan() || v <= lim
}

/// A curve end: point, unit tangent in the curve's own direction, signed curvature (positive
/// when the curve turns left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub p: Vec2,
    pub t: Vec2,
    pub k: f64,
}

impl Frame {
    /// From a point and the first and second derivatives with respect to any parameter.
    pub fn from_derivs(p: Vec2, d1: Vec2, d2: Vec2) -> Option<Frame> {
        let l = d1.len();
        if !l.is_finite() || not_above(l, 1e-300) {
            return None;
        }
        let k = d1.cross(d2) / (l * l * l);
        let f = Frame { p, t: d1 * (1.0 / l), k };
        (f.p.is_finite() && f.t.is_finite() && f.k.is_finite()).then_some(f)
    }

    /// The same end traversed the other way.
    pub fn reversed(self) -> Frame {
        Frame { p: self.p, t: -self.t, k: -self.k }
    }
}

/// An ellipse or elliptical arc in solver parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ell {
    pub c: Vec2,
    /// Major axis vector (centre to the end of the major axis).
    pub mj: Vec2,
    pub ratio: f64,
    pub t0: f64,
    pub t1: f64,
    pub arc: bool,
}

impl Ell {
    pub fn minor(&self) -> Vec2 {
        self.mj.perp() * self.ratio
    }
    pub fn at(&self, t: f64) -> Vec2 {
        self.c + self.mj * t.cos() + self.minor() * t.sin()
    }
    pub fn d1(&self, t: f64) -> Vec2 {
        self.mj * -t.sin() + self.minor() * t.cos()
    }
    pub fn d2(&self, t: f64) -> Vec2 {
        self.mj * -t.cos() - self.minor() * t.sin()
    }
    pub fn frame(&self, t: f64) -> Option<Frame> {
        Frame::from_derivs(self.at(t), self.d1(t), self.d2(t))
    }
    /// Semi-axes `(a, b)`.
    fn axes(&self) -> (f64, f64) {
        let a = self.mj.len();
        (a, a * self.ratio.abs())
    }
    /// Local coordinates of `p` along the major and minor axes.
    fn local(&self, p: Vec2) -> Option<(f64, f64, Vec2, Vec2)> {
        let (a, _) = self.axes();
        if not_above(a, 1e-300) {
            return None;
        }
        let u_dir = self.mj * (1.0 / a);
        let v_dir = u_dir.perp();
        let q = p - self.c;
        Some((q.dot(u_dir), q.dot(v_dir), u_dir, v_dir))
    }
    /// A distance-like residual of `p` from the ellipse: zero exactly on it, about the distance
    /// near it.
    pub fn on(&self, p: Vec2) -> f64 {
        let (a, b) = self.axes();
        match self.local(p) {
            Some((u, v, _, _)) if b > 1e-300 => {
                let s = a / b;
                (u * u + v * v * s * s).sqrt() - a
            }
            _ => p.dist(self.c) - a,
        }
    }
    /// Unit outward normal of the ellipse's implicit form at (or near) `p`.
    pub fn normal(&self, p: Vec2) -> Vec2 {
        let (a, b) = self.axes();
        match self.local(p) {
            Some((u, v, ud, vd)) if b > 1e-300 => (ud * (u / (a * a)) + vd * (v / (b * b))).normalized(),
            _ => (p - self.c).normalized(),
        }
    }
    /// Largest `n·(P − c)` over the ellipse for a unit direction `n` (its support distance).
    pub fn support(&self, n: Vec2) -> f64 {
        let (x, y) = (n.dot(self.mj), n.dot(self.minor()));
        (x * x + y * y).sqrt()
    }
}

/// A circle (or the circle of an arc) as a parametric curve.
pub(crate) fn circle_at(c: Vec2, r: f64, t: f64) -> (Vec2, Vec2) {
    let (s, co) = t.sin_cos();
    (c + Vec2::new(co, s) * r, Vec2::new(-s, co) * r)
}

/// Homogeneous control point.
#[derive(Clone, Copy, Debug)]
struct H {
    x: f64,
    y: f64,
    w: f64,
}

impl H {
    fn sub(self, o: H) -> H {
        H { x: self.x - o.x, y: self.y - o.y, w: self.w - o.w }
    }
    fn scale(self, f: f64) -> H {
        H { x: self.x * f, y: self.y * f, w: self.w * f }
    }
}

/// Is the knot vector clamped (end knot repeated `degree + 1` times) at the start or end?
pub(crate) fn clamped(s: &GSpline, at_end: bool) -> bool {
    let p = s.degree;
    let k = &s.knots;
    if !s.is_valid() || k.len() < 2 * (p + 1) {
        return false;
    }
    let scale = k.iter().fold(1.0f64, |m, v| m.max(v.abs()));
    let tol = 1e-12 * scale;
    let window = if at_end { k.get(k.len() - p - 1..) } else { k.get(..=p) };
    match window {
        Some(w) => w.iter().all(|v| w.first().is_some_and(|f| (v - f).abs() <= tol)),
        None => false,
    }
}

/// End frame of a clamped spline at its start (`at_end` false) or end, in the curve's own
/// direction. `None` for an unclamped end or degenerate data.
pub(crate) fn spline_end(s: &GSpline, at_end: bool) -> Option<Frame> {
    if !clamped(s, at_end) {
        return None;
    }
    let p = s.degree;
    let n = s.control.len();
    let wt = |i: usize| if s.weights.is_empty() { Some(1.0) } else { s.weights.get(i).copied() };
    let hp = |i: usize| -> Option<H> {
        let c = s.control.get(i)?;
        let w = wt(i)?;
        Some(H { x: c.x * w, y: c.y * w, w })
    };
    // Work on the start of the curve; for the end, reverse the control points and knots.
    let (q, u): (Vec<H>, Vec<f64>) = if at_end {
        let q = (0..n.min(3)).map(|i| hp(n - 1 - i)).collect::<Option<Vec<_>>>()?;
        let last = s.knots.len() - 1;
        let u = (0..s.knots.len().min(p + 3)).map(|i| s.knots.get(last - i).map(|v| -v)).collect::<Option<Vec<_>>>()?;
        (q, u)
    } else {
        let q = (0..n.min(3)).map(hp).collect::<Option<Vec<_>>>()?;
        let u = s.knots.get(..(p + 3).min(s.knots.len()))?.to_vec();
        (q, u)
    };
    let pf = p as f64;
    let (q0, q1) = (*q.first()?, *q.get(1)?);
    let den1 = u.get(p + 1)? - u.get(1)?;
    if not_above(den1.abs(), 1e-300) {
        return None;
    }
    let a1 = q1.sub(q0).scale(pf / den1);
    let a2 = if p >= 2 {
        let q2 = *q.get(2)?;
        let den2 = u.get(p + 2)? - u.get(2)?;
        let den0 = u.get(p + 1)? - u.get(2)?;
        if !(den2.abs() > 1e-300 && den0.abs() > 1e-300) {
            return None;
        }
        q2.sub(q1).scale(1.0 / den2).sub(q1.sub(q0).scale(1.0 / den1)).scale(pf * (pf - 1.0) / den0)
    } else {
        H { x: 0.0, y: 0.0, w: 0.0 }
    };
    // Quotient rule: C = A / w.
    if not_above(q0.w.abs(), 1e-300) {
        return None;
    }
    let c = Vec2::new(q0.x / q0.w, q0.y / q0.w);
    let c1 = (Vec2::new(a1.x, a1.y) - c * a1.w) / q0.w;
    let c2 = (Vec2::new(a2.x, a2.y) - c1 * (2.0 * a1.w) - c * a2.w) / q0.w;
    let f = Frame::from_derivs(c, c1, c2)?;
    // The reversed curve's start is the end traversed backwards.
    Some(if at_end { f.reversed() } else { f })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipse_residuals() {
        let e = Ell { c: Vec2::new(1.0, 2.0), mj: Vec2::new(4.0, 0.0), ratio: 0.5, t0: 0.0, t1: 1.0, arc: false };
        assert!(e.on(Vec2::new(5.0, 2.0)).abs() < 1e-12);
        assert!(e.on(Vec2::new(1.0, 4.0)).abs() < 1e-12);
        assert!(e.on(Vec2::new(6.0, 2.0)) > 0.0);
        assert!((e.support(Vec2::new(0.0, 1.0)) - 2.0).abs() < 1e-12);
        assert!((e.support(Vec2::new(1.0, 0.0)) - 4.0).abs() < 1e-12);
        assert!(e.normal(Vec2::new(1.0, 4.0)).near(Vec2::new(0.0, 1.0), 1e-12));
        // Curvature at the end of the major axis: a / b².
        let f = e.frame(0.0).unwrap();
        assert!((f.k - 4.0 / 4.0).abs() < 1e-12);
    }

    #[test]
    fn spline_end_derivatives_match_sampling() {
        let s = GSpline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 3.0), Vec2::new(6.0, 1.0)], 3);
        for at_end in [false, true] {
            let f = spline_end(&s, at_end).unwrap();
            let (lo, hi) = s.domain();
            let (t, h) = if at_end { (hi, -1e-4) } else { (lo, 1e-4) };
            let (p0, p1, p2) = (s.eval(t), s.eval(t + h), s.eval(t + 2.0 * h));
            assert!(f.p.near(p0, 1e-12));
            // One-sided differences (a negative step at the end still gives d/dt).
            let d1 = (p1 - p0) * (1.0 / h);
            let d2 = (p2 - p1 * 2.0 + p0) * (1.0 / (h * h));
            let g = Frame::from_derivs(p0, d1, d2).unwrap();
            assert!(f.t.near(g.t, 1e-3), "{f:?} {g:?}");
            assert!((f.k - g.k).abs() < 1e-2, "{f:?} {g:?}");
        }
        // An unclamped spline has no ends: one whose control points wrap round.
        let fit = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(2.0, 3.0)];
        let closed = GSpline::fit_with(&fit, true, cadcraft_geom::FitOptions { knots: cadcraft_geom::KnotParam::Uniform, ..Default::default() });
        assert!(closed.wraps() && spline_end(&closed, false).is_none());
    }
}
