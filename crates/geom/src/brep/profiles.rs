//! Exact, bounded solids built from planar profiles and profile sections.
//!
//! Display meshes remain a separate derived representation. Profile edges stay lines, while
//! loft/sweep spans are kernel homotopy surfaces and revolutions use analytic revolved surfaces.

use std::f64::consts::TAU;

use super::{Body, Mesh3, MeshError, kernel, point};
use crate::{Vec2, Vec3};
use kernel::builder;

const MAX_PROFILE_POINTS: usize = 256;
const MAX_SECTIONS: usize = 64;
const MAX_SWEEP_PATH: usize = 128;
const MAX_PROFILE_WORK: usize = 65_536;
const EPS: f64 = 1e-9;

fn point3(p: Vec3) -> Result<kernel::Point3, MeshError> {
    point(p)
}

fn wire_from_points(points: &[Vec3]) -> Result<kernel::Wire, MeshError> {
    if points.len() < 3 || points.len() > MAX_PROFILE_POINTS || points.iter().any(|p| !p.is_finite()) {
        return Err(MeshError("exact profile needs 3 to 256 finite points"));
    }
    let mut vertices = Vec::with_capacity(points.len());
    for p in points {
        vertices.push(builder::vertex(point3(*p)?));
    }
    Ok(kernel::Wire::from_iter(vertices.iter().zip(vertices.iter().cycle().skip(1)).take(vertices.len()).map(|(a, b)| builder::line(a, b))))
}

fn caps_from_boundary(shell: &mut kernel::Shell) -> Result<(), MeshError> {
    let boundaries = shell.extract_boundaries();
    if boundaries.len() != 2 {
        return Err(MeshError("exact profile shell did not produce two planar end boundaries"));
    }
    let mut caps = kernel::Shell::new();
    for mut boundary in boundaries {
        boundary.invert();
        let cap = builder::try_attach_plane(&[boundary]).map_err(|_| MeshError("exact profile end boundary is not planar"))?;
        caps.push(cap);
    }
    shell.append(&mut caps);
    Ok(())
}

fn solid_from_side_shell(mut shell: kernel::Shell, capped: bool) -> Result<Body, MeshError> {
    if capped {
        caps_from_boundary(&mut shell)?;
    }
    let solid = kernel::Solid::try_new(vec![shell]).map_err(|_| MeshError("exact profile shell is not a closed solid"))?;
    Body::from_model(&solid)
}

impl Body {
    /// Create exact ruled side faces and planar caps for a line-segment profile extrusion.
    pub fn extrude_profile(profile: &[Vec2], base_z: f64, height: f64) -> Result<Self, MeshError> {
        if profile.len() > MAX_PROFILE_POINTS || !height.is_finite() || height.abs() <= EPS {
            return Err(MeshError("exact extrusion height/profile is outside limits"));
        }
        // Body::extrude builds a planar source face before translation, preserving the top and
        // bottom caps as well as the exact line side faces.
        Body::extrude(profile, base_z, height)
    }

    /// Create an exact arbitrary-axis revolution from a closed polygon in radius/axial coordinates.
    /// Angles are radians; partial turns receive planar start/end caps.
    pub fn revolve_profile(
        profile: &[Vec2],
        origin: Vec3,
        axis: Vec3,
        radial_hint: Vec3,
        start_angle: f64,
        sweep_angle: f64,
    ) -> Result<Self, MeshError> {
        if !origin.is_finite()
            || !axis.is_finite()
            || axis.len() <= EPS
            || !radial_hint.is_finite()
            || !start_angle.is_finite()
            || !sweep_angle.is_finite()
        {
            return Err(MeshError("invalid exact revolution frame or angles"));
        }
        // Keep the mesh command's axis contacts, radius, count and self-intersection checks.
        Mesh3::revolve_axis_oriented(profile, origin, axis, Some(radial_hint), 8, start_angle, sweep_angle)?;
        let axis = axis.normalized();
        let radial = (radial_hint - axis * radial_hint.dot(axis)).normalized();
        if radial.len() <= EPS {
            return Err(MeshError("exact revolution radial hint is parallel to its axis"));
        }
        let tangent = axis.cross(radial).normalized();
        let (sin, cos) = start_angle.sin_cos();
        let start_radial = radial * cos + tangent * sin;
        let points = profile.iter().map(|p| origin + axis * p.y + start_radial * p.x).collect::<Vec<_>>();
        let wire = wire_from_points(&points)?;
        let shell = builder::rsweep(&wire, point3(origin)?, kernel::Vector3::new(axis.x, axis.y, axis.z), kernel::Rad(sweep_angle));
        let full = (sweep_angle.abs() - TAU).abs() <= 1e-9;
        solid_from_side_shell(shell, !full)
    }

