//! Apply a dash pattern along a run (a polyline), laid out as AutoCAD lays it out.
//!
//! Measured on AutoCAD 2024 plots (DWG To PDF at a fixed scale, every stroke read back), with the
//! pattern [2, -1] unless said otherwise:
//! - An open run (a line, an arc, a spline, an open polyline with PLINEGEN, each segment of one
//!   without) shorter than one period is drawn continuous. Otherwise the pattern is centred:
//!   with n = floor(L / period) and p0 = (n·period + first − L) / 2, the elements sit where they
//!   would if the pattern started at −p0, except that the first dash starts at the run's start
//!   and the last ends at its end. A line 7 long gets [0, 1.5] [2.5, 4.5] [5.5, 7]; 6 gets
//!   [0, 1] [2, 4] [5, 6]; 5.99 gets [0, 2.495] [3.495, 5.99]; 2.99 is continuous. A pattern
//!   opening with a dot puts dots at both ends ([0, −1] on 5.3: 0, 1.15, 2.15, 3.15, 4.15, 5.3).
//! - A closed run (a circle, a whole ellipse, a closed polyline with PLINEGEN) is stretched to
//!   n = round(L / period) periods, one starting at its anchor: a circle of radius 2 is dashed
//!   0°–60°, 90°–150°…; a 5 × 5 square polyline gets 7 periods.
//! - A pattern opening with a gap (e0 < 0), with r = L − n·period and x = (r + e0) / 2: for
//!   x < 0 the elements run from −p0; otherwise a piece [0, x] is drawn (a dot when x is 0),
//!   then the elements from the period's second on, n − 1 whole periods, and a last piece 2x
//!   long.

use cadcraft_doc::Linetype;
use cadcraft_geom::Vec2;

/// How a run lays its pattern out (see the module notes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    /// Centred on an open run.
    Open,
    /// Stretched around a closed run, a period starting `anchor` along it.
    Closed { anchor: f64 },
}

/// Most periods a run is dashed over in a display build (`min_pattern` above 0, a view zoomed out
/// a lot); a longer run is drawn continuous. A plot or export has no zoom (`min_pattern` is 0), so
/// it keeps the pattern, bounded by [`MAX_ELEMENTS`] (#1069).
const MAX_PERIODS: f64 = 50_000.0;

/// Most pattern elements a run is laid out over (a long pattern repeated many times); beyond
/// that it is drawn continuous too.
const MAX_ELEMENTS: f64 = 2_000_000.0;

/// The drawn stretches of a run `len` long, as (start, end) along it (a dot: start == end), in
/// order; `None` when it is drawn continuous (shorter than one period, or repeating too often).
/// `pattern` is scaled; `min_pattern` is the caller's (see [`MAX_PERIODS`]).
fn layout(len: f64, pattern: &[f64], how: Layout, min_pattern: f64) -> Option<Vec<(f64, f64)>> {
    let period: f64 = pattern.iter().map(|d| d.abs()).sum();
    if !(len.is_finite() && period > 1e-12) || len < period || (min_pattern > 0.0 && len / period > MAX_PERIODS) {
        return None;
    }
    if (len / period + 2.0) * pattern.len() as f64 > MAX_ELEMENTS {
        return None;
    }
    let mut drawn = Vec::new();
    let mut at = 0.0;
    for &d in pattern {
        if d >= 0.0 {
            drawn.push((at, d));
        }
        at += d.abs();
    }
    if drawn.is_empty() {
        return Some(Vec::new());
    }
    let first = pattern.first().copied().unwrap_or(0.0);
    let out = match how {
        Layout::Closed { anchor } => {
            let n = (len / period).round().max(1.0);
            let f = len / (n * period);
            let a = if anchor.is_finite() { anchor.rem_euclid(period * f) } else { 0.0 };
            let mut out = Vec::new();
            for k in -1..=(n as i64) {
                for &(o, e) in &drawn {
                    let s0 = a + k as f64 * period * f + o * f;
                    let s1 = s0 + e * f;
                    if e == 0.0 {
                        if (0.0..len).contains(&s0) {
                            out.push((s0, s0));
                        }
                    } else if s1.min(len) > s0.max(0.0) {
                        out.push((s0.max(0.0), s1.min(len)));
                    }
                }
            }
            out
        }
        Layout::Open if first < 0.0 => gap_first(len, pattern, period),
        Layout::Open => {
            // A length at a multiple of the period: 6.0 / 3 is 2, not 1.
            let n = (len / period + 1e-9).floor();
            let p0 = (n * period + first - len) / 2.0;
            let count = n as usize * drawn.len() + 1;
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let Some(&(o, e)) = drawn.get(i % drawn.len()) else { continue };
                let mut s0 = (i / drawn.len()) as f64 * period + o - p0;
                let mut s1 = s0 + e;
                if i == 0 {
                    s0 = 0.0;
                    if first == 0.0 {
                        s1 = 0.0;
                    }
                }
                if i + 1 == count {
                    s1 = len;
                    if first == 0.0 {
                        s0 = len;
                    }
                }
                out.push((s0, s1));
            }
            out
        }
    };
    Some(out)
}

