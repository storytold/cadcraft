//! Bounded, triangulated 3D solids. Profiles are simple polygons without a repeated endpoint.
use crate::{EPS, TAU, Vec2, Vec3};
use std::fmt;

const LIMIT: usize = 100_000;
const COORD: f64 = 1e12;

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh3 {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[usize; 3]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshError(pub &'static str);
impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for MeshError {}

fn valid(p: Vec3) -> bool {
    p.is_finite() && p.x.abs() <= COORD && p.y.abs() <= COORD && p.z.abs() <= COORD
}
fn count(a: usize, b: usize) -> Result<(), MeshError> {
    if a.checked_mul(b).is_none_or(|v| v > LIMIT) { Err(MeshError("mesh exceeds size limit")) } else { Ok(()) }
}
fn cross(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn polygon(p: &[Vec2]) -> Result<Vec<[usize; 3]>, MeshError> {
    if p.len() < 3 || p.len() > 2048 || p.iter().any(|v| !v.is_finite() || v.x.abs() > COORD || v.y.abs() > COORD) {
        return Err(MeshError("invalid profile"));
    }
    let n = p.len();
    let mut area = 0.0;
    for i in 0..n {
        let a = p[i];
        let b = p[(i + 1) % n];
        if (a - b).len() <= EPS {
            return Err(MeshError("duplicate profile points"));
        }
        let a = a - p[0];
        let b = b - p[0];
        area += a.x * b.y - b.x * a.y;
    }
    if !area.is_finite() || area.abs() <= EPS {
        return Err(MeshError("degenerate profile"));
    }
    // Reject intersections, including collinear contacts between nonadjacent edges.
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b, c, d) = (p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]);
            let (x, y, z, w) = (cross(a, b, c), cross(a, b, d), cross(c, d, a), cross(c, d, b));
            if x * y <= 0.0
                && z * w <= 0.0
                && a.x.min(b.x) <= c.x.max(d.x)
                && c.x.min(d.x) <= a.x.max(b.x)
                && a.y.min(b.y) <= c.y.max(d.y)
                && c.y.min(d.y) <= a.y.max(b.y)
            {
                return Err(MeshError("self-intersecting profile"));
            }
        }
    }
    let sign = area.signum();
    let mut ids: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    while ids.len() > 3 {
        let mut ear = None;
        for k in 0..ids.len() {
            let a = ids[(k + ids.len() - 1) % ids.len()];
            let b = ids[k];
            let c = ids[(k + 1) % ids.len()];
            if cross(p[a], p[b], p[c]) * sign <= EPS {
                continue;
            }
            if ids
                .iter()
                .copied()
                .filter(|v| *v != a && *v != b && *v != c)
                .any(|v| cross(p[a], p[b], p[v]) * sign >= -EPS && cross(p[b], p[c], p[v]) * sign >= -EPS && cross(p[c], p[a], p[v]) * sign >= -EPS)
            {
                continue;
            }
            ear = Some((k, [a, b, c]));
            break;
        }
        let Some((k, t)) = ear else {
            return Err(MeshError("profile cannot be triangulated"));
        };
        out.push(t);
        ids.remove(k);
    }
    out.push([ids[0], ids[1], ids[2]]);
    if sign < 0.0 {
        for t in &mut out {
            t.swap(1, 2);
        }
    }
    Ok(out)
}
fn finish(vertices: Vec<Vec3>, triangles: Vec<[usize; 3]>) -> Result<Mesh3, MeshError> {
    if vertices.iter().any(|p| !valid(*p)) {
        return Err(MeshError("coordinates out of range"));
    }
    for t in &triangles {
        let (Some(a), Some(b), Some(c)) = (vertices.get(t[0]), vertices.get(t[1]), vertices.get(t[2])) else {
            return Err(MeshError("invalid triangle"));
        };
        if (*b - *a).cross(*c - *a).len() <= EPS {
            return Err(MeshError("degenerate triangle"));
        }
    }
    Ok(Mesh3 { vertices, triangles })
}
fn sides(tris: &mut Vec<[usize; 3]>, n: usize, rings: usize, wrap: bool) {
    for r in 0..if wrap { rings } else { rings - 1 } {
        let next = (r + 1) % rings;
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b, c, d) = (r * n + i, r * n + j, next * n + j, next * n + i);
            tris.push([a, b, c]);
            tris.push([a, c, d]);
        }
    }
}
impl Mesh3 {
    pub fn box_solid(min: Vec3, max: Vec3) -> Result<Self, MeshError> {
        if !valid(min) || !valid(max) || max.x - min.x <= EPS || max.y - min.y <= EPS || max.z - min.z <= EPS {
            return Err(MeshError("invalid box"));
        }
        Self::extrude(&[Vec2::new(min.x, min.y), Vec2::new(max.x, min.y), Vec2::new(max.x, max.y), Vec2::new(min.x, max.y)], min.z, max.z - min.z)
    }
    pub fn extrude(profile: &[Vec2], base_z: f64, height: f64) -> Result<Self, MeshError> {
        if !base_z.is_finite() || !height.is_finite() || height <= EPS {
            return Err(MeshError("invalid extrusion"));
        }
        let cap = polygon(profile)?;
        let n = profile.len();
        count(n, 2)?;
        let area: f64 = (0..n)
            .map(|i| {
                let a = profile[i] - profile[0];
                let b = profile[(i + 1) % n] - profile[0];
                a.x * b.y - b.x * a.y
            })
            .sum();
        let mut p = profile.to_vec();
        if area < 0.0 {
            p.reverse();
        }
        let cap = if area < 0.0 { polygon(&p)? } else { cap };
        let vertices = p.iter().map(|v| Vec3::new(v.x, v.y, base_z)).chain(p.iter().map(|v| Vec3::new(v.x, v.y, base_z + height))).collect();
        let mut tris = Vec::new();
        sides(&mut tris, n, 2, false);
        for [a, b, c] in cap {
            tris.push([c, b, a]);
            tris.push([a + n, b + n, c + n]);
        }
        finish(vertices, tris)
    }
    pub fn cylinder(center: Vec3, radius: f64, height: f64, segments: usize) -> Result<Self, MeshError> {
        if !valid(center) || !radius.is_finite() || radius <= EPS || !(3..=4096).contains(&segments) {
            return Err(MeshError("invalid cylinder"));
        }
        let p = (0..segments)
            .map(|i| {
                let a = TAU * i as f64 / segments as f64;
                Vec2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
            })
            .collect::<Vec<_>>();
        Self::extrude(&p, center.z, height)
    }
    pub fn cone(center: Vec3, radius: f64, height: f64, segments: usize) -> Result<Self, MeshError> {
        if !valid(center) || !radius.is_finite() || radius <= EPS || !height.is_finite() || height <= EPS || !(3..=4096).contains(&segments) {
            return Err(MeshError("invalid cone"));
        }
        let mut v = (0..segments)
            .map(|i| {
                let a = TAU * i as f64 / segments as f64;
                Vec3::new(center.x + radius * a.cos(), center.y + radius * a.sin(), center.z)
            })
            .collect::<Vec<_>>();
        v.push(center);
        v.push(center + Vec3::new(0.0, 0.0, height));
        let mut t = Vec::new();
        for i in 0..segments {
            let j = (i + 1) % segments;
            t.push([i, j, segments + 1]);
            t.push([segments, j, i]);
        }
        finish(v, t)
    }
    pub fn sphere(center: Vec3, radius: f64, segments: usize, rings: usize) -> Result<Self, MeshError> {
        if !valid(center) || !radius.is_finite() || radius <= EPS || !(3..=4096).contains(&segments) || !(2..=4096).contains(&rings) {
            return Err(MeshError("invalid sphere"));
        }
        count(segments, rings)?;
        let mut v = Vec::new();
        for j in 1..rings {
            let phi = std::f64::consts::PI * j as f64 / rings as f64;
            for i in 0..segments {
                let a = TAU * i as f64 / segments as f64;
                v.push(center + Vec3::new(radius * phi.sin() * a.cos(), radius * phi.sin() * a.sin(), radius * phi.cos()));
            }
        }
        let top = v.len();
        v.push(center + Vec3::new(0.0, 0.0, radius));
        let bottom = v.len();
        v.push(center - Vec3::new(0.0, 0.0, radius));
        let mut t = Vec::new();
        for r in 0..rings - 2 {
            for i in 0..segments {
                let j = (i + 1) % segments;
                let (a, b, c, d) = (r * segments + i, r * segments + j, (r + 1) * segments + j, (r + 1) * segments + i);
                t.push([a, d, c]);
                t.push([a, c, b]);
            }
        }
        for i in 0..segments {
            let j = (i + 1) % segments;
            t.push([top, i, j]);
            let a = (rings - 2) * segments + i;
            let b = (rings - 2) * segments + j;
            t.push([bottom, b, a]);
        }
        finish(v, t)
    }
    pub fn torus(center: Vec3, major_radius: f64, minor_radius: f64, major_segments: usize, minor_segments: usize) -> Result<Self, MeshError> {
        if !valid(center)
            || !major_radius.is_finite()
            || !minor_radius.is_finite()
            || minor_radius <= EPS
            || major_radius <= minor_radius
            || !(3..=4096).contains(&major_segments)
            || !(3..=4096).contains(&minor_segments)
        {
            return Err(MeshError("invalid torus"));
        }
        count(major_segments, minor_segments)?;
        let mut v = Vec::new();
        for i in 0..major_segments {
            let a = TAU * i as f64 / major_segments as f64;
            for j in 0..minor_segments {
                let b = TAU * j as f64 / minor_segments as f64;
                let r = major_radius + minor_radius * b.cos();
                v.push(center + Vec3::new(r * a.cos(), r * a.sin(), minor_radius * b.sin()));
            }
        }
        let mut t = Vec::new();
        sides(&mut t, minor_segments, major_segments, true);
        for tri in &mut t {
            tri.swap(1, 2);
        }
        finish(v, t)
    }
    /// Revolve a closed (radius, z) profile around Z. Axis contacts must form one edge or point.
    pub fn revolve(profile: &[Vec2], segments: usize) -> Result<Self, MeshError> {
        Self::revolve_axis(profile, Vec3::ZERO, Vec3::Z, segments, 0.0, TAU)
    }
    /// Revolve a closed (radius, axial-distance) profile around an arbitrary axis.
    /// `start_angle` and `sweep_angle` are radians; a partial revolution is capped at both ends.
    /// Axis contacts must form one edge or one point. Negative sweeps are supported.
    pub fn revolve_axis(profile: &[Vec2], origin: Vec3, axis: Vec3, segments: usize, start_angle: f64, sweep_angle: f64) -> Result<Self, MeshError> {
        Self::revolve_axis_oriented(profile, origin, axis, None, segments, start_angle, sweep_angle)
    }
    /// Revolve around an arbitrary axis using `radial_hint` to set angle zero.
    /// The hint is projected into the plane perpendicular to `axis`.
    pub fn revolve_axis_oriented(
        profile: &[Vec2],
        origin: Vec3,
        axis: Vec3,
        radial_hint: Option<Vec3>,
        segments: usize,
        start_angle: f64,
        sweep_angle: f64,
    ) -> Result<Self, MeshError> {
        polygon(profile)?;
        if !valid(origin)
            || !axis.is_finite()
            || axis.len() <= EPS
            || !(3..=4096).contains(&segments)
            || !start_angle.is_finite()
            || !sweep_angle.is_finite()
            || sweep_angle.abs() <= EPS
            || sweep_angle.abs() > TAU + EPS
            || profile.iter().any(|p| p.x < 0.0 || (p.x > 0.0 && p.x <= EPS))
        {
            return Err(MeshError("invalid revolution"));
        }
        let axis = axis.normalized();
        // Angle zero defaults to projected world +X (then +Y when the axis is X-aligned).
        let hint = radial_hint.unwrap_or_else(|| if axis.x.abs() < 0.99 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
        if !hint.is_finite() {
            return Err(MeshError("invalid revolution frame"));
        }
        let u = (hint - axis * hint.dot(axis)).normalized();
        if u.len() <= EPS {
            return Err(MeshError("invalid revolution frame"));
        }
        let v = axis.cross(u).normalized();
        let full = (sweep_angle.abs() - TAU).abs() <= EPS;
        let sweep_angle = if full { sweep_angle.signum() * TAU } else { sweep_angle };
        let rings = if full { segments } else { segments + 1 };
        count(profile.len(), rings)?;
        let mut p = profile.to_vec();
        let area: f64 = (0..p.len())
            .map(|i| {
                let a = p[i] - p[0];
                let b = p[(i + 1) % p.len()] - p[0];
                a.x * b.y - b.x * a.y
            })
            .sum();
        if area < 0.0 {
            p.reverse();
        }
        let axis_points = p.iter().enumerate().filter_map(|(i, q)| if q.x == 0.0 { Some(i) } else { None }).collect::<Vec<_>>();
        if axis_points.len() > 2
            || (axis_points.len() == 2 && (axis_points[0] + 1) % p.len() != axis_points[1] && (axis_points[1] + 1) % p.len() != axis_points[0])
        {
            return Err(MeshError("unsupported axis contacts"));
        }
        let mut vertices = Vec::with_capacity(rings.saturating_mul(p.len()));
        let mut index = vec![vec![0; p.len()]; rings];
        for (j, q) in p.iter().enumerate() {
            if q.x == 0.0 {
                let id = vertices.len();
                vertices.push(origin + axis * q.y);
                for row in &mut index {
                    row[j] = id;
                }
            } else {
                for (i, row) in index.iter_mut().enumerate() {
                    let a = start_angle + sweep_angle * i as f64 / segments as f64;
                    let radial = u * a.cos() + v * a.sin();
                    row[j] = vertices.len();
                    vertices.push(origin + axis * q.y + radial * q.x);
                }
            }
        }
        let mut triangles = Vec::new();
        for i in 0..if full { rings } else { rings - 1 } {
            let next = (i + 1) % rings;
            for j in 0..p.len() {
                let k = (j + 1) % p.len();
                let (a, b, c, d) = (index[i][j], index[i][k], index[next][k], index[next][j]);
                if a != c && c != b && b != a {
                    triangles.push([a, c, b]);
                }
                if a != d && d != c && c != a {
                    triangles.push([a, d, c]);
                }
            }
        }
        if !full {
            // The profile's CCW cap normal maps to -v at the start and +v at the end.
            let cap = polygon(&p)?;
            for [a, b, c] in cap {
                let (a0, b0, c0) = (index[0][a], index[0][b], index[0][c]);
                let (a1, b1, c1) = (index[rings - 1][a], index[rings - 1][b], index[rings - 1][c]);
                triangles.push([a0, b0, c0]);
                triangles.push([a1, c1, b1]);
            }
        }
        if sweep_angle < 0.0 {
            for tri in &mut triangles {
                tri.swap(1, 2);
            }
        }
        finish(vertices, triangles)
    }
    /// Sweep a profile through a polyline using parallel-transported frames. Sharp 180° turns are rejected.
    pub fn sweep(profile: &[Vec2], path: &[Vec3]) -> Result<Self, MeshError> {
        Self::sweep_with_options(profile, path, 0.0, 1.0, 1.0)
    }
    /// Sweep with total twist in radians and linearly interpolated positive scale along path length.
    pub fn sweep_with_options(profile: &[Vec2], path: &[Vec3], twist: f64, scale_start: f64, scale_end: f64) -> Result<Self, MeshError> {
        polygon(profile)?;
        if path.len() < 2
            || path.len() > 4096
            || path.iter().any(|p| !valid(*p))
            || !twist.is_finite()
            || twist.abs() > TAU * 1000.0
            || !scale_start.is_finite()
            || !scale_end.is_finite()
            || scale_start <= EPS
            || scale_end <= EPS
            || scale_start > 1e6
            || scale_end > 1e6
        {
            return Err(MeshError("invalid path"));
        }
        count(profile.len(), path.len())?;
        let mut directions = Vec::new();
        let mut distances = Vec::with_capacity(path.len());
        distances.push(0.0);
        for pair in path.windows(2) {
            let delta = pair[1] - pair[0];
            if delta.len() <= EPS {
                return Err(MeshError("duplicate path points"));
            }
            directions.push(delta.normalized());
            distances.push(distances.last().copied().unwrap_or(0.0) + delta.len());
        }
        let total = distances.last().copied().unwrap_or(0.0);
        if !total.is_finite() || total <= EPS {
            return Err(MeshError("invalid path length"));
        }
        let mut tangents = Vec::with_capacity(path.len());
        tangents.push(directions[0]);
        for pair in directions.windows(2) {
            let sum = pair[0] + pair[1];
            if sum.len() <= EPS {
                return Err(MeshError("path reverses direction"));
            }
            tangents.push(sum.normalized());
        }
        tangents.push(directions[directions.len() - 1]);
        let t = tangents[0];
        let reference = if t.z.abs() < 0.9 { Vec3::Z } else { Vec3::new(1.0, 0.0, 0.0) };
        let mut u = reference.cross(t).normalized();
        let mut sections = Vec::with_capacity(path.len());
        for i in 0..path.len() {
            if i > 0 {
                let previous = tangents[i - 1];
                let current = tangents[i];
                let axis = previous.cross(current);
                let cosine = previous.dot(current);
                if cosine < -1.0 + EPS {
                    return Err(MeshError("path reverses direction"));
                }
                // Minimal rotation mapping previous tangent to current tangent.
                u = u + axis.cross(u) + axis.cross(axis.cross(u)) * (1.0 / (1.0 + cosine));
                u = (u - current * u.dot(current)).normalized();
            }
            let fraction = distances[i] / total;
            let angle = twist * fraction;
            let (sin, cos) = angle.sin_cos();
            let rotated_u = u * cos + tangents[i].cross(u) * sin;
            let v = tangents[i].cross(rotated_u);
            let scale = scale_start + (scale_end - scale_start) * fraction;
            sections.push(profile.iter().map(|p| path[i] + rotated_u * (p.x * scale) + v * (p.y * scale)).collect());
        }
        Self::loft(&sections)
    }
    /// Join equally sized, planar simple sections with matching vertex correspondence.
    /// Sections must have coherent winding; self-intersection between sections is not tested.
    pub fn loft(sections: &[Vec<Vec3>]) -> Result<Self, MeshError> {
        if sections.len() < 2 {
            return Err(MeshError("loft needs sections"));
        }
        let n = sections[0].len();
        if n < 3 {
            return Err(MeshError("invalid loft sections"));
        }
        count(n, sections.len())?;
        if sections.iter().any(|s| s.len() != n || s.iter().any(|p| !valid(*p))) {
            return Err(MeshError("invalid loft sections"));
        }
        let mut caps = Vec::new();
        let mut normals = Vec::new();
        for section in sections {
            let origin = section[0];
            let mut normal = Vec3::ZERO;
            for i in 1..n - 1 {
                normal = normal + (section[i] - origin).cross(section[i + 1] - origin);
            }
            if normal.len() <= EPS {
                return Err(MeshError("degenerate loft section"));
            }
            normal = normal.normalized();
            let u = (section[1] - origin).normalized();
            let v = normal.cross(u);
            if section.iter().any(|p| (*p - origin).dot(normal).abs() > EPS * (1.0 + (*p - origin).len())) {
                return Err(MeshError("nonplanar loft section"));
            }
            let profile = section
                .iter()
                .map(|p| {
                    let d = *p - origin;
                    Vec2::new(d.dot(u), d.dot(v))
                })
                .collect::<Vec<_>>();
            caps.push(polygon(&profile)?);
            normals.push(normal);
        }
        if normals.windows(2).any(|w| w[0].dot(w[1]) <= -1.0 + EPS) {
            return Err(MeshError("inconsistent section orientation"));
        }
        let center = |s: &[Vec3]| s.iter().copied().fold(Vec3::ZERO, |a, b| a + b) * (1.0 / n as f64);
        let travel = center(&sections[1]) - center(&sections[0]);
        if travel.len() <= EPS || travel.dot(normals[0]).abs() <= EPS {
            return Err(MeshError("loft has no normal separation"));
        }
        let reverse = travel.dot(normals[0]) < 0.0;
        let mut tris = Vec::new();
        sides(&mut tris, n, sections.len(), false);
        for [a, b, c] in &caps[0] {
            tris.push([*c, *b, *a]);
        }
        let offset = (sections.len() - 1) * n;
        for [a, b, c] in &caps[caps.len() - 1] {
            tris.push([a + offset, b + offset, c + offset]);
        }
        if reverse {
            for t in &mut tris {
                t.swap(1, 2);
            }
        }
        finish(sections.iter().flatten().copied().collect(), tris)
    }
    /// Loft planar profiles with differing vertex counts by resampling their perimeters uniformly.
    /// Sampling is bounded by the largest input section (at most 2048 vertices).
    pub fn loft_resampled(sections: &[Vec<Vec3>]) -> Result<Self, MeshError> {
        if sections.len() < 2 || sections.len() > LIMIT || sections.iter().any(|s| s.len() < 3 || s.len() > 2048 || s.iter().any(|p| !valid(*p))) {
            return Err(MeshError("invalid loft sections"));
        }
        let n = sections.iter().map(Vec::len).max().ok_or(MeshError("loft needs sections"))?;
        count(n, sections.len())?;
        let mut sampled = Vec::with_capacity(sections.len());
        for section in sections {
            if section.len() == n {
                sampled.push(section.clone());
                continue;
            }
            let mut lengths = Vec::with_capacity(section.len() + 1);
            lengths.push(0.0);
            for i in 0..section.len() {
                let edge = (section[(i + 1) % section.len()] - section[i]).len();
                if !edge.is_finite() || edge <= EPS {
                    return Err(MeshError("duplicate loft points"));
                }
                lengths.push(lengths.last().copied().unwrap_or(0.0) + edge);
            }
            let perimeter = lengths.last().copied().unwrap_or(0.0);
            if !perimeter.is_finite() || perimeter <= EPS {
                return Err(MeshError("degenerate loft section"));
            }
            let mut out = Vec::with_capacity(n);
            let mut edge = 0;
            for i in 0..n {
                let target = perimeter * i as f64 / n as f64;
                while edge + 1 < section.len() && lengths[edge + 1] < target {
                    edge += 1;
                }
                let start = lengths[edge];
                let end = lengths[edge + 1];
                let t = ((target - start) / (end - start)).clamp(0.0, 1.0);
                out.push(section[edge] + (section[(edge + 1) % section.len()] - section[edge]) * t);
            }
            sampled.push(out);
        }
        Self::loft(&sampled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    fn check(m: Mesh3) {
        let mut edges = HashMap::new();
        let mut volume = 0.0;
        for [a, b, c] in m.triangles {
            let (p, q, r) = (m.vertices[a], m.vertices[b], m.vertices[c]);
            assert!(p.is_finite() && q.is_finite() && r.is_finite());
            assert!((q - p).cross(r - p).len() > EPS);
            volume += p.dot(q.cross(r)) / 6.0;
            for (i, j) in [(a, b), (b, c), (c, a)] {
                let e = edges.entry((i.min(j), i.max(j))).or_insert((0, 0));
                e.0 += 1;
                e.1 += if i < j { 1 } else { -1 };
            }
        }
        assert!(volume > EPS, "volume {volume}");
        assert!(edges.values().all(|e| *e == (2, 0)));
    }
    #[test]
    fn primitives_are_closed_and_oriented() -> Result<(), MeshError> {
        check(Mesh3::box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0))?);
        check(Mesh3::cylinder(Vec3::ZERO, 2.0, 3.0, 16)?);
        check(Mesh3::cone(Vec3::ZERO, 2.0, 3.0, 16)?);
        check(Mesh3::sphere(Vec3::ZERO, 2.0, 16, 8)?);
        check(Mesh3::torus(Vec3::ZERO, 3.0, 1.0, 16, 8)?);
        let p = [Vec2::new(1.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 1.0), Vec2::new(1.0, 1.0)];
        check(Mesh3::revolve(&p, 16)?);
        Ok(())
    }
    fn volume(m: &Mesh3) -> f64 {
        let origin = m.vertices.first().copied().unwrap_or(Vec3::ZERO);
        m.triangles.iter().map(|t| (m.vertices[t[0]] - origin).dot((m.vertices[t[1]] - origin).cross(m.vertices[t[2]] - origin)) / 6.0).sum()
    }
    #[test]
    fn known_volumes_and_general_loft() -> Result<(), MeshError> {
        let circle_area = 8.0 * (TAU / 16.0).sin();
        assert!((volume(&Mesh3::box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0))?) - 24.0).abs() < 1e-8);
        assert!((volume(&Mesh3::cylinder(Vec3::ZERO, 1.0, 3.0, 16)?) - 3.0 * circle_area).abs() < 1e-8);
        assert!((volume(&Mesh3::cone(Vec3::ZERO, 1.0, 3.0, 16)?) - circle_area).abs() < 1e-8);
        let p = [Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)];
        let sections = vec![p.iter().map(|v| Vec3::new(0.0, v.x, v.y)).collect(), p.iter().map(|v| Vec3::new(3.0, v.x, v.y)).collect()];
        let loft = Mesh3::loft(&sections)?;
        assert!((volume(&loft) - 12.0).abs() < 1e-8);
        check(loft);
        let s = Mesh3::sphere(Vec3::ZERO, 1.0, 32, 16)?;
        assert!(volume(&s) > 4.0 && volume(&s) < 4.2);
        let t = Mesh3::torus(Vec3::ZERO, 3.0, 1.0, 32, 16)?;
        assert!(volume(&t) > 50.0 && volume(&t) < 60.0);
        Ok(())
    }
    #[test]
    fn concave_profile_and_hostile_inputs() -> Result<(), MeshError> {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 2.0), Vec2::new(0.0, 2.0)];
        check(Mesh3::extrude(&p, 0.0, 2.0)?);
        check(Mesh3::sweep(&p, &[Vec3::ZERO, Vec3::new(1.0, 0.0, 2.0)])?);
        check(Mesh3::revolve(&[Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 3.0), Vec2::new(0.0, 3.0)], 16)?);
        check(Mesh3::sweep(&p, &[Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 0.0, 10.0)])?);
        assert!(Mesh3::loft(&[vec![], vec![]]).is_err());
        assert!(Mesh3::sphere(Vec3::ZERO, f64::NAN, usize::MAX, usize::MAX).is_err());
        assert!(Mesh3::extrude(&p, 0.0, f64::INFINITY).is_err());
        Ok(())
    }

    #[test]
    fn partial_and_arbitrary_axis_revolutions_are_capped() -> Result<(), MeshError> {
        let p = [Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(1.0, 2.0)];
        let full = Mesh3::revolve(&p, 48)?;
        let partial = Mesh3::revolve_axis(&p, Vec3::new(3.0, 4.0, 5.0), Vec3::new(1.0, 2.0, 3.0), 12, 0.3, std::f64::consts::FRAC_PI_2)?;
        check(partial.clone());
        assert_eq!(partial.vertices.len(), (12 + 1) * p.len());
        assert!((volume(&partial) / volume(&full) - 0.25).abs() < 1e-8);

        let negative = Mesh3::revolve_axis(&p, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 12, 0.0, -std::f64::consts::FRAC_PI_2)?;
        check(negative);
        assert!(Mesh3::revolve_axis(&p, Vec3::ZERO, Vec3::ZERO, 12, 0.0, 1.0).is_err());
        assert!(Mesh3::revolve_axis(&p, Vec3::ZERO, Vec3::Z, 12, 0.0, 10.0).is_err());
        Ok(())
    }

    #[test]
    fn loft_resamples_different_counts_and_sweep_interpolates_options() -> Result<(), MeshError> {
        let lower = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
        // A triangle in a tilted plane, with positive travel along the first section's normal.
        let upper = vec![Vec3::new(0.0, 0.0, 3.0), Vec3::new(3.0, 0.0, 3.0), Vec3::new(1.5, 2.0, 4.0)];
        let loft = Mesh3::loft_resampled(&[lower, upper])?;
        check(loft.clone());
        assert_eq!(loft.vertices.len(), 8);

        let p = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
        let path = [Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0), Vec3::new(0.0, 0.0, 6.0)];
        let plain = Mesh3::sweep(&p, &path)?;
        let shaped = Mesh3::sweep_with_options(&p, &path, std::f64::consts::FRAC_PI_2, 1.0, 2.0)?;
        check(shaped.clone());
        assert_eq!(plain.vertices.len(), shaped.vertices.len());
        assert_ne!(plain.vertices, shaped.vertices);
        assert!(Mesh3::sweep_with_options(&p, &path, f64::NAN, 1.0, 1.0).is_err());
        assert!(Mesh3::sweep_with_options(&p, &path, 0.0, 0.0, 1.0).is_err());
        Ok(())
    }
}
