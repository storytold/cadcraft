//! Bounded plane sections and capped splits of closed, oriented triangle meshes.
//!
//! `section` returns independent contour loops, counter-clockwise when viewed along the supplied
//! plane normal. `slice` returns `(positive, negative)`: the first mesh occupies the half-space
//! where `(point - origin) · normal >= 0`. Both outputs keep the input's surface winding. A plane
//! coincident with a mesh face or a non-manifold cut is reported as an error; nested contours are
//! bridged and triangulated so holes remain open through each cap.
use crate::{Mesh3, MeshError, Vec2, Vec3};
use std::collections::HashMap;

const MAX_TRIANGLES: usize = 100_000;
const MAX_WORK: usize = 20_000_000;
const MAX_CONTOUR_VERTICES: usize = 16_384;
const MAX_COORD: f64 = 1e12;

#[derive(Clone, Copy)]
struct CutPlane {
    origin: Vec3,
    normal: Vec3,
    tolerance: f64,
}

fn plane(origin: Vec3, normal: Vec3, mesh: &Mesh3) -> Result<CutPlane, MeshError> {
    if mesh.vertices.len() > MAX_TRIANGLES * 3 || mesh.triangles.len() > MAX_TRIANGLES {
        return Err(MeshError("section mesh input size limit"));
    }
    if !origin.is_finite() || origin.x.abs() > MAX_COORD || origin.y.abs() > MAX_COORD || origin.z.abs() > MAX_COORD || !normal.is_finite() {
        return Err(MeshError("invalid section plane"));
    }
    let length = normal.len();
    if !length.is_finite() || length <= 1e-12 {
        return Err(MeshError("section plane normal is zero"));
    }
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut max_abs: f64 = origin.x.abs().max(origin.y.abs()).max(origin.z.abs());
    for p in &mesh.vertices {
        if !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD || p.z.abs() > MAX_COORD {
            return Err(MeshError("invalid mesh coordinates"));
        }
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        max_abs = max_abs.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
    }
    if mesh.vertices.is_empty() {
        return Err(MeshError("mesh must be closed and consistently oriented"));
    }
    let extent = (hi - lo).len();
    let tolerance = 1e-10_f64.max(extent * 1e-15).max(max_abs * f64::EPSILON * 4.0);
    Ok(CutPlane { origin, normal: normal * (1.0 / length), tolerance })
}

fn validate_mesh(mesh: &Mesh3, tolerance: f64) -> Result<(), MeshError> {
    if mesh.triangles.len() > MAX_TRIANGLES || mesh.vertices.len() > MAX_TRIANGLES * 3 {
        return Err(MeshError("section mesh input size limit"));
    }
    let mut edges: HashMap<(usize, usize), (usize, i32)> = HashMap::new();
    let mut work = 0usize;
    let origin = mesh.vertices.first().copied().unwrap_or(Vec3::ZERO);
    let mut volume = 0.0;
    for t in &mesh.triangles {
        let (Some(a), Some(b), Some(c)) = (mesh.vertices.get(t[0]), mesh.vertices.get(t[1]), mesh.vertices.get(t[2])) else {
            return Err(MeshError("invalid triangle index"));
        };
        if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] || (*b - *a).cross(*c - *a).len() <= tolerance * tolerance {
            return Err(MeshError("degenerate mesh triangle"));
        }
        volume += (*a - origin).dot((*b - origin).cross(*c - origin)) / 6.0;
        for (u, v) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let edge = edges.entry((u.min(v), u.max(v))).or_insert((0, 0));
            edge.0 += 1;
            edge.1 += if u < v { 1 } else { -1 };
            work += 1;
            if work > MAX_WORK {
                return Err(MeshError("section work limit"));
            }
        }
    }
    if mesh.triangles.is_empty() || edges.values().any(|e| *e != (2, 0)) {
        return Err(MeshError("mesh must be closed and consistently oriented"));
    }
    if !volume.is_finite() || volume.abs() <= tolerance * tolerance * tolerance {
        return Err(MeshError("mesh has invalid volume"));
    }
    Ok(())
}

fn distances(mesh: &Mesh3, plane: CutPlane) -> Vec<f64> {
    mesh.vertices.iter().map(|p| (*p - plane.origin).dot(plane.normal)).collect()
}