    /// Revolve a circular meridian exactly, retaining its rational arcs instead of a sampled
    /// polygon. The center uses radius/axial coordinates in the supplied axis frame.
    pub fn revolve_circle(
        center: Vec2,
        radius: f64,
        origin: Vec3,
        axis: Vec3,
        radial_hint: Vec3,
        start_angle: f64,
        sweep_angle: f64,
    ) -> Result<Self, MeshError> {
        let axis_length = axis.len();
        if !center.is_finite()
            || !radius.is_finite()
            || radius <= EPS
            || center.x <= radius + EPS
            || !origin.is_finite()
            || !axis.is_finite()
            || !axis_length.is_finite()
            || axis_length <= EPS
            || !radial_hint.is_finite()
            || !start_angle.is_finite()
            || !sweep_angle.is_finite()
            || sweep_angle.abs() <= EPS
            || sweep_angle.abs() > TAU + EPS
        {
            return Err(MeshError("invalid exact circular revolution"));
        }
        let axis = axis * (1.0 / axis_length);
        let radial = (radial_hint - axis * radial_hint.dot(axis)).normalized();
        if radial.len() <= EPS {
            return Err(MeshError("exact revolution radial hint is parallel to its axis"));
        }
        let tangent = axis.cross(radial).normalized();
        let (sin, cos) = start_angle.sin_cos();
        let u = radial * cos + tangent * sin;
        let meridian_center = origin + axis * center.y + u * center.x;
        let circle_point = |angle: f64| meridian_center + u * (radius * angle.cos()) + axis * (radius * angle.sin());
        let mut vertices = Vec::with_capacity(4);
        for quarter in 0..4 {
            vertices.push(builder::vertex(point3(circle_point(quarter as f64 * TAU / 4.0))?));
        }
        let edges = (0..4)
            .map(|quarter| {
                let a = vertices.get(quarter).ok_or(MeshError("invalid circular revolution"))?;
                let b = vertices.get((quarter + 1) % 4).ok_or(MeshError("invalid circular revolution"))?;
                Ok(builder::circle_arc(a, b, point3(circle_point((quarter as f64 + 0.5) * TAU / 4.0))?))
            })
            .collect::<Result<Vec<_>, MeshError>>()?;
        let wire = kernel::Wire::from_iter(edges);
        let shell = builder::rsweep(&wire, point3(origin)?, kernel::Vector3::new(axis.x, axis.y, axis.z), kernel::Rad(sweep_angle));
        let full = (sweep_angle.abs() - TAU).abs() <= EPS;
        solid_from_side_shell(shell, !full)
    }

    /// Join bounded planar sections with exact line boundaries and kernel homotopy surfaces.
    /// Each section may have a different perimeter point count; resampling is bounded and used
    /// only to establish matching line-boundary topology, not to promote triangles to surfaces.
    pub fn loft_sections(sections: &[Vec<Vec3>]) -> Result<Self, MeshError> {
        if sections.len() < 2
            || sections.len() > MAX_SECTIONS
            || sections.iter().any(|s| s.len() < 3 || s.len() > MAX_PROFILE_POINTS || s.iter().any(|p| !p.is_finite()))
        {
            return Err(MeshError("exact loft needs 2 to 64 sections with 3 to 256 finite points"));
        }
        let target = sections.iter().map(Vec::len).max().ok_or(MeshError("exact loft needs sections"))?;
        if target.checked_mul(sections.len()).is_none_or(|n| n > MAX_PROFILE_WORK) {
            return Err(MeshError("exact loft exceeds its bounded profile work budget"));
        }
        let mut wires = Vec::with_capacity(sections.len());
        for section in sections {
            validate_planar_section(section)?;
            let resampled = resample_section(section, target)?;
            wires.push(wire_from_points(&resampled)?);
        }
        let mut shell = kernel::Shell::new();
        for pair in wires.windows(2) {
            let [a, b] = pair else { continue };
            let mut span = builder::try_wire_homotopy(a, b).map_err(|_| MeshError("exact loft sections cannot be joined"))?;
            shell.append(&mut span);
        }
        solid_from_side_shell(shell, true)
    }

