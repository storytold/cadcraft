//! CADCraft geometry: double-precision 2D/3D math for drafting.
//!
//! Everything a CAD kernel needs below the document model: vectors and affine transforms,
//! bounding boxes, angles, lines, circular arcs (and polyline bulges), ellipses, NURBS splines,
//! intersections, closest points, offsets and tessellation. All angles are radians,
//! counter-clockwise from +X, as in DXF.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bounds;
mod curve;
mod fillet;
mod intersect;
mod mat;
mod nurbs;
pub mod region;
mod spline;
mod spline_fit;
mod vec;

pub use bounds::Bounds2;
pub use curve::{
    Arc, Circle, Ellipse, Line, PolyVertex, Polyline, Segment, arc_to_bulge, bulge_to_arc, point_in_polygon, polygon_centroid, shoelace,
};
pub use fillet::{CornerFit, TanCurve, tangent_circles};
pub use intersect::{circle_circle, intersect_ext, intersect_segments, line_circle, line_line, line_line_infinite};
pub use mat::{Mat3, Mat4};
pub use spline::{MAX_DEGREE as MAX_SPLINE_DEGREE, Spline};
pub use spline_fit::{FitOptions, KnotParam};
pub use vec::{Vec2, Vec3};

/// Geometric tolerance for coincidence tests in drawing units.
pub const EPS: f64 = 1e-9;
pub const TAU: f64 = std::f64::consts::TAU;
pub const PI: f64 = std::f64::consts::PI;

/// Normalize an angle to `[0, 2π)`.
pub fn norm_angle(a: f64) -> f64 {
    if !a.is_finite() {
        return 0.0;
    }
    let r = a.rem_euclid(TAU);
    if r >= TAU { 0.0 } else { r }
}

/// Counter-clockwise sweep from `start` to `end`, in `(0, 2π]`.
pub fn ccw_sweep(start: f64, end: f64) -> f64 {
    let s = norm_angle(end - start);
    if s <= EPS { TAU } else { s }
}

/// True when angle `a` lies on the CCW sweep from `start` to `end` (inclusive, with tolerance).
pub fn angle_in_sweep(a: f64, start: f64, end: f64) -> bool {
    let sweep = ccw_sweep(start, end);
    let d = norm_angle(a - start);
    d <= sweep + 1e-12 || (TAU - d) < 1e-12
}

pub fn deg(r: f64) -> f64 {
    r.to_degrees()
}
pub fn rad(d: f64) -> f64 {
    d.to_radians()
}

/// Number of segments needed to approximate an arc of `radius` and `sweep` within `tol`.
pub fn arc_segments(radius: f64, sweep: f64, tol: f64) -> usize {
    let r = radius.abs();
    if !(r.is_finite() && sweep.is_finite()) || r <= EPS {
        return 1;
    }
    let tol = tol.max(r * 1e-6).min(r);
    // chord error e = r(1 - cos(θ/2)) → θ = 2 acos(1 - e/r)
    let step = 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos();
    let n = if step > 1e-6 { (sweep.abs() / step).ceil() } else { 4096.0 };
    (n as usize).clamp(1, 4096)
}

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;