#[derive(Default)]
struct Work {
    vertices: Vec<Vec3>,
    intersections: HashMap<(usize, usize), usize>,
    operations: usize,
}
impl Work {
    fn new(mesh: &Mesh3) -> Self {
        Self { vertices: mesh.vertices.clone(), ..Self::default() }
    }
    fn tick(&mut self, n: usize) -> Result<(), MeshError> {
        self.operations = self.operations.checked_add(n).ok_or(MeshError("section work limit"))?;
        if self.operations > MAX_WORK { Err(MeshError("section work limit")) } else { Ok(()) }
    }
    fn intersection(&mut self, a: usize, b: usize, d: &[f64]) -> Result<usize, MeshError> {
        if d[a].abs() <= 1e-15 {
            return Ok(a);
        }
        if d[b].abs() <= 1e-15 {
            return Ok(b);
        }
        let key = (a.min(b), a.max(b));
        if let Some(i) = self.intersections.get(&key) {
            return Ok(*i);
        }
        self.tick(1)?;
        let den = d[a] - d[b];
        if den == 0.0 || !den.is_finite() {
            return Err(MeshError("unstable section intersection"));
        }
        let t = d[a] / den;
        if !(0.0..=1.0).contains(&t) || !t.is_finite() {
            return Err(MeshError("unstable section intersection"));
        }
        let p = self.vertices[a] + (self.vertices[b] - self.vertices[a]) * t;
        if !p.is_finite() {
            return Err(MeshError("invalid section intersection"));
        }
        let i = self.vertices.len();
        self.vertices.push(p);
        self.intersections.insert(key, i);
        Ok(i)
    }
}

fn class(d: f64, tolerance: f64) -> i8 {
    if d > tolerance {
        1
    } else if d < -tolerance {
        -1
    } else {
        0
    }
}

fn clip(mesh: &Mesh3, d: &[f64], work: &mut Work, positive: bool, tolerance: f64) -> Result<Vec<[usize; 3]>, MeshError> {
    let mut output = Vec::new();
    for tri in &mesh.triangles {
        work.tick(1)?;
        let mut polygon = Vec::with_capacity(4);
        for i in 0..3 {
            let a = tri[i];
            let b = tri[(i + 1) % 3];
            let ca = class(d[a], tolerance);
            let cb = class(d[b], tolerance);
            let ina = if positive { ca >= 0 } else { ca <= 0 };
            let inb = if positive { cb >= 0 } else { cb <= 0 };
            if ina {
                polygon.push(a);
            }
            if ina != inb {
                polygon.push(if ca == 0 {
                    a
                } else if cb == 0 {
                    b
                } else {
                    work.intersection(a, b, d)?
                });
            }
        }
        polygon.dedup();
        if polygon.len() > 1 && polygon.first() == polygon.last() {
            polygon.pop();
        }
        if polygon.len() < 3 {
            continue;
        }
        for i in 1..polygon.len() - 1 {
            let t = [polygon[0], polygon[i], polygon[i + 1]];
            let p = [work.vertices[t[0]], work.vertices[t[1]], work.vertices[t[2]]];
            if (p[1] - p[0]).cross(p[2] - p[0]).len() > tolerance * tolerance {
                output.push(t);
                if output.len() > MAX_TRIANGLES * 4 {
                    return Err(MeshError("section output size limit"));
                }
            }
        }
    }
    Ok(output)
}

