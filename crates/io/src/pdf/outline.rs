//! The outline of a filled primitive's triangles: a fill written as its boundary (and its holes)
//! instead of one subpath per triangle is several times smaller in the PDF. Only a clean
//! triangulation (all triangles turning the same way, no overlap, every boundary vertex on one
//! loop) is outlined; anything else keeps its triangles.

use std::collections::HashMap;

use cadcraft_geom::Vec2;

/// The most triangles outlined; a larger fill keeps its triangles (bounded work).
const MAX_TRIS: usize = 200_000;

/// A vertex as exact bits, so shared corners of neighbouring triangles match.
type Key = (u64, u64);

fn key(p: Vec2) -> Key {
    // +0.0 and -0.0 are the same corner.
    ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits())
}

fn cross(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// The loops bounding triangles `tris` (three points each), turning as the triangles do, so a
/// non-zero fill of the loops covers exactly the triangles; `None` when the triangles are not a
/// clean triangulation.
pub(super) fn loops(tris: &[Vec2]) -> Option<Vec<Vec<Vec2>>> {
    let (chunks, rest) = tris.as_chunks::<3>();
    if !rest.is_empty() || chunks.len() < 2 || chunks.len() > MAX_TRIS {
        return None;
    }
    let mut sign = 0.0f64;
    let mut area = 0.0;
    let mut edges: HashMap<(Key, Key), (Vec2, Vec2)> = HashMap::new();
    // The edges in the order the triangles give them: the loops start in that order, so the same
    // triangles always write the same PDF.
    let mut order: Vec<(Key, Key)> = Vec::new();
    for [a, b, c] in chunks {
        let w = cross(*a, *b, *c);
        if !w.is_finite() {
            return None;
        }
        if w == 0.0 {
            continue;
        }
        if sign == 0.0 {
            sign = w.signum();
        } else if w.signum() != sign {
            return None;
        }
        area += w;
        for (p, q) in [(*a, *b), (*b, *c), (*c, *a)] {
            let (kp, kq) = (key(p), key(q));
            // An inside edge is walked once each way by the triangles on either side.
            if edges.remove(&(kq, kp)).is_none() {
                if edges.insert((kp, kq), (p, q)).is_some() {
                    return None;
                }
                order.push((kp, kq));
            }
        }
    }
    if edges.is_empty() {
        return None;
    }
    // Each boundary vertex starts exactly one boundary edge.
    let mut next: HashMap<Key, (Key, Vec2)> = HashMap::with_capacity(edges.len());
    for ((kp, kq), (p, _)) in &edges {
        if next.insert(*kp, (*kq, *p)).is_some() {
            return None;
        }
    }
    let mut out = Vec::new();
    let mut loop_area = 0.0;
    for (start, _) in order {
        if !next.contains_key(&start) {
            continue;
        }
        let mut ring = Vec::new();
        let mut at = start;
        loop {
            let (to, p) = next.remove(&at)?;
            ring.push(p);
            at = to;
            if at == start {
                break;
            }
            if ring.len() > edges.len() {
                return None;
            }
        }
        if ring.len() < 3 {
            return None;
        }
        // Twice the loop's signed area (shoelace), to check against the triangles'.
        loop_area += ring.iter().zip(ring.iter().cycle().skip(1)).map(|(p, q)| p.x * q.y - q.x * p.y).sum::<f64>();
        out.push(ring);
    }
    if !next.is_empty() {
        return None;
    }
    // Overlapping triangles would leave loops that cover a different area.
    let tolerance = 1e-6 * area.abs().max(f64::MIN_POSITIVE);
    ((loop_area - area).abs() <= tolerance).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    /// A square of two triangles is one loop of its four corners.
    #[test]
    fn a_square_of_two_triangles_is_its_outline() {
        let tris = [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)];
        let l = loops(&tris).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].len(), 4);
    }

    /// A square with a square hole: the outer loop and the hole, turning the other way.
    #[test]
    fn a_hole_is_its_own_loop() {
        let (o, i) = ([v(0.0, 0.0), v(3.0, 0.0), v(3.0, 3.0), v(0.0, 3.0)], [v(1.0, 1.0), v(2.0, 1.0), v(2.0, 2.0), v(1.0, 2.0)]);
        let mut tris = Vec::new();
        for k in 0..4 {
            let (o0, o1, i0, i1) = (o[k], o[(k + 1) % 4], i[k], i[(k + 1) % 4]);
            tris.extend([o0, o1, i1, o0, i1, i0]);
        }
        let l = loops(&tris).unwrap();
        assert_eq!(l.len(), 2);
        let signed = |r: &Vec<Vec2>| r.iter().zip(r.iter().cycle().skip(1)).map(|(p, q)| p.x * q.y - q.x * p.y).sum::<f64>();
        assert!(signed(&l[0]) * signed(&l[1]) < 0.0, "the hole turns the other way");
    }

    /// Triangles turning both ways, or overlapping, keep their triangles.
    #[test]
    fn unclean_triangles_are_not_outlined() {
        let mixed = [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 0.0), v(0.0, 1.0), v(1.0, 1.0)];
        assert!(loops(&mixed).is_none());
        let overlapping = [v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 0.0), v(2.0, 0.0), v(1.0, 3.0)];
        assert!(loops(&overlapping).is_none());
        assert!(loops(&[v(0.0, 0.0), v(f64::NAN, 0.0), v(1.0, 1.0), v(0.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)]).is_none());
    }
}
