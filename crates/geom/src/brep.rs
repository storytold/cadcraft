//! Exact, serializable boundary geometry. Display triangles are derived from this model.
//!
//! Compressed vertex/edge/face indices are persistent topology identifiers within a body;
//! rigid transforms preserve their ordering. All deserialized topology is checked before
//! passing it to the kernel, whose extraction API assumes indices have been checked.
use crate::{EPS, Mesh3, MeshError, Vec2, Vec3};
use kernel::{Curve, Point3, Rad, Solid, Surface, Vector3, Wire, builder};
use serde::{Deserialize, Serialize};
use truck_meshalgo::prelude::{MeshableShape, MeshedShape};
pub use truck_modeling as kernel;
mod finish;
mod profiles;
mod validate;

pub type CompressedSolid = truck_topology::compress::CompressedSolid<Point3, Curve, Surface>;

/// Serialized exact curves, trimmed surfaces and shared boundary topology.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    pub geometry: CompressedSolid,
}

const MAX_TOPOLOGY: usize = 4096;

fn point(p: Vec3) -> Result<Point3, MeshError> {
    if !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 1e9 {
        return Err(MeshError("exact geometry coordinates must be finite and within ±1e9"));
    }
    Ok(Point3::new(p.x, p.y, p.z))
}
fn positive(n: f64) -> Result<(), MeshError> {
    if !n.is_finite() || !(1e-6..=1e9).contains(&n) {
        return Err(MeshError("exact geometry size must be between 1e-6 and 1e9"));
    }
    Ok(())
}

impl Body {
    pub fn from_model(model: &Solid) -> Result<Self, MeshError> {
        let body = Self { geometry: model.compress() };
        body.validate()?;
        Ok(body)
    }

    /// Validate counts, finite vertices, references, connectivity and closed orientation.
    pub fn validate(&self) -> Result<(), MeshError> {
        if self.geometry.boundaries.is_empty() || self.geometry.boundaries.len() > 64 {
            return Err(MeshError("invalid exact solid shell count"));
        }
        let mut total = 0usize;
        for shell in &self.geometry.boundaries {
            total = total.saturating_add(shell.vertices.len()).saturating_add(shell.edges.len()).saturating_add(shell.faces.len());
            if total > MAX_TOPOLOGY || shell.faces.is_empty() {
                return Err(MeshError("exact solid exceeds topology limits"));
            }
            for p in &shell.vertices {
                point(Vec3::new(p.x, p.y, p.z))?;
            }
            for edge in &shell.edges {
                if edge.vertices.0 >= shell.vertices.len() || edge.vertices.1 >= shell.vertices.len() {
                    return Err(MeshError("exact edge references a missing vertex"));
                }
            }
            for face in &shell.faces {
                if face.boundaries.is_empty() || face.boundaries.len() > 256 {
                    return Err(MeshError("invalid exact face boundary count"));
                }
                for wire in &face.boundaries {
                    if wire.is_empty() || wire.len() > MAX_TOPOLOGY || wire.iter().any(|edge| edge.index >= shell.edges.len()) {
                        return Err(MeshError("exact face references an invalid boundary"));
                    }
                }
            }
        }
        validate::validate_geometry(&self.geometry)?;
        Solid::extract(self.geometry.clone()).map_err(|_| MeshError("exact solid must have closed, consistently oriented boundaries"))?;
        Ok(())
    }

    pub fn model(&self) -> Result<Solid, MeshError> {
        self.validate()?;
        Solid::extract(self.geometry.clone()).map_err(|_| MeshError("invalid exact solid"))
    }

    pub fn box_solid(min: Vec3, max: Vec3) -> Result<Self, MeshError> {
        let origin = point(min)?;
        point(max)?;
        let delta = max - min;
        for n in [delta.x, delta.y, delta.z] {
            positive(n)?;
        }
        let v = builder::vertex(origin);
        let edge = builder::tsweep(&v, Vector3::new(delta.x, 0.0, 0.0));
        let face = builder::tsweep(&edge, Vector3::new(0.0, delta.y, 0.0));
        Self::from_model(&builder::tsweep(&face, Vector3::new(0.0, 0.0, delta.z)))
    }

