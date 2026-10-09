//! Polygon filling: even-odd regions to triangles (slab decomposition), wide polylines.

use cadcraft_doc::LwPolyline;
use cadcraft_geom::{Polyline, Vec2};

/// Triangulate an even-odd region bounded by closed loops (holes allowed) by cutting it into
/// horizontal slabs at every vertex y and emitting trapezoids. Robust for any input; output
/// size is O(slabs × crossings), capped for hostile inputs.
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
        let xat = |e: &(Vec2, Vec2), y: f64| {
            let t = (y - e.0.y) / (e.1.y - e.0.y);
            e.0.x + (e.1.x - e.0.x) * t
        };
        let mut xs: Vec<(f64, f64, f64)> = active.iter().map(|e| (xat(e, ym), xat(e, y0), xat(e, y1))).collect();
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in xs.chunks(2) {
            if let [l, r] = pair {
                let (a, b, c, d) = (Vec2::new(l.1, y0), Vec2::new(r.1, y0), Vec2::new(r.2, y1), Vec2::new(l.2, y1));
                out.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
        if out.len() > 6_000_000 {
            break;
        }
    }
    out
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

    #[test]
    fn degenerate_input() {
        assert!(triangulate_evenodd(&[vec![Vec2::ZERO, Vec2::X]]).is_empty());
        // NaN input must not panic; the result itself is unspecified.
        let _ = triangulate_evenodd(&[vec![Vec2::new(f64::NAN, 0.0), Vec2::X, Vec2::Y]]);
    }
}