/// [`layout`] of an open run for a pattern opening with a gap.
fn gap_first(len: f64, pattern: &[f64], period: f64) -> Vec<(f64, f64)> {
    let first = pattern.first().copied().unwrap_or(0.0);
    let n = (len / period + 1e-9).floor();
    let x = (len - n * period + first) / 2.0;
    let mut out = Vec::new();
    if x < 0.0 {
        let p0 = (n * period + first - len) / 2.0;
        let mut u = 0.0;
        for _ in 0..(n as usize + 2) {
            for &e in pattern {
                let (a, b) = (u, u + e.abs());
                u = b;
                if e < 0.0 {
                    continue;
                }
                let b = b.min(p0 + len);
                if b > a || (e == 0.0 && (p0..=p0 + len).contains(&a)) {
                    out.push((a - p0, b - p0));
                }
            }
        }
    } else {
        out.push((0.0, x));
        let mut pos = x;
        let whole = pattern.iter().cycle().take(pattern.len() * (n as usize).saturating_sub(1));
        for &e in pattern.iter().skip(1).chain(whole) {
            if e >= 0.0 {
                out.push((pos, pos + e));
            }
            pos += e.abs();
        }
        if x > 0.0 {
            out.push((pos, (pos + 2.0 * x).min(len)));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Length of a polyline.
fn length(pts: &[Vec2]) -> f64 {
    pts.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum()
}

/// The pattern scaled, or `None` when a run is drawn continuous at any length: no pattern, or one
/// shorter than `min_pattern` (sub-pixel).
fn scaled(lt: &Linetype, scale: f64, min_pattern: f64) -> Option<Vec<f64>> {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let total = lt.pattern_length() * scale;
    (total > 1e-12 && total >= min_pattern).then(|| lt.pattern.iter().map(|d| d.length * scale).collect())
}

/// The pieces of `pts` over `ranges` (in order along it), a dot as a single point.
fn cut(pts: &[Vec2], ranges: &[(f64, f64)]) -> Vec<Vec<Vec2>> {
    let mut cum = Vec::with_capacity(pts.len());
    let mut acc = 0.0;
    cum.push(0.0);
    for w in pts.windows(2) {
        acc += w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0);
        cum.push(acc);
    }
    let last = pts.len().saturating_sub(1).max(1);
    // The point `s` along, and the index of the first vertex past it.
    let at = |s: f64| -> (Vec2, usize) {
        let j = cum.partition_point(|&c| c <= s).clamp(1, last);
        let (Some(&a), Some(&b), Some(&ca), Some(&cb)) = (pts.get(j - 1), pts.get(j), cum.get(j - 1), cum.get(j)) else {
            return (pts.first().copied().unwrap_or(Vec2::ZERO), j);
        };
        let t = if cb > ca { ((s - ca) / (cb - ca)).clamp(0.0, 1.0) } else { 0.0 };
        (a + (b - a) * t, j)
    };
    let mut out = Vec::with_capacity(ranges.len());
    for &(s0, s1) in ranges {
        let (p, j) = at(s0);
        if s1 <= s0 {
            out.push(vec![p]);
            continue;
        }
        let mut piece = vec![p];
        let mut k = j;
        while let (Some(&c), Some(&q)) = (cum.get(k), pts.get(k)) {
            if c >= s1 {
                break;
            }
            piece.push(q);
            k += 1;
        }
        piece.push(at(s1).0);
        out.push(piece);
    }
    out
}

/// Split `pts`, an open run, into dashes. Dots come back as single-point runs. A run shorter than
/// one period, or with a scaled pattern shorter than `min_pattern` (sub-pixel), comes back whole.
pub fn apply(pts: &[Vec2], lt: &Linetype, scale: f64, min_pattern: f64) -> Vec<Vec<Vec2>> {
    apply_as(pts, lt, scale, min_pattern, Layout::Open)
}

/// [`apply`] with the run laid out as `how` says.
pub fn apply_as(pts: &[Vec2], lt: &Linetype, scale: f64, min_pattern: f64, how: Layout) -> Vec<Vec<Vec2>> {
    let laid = if pts.len() >= 2 { scaled(lt, scale, min_pattern).and_then(|p| layout(length(pts), &p, how, min_pattern)) } else { None };
    match laid {
        Some(r) => cut(pts, &r),
        None => vec![pts.to_vec()],
    }
}

/// The dashes of the pattern along a run of length `len` laid out as `how` says, as (start, end)
/// distances; a dot has start == end. Laid out like [`apply_as`]. `None` when the run is drawn
/// continuous: shorter than one period, a scaled pattern shorter than `min_pattern`, or too many
/// repeats (see [`MAX_PERIODS`]).
pub fn ranges(len: f64, lt: &Linetype, scale: f64, min_pattern: f64, how: Layout) -> Option<Vec<(f64, f64)>> {
    if len > 0.0 { scaled(lt, scale, min_pattern).and_then(|p| layout(len, &p, how, min_pattern)) } else { None }
}

/// Where the pattern starts on a run `len` long laid out as `how`, and how much it is stretched:
/// period `k` starts at `start + k · period · stretch`. `None` when the run is drawn continuous.
pub fn phase(len: f64, lt: &Linetype, scale: f64, min_pattern: f64, how: Layout) -> Option<(f64, f64)> {
    let pattern = scaled(lt, scale, min_pattern)?;
    let period: f64 = pattern.iter().map(|d| d.abs()).sum();
    layout(len, &pattern, how, min_pattern)?;
    let first = pattern.first().copied().unwrap_or(0.0);
    Some(match how {
        Layout::Open => {
            let n = (len / period + 1e-9).floor();
            let x = (len - n * period + first) / 2.0;
            // A gap-first pattern with room for a piece of dash at the start ([`gap_first`]) has
            // its second element at `x`, a gap after its period starts.
            (if first < 0.0 && x >= 0.0 { x + first } else { x - first }, 1.0)
        }
        Layout::Closed { anchor } => {
            let f = len / ((len / period).round().max(1.0) * period);
            (if anchor.is_finite() { anchor.rem_euclid(period * f) } else { 0.0 }, f)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This gap-only pattern draws nothing, and `ranges` returns before cycling through its repeats.
    #[test]
    fn an_all_gap_pattern_draws_nothing_without_spinning() {
        let lt = Linetype::simple("GAPS", "", &[-0.001, -0.002]);
        assert_eq!(ranges(1.0, &lt, 1.0, 0.0, Layout::Open), Some(Vec::new()));
        assert!(apply(&[Vec2::ZERO, Vec2::new(1.0, 0.0)], &lt, 1.0, 0.0).is_empty());
    }

    #[test]
    fn dashes_split_line() {
        let lt = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        let d = apply(&[Vec2::ZERO, Vec2::new(3.0, 0.0)], &lt, 1.0, 0.0);
        // 3.0 / 0.75 = 4 periods, centred: a half dash at each end and four whole ones between.
        assert_eq!(d.len(), 5);
        assert!((d[0][1].x - 0.25).abs() < 1e-9);
        assert!((d[1][0].x - 0.5).abs() < 1e-9);
        assert!((d[4][0].x - 2.75).abs() < 1e-9 && (d[4][1].x - 3.0).abs() < 1e-9);
    }

    #[test]
    fn dots_are_single_points() {
        let lt = Linetype::simple("DOT", "", &[0.0, -0.25]);
        let d = apply(&[Vec2::ZERO, Vec2::new(1.0, 0.0)], &lt, 1.0, 0.0);
        assert!(d.iter().all(|r| r.len() == 1));
        assert!(d.len() >= 4);
    }

    #[test]
    fn tiny_pattern_is_continuous() {
        let lt = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        let d = apply(&[Vec2::ZERO, Vec2::new(3.0, 0.0)], &lt, 1.0, 10.0);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn dash_continues_across_vertices() {
        let lt = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        let d = apply(&[Vec2::ZERO, Vec2::new(0.3, 0.0), Vec2::new(0.3, 1.0)], &lt, 1.0, 0.0);
        assert_eq!(d[0].len(), 3); // the first dash turns the corner
    }
    /// The drawn stretches of a run, a continuous one as one stretch, with stretches that touch
    /// joined (a plot draws them as one stroke).
    fn drawn(len: f64, pattern: &[f64], how: Layout) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (a, b) in layout(len, pattern, how, 0.0).unwrap_or_else(|| vec![(0.0, len)]) {
            match out.last_mut() {
                Some(last) if (last.1 - a).abs() < 1e-9 && b > a && last.1 > last.0 => last.1 = b,
                _ => out.push((a, b)),
            }
        }
        out
    }

    fn near(got: &[(f64, f64)], want: &[(f64, f64)], tol: f64) -> bool {
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() <= tol && (g.1 - w.1).abs() <= tol)
    }

    /// AutoCAD 2024 plots (DWG To PDF at 10:1, every stroke read back to 0.01): open runs.
    #[test]
    fn open_runs_are_laid_out_as_autocad_plots_them() {
        let dut = [2.0, -1.0];
        for (len, want) in [
            (7.0, vec![(0.0, 1.5), (2.5, 4.5), (5.5, 7.0)]),
            (6.0, vec![(0.0, 1.0), (2.0, 4.0), (5.0, 6.0)]),
            (5.99, vec![(0.0, 2.495), (3.495, 5.99)]),
            (2.99, vec![(0.0, 2.99)]),
        ] {
            assert!(near(&drawn(len, &dut, Layout::Open), &want, 1e-9), "{len}: {:?}", drawn(len, &dut, Layout::Open));
        }
        // A pattern opening with a dot: dots at both ends.
        let dots = drawn(5.3, &[0.0, -1.0], Layout::Open);
        assert!(near(&dots, &[(0.0, 0.0), (1.15, 1.15), (2.15, 2.15), (3.15, 3.15), (4.15, 4.15), (5.3, 5.3)], 1e-9), "{dots:?}");
        // Patterns opening with a gap.
        let (h1, h2, h3, h4) = ([-1.0, 2.0], [-0.5, 2.0, -0.5], [-1.0, 2.0, -2.0, 1.0], [-2.0, 1.0]);
        let table: [(&[f64], f64, &[(f64, f64)]); 33] = [
            (&h1, 3.0, &[(1.5, 3.0)]),
            (&h1, 3.5, &[(1.75, 3.5)]),
            (&h1, 4.0, &[(0.0, 0.0), (0.0, 2.0)]),
            (&h1, 4.5, &[(0.0, 2.75)]),
            (&h1, 5.0, &[(0.0, 3.5)]),
            (&h1, 5.5, &[(0.0, 4.25)]),
            (&h1, 6.0, &[(1.5, 3.5), (4.5, 6.0)]),
            (&h1, 6.5, &[(1.75, 3.75), (4.75, 6.5)]),
            (&h1, 7.0, &[(0.0, 0.0), (0.0, 2.0), (3.0, 5.0)]),
            (&h1, 7.5, &[(0.0, 2.25), (3.25, 5.75)]),
            (&h1, 8.0, &[(0.0, 2.5), (3.5, 6.5)]),
            (&h1, 9.0, &[(1.5, 3.5), (4.5, 6.5), (7.5, 9.0)]),
            (&h1, 10.0, &[(0.0, 0.0), (0.0, 2.0), (3.0, 5.0), (6.0, 8.0)]),
            (&h1, 11.0, &[(0.0, 2.5), (3.5, 5.5), (6.5, 9.5)]),
            (&h1, 12.5, &[(1.75, 3.75), (4.75, 6.75), (7.75, 9.75), (10.75, 12.5)]),
            (&h1, 1.0, &[(0.0, 1.0)]),
            (&h1, 2.0, &[(0.0, 2.0)]),
            (&h1, 2.5, &[(0.0, 2.5)]),
            (&h1, 2.9, &[(0.0, 2.9)]),
            (&h2, 4.0, &[(0.0, 2.25), (2.75, 3.25)]),
            (&h2, 5.0, &[(0.0, 2.75), (3.25, 4.75)]),
            (&h2, 6.0, &[(0.75, 2.75), (3.75, 5.75)]),
            (&h2, 7.0, &[(0.0, 2.25), (3.25, 5.25), (5.75, 6.25)]),
            (&h2, 8.0, &[(0.0, 2.75), (3.75, 5.75), (6.25, 7.75)]),
            (&h3, 10.0, &[(0.0, 3.5), (5.5, 9.5)]),
            (&h3, 13.0, &[(0.0, 0.0), (0.0, 2.0), (4.0, 5.0), (6.0, 8.0), (10.0, 11.0)]),
            (&h3, 15.5, &[(0.0, 3.25), (5.25, 6.25), (7.25, 9.25), (11.25, 14.75)]),
            (&h4, 7.0, &[(3.5, 4.5), (6.5, 7.0)]),
            (&h4, 8.0, &[(0.0, 0.0), (0.0, 1.0), (3.0, 4.0)]),
            (&h4, 10.0, &[(3.5, 4.5), (6.5, 7.5), (9.5, 10.0)]),
            (&h4, 2.0, &[(0.0, 2.0)]),
            (&h2, 2.9, &[(0.0, 2.9)]),
            (&h3, 5.9, &[(0.0, 5.9)]),
        ];
        for (pattern, len, want) in table {
            let got = drawn(len, pattern, Layout::Open);
            assert!(near(&got, want, 0.006), "{pattern:?} on {len}: {got:?}");
        }
    }

    /// Text in a linetype follows [`phase`]: it must say where the periods of a gap-first pattern
    /// start on each side of the room left for a piece of dash at the start.
    #[test]
    fn phase_follows_the_dashes_of_a_gap_first_pattern() {
        let lt = Linetype::simple("GF", "", &[-1.0, 2.0]);
        for len in [6.5, 7.0, 7.5, 8.9, 12.5] {
            let (start, f) = phase(len, &lt, 1.0, 0.0, Layout::Open).unwrap();
            assert_eq!(f, 1.0);
            // Every whole dash starts a gap (1) after a period (3) starts.
            let whole: Vec<f64> =
                ranges(len, &lt, 1.0, 0.0, Layout::Open).unwrap().into_iter().filter(|(a, b)| (b - a - 2.0).abs() < 1e-9).map(|(a, _)| a).collect();
            assert!(!whole.is_empty(), "{len}");
            for a in whole {
                let k = (a - 1.0 - start) / 3.0;
                assert!((k - k.round()).abs() < 1e-9, "{len}: a dash at {a}, periods from {start}");
            }
        }
    }

    /// Closed runs on the same plots: stretched to round(L / period) periods from their anchor.
    #[test]
    fn closed_runs_are_stretched_as_autocad_plots_them() {
        let dut = [2.0, -1.0];
        // A circle of radius 2 from angle 0: 0°–60°, 90°–150°, 180°–240°, 270°–330°.
        let r = 2.0;
        let got = drawn(std::f64::consts::TAU * r, &dut, Layout::Closed { anchor: 0.0 });
        let want: Vec<(f64, f64)> =
            [(0.0, 60.0), (90.0, 150.0), (180.0, 240.0), (270.0, 330.0)].map(|(a, b): (f64, f64)| (a.to_radians() * r, b.to_radians() * r)).into();
        assert!(near(&got, &want, 1e-9), "{got:?}");
        // Rectangles (PLINEGEN, closed), anchored at the second vertex: 4 × 4 has 5 periods of 3.2
        // from s = 0.8, 5 × 5 has 7 (round, not floor) from 2.143, 2 × 3 from 2.0, 3 × 3 from 0.
        for (w, h, periods, start) in [(4.0, 4.0, 5.0, 0.8), (5.0, 5.0, 7.0, 2.0 + 1.0 / 7.0), (2.0, 3.0, 3.0, 2.0), (3.0, 3.0, 4.0, 0.0)] {
            let len = 2.0 * (w + h);
            let got = drawn(len, &dut, Layout::Closed { anchor: w });
            let dash = 2.0 * len / (3.0 * periods);
            assert!(got.iter().any(|r| (r.0 - start).abs() < 1e-9 && (r.1 - r.0 - dash).abs() < 1e-9), "{w}×{h}: {got:?}");
        }
    }

    /// A pattern too long for the run draws it continuous.
    #[test]
    fn a_run_shorter_than_one_period_is_continuous() {
        let lt = Linetype::simple("DASHED", "", &[2.0, -1.0]);
        assert_eq!(apply(&[Vec2::ZERO, Vec2::new(2.99, 0.0)], &lt, 1.0, 0.0).len(), 1);
        assert_eq!(ranges(2.99, &lt, 1.0, 0.0, Layout::Open), None);
    }

    /// A long pattern repeated many times is drawn continuous rather than laid out element by
    /// element: 100 elements over 40,000 periods is past the cap, 400 periods is not.
    #[test]
    fn a_long_pattern_repeated_too_often_is_continuous() {
        let pattern: Vec<f64> = (0..100).map(|i| if i % 2 == 0 { 0.001 } else { -0.001 }).collect();
        let lt = Linetype::simple("MANY", "", &pattern);
        for how in [Layout::Open, Layout::Closed { anchor: 0.0 }] {
            assert_eq!(ranges(4000.0, &lt, 1.0, 0.0, how), None, "{how:?}");
            assert!(ranges(40.0, &lt, 1.0, 0.0, how).is_some_and(|r| r.len() > 10_000), "{how:?}");
        }
    }

    /// A plot or export keeps the dashes of a run more than 50,000 periods long; a display build,
    /// with a level of detail, draws it continuous (#1069).
    #[test]
    fn a_plot_keeps_the_dashes_of_a_run_of_many_periods() {
        let lt = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        let len = 50_001.0 * 0.75;
        // Centred, the first and last dashes are halves: one more dash than periods.
        for (how, dashes) in [(Layout::Open, 50_002), (Layout::Closed { anchor: 0.0 }, 50_001)] {
            assert_eq!(ranges(len, &lt, 1.0, 0.0, how).map(|r| r.len()), Some(dashes), "{how:?}");
            assert_eq!(ranges(len, &lt, 1.0, 1e-9, how), None, "{how:?}");
            // Where linetype texts are placed from.
            assert!(phase(len, &lt, 1.0, 0.0, how).is_some(), "{how:?}");
            assert_eq!(phase(len, &lt, 1.0, 1e-9, how), None, "{how:?}");
        }
        let pts = [Vec2::ZERO, Vec2::new(len, 0.0)];
        assert_eq!(apply(&pts, &lt, 1.0, 0.0).len(), 50_002);
        assert_eq!(apply(&pts, &lt, 1.0, 1e-9).len(), 1);
    }
}
