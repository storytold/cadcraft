//! Fit-point splines: knot parametrisation (chord, square root, uniform), end tangents and fit
//! tolerance (least-squares approximation with fewer control points), as AutoCAD's SPLINE Fit
//! method offers them. Construction follows the textbook global interpolation and approximation
//! schemes (Piegl & Tiller, *The NURBS Book*, ch. 9).

use serde::{Deserialize, Serialize};

use crate::spline::{basis, find_span, solve};
use crate::{Mat3, Spline, Vec2};

/// How fit points are spaced in parameter space (the SPLINE `Knots` option).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnotParam {
    /// Proportional to the distance between fit points.
    #[default]
    Chord,
    /// Proportional to the square root of the distance ("centripetal").
    SqrtChord,
    /// Equal steps whatever the distance.
    Uniform,
}

impl KnotParam {
    pub const ALL: [KnotParam; 3] = [KnotParam::Chord, KnotParam::SqrtChord, KnotParam::Uniform];

    /// The name as the prompt and the "Current settings" line show it.
    pub fn name(self) -> &'static str {
        match self {
            KnotParam::Chord => "Chord",
            KnotParam::SqrtChord => "Square root",
            KnotParam::Uniform => "Uniform",
        }
    }

    /// Parse a keyword or JSON value ("chord", "sqrt", "square root", "uniform", or 0/1/2 as in
    /// the SPLKNOTS setting).
    pub fn parse(s: &str) -> Option<KnotParam> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "c" | "chord" | "0" => KnotParam::Chord,
            "s" | "sqrt" | "square root" | "squareroot" | "centripetal" | "1" => KnotParam::SqrtChord,
            "u" | "uniform" | "2" => KnotParam::Uniform,
            _ => return None,
        })
    }

    /// The SPLKNOTS code (0 chord, 1 square root, 2 uniform).
    pub fn code(self) -> i64 {
        match self {
            KnotParam::Chord => 0,
            KnotParam::SqrtChord => 1,
            KnotParam::Uniform => 2,
        }
    }

    fn step(self, d: f64) -> f64 {
        match self {
            KnotParam::Chord => d.max(1e-12),
            KnotParam::SqrtChord => d.sqrt().max(1e-12),
            KnotParam::Uniform => 1.0,
        }
    }
}

/// How a spline is built from its fit points. The default interpolates with chord-length
/// parameters and free ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FitOptions {
    #[serde(default)]
    pub knots: KnotParam,
    /// Unit tangent direction at the first fit point; `None` lets the end follow the points.
    #[serde(default)]
    pub start_tangent: Option<Vec2>,
    /// Unit tangent direction at the last fit point.
    #[serde(default)]
    pub end_tangent: Option<Vec2>,
    /// Largest allowed distance between the curve and a fit point; 0 passes through them.
    #[serde(default)]
    pub tolerance: f64,
}

impl FitOptions {
    /// Normalised tangents (zero or non-finite directions dropped) and a finite, non-negative
    /// tolerance: what a spline stores.
    pub fn sanitized(self) -> FitOptions {
        let dir = |t: Option<Vec2>| t.filter(|v| v.x.is_finite() && v.y.is_finite() && v.len() > 1e-12).map(Vec2::normalized);
        FitOptions {
            knots: self.knots,
            start_tangent: dir(self.start_tangent),
            end_tangent: dir(self.end_tangent),
            tolerance: if self.tolerance.is_finite() { self.tolerance.max(0.0) } else { 0.0 },
        }
    }
}

/// Fit points above this count are always interpolated: the approximation search solves dense
/// systems the size of the point count several times.
const MAX_APPROX_POINTS: usize = 300;

/// Fit points above this count are not interpolated: interpolation solves a dense system the size
/// of the point count squared (twice, about 270 MB at this size), so more points use them as
/// control points instead. Above the 4,001 fit points of a 500-turn helix.
const MAX_INTERP_POINTS: usize = 4_096;

impl Spline {
    /// Build a spline through (or, with a tolerance, near) `fit`, open or closed. Closed curves
    /// are periodic and ignore end tangents; a tolerance approximates them with fewer periodic
    /// control points. A fit point given twice in a row is a kink (SPLINEDIT Fit data Kink): the
    /// curve passes through it with a corner (a knot of full multiplicity) and the pieces on
    /// either side are fitted on their own.
    pub fn fit_with(fit: &[Vec2], closed: bool, opts: FitOptions) -> Spline {
        let opts = opts.sanitized();
        let kinked = if fit.len() <= MAX_KINK_POINTS { kinked_fit(fit, closed, opts) } else { None };
        let mut s = match kinked {
            Some(s) => s,
            None => {
                // Repeats at the ends of an open curve are no corners: fit without them.
                let (pts, _) = kinks(fit, false);
                let mut s = if closed { closed_fit(&pts, opts.knots, opts.tolerance) } else { open_fit(&pts, opts) };
                if pts.len() != fit.len() && !closed {
                    s.fit = fit.to_vec();
                }
                s
            }
        };
        s.fit_opts = if closed { FitOptions { start_tangent: None, end_tangent: None, ..opts } } else { opts };
        s
    }

    /// Rebuild from new fit points with this spline's knot parametrisation, tangents and tolerance.
    pub fn refit(&self, fit: &[Vec2], closed: bool) -> Spline {
        Spline::fit_with(fit, closed, self.fit_opts)
    }

