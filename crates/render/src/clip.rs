//! Clipping against axis-aligned rectangles (viewport contents, plot sheets) and against
//! arbitrary polygons (clipped viewports, [`PolyClip`]).

use cadcraft_doc::EntityKind;
use cadcraft_geom::{Bounds2, Polyline, Vec2};

/// Clip the segment `a`→`b` to `r` (Liang–Barsky). Returns the parameter range kept.
fn clip_params(a: Vec2, d: Vec2, r: &Bounds2, mut t0: f64, mut t1: f64) -> Option<(f64, f64)> {
    for (p, q) in [(-d.x, a.x - r.min.x), (d.x, r.max.x - a.x), (-d.y, a.y - r.min.y), (d.y, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                if t > t0 {
                    t0 = t;
                }
            } else {
                if t < t0 {
                    return None;
                }
                if t < t1 {
                    t1 = t;
                }
            }
        }
    }
    (t0 <= t1 && t0.is_finite() && t1.is_finite()).then_some((t0, t1))
}

/// Clip a segment to `r`.
pub fn clip_segment(a: Vec2, b: Vec2, r: &Bounds2) -> Option<(Vec2, Vec2)> {
    if r.is_empty() || !a.is_finite() || !b.is_finite() {
        return None;
    }
    let d = b - a;
    let (t0, t1) = clip_params(a, d, r, 0.0, 1.0)?;
    Some((a + d * t0, a + d * t1))
}

/// Clip an infinite line (or a ray when `ray`) through `base` along `dir` to `r`.
pub fn clip_infinite(base: Vec2, dir: Vec2, ray: bool, r: &Bounds2) -> Option<(Vec2, Vec2)> {
    if r.is_empty() || !base.is_finite() || !dir.is_finite() || dir == Vec2::ZERO {
        return None;
    }
    let far = (r.width() + r.height() + (base - r.center()).len()) * 2.0 / dir.len().max(1e-300);
    let (t0, t1) = clip_params(base, dir, r, if ray { 0.0 } else { -far }, far)?;
    Some((base + dir * t0, base + dir * t1))
}