/// Find the oriented boundary loops of an open clipped surface.
fn boundary_loops(tris: &[[usize; 3]], vertices: &[Vec3]) -> Result<Vec<Vec<usize>>, MeshError> {
    let mut edges: HashMap<(usize, usize), (usize, usize, u8)> = HashMap::new();
    for t in tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (a.min(b), a.max(b));
            let e = edges.entry(key).or_insert((a, b, 0));
            e.2 = e.2.saturating_add(1);
            if e.2 > 2 {
                return Err(MeshError("non-manifold section surface"));
            }
            if e.2 == 2 && (e.0 != b || e.1 != a) {
                return Err(MeshError("inconsistent section surface winding"));
            }
        }
    }
    let mut next = HashMap::new();
    let mut incoming = HashMap::<usize, usize>::new();
    for (_, (a, b, count)) in edges {
        if count == 1 {
            if next.insert(a, b).is_some() {
                return Err(MeshError("branched section contour"));
            }
            *incoming.entry(b).or_insert(0) += 1;
        }
    }
    if next.is_empty() {
        return Ok(Vec::new());
    }
    if next.keys().any(|v| incoming.get(v) != Some(&1)) || incoming.keys().any(|v| !next.contains_key(v)) {
        return Err(MeshError("open section contour"));
    }
    let mut loops = Vec::new();
    while let Some(start) = next.keys().next().copied() {
        let mut ring = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut at = start;
        loop {
            if ring.len() >= MAX_CONTOUR_VERTICES {
                return Err(MeshError("section contour size limit"));
            }
            ring.push(at);
            seen.insert(at);
            let Some(to) = next.remove(&at) else {
                return Err(MeshError("open section contour"));
            };
            at = to;
            if at == start {
                break;
            }
            if seen.contains(&at) {
                return Err(MeshError("self-touching section contour"));
            }
        }
        loops.push(ring);
        if loops.len() > MAX_TRIANGLES {
            return Err(MeshError("section contour count limit"));
        }
    }
    if loops.iter().flatten().any(|i| *i >= vertices.len()) {
        return Err(MeshError("invalid section vertex"));
    }
    Ok(loops)
}

fn basis(normal: Vec3) -> (Vec3, Vec3) {
    let axis = if normal.x.abs() < 0.8 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
    let u = normal.cross(axis).normalized();
    (u, normal.cross(u).normalized())
}

fn projected(ring: &[usize], vertices: &[Vec3], plane: CutPlane) -> Vec<Vec2> {
    let (u, v) = basis(plane.normal);
    ring.iter()
        .map(|i| {
            let d = vertices[*i] - plane.origin;
            Vec2::new(d.dot(u), d.dot(v))
        })
        .collect()
}

fn signed_area(points: &[Vec2]) -> f64 {
    (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a.x * b.y - a.y * b.x
        })
        .sum::<f64>()
        * 0.5
}

fn orient_section(mut loops: Vec<Vec<usize>>, vertices: &[Vec3], plane: CutPlane) -> Vec<Vec<usize>> {
    for ring in &mut loops {
        if signed_area(&projected(ring, vertices, plane)) < 0.0 {
            ring.reverse();
        }
    }
    loops
}

/// Intersect a closed triangle mesh with a plane and return all independent contour loops.
/// Loops are counter-clockwise in the plane basis induced by `normal`; disconnected components
/// produce separate loops. A nonzero normal is normalized internally.
pub fn section(mesh: &Mesh3, origin: Vec3, normal: Vec3) -> Result<Vec<Vec<Vec3>>, MeshError> {
    let plane = plane(origin, normal, mesh)?;
    validate_mesh(mesh, plane.tolerance)?;
    let d = distances(mesh, plane);
    let mut work = Work::new(mesh);
    let positive = clip(mesh, &d, &mut work, true, plane.tolerance)?;
    let loops = orient_section(boundary_loops(&positive, &work.vertices)?, &work.vertices, plane);
    Ok(loops.into_iter().map(|ring| ring.into_iter().map(|i| work.vertices[i]).collect()).collect())
}

