//! Shared geometry helpers for commands.

use cadcraft_doc::{EntityKind, Line};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

pub fn v3(p: Vec2) -> Vec3 {
    p.to3(0.0)
}

pub fn line(a: Vec2, b: Vec2) -> EntityKind {
    EntityKind::Line(Line { a: v3(a), b: v3(b) })
}

pub fn circle(c: Vec2, r: f64) -> EntityKind {
    EntityKind::Circle(cadcraft_doc::Circle { center: v3(c), radius: r.abs() })
}

pub fn arc(a: &cadcraft_geom::Arc) -> EntityKind {
    EntityKind::Arc(cadcraft_doc::Arc { center: v3(a.center), radius: a.radius, start: a.start, end: a.end })
}

pub fn lwpoly(vs: Vec<PolyVertex>, closed: bool) -> EntityKind {
    EntityKind::LwPolyline(cadcraft_doc::LwPolyline { vertices: vs, closed, const_width: 0.0, elevation: 0.0, plinegen: false })
}

pub fn rect_vertices(a: Vec2, b: Vec2) -> Vec<PolyVertex> {
    [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)].into_iter().map(PolyVertex::new).collect()
}

/// Regular polygon vertices: inscribed (vertex at `radius`) or circumscribed (edge midpoint at `radius`).
pub fn polygon_vertices(center: Vec2, sides: usize, radius_pt: Vec2, inscribed: bool) -> Vec<PolyVertex> {
    let n = sides.clamp(3, 1024);
    let d = radius_pt - center;
    let r = d.len();
    let a0 = d.angle();
    let step = cadcraft_geom::TAU / n as f64;
    let (r, a0) = if inscribed { (r, a0) } else { (r / (step / 2.0).cos(), a0 - step / 2.0) };
    (0..n).map(|i| PolyVertex::new(Vec2::polar(center, r, a0 + step * i as f64))).collect()
}

/// The radius point for a polygon whose radius is typed: the bottom edge is horizontal in both
/// modes (inscribed: a vertex half a side's angle past straight down; circumscribed: the bottom
/// edge's midpoint straight down).
pub fn polygon_typed_radius_point(center: Vec2, sides: usize, radius: f64, inscribed: bool) -> Vec2 {
    let down = -std::f64::consts::FRAC_PI_2;
    let a = if inscribed { down + std::f64::consts::PI / sides.clamp(3, 1024) as f64 } else { down };
    Vec2::polar(center, radius, a)
}

/// Polygon from an edge (first two vertices).
pub fn polygon_from_edge(p1: Vec2, p2: Vec2, sides: usize) -> Vec<PolyVertex> {
    let n = sides.clamp(3, 1024);
    let step = cadcraft_geom::TAU / n as f64;
    let mut pts = vec![p1, p2];
    let len = p1.dist(p2);
    let mut ang = p1.angle_to(p2);
    for _ in 2..n {
        ang += step;
        let last = pts.last().copied().unwrap_or(p2);
        pts.push(Vec2::polar(last, len, ang));
    }
    pts.into_iter().map(PolyVertex::new).collect()
}

/// Rectangle vertices with fillet radius or chamfer distances at every corner.
pub fn rect_with_corners(a: Vec2, b: Vec2, fillet: f64, chamfer: (f64, f64)) -> Vec<PolyVertex> {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    let w = x1 - x0;
    let h = y1 - y0;
    if fillet > 0.0 && fillet * 2.0 <= w.min(h) + 1e-12 {
        let r = fillet;
        let bul = cadcraft_geom::arc_to_bulge(std::f64::consts::FRAC_PI_2);
        return vec![
            PolyVertex::new(Vec2::new(x0 + r, y0)),
            PolyVertex::with_bulge(Vec2::new(x1 - r, y0), bul),
            PolyVertex::new(Vec2::new(x1, y0 + r)),
            PolyVertex::with_bulge(Vec2::new(x1, y1 - r), bul),
            PolyVertex::new(Vec2::new(x1 - r, y1)),
            PolyVertex::with_bulge(Vec2::new(x0 + r, y1), bul),
            PolyVertex::new(Vec2::new(x0, y1 - r)),
            PolyVertex::with_bulge(Vec2::new(x0, y0 + r), bul),
        ];
    }
    let (c1, c2) = chamfer;
    if c1 > 0.0 && c2 > 0.0 && c1 + c1 <= w + 1e-12 && c2 + c2 <= h + 1e-12 {
        return [
            Vec2::new(x0 + c1, y0),
            Vec2::new(x1 - c1, y0),
            Vec2::new(x1, y0 + c2),
            Vec2::new(x1, y1 - c2),
            Vec2::new(x1 - c1, y1),
            Vec2::new(x0 + c1, y1),
            Vec2::new(x0, y1 - c2),
            Vec2::new(x0, y0 + c2),
        ]
        .into_iter()
        .map(PolyVertex::new)
        .collect();
    }
    rect_vertices(Vec2::new(x0, y0), Vec2::new(x1, y1))
}