    /// Which knot parametrisation produced this spline's knots from its fit points (DXF files
    /// keep the knots but not the parametrisation). Compares knot vectors only (no solving), so
    /// it is cheap for any number of fit points. Chord when nothing matches.
    pub fn infer_knot_param(&self) -> KnotParam {
        if self.fit.len() < 3 {
            return self.fit_opts.knots;
        }
        let (Some(k0), Some(k1)) = (self.knots.get(self.degree), self.knots.get(self.control.len())) else { return KnotParam::Chord };
        if !(k1 - k0).is_finite() || k1 - k0 <= 1e-300 {
            return KnotParam::Chord;
        }
        let norm: Vec<f64> = self.knots.iter().map(|k| (k - k0) / (k1 - k0)).collect();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let same = |k: &[f64]| k.len() == norm.len() && k.iter().zip(&norm).all(|(a, b)| close(*a, *b));
        let p = self.degree;
        let o = &self.fit_opts;
        KnotParam::ALL
            .into_iter()
            .find(|kind| {
                if self.closed {
                    // Our periodic knots run from the parameter of fit point 0 (at 0) to 1.
                    let mut pts = self.fit.clone();
                    if pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
                        pts.pop();
                    }
                    let t = params(&pts, *kind, true);
                    let pk = periodic_knots(&t, pts.len(), p);
                    if same(&pk) {
                        return true;
                    }
                }
                let u = params(&self.fit, *kind, self.closed);
                if !self.closed {
                    if same(&interp_knots(&u, o.start_tangent.is_some(), o.end_tangent.is_some()).1) {
                        return true;
                    }
                    let m = self.control.len();
                    if p < m && m < u.len() && same(&lsq_knots(&u, m, p)) {
                        return true;
                    }
                }
                // Other writers use the parameters themselves as interior knots. Both `u` and
                // `norm` are ascending, so a single linear merge decides containment instead of
                // re-scanning `norm` from the start for every parameter, which is quadratic on an
                // otherwise valid spline (CWE-407).
                if u.len() <= 2 {
                    return false;
                }
                let interior = u.get(1..u.len() - 1).unwrap_or(&[]);
                let mut j = 0usize;
                interior.iter().all(|q| {
                    while j < norm.len() && norm[j] < *q && !close(norm[j], *q) {
                        j += 1;
                    }
                    norm.get(j).is_some_and(|k| close(*k, *q))
                })
            })
            .unwrap_or(KnotParam::Chord)
    }

    /// Apply an affine map to the control and fit points and turn the end tangents with it.
    pub fn transform(&mut self, m: &Mat3) {
        for c in self.control.iter_mut().chain(self.fit.iter_mut()) {
            *c = m.apply(*c);
        }
        let turn = |t: Option<Vec2>| t.map(|v| m.apply_vec(v).normalized()).filter(|v| v.len() > 0.5);
        self.fit_opts.start_tangent = turn(self.fit_opts.start_tangent);
        self.fit_opts.end_tangent = turn(self.fit_opts.end_tangent);
    }
}

/// Normalised parameters of the fit points: `n` values for an open curve, `n + 1` for a closed
/// one (the last is the return to the first point).
pub(crate) fn params(pts: &[Vec2], kind: KnotParam, closed: bool) -> Vec<f64> {
    let n = pts.len();
    let count = if closed { n + 1 } else { n };
    let mut u = vec![0.0; count];
    let mut total = 0.0;
    for i in 1..count {
        total += pts.get(i - 1).zip(pts.get(i % n.max(1))).map(|(a, b)| kind.step(a.dist(*b))).unwrap_or(1.0);
        if let Some(x) = u.get_mut(i) {
            *x = total;
        }
    }
    for x in u.iter_mut() {
        *x /= total.max(1e-300);
    }
    u
}

fn chord_length(pts: &[Vec2]) -> f64 {
    pts.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum()
}

fn open_fit(fit: &[Vec2], opts: FitOptions) -> Spline {
    let n = fit.len();
    let tangents = opts.start_tangent.is_some() || opts.end_tangent.is_some();
    if n < 2 || (n == 2 && !tangents) {
        let mut s = if n < 2 {
            Spline { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], control: fit.to_vec(), ..Spline::default() }
        } else {
            Spline::from_control(fit.to_vec(), 1)
        };
        s.fit = fit.to_vec();
        return s;
    }
    let u = params(fit, opts.knots, false);
    // Tangents are directions; the derivative's length is the curve length in parameter space.
    let len = chord_length(fit).max(1e-12);
    let d0 = opts.start_tangent.map(|t| t * len);
    let d1 = opts.end_tangent.map(|t| t * len);
    if opts.tolerance > 0.0
        && n <= MAX_APPROX_POINTS
        && let Some(s) = approximate(fit, &u, d0, d1, opts.tolerance)
    {
        return s;
    }
    let through_control = || {
        let mut s = Spline::from_control(fit.to_vec(), 3.min(n - 1));
        s.fit = fit.to_vec();
        s
    };
    if n > MAX_INTERP_POINTS {
        return through_control();
    }
    interpolate(fit, &u, d0, d1).unwrap_or_else(through_control)
}

