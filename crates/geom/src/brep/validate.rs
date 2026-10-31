//! Defensive validation for deserialized analytic geometry before it reaches truck's kernel.
//!
//! The kernel's public constructors validate a subset of invariants, but serde can populate
//! unchecked spline fields directly. Keep all recursive curve/surface traversal iterative and
//! put explicit limits on both nested structure and the geometry it can ask the kernel to visit.

use super::{CompressedSolid, MeshError, kernel};
use kernel::{Curve, Surface};

const MAX_COORD: f64 = 1.0e9;
const MAX_KNOT: f64 = 1.0e12;
const MAX_WEIGHT: f64 = 1.0e9;
const MIN_WEIGHT: f64 = 1.0e-9;
const MAX_CURVE_POINTS: usize = 256;
const MAX_SURFACE_SIDE: usize = 64;
const MAX_DEGREE: usize = 16;
const MAX_GEOMETRY_ITEMS: usize = 100_000;
const MAX_NESTED_NODES: usize = 16_384;
const MAX_NESTING: usize = 32;

#[derive(Clone, Copy)]
enum Node<'a> {
    Curve(&'a Curve, usize),
    Surface(&'a Surface, usize),
}

#[derive(Default)]
struct Budget {
    remaining: usize,
}

impl Budget {
    fn new() -> Self {
        Self { remaining: MAX_GEOMETRY_ITEMS }
    }

    fn consume(&mut self, count: usize) -> Result<(), MeshError> {
        self.remaining = self.remaining.checked_sub(count).ok_or(MeshError("exact analytic geometry exceeds validation work budget"))?;
        Ok(())
    }
}

fn bounded_scalar(value: f64, max: f64) -> bool {
    value.is_finite() && value.abs() <= max
}

fn point(p: kernel::Point3) -> Result<(), MeshError> {
    if [p.x, p.y, p.z].into_iter().all(|v| bounded_scalar(v, MAX_COORD)) {
        Ok(())
    } else {
        Err(MeshError("exact analytic control point is nonfinite or exceeds coordinate limits"))
    }
}

fn vector(v: kernel::Vector3, max: f64) -> bool {
    bounded_scalar(v.x, max) && bounded_scalar(v.y, max) && bounded_scalar(v.z, max)
}

fn vector_length(v: kernel::Vector3) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

fn rational_point(v: kernel::Vector4) -> Result<(), MeshError> {
    if ![v.x, v.y, v.z].into_iter().all(|n| bounded_scalar(n, MAX_COORD * MAX_WEIGHT))
        || !v.w.is_finite()
        || !(MIN_WEIGHT..=MAX_WEIGHT).contains(&v.w)
        || ![v.x / v.w, v.y / v.w, v.z / v.w].into_iter().all(|n| bounded_scalar(n, MAX_COORD))
    {
        return Err(MeshError("exact rational control point has an invalid weight or coordinate"));
    }
    Ok(())
}

fn knots(knots: &[f64], controls: usize, max_controls: usize, budget: &mut Budget) -> Result<usize, MeshError> {
    if controls == 0 || controls > max_controls || knots.len() > max_controls + MAX_DEGREE + 1 || knots.len() <= controls + 1 {
        return Err(MeshError("exact spline control-point or knot count is outside limits"));
    }
    let degree = knots.len() - controls - 1;
    if !(1..=MAX_DEGREE).contains(&degree) {
        return Err(MeshError("exact spline degree is outside limits"));
    }
    if knots.iter().any(|k| !bounded_scalar(*k, MAX_KNOT)) || knots.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err(MeshError("exact spline knots must be finite, bounded, and nondecreasing"));
    }
    let mut run = 1usize;
    for pair in knots.windows(2) {
        if pair[0] == pair[1] {
            run += 1;
            if run > degree + 1 {
                return Err(MeshError("exact spline knot multiplicity exceeds its degree"));
            }
        } else {
            run = 1;
        }
    }
    let start = knots[degree];
    let end = knots[controls];
    if !start.is_finite() || !end.is_finite() || end - start <= 1e-12 {
        return Err(MeshError("exact spline has an empty or invalid active knot range"));
    }
    budget.consume(controls.checked_add(knots.len()).ok_or(MeshError("exact spline work count overflow"))?)?;
    Ok(degree)
}

