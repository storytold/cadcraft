//! Reading profiles (closed outlines) and paths from existing drawing objects, for EXTRUDE,
//! REVOLVE, SWEEP and LOFT. Everything here treats the drawing as hostile: sizes are capped
//! before anything is allocated and no index is trusted.

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{PolyVertex, Polyline, TAU, Vec2, Vec3};

use super::super::bad;
use crate::Result;

/// Most points one profile or path holds (the geometry crate refuses more than 2048 as well).
pub(super) const MAX_PROFILE: usize = 2048;

/// A closed outline in the XY plane at elevation `z`.
pub(super) struct Profile {
    pub pts: Vec<Vec2>,
    pub z: f64,
}

/// Map a drawn planar profile into radius/axial coordinates around an arbitrary 3D axis.
/// The outline must lie in a plane containing the axis, as required by a revolved profile.
pub(super) fn profile_about_axis(cmd: &str, pts: &[Vec2], z: f64, origin: Vec3, axis: Vec3) -> Result<(Vec<Vec2>, Vec3)> {
    if pts.len() < 3 || pts.len() > MAX_PROFILE || !z.is_finite() || !origin.is_finite() || !axis.is_finite() || axis.len() <= 1e-9 {
        return Err(bad(cmd, "invalid profile or axis"));
    }
    let axis = axis.normalized();
    let world: Vec<Vec3> = pts.iter().map(|p| Vec3::new(p.x, p.y, z)).collect();
    if world.iter().any(|p| !p.is_finite()) {
        return Err(bad(cmd, "profile coordinates are not finite"));
    }
    let radial_vectors: Vec<Vec3> = world
        .iter()
        .map(|p| {
            let d = *p - origin;
            d - axis * d.dot(axis)
        })
        .collect();
    let radial_hint =
        radial_vectors.iter().copied().find(|r| r.len() > 1e-9).ok_or_else(|| bad(cmd, "profile lies on the revolution axis"))?.normalized();
    let mut local = Vec::with_capacity(pts.len());
    for (p, radial) in world.iter().zip(radial_vectors.iter()) {
        let d = *p - origin;
        let radius = radial.dot(radial_hint);
        let off_plane = (*radial - radial_hint * radius).len();
        let tolerance = 1e-8 * (1.0 + d.len());
        if off_plane > tolerance {
            return Err(bad(cmd, "drawn profile must lie in a plane containing the revolution axis"));
        }
        if radius < -tolerance {
            return Err(bad(cmd, "drawn profile crosses the revolution axis"));
        }
        local.push(Vec2::new(if radius.abs() <= tolerance { 0.0 } else { radius }, d.dot(axis)));
    }
    Ok((local, radial_hint))
}

/// Counter-clockwise signed area (shoelace, relative to the first point).
pub(super) fn signed_area(p: &[Vec2]) -> f64 {
    let Some(o) = p.first().copied() else { return 0.0 };
    let mut a = 0.0;
    for w in p.windows(2) {
        if let (Some(u), Some(v)) = (w.first(), w.get(1)) {
            a += (*u - o).cross(*v - o);
        }
    }
    a / 2.0
}

/// `pts` without consecutive repeats and without a last point equal to the first.
fn tidy(pts: Vec<Vec2>) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        if !out.last().is_some_and(|l| l.near(p, 1e-9)) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.near(*b, 1e-9)) {
        out.pop();
    }
    out
}

fn tessellate_poly(cmd: &str, vs: &[PolyVertex], closed: bool) -> Result<Vec<Vec2>> {
    if vs.len() > MAX_PROFILE {
        return Err(bad(cmd, format!("a profile holds at most {MAX_PROFILE} vertices")));
    }
    if vs.iter().any(|v| !v.p.is_finite() || !v.bulge.is_finite()) {
        return Err(bad(cmd, "the polyline has non-finite coordinates"));
    }
    let poly = Polyline { vertices: vs.to_vec(), closed };
    let b = poly.bounds();
    let diag = (b.max - b.min).len();
    let tol = if diag.is_finite() && diag > 0.0 { (diag / 2000.0).max(1e-9) } else { 1e-3 };
    let pts = if vs.iter().all(|v| v.bulge == 0.0) { vs.iter().map(|v| v.p).collect() } else { poly.tessellate(tol) };
    if pts.len() > MAX_PROFILE * 4 {
        return Err(bad(cmd, "the polyline curves are too fine to profile"));
    }
    Ok(tidy(pts))
}