    /// A true circle consists of rational curve segments, independently of display tolerance.
    pub fn cylinder(center: Vec3, radius: f64, height: f64) -> Result<Self, MeshError> {
        positive(radius)?;
        positive(height.abs())?;
        let base = if height < 0.0 { center + Vec3::new(0.0, 0.0, height) } else { center };
        let origin = point(base)?;
        point(base + Vec3::new(radius, radius, height.abs()))?;
        let v = builder::vertex(point(base + Vec3::new(radius, 0.0, 0.0))?);
        let circle = builder::rsweep(&v, origin, Vector3::new(0.0, 0.0, 1.0), Rad(std::f64::consts::TAU));
        let face = builder::try_attach_plane(&[circle]).map_err(|_| MeshError("cannot create exact circular cap"))?;
        Self::from_model(&builder::tsweep(&face, Vector3::new(0.0, 0.0, height.abs())))
    }

    /// Revolve a meridian that touches the axis at both ends (sphere/cone).
    fn meridian_solid(wire: &Wire, axis: Vector3) -> Result<Self, MeshError> {
        let shell = builder::cone(wire, axis, Rad(std::f64::consts::TAU));
        let solid = Solid::try_new(vec![shell]).map_err(|_| MeshError("cannot close exact revolved surface"))?;
        Self::from_model(&solid)
    }

    pub fn sphere(center: Vec3, radius: f64) -> Result<Self, MeshError> {
        positive(radius)?;
        let north = builder::vertex(point(center + Vec3::new(0.0, 0.0, radius))?);
        let south = builder::vertex(point(center - Vec3::new(0.0, 0.0, radius))?);
        let side = builder::circle_arc(&north, &south, point(center + Vec3::new(radius, 0.0, 0.0))?);
        Self::meridian_solid(&Wire::from_iter([side]), Vector3::new(0.0, 0.0, 1.0))
    }

    pub fn cone(center: Vec3, radius: f64, height: f64) -> Result<Self, MeshError> {
        positive(radius)?;
        positive(height.abs())?;
        let foot = builder::vertex(point(center)?);
        let rim = builder::vertex(point(center + Vec3::new(radius, 0.0, 0.0))?);
        let apex = builder::vertex(point(center + Vec3::new(0.0, 0.0, height))?);
        let wire = Wire::from_iter([builder::line(&foot, &rim), builder::line(&rim, &apex)]);
        let mut body = Self::meridian_solid(&wire, Vector3::new(0.0, 0.0, 1.0))?;
        if height < 0.0 {
            for shell in &mut body.geometry.boundaries {
                for face in &mut shell.faces {
                    face.orientation = !face.orientation;
                }
            }
        }
        Ok(body)
    }

    pub fn torus(center: Vec3, radius: f64, tube: f64) -> Result<Self, MeshError> {
        positive(radius)?;
        positive(tube)?;
        if tube >= radius {
            return Err(MeshError("exact torus tube radius must be smaller than major radius"));
        }
        let origin = point(center)?;
        let ring_center = point(center + Vec3::new(radius, 0.0, 0.0))?;
        let v = builder::vertex(point(center + Vec3::new(radius + tube, 0.0, 0.0))?);
        let circle = builder::rsweep(&v, ring_center, Vector3::new(0.0, 1.0, 0.0), Rad(std::f64::consts::TAU));
        let shell = builder::rsweep(&circle, origin, Vector3::new(0.0, 0.0, 1.0), Rad(std::f64::consts::TAU));
        let solid = Solid::try_new(vec![shell]).map_err(|_| MeshError("cannot close exact torus"))?;
        Self::from_model(&solid)
    }