fn bspline_curve_points(points: &[kernel::Point3], knot_values: &[f64], budget: &mut Budget) -> Result<(), MeshError> {
    knots(knot_values, points.len(), MAX_CURVE_POINTS, budget)?;
    for p in points {
        point(*p)?;
    }
    Ok(())
}

fn nurbs_curve(curve: &kernel::NurbsCurve<kernel::Vector4>, budget: &mut Budget) -> Result<(), MeshError> {
    let spline = curve.non_rationalized();
    knots(spline.knot_vec().as_ref(), spline.control_points().len(), MAX_CURVE_POINTS, budget)?;
    for p in spline.control_points() {
        rational_point(*p)?;
    }
    Ok(())
}

fn bspline_surface_points(points: &[Vec<kernel::Point3>], uknots: &[f64], vknots: &[f64], budget: &mut Budget) -> Result<(), MeshError> {
    let Some(first_row) = points.first() else {
        return Err(MeshError("exact spline surface has no control points"));
    };
    let (u_count, v_count) = (points.len(), first_row.len());
    if v_count == 0 || u_count > MAX_SURFACE_SIDE || v_count > MAX_SURFACE_SIDE || points.iter().any(|r| r.len() != v_count) {
        return Err(MeshError("exact spline surface control grid is irregular or outside limits"));
    }
    knots(uknots, u_count, MAX_SURFACE_SIDE, budget)?;
    knots(vknots, v_count, MAX_SURFACE_SIDE, budget)?;
    budget.consume(u_count.checked_mul(v_count).ok_or(MeshError("exact surface work count overflow"))?)?;
    for p in points.iter().flatten() {
        point(*p)?;
    }
    Ok(())
}

fn nurbs_surface(surface: &kernel::NurbsSurface<kernel::Vector4>, budget: &mut Budget) -> Result<(), MeshError> {
    let spline = surface.non_rationalized();
    let (uknots, vknots) = spline.knot_vecs();
    let points = spline.control_points();
    let Some(first_row) = points.first() else {
        return Err(MeshError("exact rational surface has no control points"));
    };
    let (u_count, v_count) = (points.len(), first_row.len());
    if v_count == 0 || u_count > MAX_SURFACE_SIDE || v_count > MAX_SURFACE_SIDE || points.iter().any(|r| r.len() != v_count) {
        return Err(MeshError("exact rational surface control grid is irregular or outside limits"));
    }
    knots(uknots.as_ref(), u_count, MAX_SURFACE_SIDE, budget)?;
    knots(vknots.as_ref(), v_count, MAX_SURFACE_SIDE, budget)?;
    budget.consume(u_count.checked_mul(v_count).ok_or(MeshError("exact surface work count overflow"))?)?;
    for p in points.iter().flatten() {
        rational_point(*p)?;
    }
    Ok(())
}

fn matrix_is_invertible(m: kernel::Matrix4) -> bool {
    let columns = [m.x, m.y, m.z, m.w];
    let mut rows = [[0.0; 4]; 4];
    let mut scale: f64 = 0.0;
    for (column_index, column) in columns.into_iter().enumerate() {
        for (row_index, value) in [column.x, column.y, column.z, column.w].into_iter().enumerate() {
            if !bounded_scalar(value, MAX_COORD) {
                return false;
            }
            scale = scale.max(value.abs());
            rows[row_index][column_index] = value;
        }
    }
    if scale <= 0.0 {
        return false;
    }
    for row in &mut rows {
        for value in row {
            *value /= scale;
        }
    }
    for col in 0..4 {
        let mut pivot = col;
        for row in col + 1..4 {
            if rows[row][col].abs() > rows[pivot][col].abs() {
                pivot = row;
            }
        }
        if !rows[pivot][col].is_finite() || rows[pivot][col].abs() < 1e-12 {
            return false;
        }
        rows.swap(col, pivot);
        let divisor = rows[col][col];
        for value in rows[col].iter_mut().skip(col) {
            *value /= divisor;
        }
        let pivot_row = rows[col];
        for row in rows.iter_mut().skip(col + 1) {
            let factor = row[col];
            for (value, pivot_value) in row.iter_mut().zip(pivot_row).skip(col) {
                *value -= factor * pivot_value;
            }
        }
    }
    true
}