/// Global interpolation: one control point per fit point plus one per end-derivative
/// constraint, knots by averaging the parameters (an end with a derivative counts its
/// parameter twice).
fn interpolate(fit: &[Vec2], u: &[f64], d0: Option<Vec2>, d1: Option<Vec2>) -> Option<Spline> {
    let (p, knots) = interp_knots(u, d0.is_some(), d1.is_some());
    let m = knots.len().checked_sub(p + 1)?;
    let mut a = vec![vec![0.0; m]; m];
    let (mut xs, mut ys) = (vec![0.0; m], vec![0.0; m]);
    let mut row = 0;
    let mut put = |coeffs: &[(usize, f64)], rhs: Vec2, a: &mut Vec<Vec<f64>>| {
        if let Some(r) = a.get_mut(row) {
            for (j, v) in coeffs {
                if let Some(c) = r.get_mut(*j) {
                    *c += *v;
                }
            }
        }
        if let (Some(x), Some(y)) = (xs.get_mut(row), ys.get_mut(row)) {
            (*x, *y) = (rhs.x, rhs.y);
        }
        row += 1;
    };
    for (ui, q) in u.iter().zip(fit) {
        let span = find_span(m - 1, p, *ui, &knots);
        let coeffs: Vec<(usize, f64)> = basis(span, *ui, p, &knots).into_iter().enumerate().map(|(k, v)| (span + k - p, v)).collect();
        put(&coeffs, *q, &mut a);
    }
    // Clamped ends: C'(0) = p / u_(p+1) (P1 - P0), C'(1) = p / (1 - u_(m-1)) (P_(m-1) - P_(m-2)).
    if let Some(d) = d0 {
        let c = p as f64 / knots.get(p + 1).copied().unwrap_or(1.0).max(1e-12);
        put(&[(0, -c), (1, c)], d, &mut a);
    }
    if let Some(d) = d1 {
        let c = p as f64 / (1.0 - knots.get(m - 1).copied().unwrap_or(0.0)).max(1e-12);
        put(&[(m - 2, -c), (m - 1, c)], d, &mut a);
    }
    let (cx, cy) = (solve(a.clone(), xs)?, solve(a, ys)?);
    let control: Vec<Vec2> = cx.into_iter().zip(cy).map(|(x, y)| Vec2::new(x, y)).collect();
    if control.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
        return None;
    }
    Some(Spline { degree: p, knots, control, fit: fit.to_vec(), ..Spline::default() })
}

