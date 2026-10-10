use serde::{Deserialize, Serialize};

use crate::{Bounds2, Vec2};

/// A (possibly rational) B-spline curve, DXF SPLINE style.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spline {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub control: Vec<Vec2>,
    /// Empty = non-rational (all weights 1).
    #[serde(default)]
    pub weights: Vec<f64>,
    #[serde(default)]
    pub fit: Vec<Vec2>,
    #[serde(default)]
    pub closed: bool,
}

impl Spline {
    /// A clamped uniform spline over control points.
    pub fn from_control(control: Vec<Vec2>, degree: usize) -> Spline {
        let n = control.len();
        let degree = degree.clamp(1, n.saturating_sub(1).max(1));
        let knots = clamped_uniform_knots(n, degree);
        Spline { degree, knots, control, weights: Vec::new(), fit: Vec::new(), closed: false }
    }

    /// Cubic interpolation through fit points (chord-length parametrisation, natural-ish ends
    /// via a clamped knot vector: solve the banded system for control points).
    pub fn from_fit_points(fit: &[Vec2]) -> Spline {
        let n = fit.len();
        if n < 2 {
            return Spline {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control: fit.to_vec(),
                weights: Vec::new(),
                fit: fit.to_vec(),
                closed: false,
            };
        }
        if n == 2 {
            let mut s = Spline::from_control(fit.to_vec(), 1);
            s.fit = fit.to_vec();
            return s;
        }
        let degree = 3.min(n - 1);
        // Chord-length parameters.
        let mut u = vec![0.0; n];
        let mut total = 0.0;
        for i in 1..n {
            total += fit.get(i).zip(fit.get(i - 1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0).max(1e-12);
            if let Some(x) = u.get_mut(i) {
                *x = total;
            }
        }
        for x in u.iter_mut() {
            *x /= total.max(1e-300);
        }
        // Knots by averaging.
        let mut knots = vec![0.0; degree + 1];
        for j in 1..n.saturating_sub(degree) {
            let s: f64 = (j..j + degree).filter_map(|i| u.get(i)).sum();
            knots.push(s / degree as f64);
        }
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        // Basis matrix N[i][j] = N_j(u_i).
        let mut m = vec![vec![0.0; n]; n];
        for (i, ui) in u.iter().enumerate() {
            let span = find_span(n - 1, degree, *ui, &knots);
            let b = basis(span, *ui, degree, &knots);
            for (k, v) in b.iter().enumerate() {
                if let Some(cell) = m.get_mut(i).and_then(|r| r.get_mut(span + k - degree)) {
                    *cell = *v;
                }
            }
        }
        let xs: Vec<f64> = fit.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = fit.iter().map(|p| p.y).collect();
        let (Some(cx), Some(cy)) = (solve(m.clone(), xs), solve(m, ys)) else {
            let mut s = Spline::from_control(fit.to_vec(), degree);
            s.fit = fit.to_vec();
            return s;
        };
        let control = cx.into_iter().zip(cy).map(|(x, y)| Vec2::new(x, y)).collect();
        Spline { degree, knots, control, weights: Vec::new(), fit: fit.to_vec(), closed: false }
    }

    /// Smooth closed (periodic) cubic interpolation through fit points. Chord-length parameters
    /// include the closing chord; the knot vector is the periodic (unclamped) extension and the
    /// first `degree` control points repeat at the end, so the curve has continuous tangent and
    /// curvature at the seam. A repeated closing fit point is dropped.
    pub fn from_fit_points_closed(fit: &[Vec2]) -> Spline {
        let mut pts = fit.to_vec();
        if pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
            pts.pop();
        }
        let n = pts.len();
        let fallback = |pts: &[Vec2]| {
            let mut f = pts.to_vec();
            f.extend(pts.first().copied());
            let mut s = Spline::from_fit_points(&f);
            s.fit = pts.to_vec();
            s.closed = true;
            s
        };
        let p = 3;
        if n < p {
            return fallback(&pts);
        }
        // Parameters t_0 = 0 .. t_n = 1 (t_n is the return to the first point).
        let mut t = vec![0.0; n + 1];
        let mut total = 0.0;
        for i in 1..=n {
            total += pts.get(i - 1).zip(pts.get(i % n)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0).max(1e-12);
            if let Some(x) = t.get_mut(i) {
                *x = total;
            }
        }
        for x in t.iter_mut() {
            *x /= total;
        }
        // Periodic knots: knots[j] = t_(j-p), extended by whole periods on both sides.
        let tk = |i: isize| {
            let (q, r) = (i.div_euclid(n as isize), i.rem_euclid(n as isize) as usize);
            t.get(r).copied().unwrap_or(0.0) + q as f64
        };
        let m = n + p;
        let knots: Vec<f64> = (0..m + p + 1).map(|j| tk(j as isize - p as isize)).collect();
        // Interpolation at t_i; control index k and k + n are the same unknown.
        let mut a = vec![vec![0.0; n]; n];
        for (i, row) in a.iter_mut().enumerate() {
            let u = t.get(i).copied().unwrap_or(0.0);
            let span = find_span(m - 1, p, u, &knots);
            for (k, v) in basis(span, u, p, &knots).iter().enumerate() {
                if let Some(cell) = row.get_mut((span + k - p) % n) {
                    *cell += *v;
                }
            }
        }
        let xs: Vec<f64> = pts.iter().map(|q| q.x).collect();
        let ys: Vec<f64> = pts.iter().map(|q| q.y).collect();
        let (Some(cx), Some(cy)) = (solve(a.clone(), xs), solve(a, ys)) else { return fallback(&pts) };
        let mut control: Vec<Vec2> = cx.into_iter().zip(cy).map(|(x, y)| Vec2::new(x, y)).collect();
        if control.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
            return fallback(&pts);
        }
        control.extend_from_within(..p);
        Spline { degree: p, knots, control, weights: Vec::new(), fit: pts, closed: true }
    }

    /// Interpolation through fit points, open or closed (see `from_fit_points_closed`).
    pub fn from_fit(fit: &[Vec2], closed: bool) -> Spline {
        if closed { Spline::from_fit_points_closed(fit) } else { Spline::from_fit_points(fit) }
    }

    /// Closed with an unclamped (periodic) knot vector, as DXF flags it (group 70 bit 2).
    pub fn is_periodic(&self) -> bool {
        self.closed && self.knots.first() != self.knots.get(self.degree)
    }

    pub fn is_valid(&self) -> bool {
        let n = self.control.len();
        n > self.degree && self.degree >= 1 && self.knots.len() == n + self.degree + 1 && self.knots.windows(2).all(|w| w.first() <= w.get(1))
    }

    pub fn domain(&self) -> (f64, f64) {
        let lo = self.knots.get(self.degree).copied().unwrap_or(0.0);
        let hi = self.knots.get(self.control.len()).copied().unwrap_or(1.0);
        (lo, hi)
    }

    /// Evaluate at parameter `t` (de Boor via basis functions).
    pub fn eval(&self, t: f64) -> Vec2 {
        if !self.is_valid() {
            return self.control.first().copied().unwrap_or_default();
        }
        let n = self.control.len() - 1;
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        let span = find_span(n, self.degree, t, &self.knots);
        let b = basis(span, t, self.degree, &self.knots);
        let mut p = Vec2::ZERO;
        let mut wsum = 0.0;
        for (k, nb) in b.iter().enumerate() {
            let idx = span + k - self.degree;
            let w = if self.weights.is_empty() { 1.0 } else { self.weights.get(idx).copied().unwrap_or(1.0) };
            if let Some(c) = self.control.get(idx) {
                p += *c * (nb * w);
                wsum += nb * w;
            }
        }
        if wsum.abs() > 1e-300 { p / wsum } else { p }
    }

    pub fn tessellate(&self, tol: f64) -> Vec<Vec2> {
        if !self.is_valid() {
            return self.control.clone();
        }
        let (lo, hi) = self.domain();
        // Control-polygon length guides the sample count.
        let poly: f64 = self.control.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum();
        let n = ((poly / tol.max(1e-9)).sqrt() * 2.0).clamp(8.0, 2000.0) as usize * self.degree.max(1);
        let n = n.min(4000);
        (0..=n).map(|i| self.eval(lo + (hi - lo) * i as f64 / n as f64)).collect()
    }

    pub fn bounds(&self) -> Bounds2 {
        Bounds2::from_points(self.tessellate(1e-3))
    }
}

