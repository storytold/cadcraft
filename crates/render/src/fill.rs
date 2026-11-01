//! Polygon filling: even-odd regions to triangles (slab decomposition), wide polylines.

use cadcraft_doc::LwPolyline;
use cadcraft_geom::{Polyline, Vec2};

/// Triangulate an even-odd region bounded by closed loops (holes allowed) by cutting it into
/// horizontal slabs at every vertex y and emitting trapezoids. Two edges that cross inside a slab
/// (a bowtie, two loops crossing each other) swap order there, so such a slab is cut again at the
/// crossing and each part is paired on its own. Robust for any input; output size is
/// O(slabs × crossings), capped for hostile inputs.
pub fn triangulate_evenodd(loops: &[Vec<Vec2>]) -> Vec<Vec2> {
    let mut edges: Vec<(Vec2, Vec2)> = Vec::new();
    for l in loops {
        let n = l.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let (Some(&a), Some(&b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
            if (a.y - b.y).abs() > 1e-15 && a.is_finite() && b.is_finite() {
                edges.push(if a.y < b.y { (a, b) } else { (b, a) });
            }
        }
    }
    if edges.is_empty() || edges.len() > 200_000 {
        return Vec::new();
    }
    let mut ys: Vec<f64> = edges.iter().flat_map(|(a, b)| [a.y, b.y]).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    edges.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    let mut out = Vec::new();
    let mut active: Vec<(Vec2, Vec2)> = Vec::new();
    let mut next = 0usize;
    // Slabs created by cutting at crossings, over the whole region; past this the remaining
    // crossings are left unsplit rather than letting a hostile input run away.
    let mut cuts_left = MAX_CROSSING_CUTS;
    let mut pending: Vec<(f64, f64)> = Vec::new();
    for w in ys.windows(2) {
        let (Some(&y0), Some(&y1)) = (w.first(), w.get(1)) else { continue };
        let ym = (y0 + y1) / 2.0;
        while let Some(e) = edges.get(next) {
            if e.0.y <= ym {
                active.push(*e);
                next += 1;
            } else {
                break;
            }
        }
        active.retain(|e| e.1.y > ym);
        pending.clear();
        pending.push((y0, y1));
        while let Some((ya, yb)) = pending.pop() {
            let ymid = (ya + yb) / 2.0;
            let mut xs: Vec<(f64, f64, f64)> = active.iter().map(|e| (x_at(e, ymid), x_at(e, ya), x_at(e, yb))).collect();
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            if cuts_left > 0 {
                let mut cuts: Vec<f64> = Vec::new();
                for pair in xs.windows(2) {
                    let (Some(l), Some(r)) = (pair.first(), pair.get(1)) else { continue };
                    if let Some(y) = crossing_y(l, r, ya, yb) {
                        cuts.push(y);
                    }
                }
                if !cuts.is_empty() {
                    cuts.sort_by(f64::total_cmp);
                    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
                    cuts.truncate(cuts_left);
                    cuts_left -= cuts.len();
                    // Push the parts top-down so the lowest is processed first.
                    let mut top = yb;
                    for &c in cuts.iter().rev() {
                        pending.push((c, top));
                        top = c;
                    }
                    pending.push((ya, top));
                    continue;
                }
            }
            for pair in xs.chunks(2) {
                if let [l, r] = pair {
                    let (a, b, c, d) = (Vec2::new(l.1, ya), Vec2::new(r.1, ya), Vec2::new(r.2, yb), Vec2::new(l.2, yb));
                    out.extend_from_slice(&[a, b, c, a, c, d]);
                }
            }
            if out.len() > 6_000_000 {
                return out;
            }
        }
    }
    out
}

/// How many extra slab boundaries crossings may add over one region.
const MAX_CROSSING_CUTS: usize = 100_000;

fn x_at(e: &(Vec2, Vec2), y: f64) -> f64 {
    let t = (y - e.0.y) / (e.1.y - e.0.y);
    e.0.x + (e.1.x - e.0.x) * t
}

/// Where two edges ordered `l` before `r` at the slab's middle swap sides, if that happens
/// strictly inside the slab. Each entry is (x at the middle, x at `ya`, x at `yb`).
fn crossing_y(l: &(f64, f64, f64), r: &(f64, f64, f64), ya: f64, yb: f64) -> Option<f64> {
    let (da, db) = (l.1 - r.1, l.2 - r.2);
    // Ordered at the middle, so a swap shows as `l` right of `r` at one boundary.
    if da <= 0.0 && db <= 0.0 {
        return None;
    }
    // The gap is linear in y: it is zero at this fraction of the way from ya to yb.
    let t = da / (da - db);
    if !t.is_finite() {
        return None;
    }
    let y = ya + (yb - ya) * t;
    (y > ya && y < yb && y - ya > 1e-12 && yb - y > 1e-12).then_some(y)
}

/// Triangles for a polyline with widths (constant or tapered per segment).
pub fn wide_polyline(p: &LwPolyline, tol: f64) -> Vec<Vec2> {
    let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
    let mut out = Vec::new();
    let n = p.vertices.len();
    for (i, seg) in pl.segments().iter().enumerate() {
        let v = p.vertices.get(i % n.max(1)).copied().unwrap_or_default();
        let (w0, w1) = if p.const_width > 0.0 { (p.const_width, p.const_width) } else { (v.start_width, v.end_width) };
        let mut pts = Vec::new();
        seg.tessellate(tol, &mut pts);
        let m = pts.len();
        if m < 2 {
            continue;
        }
        let mut prev: Option<(Vec2, Vec2)> = None;
        for (k, q) in pts.iter().enumerate() {
            let t = k as f64 / (m - 1) as f64;
            let w = (w0 + (w1 - w0) * t) / 2.0;
            let dir = if k + 1 < m { pts.get(k + 1).map(|n| *n - *q) } else { pts.get(k.wrapping_sub(1)).map(|pv| *q - *pv) }
                .unwrap_or(Vec2::X)
                .normalized();
            let nrm = dir.perp() * w;
            let cur = (*q + nrm, *q - nrm);
            if let Some((a, b)) = prev {
                out.extend_from_slice(&[a, b, cur.1, a, cur.1, cur.0]);
            }
            prev = Some(cur);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(tris: &[Vec2]) -> f64 {
        tris.chunks(3).map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs()).sum()
    }

    #[test]
    fn square_with_hole() {
        let outer = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let hole = vec![Vec2::new(4.0, 4.0), Vec2::new(6.0, 4.0), Vec2::new(6.0, 6.0), Vec2::new(4.0, 6.0)];
        let t = triangulate_evenodd(&[outer, hole]);
        assert!((area(&t) - 96.0).abs() < 1e-9);
    }

    #[test]
    fn triangle_area() {
        let t = triangulate_evenodd(&[vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 3.0)]]);
        assert!((area(&t) - 6.0).abs() < 1e-9);
    }

    /// How many of the triangles contain `p` (points on an edge are avoided by the callers).
    fn cover(tris: &[Vec2], p: Vec2) -> usize {
        tris.chunks(3)
            .filter(|t| {
                let s = |a: Vec2, b: Vec2| (b - a).cross(p - a);
                let (u, v, w) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
                (u >= 0.0 && v >= 0.0 && w >= 0.0) || (u <= 0.0 && v <= 0.0 && w <= 0.0)
            })
            .count()
    }

    /// Even-odd parity of `p` against the loops, as an oracle independent of the triangulation.
    fn inside(loops: &[Vec<Vec2>], p: Vec2) -> bool {
        let mut crossings = 0;
        for l in loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if x > p.x {
                        crossings += 1;
                    }
                }
            }
        }
        crossings % 2 == 1
    }

    /// Every sample point off the edges is covered exactly once when the oracle says inside and
    /// never when it says outside.
    fn assert_matches_oracle(loops: &[Vec<Vec2>], lo: f64, hi: f64) {
        let t = triangulate_evenodd(loops);
        let n = 23;
        for i in 0..n {
            for j in 0..n {
                // Irrational-ish offsets keep the samples off every vertex, edge and crossing.
                let p = Vec2::new(lo + (hi - lo) * (i as f64 + 0.3137) / n as f64, lo + (hi - lo) * (j as f64 + 0.6180) / n as f64);
                let want = usize::from(inside(loops, p));
                assert_eq!(cover(&t, p), want, "coverage at {p:?}");
            }
        }
    }

    #[test]
    fn bowtie_crossing_inside_a_slab() {
        // The crossing at (1, 1) lies strictly inside the only slab, y in [0, 2].
        let bowtie = vec![Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0)];
        let t = triangulate_evenodd(std::slice::from_ref(&bowtie));
        assert!((area(&t) - 2.0).abs() < 1e-9, "two lobes of area 1, got {}", area(&t));
        assert_matches_oracle(&[bowtie], -0.5, 2.5);
    }

    #[test]
    fn bowtie_off_centre_crossing() {
        // Crossing at y = 1.2, not at the slab's middle, so the middle ordering alone is wrong too.
        let bowtie = vec![Vec2::new(0.0, 0.0), Vec2::new(3.0, 3.0), Vec2::new(3.0, 0.0), Vec2::new(0.0, 2.0)];
        assert_matches_oracle(&[bowtie], -0.5, 3.5);
    }

    #[test]
    fn two_loops_crossing_each_other() {
        // Two squares overlapping: even-odd leaves the overlap empty; their crossings sit inside
        // the slabs between the vertex rows.
        let a = vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 1.0), Vec2::new(3.0, 5.0), Vec2::new(-1.0, 4.0)];
        let b = vec![Vec2::new(2.0, 2.0), Vec2::new(6.0, 3.0), Vec2::new(5.0, 7.0), Vec2::new(1.0, 6.0)];
        let t = triangulate_evenodd(&[a.clone(), b.clone()]);
        // Each parallelogram has area 17; even-odd drops the overlap twice. The overlap is
        // sampled on a grid, so compare loosely; the oracle below is the exact check.
        assert!((area(&t) - (2.0 * 17.0 - 2.0 * overlap_area(&a, &b))).abs() < 0.3, "area {}", area(&t));
        assert_matches_oracle(&[a, b], -1.5, 7.5);
    }

    /// Area where both loops are inside, sampled on a fine grid; good to a few hundredths.
    fn overlap_area(a: &[Vec2], b: &[Vec2]) -> f64 {
        let (la, lb) = (vec![a.to_vec()], vec![b.to_vec()]);
        let n = 400;
        let (lo, hi) = (-2.0, 8.0);
        let cell = (hi - lo) / n as f64;
        let mut both = 0usize;
        for i in 0..n {
            for j in 0..n {
                let p = Vec2::new(lo + (i as f64 + 0.5) * cell, lo + (j as f64 + 0.5) * cell);
                if inside(&la, p) && inside(&lb, p) {
                    both += 1;
                }
            }
        }
        both as f64 * cell * cell
    }

    #[test]
    fn simple_loops_still_match_the_oracle() {
        let outer = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let hole = vec![Vec2::new(4.0, 4.0), Vec2::new(6.0, 4.0), Vec2::new(6.0, 6.0), Vec2::new(4.0, 6.0)];
        assert_matches_oracle(&[outer.clone(), hole.clone()], -1.0, 11.0);
        let reversed: Vec<Vec2> = outer.iter().rev().copied().collect();
        assert_matches_oracle(&[reversed, hole], -1.0, 11.0);
        let tri = vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 3.0)];
        assert_matches_oracle(&[tri], -1.0, 5.0);
    }

    #[test]
    fn degenerate_input() {
        assert!(triangulate_evenodd(&[vec![Vec2::ZERO, Vec2::X]]).is_empty());
        // NaN input must not panic; the result itself is unspecified.
        let _ = triangulate_evenodd(&[vec![Vec2::new(f64::NAN, 0.0), Vec2::X, Vec2::Y]]);
    }
}