/// Degree and clamped knots for interpolating at parameters `u`, with optional end derivatives:
/// knots average the parameters, an end with a derivative counting its parameter twice.
fn interp_knots(u: &[f64], d0: bool, d1: bool) -> (usize, Vec<f64>) {
    let m = u.len() + usize::from(d0) + usize::from(d1);
    let p = 3.min(m.saturating_sub(1)).max(1);
    let mut seq = Vec::with_capacity(m);
    seq.extend(u.first().copied().filter(|_| d0));
    seq.extend_from_slice(u);
    seq.extend(u.last().copied().filter(|_| d1));
    let mut knots = vec![0.0; p + 1];
    for j in 1..m.saturating_sub(p) {
        let s: f64 = (j..j + p).filter_map(|i| seq.get(i)).sum();
        knots.push(s / p as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    (p, knots)
}

/// Clamped knots for a least-squares fit with `m` control points of degree `p` so every span
/// holds parameters (The NURBS Book eq. 9.69). Needs `p < m < u.len()`.
fn lsq_knots(u: &[f64], m: usize, p: usize) -> Vec<f64> {
    let n = u.len();
    let d = n as f64 / m.saturating_sub(p).max(1) as f64;
    let mut knots = vec![0.0; p + 1];
    for j in 1..m.saturating_sub(p) {
        let jd = j as f64 * d;
        let i = (jd.floor() as usize).clamp(1, n.saturating_sub(1).max(1));
        let a = jd - i as f64;
        knots.push((1.0 - a) * u.get(i - 1).copied().unwrap_or(0.0) + a * u.get(i).copied().unwrap_or(1.0));
    }
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    knots
}

/// Periodic knots for `n` points with closed parameters `t` (`n + 1` values): knots[j] =
/// t_(j-p), extended by whole periods on both sides.
fn periodic_knots(t: &[f64], n: usize, p: usize) -> Vec<f64> {
    let tk = |i: isize| {
        let (q, r) = (i.div_euclid(n.max(1) as isize), i.rem_euclid(n.max(1) as isize) as usize);
        t.get(r).copied().unwrap_or(0.0) + q as f64
    };
    (0..n + 2 * p + 1).map(|j| tk(j as isize - p as isize)).collect()
}

/// Least-squares approximation within `tol`: the fewest control points (searched by bisection)
/// whose curve passes within `tol` of every fit point, the end points (and end tangents) kept
/// exact. `None` when only interpolation meets the tolerance.
fn approximate(fit: &[Vec2], u: &[f64], d0: Option<Vec2>, d1: Option<Vec2>, tol: f64) -> Option<Spline> {
    let n = fit.len();
    let p = 3;
    // Fewer control points than fit points, or it is not an approximation.
    let (mut lo, mut hi) = (p + 1, n.checked_sub(1)?);
    let mut best = None;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        match least_squares(fit, u, mid, p, d0, d1) {
            Some(s) if max_deviation(&s, fit, u) <= tol => {
                best = Some(s);
                hi = mid - 1;
            }
            _ => lo = mid + 1,
        }
    }
    best
}

fn max_deviation(s: &Spline, fit: &[Vec2], u: &[f64]) -> f64 {
    fit.iter().zip(u).map(|(q, t)| s.eval(*t).dist(*q)).fold(0.0, f64::max)
}

fn least_squares(fit: &[Vec2], u: &[f64], m: usize, p: usize, d0: Option<Vec2>, d1: Option<Vec2>) -> Option<Spline> {
    let n = fit.len();
    if m <= p || m >= n {
        return None;
    }
    let knots = lsq_knots(u, m, p);
    let (first, last) = (*fit.first()?, *fit.last()?);
    let mut fixed: Vec<Option<Vec2>> = vec![None; m];
    let mut fix = |j: usize, q: Vec2| {
        if let Some(f) = fixed.get_mut(j) {
            *f = Some(q);
        }
    };
    fix(0, first);
    fix(m - 1, last);
    // An end tangent fixes the neighbouring control point (clamped-end derivative).
    if let Some(d) = d0 {
        fix(1, first + d * (knots.get(p + 1).copied().unwrap_or(1.0) / p as f64));
    }
    if let Some(d) = d1 {
        fix(m - 2, last - d * ((1.0 - knots.get(m - 1).copied().unwrap_or(0.0)) / p as f64));
    }
    let free: Vec<usize> = (0..m).filter(|j| fixed.get(*j).is_some_and(Option::is_none)).collect();
    let k = free.len();
    let mut a = vec![vec![0.0; k]; k];
    let (mut bx, mut by) = (vec![0.0; k], vec![0.0; k]);
    for (t, q) in u.iter().zip(fit).skip(1).take(n.saturating_sub(2)) {
        let span = find_span(m - 1, p, *t, &knots);
        let mut row = vec![0.0; m];
        for (i, v) in basis(span, *t, p, &knots).into_iter().enumerate() {
            if let Some(c) = row.get_mut(span + i - p) {
                *c = v;
            }
        }
        let mut r = *q;
        for (j, c) in row.iter().enumerate() {
            if let Some(Some(f)) = fixed.get(j) {
                r -= *f * *c;
            }
        }
        for (ri, &fi) in free.iter().enumerate() {
            let ni = row.get(fi).copied().unwrap_or(0.0);
            if ni == 0.0 {
                continue;
            }
            if let (Some(x), Some(y)) = (bx.get_mut(ri), by.get_mut(ri)) {
                *x += ni * r.x;
                *y += ni * r.y;
            }
            for (ci, &fj) in free.iter().enumerate() {
                if let Some(cell) = a.get_mut(ri).and_then(|rr| rr.get_mut(ci)) {
                    *cell += ni * row.get(fj).copied().unwrap_or(0.0);
                }
            }
        }
    }
    let (cx, cy) = if k == 0 { (Vec::new(), Vec::new()) } else { (solve(a.clone(), bx)?, solve(a, by)?) };
    let mut solved = free.iter().zip(cx.into_iter().zip(cy));
    let control: Vec<Vec2> = fixed.into_iter().map(|f| f.or_else(|| solved.next().map(|(_, (x, y))| Vec2::new(x, y)))).collect::<Option<_>>()?;
    if control.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
        return None;
    }
    Some(Spline { degree: p, knots, control, fit: fit.to_vec(), ..Spline::default() })
}

/// Fit point counts above this ignore kinks (each piece is fitted and joined separately).
const MAX_KINK_POINTS: usize = 20_000;

/// Kink positions: `fit` with consecutive repeats removed and, per kept point, whether it was
/// repeated (a kink). A closed list's repeated closing point is not a kink.
fn kinks(fit: &[Vec2], closed: bool) -> (Vec<Vec2>, Vec<bool>) {
    let mut pts: Vec<Vec2> = Vec::with_capacity(fit.len());
    let mut kink: Vec<bool> = Vec::with_capacity(fit.len());
    for q in fit {
        if pts.last().is_some_and(|l| l.near(*q, 1e-12)) {
            if let Some(k) = kink.last_mut() {
                *k = true;
            }
        } else {
            pts.push(*q);
            kink.push(false);
        }
    }
    if closed && pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
        pts.pop();
        let last = kink.pop().unwrap_or(false);
        if let Some(k) = kink.first_mut() {
            *k |= last;
        }
    }
    (pts, kink)
}