pub(crate) fn clamped_uniform_knots(n: usize, degree: usize) -> Vec<f64> {
    let mut k = vec![0.0; degree + 1];
    let inner = n.saturating_sub(degree + 1);
    for i in 1..=inner {
        k.push(i as f64 / (inner + 1) as f64);
    }
    k.extend(std::iter::repeat_n(1.0, degree + 1));
    k
}

fn find_span(n: usize, p: usize, u: f64, knots: &[f64]) -> usize {
    let kn = knots.get(n + 1).copied().unwrap_or(1.0);
    if u >= kn {
        return n;
    }
    let mut low = p;
    let mut high = n + 1;
    let mut mid = (low + high) / 2;
    let mut guard = 0;
    while guard < 200 {
        guard += 1;
        let km = knots.get(mid).copied().unwrap_or(0.0);
        let km1 = knots.get(mid + 1).copied().unwrap_or(1.0);
        if u < km {
            high = mid;
        } else if u >= km1 {
            low = mid;
        } else {
            break;
        }
        mid = (low + high) / 2;
    }
    mid
}

fn basis(span: usize, u: f64, p: usize, knots: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    if let Some(x) = n.get_mut(0) {
        *x = 1.0;
    }
    for j in 1..=p {
        let k = |i: isize| knots.get(i.max(0) as usize).copied().unwrap_or(0.0);
        left[j] = u - k(span as isize + 1 - j as isize);
        right[j] = k(span as isize + j as isize) - u;
        let mut saved = 0.0;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let tmp = if den.abs() > 1e-300 { n[r] / den } else { 0.0 };
            n[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        n[j] = saved;
    }
    n
}

/// Gaussian elimination with partial pivoting.
#[allow(clippy::needless_range_loop)]
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let piv = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[piv][c].abs() < 1e-14 {
            return None;
        }
        a.swap(c, piv);
        b.swap(c, piv);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            if f != 0.0 {
                for k in c..n {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let s: f64 = (r + 1..n).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}