/// The closed outline of the object `h`: a closed (or end-to-start) polyline with bulges
/// flattened, a circle (`segments` corners) or a full ellipse.
pub(super) fn profile_of(cmd: &str, d: &Drawing, h: Handle, segments: usize) -> Result<Profile> {
    let e = d.entity(h).ok_or_else(|| bad(cmd, format!("no object with handle {}", h.hex())))?;
    let name = e.kind.type_name();
    let not_closed = || bad(cmd, format!("{} {} is not a closed profile", name, h.hex()));
    let (pts, z) = match &e.kind {
        EntityKind::LwPolyline(p) => {
            let mut closed = p.closed;
            if !closed {
                closed =
                    p.vertices.len() > 2 && p.vertices.first().zip(p.vertices.last()).is_some_and(|(a, b)| a.p.near(b.p, 1e-9) && b.bulge == 0.0);
            }
            if !closed {
                return Err(not_closed());
            }
            (tessellate_poly(cmd, &p.vertices, true)?, p.elevation)
        }
        EntityKind::Circle(c) => {
            if !c.center.is_finite() || !c.radius.is_finite() || c.radius <= 0.0 {
                return Err(bad(cmd, "the circle is degenerate"));
            }
            let pts = (0..segments).map(|i| {
                let a = TAU * i as f64 / segments as f64;
                Vec2::new(c.center.x + c.radius * a.cos(), c.center.y + c.radius * a.sin())
            });
            (pts.collect(), c.center.z)
        }
        EntityKind::Ellipse(el) => {
            if !el.center.is_finite() || !el.major.is_finite() || !el.ratio.is_finite() || el.ratio <= 0.0 {
                return Err(bad(cmd, "the ellipse is degenerate"));
            }
            let full = (el.end - el.start).abs() >= TAU - 1e-9 || (el.end - el.start).abs() < 1e-12;
            if !full {
                return Err(not_closed());
            }
            let major = el.major.xy();
            let minor = major.perp() * el.ratio;
            let c = el.center.xy();
            let pts = (0..segments).map(|i| {
                let a = TAU * i as f64 / segments as f64;
                c + major * a.cos() + minor * a.sin()
            });
            (pts.collect(), el.center.z)
        }
        _ => return Err(bad(cmd, format!("{} {} cannot be used as a profile (use a closed polyline, circle or ellipse)", name, h.hex()))),
    };
    if pts.len() < 3 {
        return Err(bad(cmd, format!("{} {} has too few distinct points for a profile", name, h.hex())));
    }
    Ok(Profile { pts, z })
}

/// The points of the object `h` as a path: a line, polyline (3D or lightweight, bulges
/// flattened) or arc. A closed polyline repeats its first point.
pub(super) fn path_of(cmd: &str, d: &Drawing, h: Handle) -> Result<Vec<Vec3>> {
    let e = d.entity(h).ok_or_else(|| bad(cmd, format!("no object with handle {}", h.hex())))?;
    let pts: Vec<Vec3> = match &e.kind {
        EntityKind::Line(l) => vec![l.a, l.b],
        EntityKind::LwPolyline(p) => {
            let mut pts = tessellate_poly(cmd, &p.vertices, p.closed)?;
            if p.closed
                && let Some(f) = pts.first().copied()
            {
                pts.push(f);
            }
            pts.into_iter().map(|q| q.to3(p.elevation)).collect()
        }
        EntityKind::Polyline3d(p) => {
            if p.points.len() > MAX_PROFILE {
                return Err(bad(cmd, format!("a path holds at most {MAX_PROFILE} points")));
            }
            let mut v = p.points.clone();
            if p.closed
                && let Some(f) = v.first().copied()
            {
                v.push(f);
            }
            v
        }
        EntityKind::Arc(a) => {
            if !a.center.is_finite() || !a.radius.is_finite() || a.radius <= 0.0 || !a.start.is_finite() || !a.end.is_finite() {
                return Err(bad(cmd, "the arc is degenerate"));
            }
            let sweep = match (a.end - a.start).rem_euclid(TAU) {
                s if s < 1e-12 => TAU,
                s => s,
            };
            let n = ((sweep / (TAU / 64.0)).ceil() as usize).clamp(2, 64);
            (0..=n)
                .map(|i| {
                    let t = a.start + sweep * i as f64 / n as f64;
                    Vec3::new(a.center.x + a.radius * t.cos(), a.center.y + a.radius * t.sin(), a.center.z)
                })
                .collect()
        }
        other => {
            return Err(bad(cmd, format!("{} {} cannot be used as a path (use a line, polyline or arc)", other.type_name(), h.hex())));
        }
    };
    let mut out: Vec<Vec3> = Vec::with_capacity(pts.len());
    for p in pts {
        if !p.is_finite() {
            return Err(bad(cmd, "the path has non-finite coordinates"));
        }
        if !out.last().is_some_and(|l| (*l - p).len() <= 1e-9) {
            out.push(p);
        }
    }
    if out.len() < 2 {
        return Err(bad(cmd, "the path needs at least two distinct points"));
    }
    Ok(out)
}