/// A curve with corners at repeated fit points: each run between kinks is fitted open (the
/// outer ends keep the end tangents) and the cubic pieces are joined end to end. A closed curve
/// starts and ends at its first kink. `None` without kinks.
fn kinked_fit(fit: &[Vec2], closed: bool, opts: FitOptions) -> Option<Spline> {
    let (mut pts, mut kink) = kinks(fit, closed);
    let n = pts.len();
    if n < 2 {
        return None;
    }
    // Ends of an open curve are corners anyway, so only interior kinks count.
    let interior = |i: usize| closed || (i > 0 && i + 1 < n);
    let first = (0..n).find(|i| kink.get(*i).copied().unwrap_or(false) && interior(*i))?;
    if closed {
        pts.rotate_left(first);
        kink.rotate_left(first);
        pts.push(*pts.first()?);
        kink.push(true);
    }
    let last = pts.len() - 1;
    let mut pieces: Vec<Spline> = Vec::new();
    let mut start = 0;
    for i in 1..=last {
        if i < last && !kink.get(i).copied().unwrap_or(false) {
            continue;
        }
        let run = pts.get(start..=i)?;
        let o = FitOptions {
            start_tangent: if start == 0 && !closed { opts.start_tangent } else { None },
            end_tangent: if i == last && !closed { opts.end_tangent } else { None },
            ..opts
        };
        pieces.push(cubic(open_fit(run, o))?);
        start = i;
    }
    let lens: Vec<f64> = pieces.iter().map(|p| chord_length(&p.fit).max(1e-12)).collect();
    let total: f64 = lens.iter().sum();
    let p = 3;
    let mut knots: Vec<f64> = Vec::new();
    let mut control: Vec<Vec2> = Vec::new();
    let mut at = 0.0;
    let count = pieces.len();
    for (j, (piece, len)) in pieces.iter().zip(&lens).enumerate() {
        let (lo, hi) = piece.domain();
        let scale = len / total.max(1e-300) / (hi - lo).max(1e-300);
        let mapped = piece.knots.iter().map(|k| at + (k - lo) * scale);
        let keep = piece.knots.len() - usize::from(j + 1 < count);
        knots.extend(mapped.take(keep).skip(if j == 0 { 0 } else { p + 1 }));
        control.extend(piece.control.iter().skip(usize::from(j > 0)).copied());
        at += len / total.max(1e-300);
    }
    let s = Spline { degree: p, knots, control, fit: fit.to_vec(), closed, ..Spline::default() };
    s.is_valid().then_some(s)
}

/// A clamped, non-rational fitted piece raised to degree 3: interpolation through two or three
/// points gives a single Bézier segment of lower degree, which is raised exactly.
fn cubic(mut s: Spline) -> Option<Spline> {
    if s.degree == 3 {
        return Some(s);
    }
    if s.degree > 3 || s.control.len() != s.degree + 1 {
        return None;
    }
    let (lo, hi) = s.domain();
    while s.degree < 3 {
        let p = s.degree;
        let c = &s.control;
        let mut q = Vec::with_capacity(p + 2);
        q.push(*c.first()?);
        for i in 1..=p {
            let a = i as f64 / (p + 1) as f64;
            q.push(*c.get(i - 1)? * a + *c.get(i)? * (1.0 - a));
        }
        q.push(*c.last()?);
        s.degree = p + 1;
        s.control = q;
    }
    s.knots = [lo; 4].into_iter().chain([hi; 4]).collect();
    Some(s)
}

/// Least-squares approximation of a closed curve within `tol`: the fewest periodic control
/// points (searched by bisection) whose curve passes within `tol` of every fit point. `None`
/// when only interpolation meets the tolerance.
fn approximate_closed(pts: &[Vec2], t: &[f64], p: usize, tol: f64) -> Option<Spline> {
    let n = pts.len();
    let (mut lo, mut hi) = (p + 1, n.checked_sub(1)?);
    let mut best = None;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        match periodic_least_squares(pts, t, mid, p) {
            Some(s) if pts.iter().zip(t).all(|(q, u)| s.eval(*u).dist(*q) <= tol) => {
                best = Some(s);
                hi = mid - 1;
            }
            _ => lo = mid + 1,
        }
    }
    best
}

/// A periodic spline with `m` distinct control points (`p < m < pts.len()`) closest in the
/// least-squares sense to `pts` at closed parameters `t`. The period's breakpoints spread over
/// the parameters so every span holds some (the closed counterpart of `lsq_knots`).
fn periodic_least_squares(pts: &[Vec2], t: &[f64], m: usize, p: usize) -> Option<Spline> {
    let n = pts.len();
    if m <= p || m >= n || t.len() != n + 1 {
        return None;
    }
    let mut b = Vec::with_capacity(m + 1);
    for j in 0..=m {
        let jd = j as f64 * n as f64 / m as f64;
        let i = (jd.floor() as usize).min(n - 1);
        let a = jd - i as f64;
        b.push((1.0 - a) * t.get(i).copied()? + a * t.get(i + 1).copied()?);
    }
    if b.windows(2).any(|w| w.first().zip(w.get(1)).is_none_or(|(x, y)| y - x <= 1e-12)) {
        return None;
    }
    let knots = periodic_knots(&b, m, p);
    let mut a = vec![vec![0.0; m]; m];
    let (mut bx, mut by) = (vec![0.0; m], vec![0.0; m]);
    for (q, u) in pts.iter().zip(t) {
        let span = find_span(m + p - 1, p, *u, &knots);
        let mut row = vec![0.0; m];
        for (k, v) in basis(span, *u, p, &knots).into_iter().enumerate() {
            if let Some(c) = (span + k).checked_sub(p).and_then(|i| row.get_mut(i % m)) {
                *c += v;
            }
        }
        for (i, ri) in row.iter().enumerate() {
            if *ri == 0.0 {
                continue;
            }
            if let (Some(x), Some(y)) = (bx.get_mut(i), by.get_mut(i)) {
                *x += ri * q.x;
                *y += ri * q.y;
            }
            if let Some(ar) = a.get_mut(i) {
                for (cell, rj) in ar.iter_mut().zip(&row) {
                    *cell += ri * rj;
                }
            }
        }
    }
    let (cx, cy) = (solve(a.clone(), bx)?, solve(a, by)?);
    let mut control: Vec<Vec2> = cx.into_iter().zip(cy).map(|(x, y)| Vec2::new(x, y)).collect();
    if control.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
        return None;
    }
    control.extend_from_within(..p);
    Some(Spline { degree: p, knots, control, fit: pts.to_vec(), closed: true, ..Spline::default() })
}

