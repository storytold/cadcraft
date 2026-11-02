//! Wide polylines with a linetype: each dash is a filled piece of the band, gaps are empty.

use cadcraft_doc::{Linetype, LwPolyline};
use cadcraft_geom::{Polyline, Vec2};

use crate::linetype;

/// A point on the centre line: position, unit direction, half width and distance along the run.
#[derive(Clone, Copy, Debug)]
struct Sample {
    p: Vec2,
    dir: Vec2,
    hw: f64,
    s: f64,
}

/// Centre-line samples of each segment, widths interpolated as [`crate::fill::wide_polyline`]
/// does; distances start at 0 for each segment.
fn segments(p: &LwPolyline, tol: f64) -> Vec<Vec<Sample>> {
    let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
    let n = p.vertices.len();
    let mut out = Vec::new();
    for (i, seg) in pl.segments().iter().enumerate() {
        let v = p.vertices.get(i % n.max(1)).copied().unwrap_or_default();
        let (w0, w1) = if p.const_width > 0.0 { (p.const_width, p.const_width) } else { (v.start_width, v.end_width) };
        let mut pts = Vec::new();
        seg.tessellate(tol, &mut pts);
        let m = pts.len();
        if m < 2 {
            continue;
        }
        let mut run = Vec::with_capacity(m);
        let mut s = 0.0;
        for (k, q) in pts.iter().enumerate() {
            if let Some(prev) = k.checked_sub(1).and_then(|j| pts.get(j)) {
                s += prev.dist(*q);
            }
            let t = k as f64 / (m - 1) as f64;
            let dir = if k + 1 < m { pts.get(k + 1).map(|nx| *nx - *q) } else { pts.get(k.wrapping_sub(1)).map(|pv| *q - *pv) }
                .unwrap_or(Vec2::X)
                .normalized();
            run.push(Sample { p: *q, dir, hw: (w0 + (w1 - w0) * t) / 2.0, s });
        }
        out.push(run);
    }
    out
}

/// The sample at distance `s` along `run` (clamped to its ends).
fn at(run: &[Sample], s: f64) -> Option<Sample> {
    let i = run.partition_point(|x| x.s <= s).saturating_sub(1);
    let a = *run.get(i)?;
    let Some(b) = run.get(i + 1).copied() else { return Some(a) };
    let len = b.s - a.s;
    if len <= 1e-15 {
        return Some(a);
    }
    let t = ((s - a.s) / len).clamp(0.0, 1.0);
    let dir = (b.p - a.p).normalized();
    Some(Sample { p: a.p + (b.p - a.p) * t, dir, hw: a.hw + (b.hw - a.hw) * t, s })
}

/// The two band edges at a sample.
fn edges(x: &Sample) -> (Vec2, Vec2) {
    let nrm = x.dir.perp() * x.hw;
    (x.p + nrm, x.p - nrm)
}

/// Triangles for the dashes of a wide polyline drawn with `lt` at `scale`, a line across the band
/// for each dot, and the centre line of each dash. With PLINEGEN the pattern runs along the whole
/// polyline; otherwise it restarts at every vertex. A pattern shorter than `min_pattern` fills
/// the whole band.
pub fn dashed(p: &LwPolyline, tol: f64, lt: &Linetype, scale: f64, min_pattern: f64) -> (Vec<Vec2>, Vec<Vec<Vec2>>, Vec<Vec<Vec2>>) {
    let mut runs = segments(p, tol);
    if p.plinegen && runs.len() > 1 {
        let mut whole: Vec<Sample> = Vec::new();
        for run in runs {
            let base = whole.last().map(|x| x.s).unwrap_or(0.0);
            whole.extend(run.into_iter().map(|x| Sample { s: x.s + base, ..x }));
        }
        runs = vec![whole];
    }
    let mut tris = Vec::new();
    let mut ticks = Vec::new();
    let mut centres = Vec::new();
    for run in &runs {
        let len = run.last().map(|x| x.s).unwrap_or(0.0);
        let dashes = linetype::ranges(len, lt, scale, min_pattern).unwrap_or_else(|| vec![(0.0, len)]);
        for (a, b) in dashes {
            if b - a <= 1e-12 {
                if let Some(x) = at(run, a) {
                    let (l, r) = edges(&x);
                    ticks.push(if x.hw > 0.0 { vec![l, r] } else { vec![x.p] });
                }
                continue;
            }
            let (Some(first), Some(last)) = (at(run, a), at(run, b)) else { continue };
            let inner = run.iter().filter(|x| x.s > a && x.s < b).copied();
            let pts: Vec<Sample> = std::iter::once(first).chain(inner).chain(std::iter::once(last)).collect();
            for w in pts.windows(2) {
                if let [x, y] = w {
                    let (a0, a1) = edges(x);
                    let (b0, b1) = edges(y);
                    tris.extend_from_slice(&[a0, a1, b1, a0, b1, b0]);
                }
            }
            centres.push(pts.iter().map(|x| x.p).collect());
            if tris.len() > 6_000_000 {
                return (tris, ticks, centres);
            }
        }
    }
    (tris, ticks, centres)
}