/// Clip a polyline to `r`; returns the visible pieces (each with at least two points).
pub fn clip_polyline(pts: &[Vec2], r: &Bounds2) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    if r.is_empty() {
        return out;
    }
    // Fast path: everything inside.
    if pts.iter().all(|p| r.contains(*p)) {
        if pts.len() >= 2 {
            out.push(pts.to_vec());
        }
        return out;
    }
    let mut cur: Vec<Vec2> = Vec::new();
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        match clip_segment(*a, *b, r) {
            Some((p, q)) => {
                let joined = cur.last().is_some_and(|l| l.near(p, 1e-12));
                if !joined {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(p);
                }
                cur.push(q);
                // Leaving the rectangle ends the piece.
                if !q.near(*b, 1e-12) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
            None => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Clip a convex polygon to `r` (Sutherland–Hodgman).
pub fn clip_polygon(poly: &[Vec2], r: &Bounds2) -> Vec<Vec2> {
    if r.is_empty() {
        return Vec::new();
    }
    let mut cur: Vec<Vec2> = poly.to_vec();
    // Each edge: inside test and intersection with the boundary line.
    let edges: [(u8, f64); 4] = [(0, r.min.x), (1, r.max.x), (2, r.min.y), (3, r.max.y)];
    for (side, v) in edges {
        let inside = |p: &Vec2| match side {
            0 => p.x >= v,
            1 => p.x <= v,
            2 => p.y >= v,
            _ => p.y <= v,
        };
        let cross = |a: Vec2, b: Vec2| -> Vec2 {
            let t = match side {
                0 | 1 => (v - a.x) / (b.x - a.x),
                _ => (v - a.y) / (b.y - a.y),
            };
            let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
            a + (b - a) * t
        };
        let mut next = Vec::with_capacity(cur.len() + 2);
        let n = cur.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (cur.get(i).copied(), cur.get((i + 1) % n).copied()) else { continue };
            match (inside(&a), inside(&b)) {
                (true, true) => next.push(b),
                (true, false) => next.push(cross(a, b)),
                (false, true) => {
                    next.push(cross(a, b));
                    next.push(b);
                }
                (false, false) => {}
            }
        }
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    cur
}

/// Clip a triangle list (three vertices per triangle) to `r`, re-triangulating as fans.
pub fn clip_triangles(tris: &[Vec2], r: &Bounds2) -> Vec<Vec2> {
    let mut out = Vec::new();
    for t in tris.as_chunks::<3>().0 {
        if t.iter().all(|p| r.contains(*p)) {
            out.extend_from_slice(t);
            continue;
        }
        let poly = clip_polygon(t, r);
        if let Some(first) = poly.first().copied() {
            for w in poly.get(1..).unwrap_or(&[]).windows(2) {
                if let (Some(b), Some(c)) = (w.first(), w.get(1)) {
                    out.extend_from_slice(&[first, *b, *c]);
                }
            }
        }
    }
    out
}

/// Most vertices a clip boundary keeps (denser outlines are thinned; hostile files).
pub const MAX_CLIP_POINTS: usize = 4096;
/// Horizontal bands the clip polygon's edges are bucketed into.
const BANDS: usize = 64;

/// The outline of a closed paper-space entity as a polygon, for a viewport clip boundary:
/// a closed polyline (arcs tessellated), circle, full ellipse or closed spline. `None` for
/// anything else, an open shape or one without area. At most [`MAX_CLIP_POINTS`] vertices.
pub fn boundary_polygon(k: &EntityKind) -> Option<Vec<Vec2>> {
    let closed_ends = |a: Option<&Vec2>, b: Option<&Vec2>| a.zip(b).is_some_and(|(a, b)| a.near(*b, 1e-9));
    let mut pts: Vec<Vec2> = Vec::new();
    // Chords within about a thousandth of the shape's size.
    let tol_for = |b: Bounds2| (b.width().max(b.height()) * 1e-3).max(1e-9);
    match k {
        EntityKind::LwPolyline(p) => {
            if p.vertices.len() > 1_000_000 {
                return None;
            }
            let pl = Polyline { vertices: p.vertices.clone(), closed: true };
            let ends = closed_ends(p.vertices.first().map(|v| &v.p), p.vertices.last().map(|v| &v.p));
            if !p.closed && !ends {
                return None;
            }
            pts = pl.tessellate(tol_for(pl.bounds()));
        }
        EntityKind::Polyline3d(p) => {
            let ends = closed_ends(p.points.first().map(|v| v.xy()).as_ref(), p.points.last().map(|v| v.xy()).as_ref());
            if !p.closed && !ends {
                return None;
            }
            pts.extend(p.points.iter().take(1_000_000).map(|v| v.xy()));
        }
        EntityKind::Circle(c) => {
            let g = cadcraft_geom::Circle::new(c.center.xy(), c.radius);
            if !(c.radius.is_finite() && c.radius > 0.0) {
                return None;
            }
            g.tessellate(c.radius * 2e-3, &mut pts);
        }
        EntityKind::Ellipse(e) => {
            let g = cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end };
            let r = g.major.len();
            if !g.is_full() || !(r.is_finite() && r > 0.0 && e.ratio.is_finite() && e.ratio > 0.0) {
                return None;
            }
            g.tessellate(r * e.ratio.min(1.0) * 2e-3, &mut pts);
        }
        EntityKind::Spline(s) => {
            if s.control.len() > 100_000 || s.knots.len() > 100_000 || s.degree > cadcraft_geom::MAX_SPLINE_DEGREE {
                return None;
            }
            pts = s.tessellate(tol_for(Bounds2::from_points(s.control.iter().copied())));
            if !s.closed && !closed_ends(pts.first(), pts.last()) {
                return None;
            }
        }
        _ => return None,
    }
    clean_polygon(pts)
}

/// Finite points without repeats or the closing duplicate, thinned to [`MAX_CLIP_POINTS`];
/// `None` without at least three points and some area.
fn clean_polygon(pts: Vec<Vec2>) -> Option<Vec<Vec2>> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len().min(MAX_CLIP_POINTS));
    let stride = pts.len().div_ceil(MAX_CLIP_POINTS).max(1);
    for p in pts.into_iter().step_by(stride) {
        if p.is_finite() && !out.last().is_some_and(|l| l.near(p, 1e-12)) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
        out.pop();
    }
    let b = Bounds2::from_points(out.iter().copied());
    (out.len() >= 3 && b.width() > 1e-12 && b.height() > 1e-12 && cadcraft_geom::shoelace(&out).abs() > 1e-18).then_some(out)
}