fn point_in_polygon(p: Vec2, ring: &[Vec2]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

fn orient2(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2, tolerance: f64) -> bool {
    let (ab_c, ab_d, cd_a, cd_b) = (orient2(a, b, c), orient2(a, b, d), orient2(c, d, a), orient2(c, d, b));
    let eps = tolerance * tolerance;
    if ab_c * ab_d < -eps && cd_a * cd_b < -eps {
        return true;
    }
    let on_segment = |p: Vec2, x: Vec2, y: Vec2, area: f64| {
        area.abs() <= eps
            && p.x >= x.x.min(y.x) - tolerance
            && p.x <= x.x.max(y.x) + tolerance
            && p.y >= x.y.min(y.y) - tolerance
            && p.y <= x.y.max(y.y) + tolerance
    };
    on_segment(c, a, b, ab_c) || on_segment(d, a, b, ab_d) || on_segment(a, c, d, cd_a) || on_segment(b, c, d, cd_b)
}

fn oriented(mut ring: Vec<usize>, vertices: &[Vec3], plane: CutPlane, sign: f64) -> Vec<usize> {
    if signed_area(&projected(&ring, vertices, plane)).signum() != sign {
        ring.reverse();
    }
    ring
}

struct BridgeContext<'a> {
    outer: &'a [Vec2],
    holes: &'a [Vec<Vec2>],
    all_ids: &'a [Vec<usize>],
    all_points: &'a [Vec<Vec2>],
    vertices: &'a [Vec3],
    plane: CutPlane,
}
fn bridge_hole(
    merged: &[usize],
    hole: &[usize],
    context: BridgeContext<'_>,
    bridges: &mut Vec<(usize, usize)>,
    budget: &mut usize,
) -> Result<Vec<usize>, MeshError> {
    let BridgeContext { outer, holes, all_ids, all_points, vertices, plane } = context;
    let hp = projected(hole, vertices, plane);
    let hi = (0..hole.len())
        .max_by(|a, b| hp[*a].x.total_cmp(&hp[*b].x).then_with(|| hp[*b].y.total_cmp(&hp[*a].y)))
        .ok_or(MeshError("empty cap hole"))?;
    let h_id = hole[hi];
    let h = hp[hi];
    let mut candidates: Vec<(f64, usize, usize, Vec2)> = Vec::new();
    for (oi, &o_id) in merged.iter().enumerate() {
        if o_id == h_id || candidates.iter().any(|(_, id, _, _)| *id == o_id) {
            continue;
        }
        let op = projected(&[o_id], vertices, plane)[0];
        candidates.push((op.dist(h), o_id, oi, op));
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut selected = None;
    for (_, o_id, oi, o) in candidates {
        let mid = (h + o) * 0.5;
        if !point_in_polygon(mid, outer) || holes.iter().any(|poly| point_in_polygon(mid, poly)) {
            continue;
        }
        let mut blocked = false;
        for (loop_ids, loop_points) in all_ids.iter().zip(all_points) {
            for i in 0..loop_ids.len() {
                *budget = budget.checked_add(1).ok_or(MeshError("cap bridge work limit"))?;
                if *budget > MAX_WORK {
                    return Err(MeshError("cap bridge work limit"));
                }
                let a_id = loop_ids[i];
                let b_id = loop_ids[(i + 1) % loop_ids.len()];
                if a_id == h_id || b_id == h_id || a_id == o_id || b_id == o_id {
                    continue;
                }
                let a = loop_points[i];
                let b = loop_points[(i + 1) % loop_points.len()];
                if segments_intersect(h, o, a, b, plane.tolerance) {
                    blocked = true;
                    break;
                }
            }
            if blocked {
                break;
            }
        }
        if !blocked {
            for (bridge_a, bridge_b) in bridges.iter() {
                if *bridge_a == o_id || *bridge_b == o_id || *bridge_a == h_id || *bridge_b == h_id {
                    continue;
                }
                let a = projected(&[*bridge_a], vertices, plane)[0];
                let b = projected(&[*bridge_b], vertices, plane)[0];
                if segments_intersect(h, o, a, b, plane.tolerance) {
                    blocked = true;
                    break;
                }
            }
        }
        if !blocked {
            selected = Some((o_id, oi));
            break;
        }
    }
    let (o_id, oi) = selected.ok_or(MeshError("no visible bridge for cap hole"))?;
    bridges.push((o_id, h_id));
    let mut out = Vec::with_capacity(merged.len() + hole.len() + 2);
    out.extend_from_slice(&merged[..=oi]);
    for k in 0..hole.len() {
        out.push(hole[(hi + k) % hole.len()]);
    }
    out.push(h_id);
    out.push(o_id);
    out.extend_from_slice(&merged[oi + 1..]);
    Ok(out)
}

fn triangulate(ring: &[usize], vertices: &[Vec3], plane: CutPlane) -> Result<Vec<[usize; 3]>, MeshError> {
    if ring.len() < 3 || ring.len() > MAX_CONTOUR_VERTICES {
        return Err(MeshError("invalid cap contour"));
    }
    let points = projected(ring, vertices, plane);
    let area = signed_area(&points);
    if !area.is_finite() || area.abs() <= plane.tolerance * plane.tolerance {
        return Err(MeshError("degenerate cap contour"));
    }
    let sign = area.signum();
    let mut ids: Vec<usize> = (0..ring.len()).collect();
    let mut triangles = Vec::with_capacity(ring.len() - 2);
    let mut operations = 0usize;
    while ids.len() > 3 {
        let mut ear = None;
        for k in 0..ids.len() {
            operations += ids.len();
            if operations > MAX_WORK {
                return Err(MeshError("cap triangulation work limit"));
            }
            let ia = ids[(k + ids.len() - 1) % ids.len()];
            let ib = ids[k];
            let ic = ids[(k + 1) % ids.len()];
            let a = points[ia];
            let b = points[ib];
            let c = points[ic];
            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if cross * sign <= plane.tolerance * plane.tolerance {
                continue;
            }
            // Collinear cut points can leave a zero-area final triple if we take a valid ear
            // greedily. When four vertices remain, preserve at least one non-degenerate triple.
            if ids.len() == 4 {
                let remaining: Vec<_> = ids.iter().copied().filter(|i| *i != ib).collect();
                let (a, b, c) = (points[remaining[0]], points[remaining[1]], points[remaining[2]]);
                let last_area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                if last_area * sign <= plane.tolerance * plane.tolerance {
                    continue;
                }
            }
            let occupied = ids.iter().copied().filter(|j| *j != ia && *j != ib && *j != ic).any(|j| {
                let p = points[j];
                let x = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
                let y = (c.x - b.x) * (p.y - b.y) - (c.y - b.y) * (p.x - b.x);
                let z = (a.x - c.x) * (p.y - c.y) - (a.y - c.y) * (p.x - c.x);
                x * sign > plane.tolerance && y * sign > plane.tolerance && z * sign > plane.tolerance
            });
            if !occupied {
                ear = Some((k, [ring[ia], ring[ib], ring[ic]]));
                break;
            }
        }
        let Some((k, tri)) = ear else {
            return Err(MeshError("cap contour cannot be triangulated"));
        };
        triangles.push(tri);
        ids.remove(k);
    }
    let final_tri = [ring[ids[0]], ring[ids[1]], ring[ids[2]]];
    let a = points[ids[0]];
    let b = points[ids[1]];
    let c = points[ids[2]];
    let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if cross * sign <= plane.tolerance * plane.tolerance {
        return Err(MeshError("degenerate cap triangle"));
    }
    triangles.push(final_tri);
    Ok(triangles)
}

fn cap(tris: &mut Vec<[usize; 3]>, loops: Vec<Vec<usize>>, vertices: &[Vec3], plane: CutPlane) -> Result<(), MeshError> {
    let projected_loops: Vec<_> = loops.iter().map(|r| projected(r, vertices, plane)).collect();
    let areas: Vec<_> = projected_loops.iter().map(|r| signed_area(r)).collect();
    let mut parent: Vec<Option<usize>> = vec![None; loops.len()];
    let mut work = 0usize;
    for i in 0..projected_loops.len() {
        for j in 0..projected_loops.len() {
            if i != j && areas[j].abs() > areas[i].abs() + plane.tolerance * plane.tolerance {
                work = work.checked_add(projected_loops[j].len()).ok_or(MeshError("cap nesting work limit"))?;
                if work > MAX_WORK {
                    return Err(MeshError("cap nesting work limit"));
                }
                if point_in_polygon(projected_loops[i][0], &projected_loops[j]) && parent[i].is_none_or(|old| areas[j].abs() < areas[old].abs()) {
                    parent[i] = Some(j);
                }
            }
        }
    }

    let mut depth = vec![0usize; loops.len()];
    for i in 0..loops.len() {
        let mut at = parent[i];
        let mut hops = 0;
        while let Some(p) = at {
            depth[i] += 1;
            at = parent[p];
            hops += 1;
            if hops > loops.len() {
                return Err(MeshError("invalid nested cap contours"));
            }
        }
    }
    let mut bridged_work = 0usize;
    for outer_i in 0..loops.len() {
        if !depth[outer_i].is_multiple_of(2) {
            continue;
        }
        let desired = -areas[outer_i].signum();
        if desired == 0.0 {
            return Err(MeshError("degenerate cap contour"));
        }
        let mut outer = oriented(loops[outer_i].clone(), vertices, plane, desired);
        let outer_boundary = outer.clone();
        let mut holes = Vec::new();
        let mut hole_ids = Vec::new();
        for i in 0..loops.len() {
            if parent[i] == Some(outer_i) && depth[i] == depth[outer_i] + 1 {
                holes.push(projected(&loops[i], vertices, plane));
                hole_ids.push(oriented(loops[i].clone(), vertices, plane, -desired));
            }
        }
        let all_boundary_ids: Vec<Vec<usize>> = std::iter::once(outer_boundary.clone()).chain(hole_ids.iter().cloned()).collect();
        let all_boundary_points: Vec<Vec<Vec2>> = all_boundary_ids.iter().map(|r| projected(r, vertices, plane)).collect();
        // Bridge each immediate hole into the weakly-simple cap boundary. Keeping the hole's
        // opposite winding makes ear clipping triangulate the annulus instead of filling it.
        let mut bridge_edges = Vec::new();
        for hole in &hole_ids {
            outer = bridge_hole(
                &outer,
                hole,
                BridgeContext {
                    outer: &all_boundary_points[0],
                    holes: &all_boundary_points[1..],
                    all_ids: &all_boundary_ids,
                    all_points: &all_boundary_points,
                    vertices,
                    plane,
                },
                &mut bridge_edges,
                &mut bridged_work,
            )?;
        }
        tris.extend(triangulate(&outer, vertices, plane)?);
        if tris.len() > MAX_TRIANGLES * 4 {
            return Err(MeshError("slice output size limit"));
        }
    }
    Ok(())
}

fn compact(vertices: &[Vec3], triangles: Vec<[usize; 3]>) -> Result<Mesh3, MeshError> {
    if triangles.is_empty() {
        return Ok(Mesh3 { vertices: Vec::new(), triangles });
    }
    let mut map = HashMap::new();
    let mut out_vertices = Vec::new();
    let mut out_triangles = Vec::with_capacity(triangles.len());
    for tri in triangles {
        let mut t = [0; 3];
        for i in 0..3 {
            let Some(p) = vertices.get(tri[i]).copied() else {
                return Err(MeshError("invalid slice vertex"));
            };
            t[i] = *map.entry(tri[i]).or_insert_with(|| {
                out_vertices.push(p);
                out_vertices.len() - 1
            });
        }
        out_triangles.push(t);
    }
    if out_vertices.len() > MAX_TRIANGLES * 3 || out_triangles.len() > MAX_TRIANGLES * 4 {
        return Err(MeshError("slice output size limit"));
    }
    Ok(Mesh3 { vertices: out_vertices, triangles: out_triangles })
}

/// Split a closed mesh with a plane and cap both halves. Returns `(positive, negative)` according
/// to the sign of `(point - origin) · normal`. Disconnected contours and nested loops are supported;
/// cap triangulation bridges each hole without filling its interior.
pub fn slice(mesh: &Mesh3, origin: Vec3, normal: Vec3) -> Result<(Mesh3, Mesh3), MeshError> {
    let plane = plane(origin, normal, mesh)?;
    validate_mesh(mesh, plane.tolerance)?;
    let d = distances(mesh, plane);
    if d.iter().all(|v| *v > plane.tolerance) || d.iter().all(|v| *v < -plane.tolerance) {
        return Err(MeshError("slice plane does not intersect mesh"));
    }
    if mesh.triangles.iter().any(|t| t.iter().all(|i| d[*i].abs() <= plane.tolerance)) {
        return Err(MeshError("slice plane coincides with a mesh face"));
    }
    let mut work = Work::new(mesh);
    let mut pos = clip(mesh, &d, &mut work, true, plane.tolerance)?;
    let mut neg = clip(mesh, &d, &mut work, false, plane.tolerance)?;
    let pos_loops = boundary_loops(&pos, &work.vertices)?;
    let neg_loops = boundary_loops(&neg, &work.vertices)?;
    if pos_loops.is_empty() || neg_loops.is_empty() {
        return Err(MeshError("slice does not produce two solid halves"));
    }
    cap(&mut pos, pos_loops, &work.vertices, plane)?;
    cap(&mut neg, neg_loops, &work.vertices, plane)?;
    let pos = compact(&work.vertices, pos)?;
    let neg = compact(&work.vertices, neg)?;
    validate_mesh(&pos, plane.tolerance)?;
    validate_mesh(&neg, plane.tolerance)?;
    Ok((pos, neg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mesh() -> Result<Mesh3, MeshError> {
        Mesh3::box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0))
    }

    fn volume(mesh: &Mesh3) -> f64 {
        let origin = mesh.vertices.first().copied().unwrap_or(Vec3::ZERO);
        mesh.triangles
            .iter()
            .map(|t| {
                let a = mesh.vertices[t[0]] - origin;
                let b = mesh.vertices[t[1]] - origin;
                let c = mesh.vertices[t[2]] - origin;
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    fn closed(mesh: &Mesh3) {
        let mut edges = HashMap::<(usize, usize), (usize, i32)>::new();
        for t in &mesh.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let e = edges.entry((a.min(b), a.max(b))).or_insert((0, 0));
                e.0 += 1;
                e.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(edges.values().all(|e| *e == (2, 0)));
    }

    #[test]
    fn section_returns_oriented_contour_for_axis_and_oblique_planes() -> Result<(), MeshError> {
        let m = box_mesh()?;
        let z = section(&m, Vec3::new(0.0, 0.0, 1.0), Vec3::Z)?;
        assert_eq!(z.len(), 1);
        assert!(z[0].len() >= 4);
        assert!(z[0].iter().all(|p| (p.z - 1.0).abs() < 1e-8));
        let d = section(&m, Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, 1.0))?;
        assert_eq!(d.len(), 1);
        assert!(d[0].len() >= 6);
        Ok(())
    }

    #[test]
    fn slice_is_closed_oriented_and_preserves_volume() -> Result<(), MeshError> {
        let mesh = box_mesh()?;
        let (positive, negative) = slice(&mesh, Vec3::new(0.0, 0.0, 1.0), Vec3::Z)?;
        closed(&positive);
        closed(&negative);
        assert!((volume(&positive) - 4.0).abs() < 1e-8);
        assert!((volume(&negative) - 4.0).abs() < 1e-8);
        let (oblique_a, oblique_b) = slice(&mesh, Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, 1.0))?;
        closed(&oblique_a);
        closed(&oblique_b);
        assert!((volume(&oblique_a) + volume(&oblique_b) - volume(&mesh)).abs() < 1e-8);
        let mut inward = mesh.clone();
        for t in &mut inward.triangles {
            t.swap(1, 2);
        }
        let (inward_a, inward_b) = slice(&inward, Vec3::new(0.0, 0.0, 1.0), Vec3::Z)?;
        closed(&inward_a);
        closed(&inward_b);
        assert!(volume(&inward_a) < 0.0 && volume(&inward_b) < 0.0);
        Ok(())
    }

    #[test]
    fn disconnected_loops_are_reported_and_hostile_inputs_fail() -> Result<(), MeshError> {
        let a = box_mesh()?;
        let b = Mesh3::box_solid(Vec3::new(4.0, 0.0, 0.0), Vec3::new(6.0, 2.0, 2.0))?;
        let mut combined = a.clone();
        let offset = combined.vertices.len();
        combined.vertices.extend(b.vertices);
        combined.triangles.extend(b.triangles.into_iter().map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]));
        assert_eq!(section(&combined, Vec3::new(0.0, 0.0, 1.0), Vec3::Z)?.len(), 2);
        let (positive, negative) = slice(&combined, Vec3::new(0.0, 0.0, 1.0), Vec3::Z)?;
        closed(&positive);
        closed(&negative);
        assert!((volume(&positive) - 8.0).abs() < 1e-8);
        assert!((volume(&negative) - 8.0).abs() < 1e-8);
        assert!(section(&a, Vec3::ZERO, Vec3::ZERO).is_err());
        assert!(slice(&a, Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0)).is_err());
        assert!(slice(&a, Vec3::new(0.0, 0.0, 3.0), Vec3::Z).is_err());
        Ok(())
    }

    #[test]
    fn slicing_coplanar_face_errors_and_torus_hole_is_preserved() -> Result<(), MeshError> {
        let mesh = box_mesh()?;
        assert!(slice(&mesh, Vec3::new(0.0, 0.0, 2.0), Vec3::Z).is_err());
        let torus = Mesh3::torus(Vec3::ZERO, 3.0, 1.0, 32, 16)?;
        assert_eq!(section(&torus, Vec3::ZERO, Vec3::Z)?.len(), 2);
        let (positive, negative) = slice(&torus, Vec3::ZERO, Vec3::Z)?;
        closed(&positive);
        closed(&negative);
        assert!((volume(&positive) + volume(&negative) - volume(&torus)).abs() < 1e-7);
        assert!((volume(&positive) - volume(&torus) * 0.5).abs() < 1e-7);
        Ok(())
    }
}