    /// Sweep a polygon through a polyline using parallel-transported frames, interpolated twist,
    /// and scale. Every adjacent pair of stations is connected by exact ruled NURBS surfaces.
    pub fn sweep_profile(profile: &[Vec2], path: &[Vec3], twist: f64, scale_start: f64, scale_end: f64) -> Result<Self, MeshError> {
        Mesh3::sweep_with_options(profile, path, twist, scale_start, scale_end)?;
        if profile.len() > MAX_PROFILE_POINTS || path.len() > MAX_SWEEP_PATH {
            return Err(MeshError("exact sweep exceeds its profile/path count limit"));
        }
        if profile.len().checked_mul(path.len()).is_none_or(|n| n > MAX_PROFILE_WORK) {
            return Err(MeshError("exact sweep exceeds its bounded profile work budget"));
        }
        let directions: Vec<Vec3> = path.windows(2).map(|pair| (pair[1] - pair[0]).normalized()).collect();
        let mut distances = Vec::with_capacity(path.len());
        distances.push(0.0);
        for pair in path.windows(2) {
            let distance = (pair[1] - pair[0]).len();
            if !distance.is_finite() || distance <= EPS {
                return Err(MeshError("exact sweep path has duplicate points"));
            }
            distances.push(distances.last().copied().unwrap_or(0.0) + distance);
        }
        let total = distances.last().copied().unwrap_or(0.0);
        if !total.is_finite() || total <= EPS {
            return Err(MeshError("exact sweep path has no length"));
        }
        let mut tangents = Vec::with_capacity(path.len());
        tangents.push(*directions.first().ok_or(MeshError("exact sweep path needs two points"))?);
        for pair in directions.windows(2) {
            let sum = pair[0] + pair[1];
            if sum.len() <= EPS {
                return Err(MeshError("exact sweep path reverses direction"));
            }
            tangents.push(sum.normalized());
        }
        tangents.push(*directions.last().ok_or(MeshError("exact sweep path needs two points"))?);

        let first_tangent = tangents[0];
        let reference = if first_tangent.z.abs() < 0.9 { Vec3::Z } else { Vec3::new(1.0, 0.0, 0.0) };
        let mut u = reference.cross(first_tangent).normalized();
        let mut sections = Vec::with_capacity(path.len());
        for i in 0..path.len() {
            if i > 0 {
                let previous = tangents[i - 1];
                let current = tangents[i];
                let axis = previous.cross(current);
                let cosine = previous.dot(current);
                if cosine < -1.0 + EPS {
                    return Err(MeshError("exact sweep path reverses direction"));
                }
                u = u + axis.cross(u) + axis.cross(axis.cross(u)) * (1.0 / (1.0 + cosine));
                u = (u - current * u.dot(current)).normalized();
            }
            let fraction = distances[i] / total;
            let (sin, cos) = (twist * fraction).sin_cos();
            let rotated_u = u * cos + tangents[i].cross(u) * sin;
            let v = tangents[i].cross(rotated_u);
            let scale = scale_start + (scale_end - scale_start) * fraction;
            sections.push(profile.iter().map(|p| path[i] + rotated_u * (p.x * scale) + v * (p.y * scale)).collect::<Vec<_>>());
        }
        Body::loft_sections(&sections)
    }
}

fn validate_planar_section(section: &[Vec3]) -> Result<(), MeshError> {
    let origin = section[0];
    let mut normal = Vec3::ZERO;
    for i in 1..section.len() - 1 {
        normal = normal + (section[i] - origin).cross(section[i + 1] - origin);
    }
    if !normal.is_finite() || normal.len() <= EPS {
        return Err(MeshError("exact loft section is degenerate"));
    }
    let normal = normal.normalized();
    if section.iter().any(|p| (*p - origin).dot(normal).abs() > EPS * (1.0 + (*p - origin).len())) {
        return Err(MeshError("exact loft sections must be planar"));
    }
    let u = (section[1] - origin).normalized();
    let v = normal.cross(u);
    let outline = section
        .iter()
        .map(|p| {
            let d = *p - origin;
            Vec2::new(d.dot(u), d.dot(v))
        })
        .collect::<Vec<_>>();
    Mesh3::extrude(&outline, 0.0, 1.0)?;
    Ok(())
}

fn resample_section(section: &[Vec3], count: usize) -> Result<Vec<Vec3>, MeshError> {
    if section.len() == count {
        return Ok(section.to_vec());
    }
    let mut lengths = Vec::with_capacity(section.len() + 1);
    lengths.push(0.0);
    for i in 0..section.len() {
        let edge = (section[(i + 1) % section.len()] - section[i]).len();
        if !edge.is_finite() || edge <= EPS {
            return Err(MeshError("exact loft section contains duplicate points"));
        }
        lengths.push(lengths.last().copied().unwrap_or(0.0) + edge);
    }
    let perimeter = *lengths.last().ok_or(MeshError("exact loft section has no perimeter"))?;
    if !perimeter.is_finite() || perimeter <= EPS {
        return Err(MeshError("exact loft section has a degenerate perimeter"));
    }
    let mut output = Vec::with_capacity(count);
    let mut edge_index = 0usize;
    for i in 0..count {
        let target = perimeter * i as f64 / count as f64;
        while edge_index + 1 < section.len() && lengths[edge_index + 1] < target {
            edge_index += 1;
        }
        let a = section[edge_index];
        let b = section[(edge_index + 1) % section.len()];
        let span = lengths[edge_index + 1] - lengths[edge_index];
        if span <= EPS {
            return Err(MeshError("exact loft section contains a zero-length edge"));
        }
        output.push(a + (b - a) * ((target - lengths[edge_index]) / span));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn profile_builders_make_kernel_surfaces_and_closed_bodies() {
        let square = [Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 1.0), Vec2::new(1.0, 1.0)];
        let partial = Body::revolve_profile(&square, Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 0.0, 0.0), 0.25, PI).unwrap();
        assert!(partial.validate().is_ok());
        assert!(partial.geometry.boundaries.iter().flat_map(|s| &s.faces).any(|f| matches!(f.surface, kernel::Surface::RevolutedCurve(_))));

        let lower = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
        let upper = vec![Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 0.0, 2.0), Vec3::new(1.0, 1.0, 2.0), Vec3::new(0.0, 1.0, 2.0)];
        let loft = Body::loft_sections(&[lower, upper]).unwrap();
        assert!(loft.validate().is_ok());
    }
}
