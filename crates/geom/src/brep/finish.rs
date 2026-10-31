//! Exact finishing of straight vertical prisms. Unsupported edge configurations are rejected
//! before any document mutation. Circular fillets are rational curves, never polygon bevels.
use super::{Body, Curve, EPS, MAX_TOPOLOGY, Mesh3, MeshError, Solid, Surface, Vec2, Vec3, Vector3, Wire, builder, kernel, point, positive};
use std::collections::HashMap;

impl Body {
    /// Promote a closed polyhedral mesh to shared line/plane boundary topology. This preserves
    /// polyhedral geometry exactly; it does not infer analytic surfaces from an imported mesh.
    pub fn polyhedron(mesh: &Mesh3) -> Result<Self, MeshError> {
        if mesh.vertices.len() > MAX_TOPOLOGY || mesh.triangles.len() > 1024 {
            return Err(MeshError("polyhedral exact conversion exceeds topology budget"));
        }
        let vertices = mesh.vertices.iter().copied().map(|p| point(p).map(builder::vertex)).collect::<Result<Vec<_>, _>>()?;
        let mut edges = HashMap::new();
        let mut faces = Vec::new();
        for t in &mesh.triangles {
            let mut wire = Wire::new();
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (Some(va), Some(vb)) = (vertices.get(a), vertices.get(b)) else { return Err(MeshError("invalid polyhedron index")) };
                let key = (a.min(b), a.max(b));
                let edge = edges.entry(key).or_insert_with(|| if a < b { builder::line(va, vb) } else { builder::line(vb, va) });
                wire.push_back(if a < b { edge.clone() } else { edge.inverse() });
            }
            faces.push(builder::try_attach_plane(&[wire]).map_err(|_| MeshError("invalid polyhedral face"))?);
        }
        let solid = Solid::try_new(vec![faces.into()]).map_err(|_| MeshError("polyhedron must be closed and consistently oriented"))?;
        Self::from_model(&solid)
    }

    /// Source-only affine transform, preserving all persistent compressed topology indices.
    pub fn transformed(&self, matrix: crate::Mat4) -> Result<Self, MeshError> {
        use kernel::{Transform, Transformed};
        if matrix.m.iter().flatten().any(|n| !n.is_finite()) || matrix.m[3] != [0.0, 0.0, 0.0, 1.0] {
            return Err(MeshError("exact transform must be finite and affine"));
        }
        let a = matrix.m;
        let transform = kernel::Matrix4::new(
            a[0][0], a[1][0], a[2][0], a[3][0], a[0][1], a[1][1], a[2][1], a[3][1], a[0][2], a[1][2], a[2][2], a[3][2], a[0][3], a[1][3], a[2][3],
            a[3][3],
        );
        if transform.inverse_transform().is_none() {
            return Err(MeshError("exact transform must be invertible"));
        }
        let mut out = self.clone();
        for shell in &mut out.geometry.boundaries {
            for p in &mut shell.vertices {
                *p = transform.transform_point(*p);
            }
            for edge in &mut shell.edges {
                edge.curve.transform_by(transform);
            }
            for face in &mut shell.faces {
                face.surface.transform_by(transform);
            }
        }
        out.validate()?;
        Ok(out)
    }

    /// Recover the straight polygon of a vertical extrusion. Curved, tilted and holed profiles
    /// are refused here rather than approximating their finishing operation.
    fn prism(&self) -> Result<(Vec<Vec2>, f64, f64), MeshError> {
        self.validate()?;
        let [shell] = self.geometry.boundaries.as_slice() else { return Err(MeshError("finishing requires a single straight vertical prism")) };
        if shell.edges.iter().any(|e| !matches!(e.curve, Curve::Line(_))) || shell.faces.iter().any(|f| !matches!(f.surface, Surface::Plane(_))) {
            return Err(MeshError("this finishing operation requires a straight polygonal prism"));
        }
        let low = shell.vertices.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        let high = shell.vertices.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
        let tol = (high - low).abs().max(1.0) * 1e-8;
        if high - low <= tol || shell.vertices.iter().any(|p| (p.z - low).abs() > tol && (p.z - high).abs() > tol) {
            return Err(MeshError("finishing requires a vertical extrusion with planar ends"));
        }
        for face in &shell.faces {
            let [wire] = face.boundaries.as_slice() else { continue };
            let pts = wire
                .iter()
                .map(|item| {
                    let edge = shell.edges.get(item.index).ok_or(MeshError("invalid profile edge"))?;
                    let i = if item.orientation { edge.vertices.0 } else { edge.vertices.1 };
                    shell.vertices.get(i).copied().ok_or(MeshError("invalid profile vertex"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if pts.len() >= 3 && pts.iter().all(|p| (p.z - low).abs() <= tol) {
                let mut profile = pts.iter().map(|p| Vec2::new(p.x, p.y)).collect::<Vec<_>>();
                let area: f64 = profile.iter().zip(profile.iter().cycle().skip(1)).take(profile.len()).map(|(a, b)| a.cross(*b)).sum();
                if area < 0.0 {
                    profile.reverse();
                }
                // Every top vertex must have a bottom counterpart and vice versa.
                if shell.vertices.iter().any(|p| !profile.iter().any(|q| (p.x - q.x).hypot(p.y - q.y) <= tol)) {
                    return Err(MeshError("finishing requires a matching vertical profile at both ends"));
                }
                return Ok((profile, low, high - low));
            }
        }
        Err(MeshError("finishing requires an untriangulated planar end face"))
    }

    fn selected_corners(&self, profile: &[Vec2], edges: &[(usize, usize)]) -> Result<Vec<bool>, MeshError> {
        if edges.is_empty() || edges.len() > 256 {
            return Err(MeshError("select between 1 and 256 vertical edges"));
        }
        let mut selected = vec![false; profile.len()];
        for &(s, e) in edges {
            let shell = self.geometry.boundaries.get(s).ok_or(MeshError("no such exact shell"))?;
            let edge = shell.edges.get(e).ok_or(MeshError("no such exact edge"))?;
            let a = shell.vertices.get(edge.vertices.0).ok_or(MeshError("invalid exact vertex"))?;
            let b = shell.vertices.get(edge.vertices.1).ok_or(MeshError("invalid exact vertex"))?;
            if (a.x - b.x).hypot(a.y - b.y) > 1e-8 || (a.z - b.z).abs() <= EPS {
                return Err(MeshError("select a vertical extrusion edge for prism finishing"));
            }
            let Some(i) = profile.iter().position(|p| (p.x - a.x).hypot(p.y - a.y) <= 1e-8) else {
                return Err(MeshError("edge is not a profile corner"));
            };
            if let Some(v) = selected.get_mut(i) {
                *v = true;
            }
        }
        Ok(selected)
    }

    fn finish_vertical(&self, edges: &[(usize, usize)], size: f64, rounded: bool) -> Result<Self, MeshError> {
        positive(size)?;
        let (profile, z, height) = self.prism()?;
        let selected = self.selected_corners(&profile, edges)?;
        let n = profile.len();
        let mut entry = Vec::new();
        let mut exit = Vec::new();
        let mut mids = Vec::new();
        let mut cuts = Vec::new();
        for (i, p) in profile.iter().copied().enumerate() {
            let prev = profile.get((i + n - 1) % n).copied().ok_or(MeshError("invalid profile"))?;
            let next = profile.get((i + 1) % n).copied().ok_or(MeshError("invalid profile"))?;
            let u = (prev - p).normalized();
            let v = (next - p).normalized();
            if selected.get(i) == Some(&true) {
                if (p - prev).cross(next - p) <= EPS {
                    return Err(MeshError("selected corner must be convex"));
                }
                let angle = u.dot(v).clamp(-1.0, 1.0).acos();
                let distance = if rounded { size / (angle * 0.5).tan() } else { size };
                if !distance.is_finite() || distance <= EPS {
                    return Err(MeshError("invalid corner size"));
                }
                let a = p + u * distance;
                let b = p + v * distance;
                let center = p + (u + v).normalized() * (size / (angle * 0.5).sin());
                let mid = center + (p - center).normalized() * size;
                entry.push(builder::vertex(point(Vec3::new(a.x, a.y, z))?));
                exit.push(builder::vertex(point(Vec3::new(b.x, b.y, z))?));
                mids.push(point(Vec3::new(mid.x, mid.y, z))?);
                cuts.push(distance);
            } else {
                let vertex = builder::vertex(point(Vec3::new(p.x, p.y, z))?);
                entry.push(vertex.clone());
                exit.push(vertex);
                mids.push(point(Vec3::new(p.x, p.y, z))?);
                cuts.push(0.0);
            }
        }
        let mut wire = Wire::new();
        for i in 0..n {
            let j = (i + 1) % n;
            let (Some(a), Some(b), Some(next)) = (entry.get(i), exit.get(i), entry.get(j)) else { return Err(MeshError("invalid corner topology")) };
            if cuts.get(i).copied().unwrap_or(0.0) + cuts.get(j).copied().unwrap_or(0.0) >= (profile[j] - profile[i]).len() - EPS {
                return Err(MeshError("corner size would overlap an adjacent corner"));
            }
            if selected.get(i) == Some(&true) {
                let edge =
                    if rounded { builder::circle_arc(a, b, *mids.get(i).ok_or(MeshError("invalid arc midpoint"))?) } else { builder::line(a, b) };
                wire.push_back(edge);
            }
            wire.push_back(builder::line(b, next));
        }
        let face = builder::try_attach_plane(&[wire]).map_err(|_| MeshError("finished profile is invalid"))?;
        Self::from_model(&builder::tsweep(&face, Vector3::new(0.0, 0.0, height)))
    }
    pub fn fillet_vertical_edges(&self, edges: &[(usize, usize)], radius: f64) -> Result<Self, MeshError> {
        self.finish_vertical(edges, radius, true)
    }
    pub fn chamfer_vertical_edges(&self, edges: &[(usize, usize)], distance: f64) -> Result<Self, MeshError> {
        self.finish_vertical(edges, distance, false)
    }

    /// Hollow a convex polygonal vertical prism, optionally opening its upper face. Offsetting
    /// exact side planes gives a constant normal wall thickness. The resulting polyhedron is
    /// converted back to exact shared line/plane topology, including the cavity and rim.
    pub fn shell_prism(&self, thickness: f64, open_top: bool) -> Result<Self, MeshError> {
        positive(thickness)?;
        let (profile, z, height) = self.prism()?;
        if thickness * (if open_top { 1.0 } else { 2.0 }) >= height - EPS {
            return Err(MeshError("shell thickness consumes the solid height"));
        }
        let n = profile.len();
        let mut inner = Vec::new();
        for i in 0..n {
            let (prev, p, next) = (profile[(i + n - 1) % n], profile[i], profile[(i + 1) % n]);
            let u = (p - prev).normalized();
            let v = (next - p).normalized();
            let cross = u.cross(v);
            if cross <= EPS {
                return Err(MeshError("shell requires a strictly convex polygonal profile"));
            }
            let a = p + u.perp() * thickness;
            let b = p + v.perp() * thickness;
            let q = a + u * ((b - a).cross(v) / cross);
            inner.push(q);
        }
        for q in &inner {
            for i in 0..n {
                let edge = profile[(i + 1) % n] - profile[i];
                if edge.normalized().cross(*q - profile[i]) < thickness - 1e-8 {
                    return Err(MeshError("shell thickness collapses the cavity"));
                }
            }
        }
        let outer = Mesh3::extrude(&profile, z, height)?;
        let void_height = if open_top { height } else { height - 2.0 * thickness };
        let void = Mesh3::extrude(&inner, z + thickness, void_height)?;
        Self::polyhedron(&outer.subtract(&void)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vertical(body: &Body) -> Vec<(usize, usize)> {
        body.geometry
            .boundaries
            .iter()
            .enumerate()
            .flat_map(|(s, b)| {
                b.edges.iter().enumerate().filter_map(move |(i, e)| {
                    let a = b.vertices.get(e.vertices.0)?;
                    let c = b.vertices.get(e.vertices.1)?;
                    ((a.x - c.x).hypot(a.y - c.y) < 1e-8 && (a.z - c.z).abs() > EPS).then_some((s, i))
                })
            })
            .collect()
    }
    #[test]
    fn circular_edge_fillets_and_chamfers_are_exact_and_bounded() {
        let body = Body::box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 3.0)).unwrap();
        let edges = vertical(&body);
        assert_eq!(edges.len(), 4);
        let fillet = body.fillet_vertical_edges(&edges, 0.5).unwrap();
        assert!(fillet.geometry.boundaries[0].edges.iter().any(|e| matches!(e.curve, Curve::NurbsCurve(_))));
        assert!(!fillet.tessellate(0.05).unwrap().triangles.is_empty());
        assert!(body.fillet_vertical_edges(&edges, 3.0).is_err());
        assert!(body.chamfer_vertical_edges(&edges, 0.3).unwrap().validate().is_ok());
    }
    #[test]
    fn shell_has_a_real_cavity_and_rejects_collapse() {
        let body = Body::box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 3.0)).unwrap();
        let shell = body.shell_prism(0.3, true).unwrap();
        assert!(shell.geometry.boundaries.iter().map(|s| s.faces.len()).sum::<usize>() > 6);
        assert!(!shell.tessellate(0.05).unwrap().triangles.is_empty());
        assert!(body.shell_prism(3.0, true).is_err());
    }
}