    pub fn extrude(profile: &[Vec2], base_z: f64, height: f64) -> Result<Self, MeshError> {
        // Reuse the existing profile validator (including crossings and count limits).
        Mesh3::extrude(profile, base_z.min(base_z + height), height.abs())?;
        if profile.len() > 256 {
            return Err(MeshError("at most 256 exact profile vertices"));
        }
        let mut vertices = profile.iter().map(|v| point(Vec3::new(v.x, v.y, base_z)).map(builder::vertex)).collect::<Result<Vec<_>, _>>()?;
        let area: f64 = profile.iter().zip(profile.iter().cycle().skip(1)).take(profile.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum();
        if area * height < 0.0 {
            vertices.reverse();
        }
        let wire = Wire::from_iter(vertices.iter().zip(vertices.iter().cycle().skip(1)).take(vertices.len()).map(|(a, b)| builder::line(a, b)));
        let face = builder::try_attach_plane(&[wire]).map_err(|_| MeshError("profile must be planar"))?;
        Self::from_model(&builder::tsweep(&face, Vector3::new(0.0, 0.0, height)))
    }

    /// Derive display triangles. Tolerance is in drawing units, never changes source geometry.
    pub fn tessellate(&self, tolerance: f64) -> Result<Mesh3, MeshError> {
        if !tolerance.is_finite() || !(1e-5..=1e9).contains(&tolerance) {
            return Err(MeshError("exact tessellation tolerance must be between 1e-5 and 1e9"));
        }
        let model = self.model()?;
        validate::validate_tessellation_request(&self.geometry, tolerance)?;
        let meshed = model.triangulation(tolerance);
        if meshed.boundaries().iter().flat_map(|s| s.iter()).any(|f| f.surface().is_none()) {
            return Err(MeshError("an exact face could not be tessellated"));
        }
        let poly = meshed.to_polygon();
        if poly.positions().len() > 100_000 {
            return Err(MeshError("exact display exceeds vertex budget"));
        }
        let vertices = poly.positions().iter().map(|p| Vec3::new(p.x, p.y, p.z)).collect::<Vec<_>>();
        let mut triangles = Vec::new();
        for face in poly.face_iter() {
            let Some(&first) = face.first() else { continue };
            for pair in face.get(1..).unwrap_or_default().windows(2) {
                if let [a, b] = pair {
                    triangles.push([first.pos, a.pos, b.pos]);
                }
                if triangles.len() > 100_000 {
                    return Err(MeshError("exact display exceeds face budget"));
                }
            }
        }
        if triangles.is_empty() || vertices.iter().any(|p| !p.is_finite()) {
            return Err(MeshError("invalid exact display mesh"));
        }
        let mut mesh = Mesh3 { vertices, triangles };
        let volume: f64 = mesh
            .triangles
            .iter()
            .filter_map(|t| Some(mesh.vertices.get(t[0])?.dot(mesh.vertices.get(t[1])?.cross(*mesh.vertices.get(t[2])?)) / 6.0))
            .sum();
        if volume < -EPS {
            for t in &mut mesh.triangles {
                t.swap(1, 2);
            }
        }
        Ok(mesh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_primitives_have_curves_surfaces_and_derived_meshes() {
        for body in [
            Body::box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0)),
            Body::cylinder(Vec3::new(1.0, 2.0, 3.0), 2.0, 4.0),
            Body::sphere(Vec3::ZERO, 2.0),
            Body::cone(Vec3::ZERO, 2.0, 4.0),
            Body::torus(Vec3::ZERO, 4.0, 1.0),
            Body::extrude(&[Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)], 0.0, 3.0),
        ] {
            let body = body.unwrap();
            body.validate().unwrap();
            assert!(!body.tessellate(0.1).unwrap().triangles.is_empty());
        }
    }
    #[test]
    fn hostile_topology_is_rejected_before_kernel_extraction() {
        let mut body = Body::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        body.geometry.boundaries[0].edges[0].vertices.0 = usize::MAX;
        assert!(body.validate().is_err());
        assert!(Body::cylinder(Vec3::ZERO, f64::NAN, 1.0).is_err());
    }
}