/// Reject tolerances whose conservative curved-geometry work estimate exceeds the public
/// tessellation budget. This check is also applied immediately before calling truck-meshalgo.
pub(super) fn validate_tessellation_request(geometry: &CompressedSolid, tolerance: f64) -> Result<(), MeshError> {
    if !tolerance.is_finite() || !(1e-5..=1e9).contains(&tolerance) {
        return Err(MeshError("exact tessellation tolerance must be between 1e-5 and 1e9"));
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut curved_items = 0usize;
    let mut transform_scale: f64 = 1.0;
    for shell in &geometry.boundaries {
        for p in &shell.vertices {
            let values = [p.x, p.y, p.z];
            for axis in 0..3 {
                lo[axis] = lo[axis].min(values[axis]);
                hi[axis] = hi[axis].max(values[axis]);
            }
        }
        for edge in &shell.edges {
            if !matches!(&edge.curve, Curve::Line(_)) {
                curved_items = curved_items.saturating_add(1);
            }
        }
        for face in &shell.faces {
            match &face.surface {
                Surface::Plane(_) => {}
                Surface::BSplineSurface(_) | Surface::NurbsSurface(_) => curved_items = curved_items.saturating_add(1),
                Surface::RevolutedCurve(processor) => {
                    curved_items = curved_items.saturating_add(1);
                    let m = processor.transform();
                    let rows =
                        [m.x.x.abs() + m.y.x.abs() + m.z.x.abs(), m.x.y.abs() + m.y.y.abs() + m.z.y.abs(), m.x.z.abs() + m.y.z.abs() + m.z.z.abs()];
                    transform_scale = transform_scale.max(rows.into_iter().fold(0.0, f64::max));
                }
            }
        }
    }
    if curved_items == 0 {
        return Ok(());
    }
    let extent = (0..3).map(|axis| hi[axis] - lo[axis]).fold(0.0, f64::max) * transform_scale * 2.0;
    let minimum = extent * curved_items as f64 / MAX_GEOMETRY_ITEMS as f64;
    if !minimum.is_finite() || tolerance < minimum.max(1e-5) {
        return Err(MeshError("exact tessellation tolerance exceeds the bounded curved-geometry work budget"));
    }
    Ok(())
}

/// Validate every unique kernel geometry record without invoking kernel extraction/evaluation.
pub(super) fn validate_geometry(geometry: &CompressedSolid) -> Result<(), MeshError> {
    let mut stack = Vec::new();
    for shell in &geometry.boundaries {
        stack.extend(shell.edges.iter().map(|edge| Node::Curve(&edge.curve, 0)));
        stack.extend(shell.faces.iter().map(|face| Node::Surface(&face.surface, 0)));
    }
    let mut budget = Budget::new();
    let mut visited = 0usize;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > MAX_NESTED_NODES {
            return Err(MeshError("exact analytic geometry exceeds nested-item limit"));
        }
        match node {
            Node::Curve(curve, depth) => {
                if depth > MAX_NESTING {
                    return Err(MeshError("exact analytic curves/surfaces exceed nesting limit"));
                }
                match curve {
                    Curve::Line(line) => {
                        point(line.0)?;
                        point(line.1)?;
                    }
                    Curve::BSplineCurve(curve) => bspline_curve_points(curve.control_points(), curve.knot_vec().as_ref(), &mut budget)?,
                    Curve::NurbsCurve(curve) => nurbs_curve(curve, &mut budget)?,
                    // truck-modeling has explicit unimplemented branches when a face surface
                    // evaluates/includes an intersection curve. Reject it before extraction.
                    Curve::IntersectionCurve(_) => return Err(MeshError("exact intersection curves are not supported by safe kernel extraction")),
                }
            }
            Node::Surface(surface, depth) => {
                if depth > MAX_NESTING {
                    return Err(MeshError("exact analytic curves/surfaces exceed nesting limit"));
                }
                match surface {
                    Surface::Plane(plane) => {
                        point(plane.origin())?;
                        let u = plane.u_axis();
                        let v = plane.v_axis();
                        if !vector(u, MAX_COORD) || !vector(v, MAX_COORD) || !vector(u.cross(v), MAX_COORD * MAX_COORD) {
                            return Err(MeshError("exact plane basis is nonfinite or outside limits"));
                        }
                        let area = vector_length(u.cross(v));
                        if !area.is_finite() || area <= 1e-12 {
                            return Err(MeshError("exact plane basis is degenerate"));
                        }
                        point(plane.origin() + u)?;
                        point(plane.origin() + v)?;
                    }
                    Surface::BSplineSurface(surface) => {
                        let (uknots, vknots) = surface.knot_vecs();
                        bspline_surface_points(surface.control_points(), uknots.as_ref(), vknots.as_ref(), &mut budget)?;
                    }
                    Surface::NurbsSurface(surface) => nurbs_surface(surface, &mut budget)?,
                    Surface::RevolutedCurve(processor) => {
                        if !matrix_is_invertible(*processor.transform()) {
                            return Err(MeshError("exact revolved surface transform must be finite and invertible"));
                        }
                        let revolved = processor.entity();
                        let origin = revolved.origin();
                        let axis = revolved.axis();
                        point(origin)?;
                        let axis_length = vector_length(axis);
                        if !vector(axis, MAX_COORD) || !axis_length.is_finite() || axis_length <= 1e-12 {
                            return Err(MeshError("exact revolution axis is nonfinite or degenerate"));
                        }
                        stack.push(Node::Curve(revolved.entity_curve(), depth + 1));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_body() -> super::super::Body {
        super::super::Body::box_solid(crate::Vec3::ZERO, crate::Vec3::new(1.0, 1.0, 1.0)).unwrap()
    }

    fn malformed_nurbs(knots: Vec<f64>, weights: [f64; 2]) -> Curve {
        let spline = kernel::BSplineCurve::new_unchecked(
            kernel::KnotVec::from(knots),
            vec![kernel::Vector4::new(0.0, 0.0, 0.0, weights[0]), kernel::Vector4::new(1.0, 0.0, 0.0, weights[1])],
        );
        Curve::NurbsCurve(kernel::NurbsCurve::new(spline))
    }

    #[test]
    fn malformed_rational_curves_are_rejected_before_kernel_extraction() {
        let mut zero_weight = box_body();
        zero_weight.geometry.boundaries[0].edges[0].curve = malformed_nurbs(vec![0.0, 0.0, 1.0, 1.0], [1.0, 0.0]);
        assert!(zero_weight.validate().is_err());

        // KnotVec::from sorts its input; excessive multiplicity remains invalid after sorting.
        let mut repeated_knots = box_body();
        repeated_knots.geometry.boundaries[0].edges[0].curve = malformed_nurbs(vec![0.0, 0.0, 0.0, 1.0], [1.0, 1.0]);
        assert!(repeated_knots.validate().is_err());
    }

    #[test]
    fn singular_revolved_surface_transform_is_rejected_without_kernel_evaluation() {
        use kernel::Transformed;

        let mut body = box_body();
        let meridian = Curve::Line(kernel::Line(kernel::Point3::new(1.0, 0.0, 0.0), kernel::Point3::new(1.0, 0.0, 1.0)));
        let revolved = kernel::RevolutedCurve::by_revolution(meridian, kernel::Point3::new(0.0, 0.0, 0.0), kernel::Vector3::new(0.0, 0.0, 1.0));
        let mut processor = kernel::Processor::<_, kernel::Matrix4>::new(revolved);
        processor.transform_by(kernel::Matrix4::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        body.geometry.boundaries[0].faces[0].surface = Surface::RevolutedCurve(processor);
        assert!(body.validate().is_err());
    }

    #[test]
    fn curved_tessellation_rejects_a_work_exceeding_tolerance_before_meshing() {
        let body = super::super::Body::torus(crate::Vec3::ZERO, 4.0, 1.0).unwrap();
        assert!(body.tessellate(1e-5).is_err());
        assert!(body.tessellate(0.1).is_ok());
    }
}