fn closed_fit(fit: &[Vec2], kind: KnotParam, tol: f64) -> Spline {
    let mut pts = fit.to_vec();
    if pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
        pts.pop();
    }
    let n = pts.len();
    let fallback = |pts: &[Vec2]| {
        let mut f = pts.to_vec();
        f.extend(pts.first().copied());
        let mut s = open_fit(&f, FitOptions { knots: kind, ..FitOptions::default() });
        s.fit = pts.to_vec();
        s.closed = true;
        s
    };
    let p = 3;
    // The open fallback runs through n + 1 points, so above the cap it does not interpolate.
    if n < p || n > MAX_INTERP_POINTS {
        return fallback(&pts);
    }
    // Parameters t_0 = 0 .. t_n = 1 (t_n is the return to the first point).
    let t = params(&pts, kind, true);
    if tol > 0.0
        && n <= MAX_APPROX_POINTS
        && let Some(s) = approximate_closed(&pts, &t, p, tol)
    {
        return s;
    }
    let m = n + p;
    let knots = periodic_knots(&t, n, p);
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
    Spline { degree: p, knots, control, fit: pts, closed: true, ..Spline::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts() -> Vec<Vec2> {
        vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.2), Vec2::new(5.0, 3.0), Vec2::new(6.0, 3.1), Vec2::new(9.0, 0.0)]
    }

    fn passes_through(s: &Spline, fit: &[Vec2], tol: f64) -> bool {
        let u = params(fit, s.fit_opts.knots, s.closed);
        fit.iter().zip(&u).all(|(f, t)| s.eval(*t).dist(*f) < tol)
    }

    #[test]
    fn default_options_match_the_plain_fit() {
        let fit = pts();
        assert_eq!(Spline::fit_with(&fit, false, FitOptions::default()), Spline::from_fit_points(&fit));
        assert_eq!(Spline::fit_with(&fit, true, FitOptions::default()), Spline::from_fit_points_closed(&fit));
    }

    #[test]
    fn knot_parametrisations_differ_and_still_interpolate() {
        let fit = pts();
        let knots: Vec<Vec<f64>> = KnotParam::ALL
            .iter()
            .map(|k| {
                let s = Spline::fit_with(&fit, false, FitOptions { knots: *k, ..FitOptions::default() });
                assert!(s.is_valid() && passes_through(&s, &fit, 1e-3), "{k:?}");
                assert_eq!(s.fit_opts.knots, *k);
                assert_eq!(s.infer_knot_param(), *k, "the knots identify the parametrisation");
                s.knots
            })
            .collect();
        assert!(knots[0] != knots[1] && knots[1] != knots[2] && knots[0] != knots[2]);
        // Uniform: equally spaced parameters, so the interior knots are 1/2 (five points, cubic).
        assert!(knots[2].iter().any(|k| (k - 0.5).abs() < 1e-12));
        for k in KnotParam::ALL {
            let s = Spline::fit_with(&fit, true, FitOptions { knots: k, ..FitOptions::default() });
            assert!(s.is_valid() && s.is_periodic() && passes_through(&s, &fit, 1e-3), "closed {k:?}");
            assert_eq!(s.infer_knot_param(), k);
        }
    }

    #[test]
    fn end_tangents_set_the_end_directions() {
        let fit = pts();
        let opts = FitOptions { start_tangent: Some(Vec2::new(0.0, 2.0)), end_tangent: Some(Vec2::new(1.0, -1.0)), ..FitOptions::default() };
        let s = Spline::fit_with(&fit, false, opts);
        assert!(s.is_valid() && passes_through(&s, &fit, 1e-3));
        assert_eq!(s.control.len(), fit.len() + 2);
        assert_eq!(s.fit_opts.start_tangent, Some(Vec2::new(0.0, 1.0)), "stored as a unit vector");
        let (lo, hi) = s.domain();
        let h = 1e-7;
        let start = (s.eval(lo + h) - s.eval(lo)).normalized();
        let end = (s.eval(hi) - s.eval(hi - h)).normalized();
        assert!(start.near(Vec2::new(0.0, 1.0), 1e-4), "{start:?}");
        assert!(end.near(Vec2::new(1.0, -1.0).normalized(), 1e-4), "{end:?}");
        // One tangent only, and two fit points with tangents (a cubic Bezier).
        let one = Spline::fit_with(&fit, false, FitOptions { start_tangent: Some(Vec2::Y), ..FitOptions::default() });
        assert!(one.is_valid() && one.control.len() == fit.len() + 1 && passes_through(&one, &fit, 1e-3));
        let two = [Vec2::ZERO, Vec2::new(4.0, 0.0)];
        let b = Spline::fit_with(
            &two,
            false,
            FitOptions { start_tangent: Some(Vec2::Y), end_tangent: Some(Vec2::new(0.0, -1.0)), ..FitOptions::default() },
        );
        assert!(b.is_valid() && b.degree == 3 && b.control.len() == 4);
        assert!(b.eval(0.5).y > 0.5, "the tangents bow the curve up");
        // Closed curves ignore tangents.
        assert_eq!(Spline::fit_with(&fit, true, opts).fit_opts.start_tangent, None);
    }

    #[test]
    fn tolerance_approximates_with_fewer_control_points() {
        // Thirty noisy points along a gentle arc.
        let fit: Vec<Vec2> = (0..30)
            .map(|i| {
                let t = i as f64 / 29.0;
                Vec2::new(t * 10.0, (t * 3.0).sin() * 2.0 + if i % 2 == 0 { 0.01 } else { -0.01 })
            })
            .collect();
        let exact = Spline::fit_with(&fit, false, FitOptions::default());
        let tol = 0.1;
        let s = Spline::fit_with(&fit, false, FitOptions { tolerance: tol, ..FitOptions::default() });
        assert!(s.is_valid());
        assert!(s.control.len() < exact.control.len(), "{} control points", s.control.len());
        let u = params(&fit, KnotParam::Chord, false);
        assert!(max_deviation(&s, &fit, &u) <= tol + 1e-9);
        assert!(s.eval(0.0).near(fit[0], 1e-9) && s.eval(1.0).near(fit[29], 1e-9), "ends stay exact");
        assert_eq!(s.fit, fit, "the fit points are kept");
        assert_eq!(s.fit_opts.tolerance, tol);
        // With tangents too.
        let t = Spline::fit_with(
            &fit,
            false,
            FitOptions { tolerance: tol, start_tangent: Some(Vec2::X), end_tangent: Some(Vec2::X), ..FitOptions::default() },
        );
        assert!(t.is_valid() && max_deviation(&t, &fit, &u) <= tol + 1e-9);
        let h = 1e-7;
        assert!((t.eval(h) - t.eval(0.0)).normalized().near(Vec2::X, 1e-4));
        // A tolerance too small to approximate interpolates instead.
        let tiny = Spline::fit_with(&fit, false, FitOptions { tolerance: 1e-9, ..FitOptions::default() });
        assert_eq!(tiny.control.len(), exact.control.len());
    }

    #[test]
    fn tolerance_approximates_closed_curves() {
        // Forty slightly noisy points round a circle.
        let fit: Vec<Vec2> = (0..40)
            .map(|i| {
                let a = i as f64 / 40.0 * std::f64::consts::TAU;
                let r = 5.0 + if i % 2 == 0 { 0.01 } else { -0.01 };
                Vec2::new(a.cos() * r, a.sin() * r)
            })
            .collect();
        let exact = Spline::fit_with(&fit, true, FitOptions::default());
        let tol = 0.05;
        let s = Spline::fit_with(&fit, true, FitOptions { tolerance: tol, ..FitOptions::default() });
        assert!(s.is_valid() && s.is_periodic() && s.closed);
        assert!(s.control.len() < exact.control.len(), "{} control points", s.control.len());
        let u = params(&fit, KnotParam::Chord, true);
        assert!(fit.iter().zip(&u).all(|(q, t)| s.eval(*t).dist(*q) <= tol + 1e-9));
        let (lo, hi) = s.domain();
        assert!(s.eval(lo).near(s.eval(hi), 1e-9), "still closed");
        assert_eq!(s.fit, fit);
        assert_eq!(s.fit_opts.tolerance, tol);
        // Too tight a tolerance interpolates.
        let tiny = Spline::fit_with(&fit, true, FitOptions { tolerance: 1e-9, ..FitOptions::default() });
        assert_eq!(tiny.control.len(), exact.control.len());
        // Every parametrisation approximates.
        for k in KnotParam::ALL {
            let s = Spline::fit_with(&fit, true, FitOptions { tolerance: tol, knots: k, ..FitOptions::default() });
            assert!(s.is_valid() && s.control.len() < exact.control.len(), "{k:?}");
        }
    }

    fn on_curve(s: &Spline, q: Vec2) -> bool {
        let (lo, hi) = s.domain();
        (0..=4000).any(|i| s.eval(lo + (hi - lo) * i as f64 / 4000.0).dist(q) < 1e-2)
    }

    #[test]
    fn repeated_fit_points_make_kinks() {
        let fit = [Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(6.0, 2.0), Vec2::new(8.0, 0.0)];
        let s = Spline::fit_with(&fit, false, FitOptions::default());
        assert!(s.is_valid() && s.degree == 3 && !s.closed);
        assert_eq!(s.fit, fit, "the kink stays in the fit data");
        assert!(fit.iter().all(|q| on_curve(&s, *q)));
        // A knot of multiplicity 3 at the corner, where the tangent turns.
        let k = s.knots.iter().copied().find(|k| s.knots.iter().filter(|x| (*x - k).abs() < 1e-12).count() == 3).unwrap();
        assert!(s.eval(k).near(Vec2::new(4.0, 0.0), 1e-9));
        let h = 1e-6;
        let before = (s.eval(k) - s.eval(k - h)).normalized();
        let after = (s.eval(k + h) - s.eval(k)).normalized();
        assert!(before.dot(after) < 0.5, "a corner: {before:?} {after:?}");
        // Two-point pieces become straight cubic pieces.
        let l = [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)];
        let s = Spline::fit_with(&l, false, FitOptions::default());
        assert!(s.is_valid() && s.degree == 3 && s.control.len() == 7);
        assert!(on_curve(&s, Vec2::new(0.5, 0.0)) && on_curve(&s, Vec2::new(1.0, 0.5)));
        // Closed: the curve starts and ends at the kink, clamped.
        let c = [Vec2::ZERO, Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)];
        let s = Spline::fit_with(&c, true, FitOptions::default());
        assert!(s.is_valid() && s.closed && !s.is_periodic());
        let (lo, hi) = s.domain();
        assert!(s.eval(lo).near(Vec2::ZERO, 1e-9) && s.eval(hi).near(Vec2::ZERO, 1e-9));
        assert!(c.iter().all(|q| on_curve(&s, *q)));
        // Kinks at the ends of an open curve change nothing.
        let e = [Vec2::ZERO, Vec2::ZERO, Vec2::new(2.0, 2.0), Vec2::new(4.0, 0.0)];
        let plain = Spline::fit_with(&e[1..], false, FitOptions::default());
        assert_eq!(Spline::fit_with(&e, false, FitOptions::default()).control.len(), plain.control.len());
    }

    #[test]
    fn hostile_options_never_panic() {
        let fit = pts();
        let bad = FitOptions {
            knots: KnotParam::Uniform,
            start_tangent: Some(Vec2::new(f64::NAN, 1.0)),
            end_tangent: Some(Vec2::ZERO),
            tolerance: f64::INFINITY,
        };
        let s = Spline::fit_with(&fit, false, bad);
        assert!(s.is_valid() && s.fit_opts.start_tangent.is_none() && s.fit_opts.end_tangent.is_none() && s.fit_opts.tolerance == 0.0);
        for n in 0..4 {
            let few: Vec<Vec2> = fit.iter().take(n).copied().collect();
            let _ = Spline::fit_with(
                &few,
                false,
                FitOptions { start_tangent: Some(Vec2::Y), end_tangent: Some(Vec2::X), tolerance: 1.0, ..FitOptions::default() },
            );
            let _ = Spline::fit_with(&few, true, FitOptions { knots: KnotParam::SqrtChord, ..FitOptions::default() });
        }
        let same = [Vec2::ZERO; 5];
        let _ = Spline::fit_with(&same, false, FitOptions { tolerance: 0.5, start_tangent: Some(Vec2::X), ..FitOptions::default() });
        let _ = Spline::fit_with(&same, true, FitOptions { tolerance: 0.5, ..FitOptions::default() });
        let pairs = [Vec2::ZERO, Vec2::ZERO, Vec2::X, Vec2::X, Vec2::Y, Vec2::Y];
        for closed in [false, true] {
            for n in 0..=pairs.len() {
                let _ = Spline::fit_with(&pairs[..n], closed, FitOptions { tolerance: 0.1, ..FitOptions::default() });
            }
        }
        let tight: Vec<Vec2> = (0..8).map(|i| Vec2::new(i as f64 * 1e-13, 0.0)).collect();
        let _ = Spline::fit_with(&tight, true, FitOptions { tolerance: 1.0, ..FitOptions::default() });
    }

    /// Above the interpolation cap the fit points become control points instead of a dense
    /// system the size of their count squared (issue #702); just below it they still interpolate.
    #[test]
    fn too_many_fit_points_are_not_interpolated() {
        let wave = |n: usize| -> Vec<Vec2> { (0..n).map(|i| Vec2::new((i * i) as f64 * 1e-3, (i % 5) as f64)).collect() };
        let fit = wave(MAX_INTERP_POINTS + 1);
        let s = Spline::fit_with(&fit, false, FitOptions::default());
        assert_eq!(s.control, fit);
        assert_eq!(s.knots, Spline::from_control(fit.clone(), 3).knots);
        assert_eq!(s.fit, fit);

        let c = Spline::fit_with(&fit, true, FitOptions::default());
        let mut ring = fit.clone();
        ring.extend(fit.first().copied());
        assert!(c.closed);
        assert_eq!(c.control, ring);
        assert_eq!(c.fit, fit);

        let few = wave(64);
        assert!(passes_through(&Spline::fit_with(&few, false, FitOptions::default()), &few, 1e-6));
        assert!(passes_through(&Spline::fit_with(&few, true, FitOptions::default()), &few, 1e-6));
    }

    #[test]
    fn transform_turns_the_tangents() {
        let fit = pts();
        let mut s = Spline::fit_with(&fit, false, FitOptions { start_tangent: Some(Vec2::X), ..FitOptions::default() });
        s.transform(&Mat3::rotate(std::f64::consts::FRAC_PI_2));
        assert!(s.fit_opts.start_tangent.is_some_and(|t| t.near(Vec2::Y, 1e-12)));
    }
}

#[cfg(test)]
mod knot_boundary_tests {
    use super::*;
    #[test]
    fn uniform_knots_skip_the_strict_lower_tolerance_boundary() {
        let q = 2.0 / 3.0;
        let mut s = Spline::from_control(vec![Vec2::ZERO; 5], 1);
        s.fit = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(9.0, 0.0)];
        s.knots = vec![0.0, 0.0, 1.0 / 3.0, q - 1e-6, q, 1.0, 1.0];
        assert!(s.is_valid());
        assert_eq!(s.infer_knot_param(), KnotParam::Uniform);
    }
}
