//! Apply a dash pattern along a polyline.

use cadcraft_doc::Linetype;
use cadcraft_geom::Vec2;

/// Split `pts` into dashes. Dots come back as single-point runs. When the scaled pattern is
/// shorter than `min_pattern` (sub-pixel), the polyline is returned whole; a display build
/// (`min_pattern` above 0) also returns it whole when the pattern would repeat too often (see
/// [`ranges`]).
pub fn apply(pts: &[Vec2], lt: &Linetype, scale: f64, min_pattern: f64) -> Vec<Vec<Vec2>> {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let total = lt.pattern_length() * scale;
    if pts.len() < 2 || total <= 1e-12 || total < min_pattern {
        return vec![pts.to_vec()];
    }
    // A display build zoomed out a lot falls back to a continuous line rather than expanding a
    // huge dash count. A plot or export has no zoom — `min_pattern` is 0 — so it keeps the
    // pattern (#1069).
    let len: f64 = pts.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum();
    if min_pattern > 0.0 && len / total > 50_000.0 {
        return vec![pts.to_vec()];
    }
    let pattern: Vec<f64> = lt.pattern.iter().map(|d| d.length * scale).collect();
    // AutoCAD centres the pattern so lines start and end with a dash: start at half the first dash.
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    let mut idx = 0usize;
    let first = pattern.first().copied().unwrap_or(0.0);
    let mut remaining = if first > 0.0 { first } else { first.abs() };
    let mut on = first >= 0.0;
    if on && first == 0.0 {
        if let Some(p) = pts.first() {
            out.push(vec![*p]);
        }
        idx = 1 % pattern.len();
        let n = pattern.get(idx).copied().unwrap_or(0.0);
        remaining = n.abs();
        on = n > 0.0;
    }
    if on && let Some(p) = pts.first() {
        cur.push(*p);
    }
    let mut guard = 0usize;
    for w in pts.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        let seg = a.dist(b);
        if seg <= 1e-15 {
            continue;
        }
        let dir = (b - a) / seg;
        let mut pos = 0.0;
        while seg - pos > remaining {
            guard += 1;
            if guard > 2_000_000 {
                return vec![pts.to_vec()];
            }
            pos += remaining;
            let p = a + dir * pos;
            if on {
                cur.push(p);
                out.push(std::mem::take(&mut cur));
            }
            idx = (idx + 1) % pattern.len();
            let n = pattern.get(idx).copied().unwrap_or(0.0);
            if n == 0.0 {
                out.push(vec![p]);
                idx = (idx + 1) % pattern.len();
                let n2 = pattern.get(idx).copied().unwrap_or(0.0);
                remaining = n2.abs();
                on = n2 > 0.0;
            } else {
                remaining = n.abs();
                on = n > 0.0;
            }
            if on {
                cur.push(p);
            }
        }
        remaining -= seg - pos;
        if on {
            cur.push(b);
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// The dashes of the pattern along a run of length `len`, as (start, end) distances; a dot has
/// start == end. Laid out like [`apply`]. `None` when the run is drawn continuous: the scaled
/// pattern is shorter than `min_pattern`, or, in a display build (`min_pattern` above 0), it
/// would repeat too often (see [`apply`]).
pub fn ranges(len: f64, lt: &Linetype, scale: f64, min_pattern: f64) -> Option<Vec<(f64, f64)>> {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let total = lt.pattern_length() * scale;
    if !(len.is_finite() && len > 0.0) || total <= 1e-12 || total < min_pattern || (min_pattern > 0.0 && len / total > 50_000.0) {
        return None;
    }
    let mut out = Vec::new();
    let mut pos = 0.0;
    // Gap-only patterns draw nothing, matching `apply`, without walking every repeat.
    if !lt.pattern.iter().any(|d| d.length >= 0.0) {
        return Some(Vec::new());
    }
    // Bound the pattern elements visited, independent of emitted ranges, so a pattern that emits
    // little still cannot cycle hundreds of millions of times.
    let mut visits: u64 = 0;
    for d in lt.pattern.iter().map(|d| d.length * scale).cycle() {
        visits += 1;
        if pos >= len || out.len() > 2_000_000 || visits > 4_000_000 {
            break;
        }
        if d >= 0.0 {
            out.push((pos, (pos + d).min(len)));
        }
        pos += d.abs();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This gap-only pattern draws nothing, and `ranges` returns before cycling through its repeats.
    #[test]
    fn an_all_gap_pattern_draws_nothing_without_spinning() {
        let lt = Linetype::simple("GAPS", "", &[-0.001, -0.002]);
        assert_eq!(ranges(1.0, &lt, 1.0, 0.0), Some(Vec::new()));
        assert!(apply(&[Vec2::ZERO, Vec2::new(1.0, 0.0)], &lt, 1.0, 0.0).is_empty());
    }

    #[test]
    fn dashes_split_line() {
        let lt = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        let d = apply(&[Vec2::ZERO, Vec2::new(3.0, 0.0)], &lt, 1.0, 0.0);
        // 3.0 / 0.75 = 4 patterns → 4 dashes.
        assert_eq!(d.len(), 4);
        assert!((d[0][1].x - 0.5).abs() < 1e-9);
        assert!((d[1][0].x - 0.75).abs() < 1e-9);
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

    #[test]
    fn a_plot_keeps_the_dashes_of_a_long_line() {
        // 50,001 pattern periods. A display build may fall back to a continuous line (zoomed out
        // a lot); a plot or export has no zoom, so it keeps the pattern (#1069).
        let lt = Linetype::simple("DASHED", "", &[0.5, -0.25]);
        let len = 50_001.0 * 0.75;
        let pts = [Vec2::ZERO, Vec2::new(len, 0.0)];
        assert_eq!(apply(&pts, &lt, 1.0, 0.0).len(), 50_001);
        assert_eq!(ranges(len, &lt, 1.0, 0.0).expect("the pattern applies").len(), 50_001);
        // With a level of detail (a display build) the fallback stays.
        assert_eq!(apply(&pts, &lt, 1.0, 1e-9).len(), 1);
        assert!(ranges(len, &lt, 1.0, 1e-9).is_none());
    }
}