/// A clip region bounded by a polygon (even-odd, so self-touching outlines work), with its
/// edges bucketed into horizontal bands to keep tests fast on large display lists.
#[derive(Clone, Debug)]
pub struct PolyClip {
    poly: Vec<Vec2>,
    bounds: Bounds2,
    /// Edge indices whose y range meets each band.
    bands: Vec<Vec<u32>>,
    band_h: f64,
    /// The region as triangles (for clipping fills), with each triangle's bounds.
    tris: Vec<([Vec2; 3], Bounds2)>,
}

impl PolyClip {
    /// A clip region for `poly` (see [`clean_polygon`]); `None` for a degenerate outline.
    pub fn new(poly: &[Vec2]) -> Option<PolyClip> {
        let poly = clean_polygon(poly.to_vec())?;
        let bounds = Bounds2::from_points(poly.iter().copied());
        let band_h = bounds.height() / BANDS as f64;
        let mut bands: Vec<Vec<u32>> = vec![Vec::new(); BANDS];
        let n = poly.len();
        let mut clip = PolyClip { poly: Vec::new(), bounds, bands: Vec::new(), band_h, tris: Vec::new() };
        for i in 0..n {
            let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            let (lo, hi) = (clip.band(a.y.min(b.y)), clip.band(a.y.max(b.y)));
            for band in bands.iter_mut().take(hi + 1).skip(lo) {
                band.push(i as u32);
            }
        }
        let tris = crate::fill::triangulate_evenodd(std::slice::from_ref(&poly));
        clip.tris = tris
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| (*t, Bounds2::from_points(t.iter().copied())))
            .filter(|(_, b)| b.width() > 0.0 && b.height() > 0.0)
            .collect();
        clip.poly = poly;
        clip.bands = bands;
        Some(clip)
    }
    /// The outline's vertices.
    pub fn polygon(&self) -> &[Vec2] {
        &self.poly
    }
    pub fn bounds(&self) -> Bounds2 {
        self.bounds
    }
    fn band(&self, y: f64) -> usize {
        let k = (y - self.bounds.min.y) / self.band_h;
        if k.is_finite() && k > 0.0 { (k as usize).min(BANDS - 1) } else { 0 }
    }
    fn edge(&self, i: u32) -> Option<(Vec2, Vec2)> {
        let i = i as usize;
        Some((*self.poly.get(i)?, *self.poly.get((i + 1) % self.poly.len().max(1))?))
    }
    /// Even-odd point-in-polygon test.
    pub fn contains(&self, p: Vec2) -> bool {
        if !p.is_finite() || !self.bounds.contains(p) {
            return false;
        }
        let mut inside = false;
        for &i in self.bands.get(self.band(p.y)).map(Vec::as_slice).unwrap_or(&[]) {
            let Some((a, b)) = self.edge(i) else { continue };
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside
    }
    /// Parameters along `a`→`b` (0..1) where the segment crosses the outline, sorted.
    fn crossings(&self, a: Vec2, b: Vec2) -> Vec<f64> {
        let d = b - a;
        let (lo, hi) = (self.band(a.y.min(b.y)), self.band(a.y.max(b.y)));
        let mut ts = Vec::new();
        for band in self.bands.iter().take(hi + 1).skip(lo) {
            for &i in band {
                let Some((p, q)) = self.edge(i) else { continue };
                let e = q - p;
                let den = d.cross(e);
                if den.abs() < 1e-300 {
                    continue;
                }
                let t = (p - a).cross(e) / den;
                let u = (p - a).cross(d) / den;
                if t > 0.0 && t < 1.0 && (-1e-12..=1.0 + 1e-12).contains(&u) {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        ts
    }
    /// Clip a segment: the pieces inside the region.
    fn clip_segment(&self, a: Vec2, b: Vec2, out: &mut Vec<(Vec2, Vec2)>) {
        if !a.is_finite() || !b.is_finite() || clip_segment(a, b, &self.bounds).is_none() {
            return;
        }
        let d = b - a;
        let mut prev = 0.0;
        for t in self.crossings(a, b).into_iter().chain(std::iter::once(1.0)) {
            if t - prev > 1e-12 && self.contains(a + d * ((prev + t) / 2.0)) {
                out.push((a + d * prev, a + d * t));
            }
            prev = t;
        }
    }
    /// Clip a polyline; returns the visible pieces (each with at least two points).
    pub fn clip_polyline(&self, pts: &[Vec2]) -> Vec<Vec<Vec2>> {
        let mut out: Vec<Vec<Vec2>> = Vec::new();
        let mut cur: Vec<Vec2> = Vec::new();
        let mut pieces = Vec::new();
        for w in pts.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            pieces.clear();
            self.clip_segment(*a, *b, &mut pieces);
            if pieces.is_empty() {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
            for (p, q) in pieces.drain(..) {
                if !cur.last().is_some_and(|l| l.near(p, 1e-12)) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(p);
                }
                cur.push(q);
            }
        }
        if cur.len() >= 2 {
            out.push(cur);
        }
        out
    }
    /// Clip an infinite line (or a ray when `ray`) to the region.
    pub fn clip_infinite(&self, base: Vec2, dir: Vec2, ray: bool) -> Vec<Vec<Vec2>> {
        match clip_infinite(base, dir, ray, &self.bounds) {
            Some((a, b)) => self.clip_polyline(&[a, b]),
            None => Vec::new(),
        }
    }
    /// Clip a triangle list (three vertices per triangle) to the region, re-triangulating as fans.
    pub fn clip_triangles(&self, tris: &[Vec2]) -> Vec<Vec2> {
        let mut out = Vec::new();
        for t in tris.as_chunks::<3>().0 {
            let tb = Bounds2::from_points(t.iter().copied());
            if !tb.intersects(&self.bounds) {
                continue;
            }
            // Wholly inside: no outline edge crosses it.
            if t.iter().all(|p| self.contains(*p)) && (0..3).all(|i| self.crossings(t[i], t[(i + 1) % 3]).is_empty()) {
                out.extend_from_slice(t);
                continue;
            }
            for (c, cb) in &self.tris {
                if !cb.intersects(&tb) {
                    continue;
                }
                let poly = clip_convex(t, c);
                if let Some(first) = poly.first().copied() {
                    for w in poly.get(1..).unwrap_or(&[]).windows(2) {
                        if let (Some(b), Some(c)) = (w.first(), w.get(1)) {
                            out.extend_from_slice(&[first, *b, *c]);
                        }
                    }
                }
                if out.len() > 6_000_000 {
                    return out;
                }
            }
        }
        out
    }
}

/// Clip a polygon to a triangle (Sutherland–Hodgman; the triangle is convex).
fn clip_convex(poly: &[Vec2], tri: &[Vec2; 3]) -> Vec<Vec2> {
    let [a, b, c] = *tri;
    let ccw = (b - a).cross(c - a) >= 0.0;
    let mut cur: Vec<Vec2> = poly.to_vec();
    for (p, q) in [(a, b), (b, c), (c, a)] {
        let (p, q) = if ccw { (p, q) } else { (q, p) };
        let e = q - p;
        let side = |x: Vec2| e.cross(x - p);
        let mut next = Vec::with_capacity(cur.len() + 2);
        let n = cur.len();
        for i in 0..n {
            let (Some(s), Some(t)) = (cur.get(i).copied(), cur.get((i + 1) % n).copied()) else { continue };
            let (ds, dt) = (side(s), side(t));
            if ds >= 0.0 {
                next.push(s);
            }
            if (ds >= 0.0) != (dt >= 0.0) {
                let k = ds / (ds - dt);
                if k.is_finite() {
                    next.push(s + (t - s) * k);
                }
            }
        }
        cur = next;
        if cur.len() < 3 {
            return Vec::new();
        }
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Bounds2 {
        Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0))
    }

    #[test]
    fn segments_and_polylines() {
        let (a, b) = clip_segment(Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0), &r()).unwrap();
        assert!(a.near(Vec2::new(0.0, 5.0), 1e-9) && b.near(Vec2::new(10.0, 5.0), 1e-9));
        assert!(clip_segment(Vec2::new(-5.0, -5.0), Vec2::new(-1.0, 20.0), &r()).is_none());
        // In, out, back in: two pieces.
        let pieces = clip_polyline(&[Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0), Vec2::new(15.0, 8.0), Vec2::new(5.0, 8.0)], &r());
        assert_eq!(pieces.len(), 2);
        let (a, b) = clip_infinite(Vec2::new(5.0, 5.0), Vec2::new(1.0, 0.0), false, &r()).unwrap();
        assert!(a.near(Vec2::new(0.0, 5.0), 1e-9) && b.near(Vec2::new(10.0, 5.0), 1e-9));
        let (a, _) = clip_infinite(Vec2::new(5.0, 5.0), Vec2::new(1.0, 0.0), true, &r()).unwrap();
        assert!(a.near(Vec2::new(5.0, 5.0), 1e-9));
        assert!(clip_segment(Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0), &r()).is_none());
    }

    fn notched() -> PolyClip {
        // A concave "L": the square 0..10 without its top-right quarter.
        PolyClip::new(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 5.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(5.0, 10.0),
            Vec2::new(0.0, 10.0),
        ])
        .unwrap()
    }

    #[test]
    fn polygon_clip_lines_points_and_fills() {
        let c = notched();
        assert!(c.contains(Vec2::new(2.0, 8.0)) && !c.contains(Vec2::new(8.0, 8.0)));
        // A horizontal line through the notch keeps only the part left of x = 5.
        let pieces = c.clip_polyline(&[Vec2::new(-5.0, 8.0), Vec2::new(15.0, 8.0)]);
        assert_eq!(pieces.len(), 1);
        let p = &pieces[0];
        assert!(p[0].near(Vec2::new(0.0, 8.0), 1e-9) && p[1].near(Vec2::new(5.0, 8.0), 1e-9), "{p:?}");
        // Through the lower part: the whole width.
        let pieces = c.clip_polyline(&[Vec2::new(-5.0, 2.0), Vec2::new(15.0, 2.0)]);
        assert!(pieces[0][1].near(Vec2::new(10.0, 2.0), 1e-9));
        // An infinite line along the diagonal crosses the notch corner.
        assert!(!c.clip_infinite(Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), false).is_empty());
        // A fill over the whole square loses the notch: three quarters of the area.
        let fill =
            [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let t = c.clip_triangles(&fill);
        let area: f64 = t.as_chunks::<3>().0.iter().map(|t| cadcraft_geom::shoelace(t).abs()).sum();
        assert!((area - 75.0).abs() < 1e-6, "{area}");
        assert!(t.iter().all(|p| !(p.x > 5.0 + 1e-9 && p.y > 5.0 + 1e-9)));
    }

    #[test]
    fn degenerate_and_hostile_outlines() {
        assert!(PolyClip::new(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]).is_none());
        assert!(PolyClip::new(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)]).is_none());
        assert!(PolyClip::new(&[Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, f64::INFINITY), Vec2::new(2.0, 2.0)]).is_none());
        // Very dense outlines are thinned.
        let many: Vec<Vec2> = (0..100_000).map(|i| Vec2::polar(Vec2::ZERO, 5.0, i as f64 * std::f64::consts::TAU / 100_000.0)).collect();
        let c = PolyClip::new(&many).unwrap();
        assert!(c.polygon().len() <= MAX_CLIP_POINTS);
        assert!(c.contains(Vec2::ZERO));
        assert!(c.clip_polyline(&[Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0)]).is_empty());
    }

    #[test]
    fn triangles() {
        let t = clip_triangles(&[Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), Vec2::new(5.0, 10.0)], &r());
        assert!(!t.is_empty() && t.len().is_multiple_of(3));
        assert!(t.iter().all(|p| p.x >= -1e-9));
    }
}
