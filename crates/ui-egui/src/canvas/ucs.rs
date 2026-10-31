//! 3D point picking in the active UCS plane.

use cadcraft_doc::{Drawing, EntityKind, Space};
#[cfg(test)]
use cadcraft_engine::View3d;
use cadcraft_engine::{snap::mode, ucs::Frame};
use cadcraft_geom::{Vec2, Vec3};
use egui::{Color32, Pos2, Stroke};

use super::Xf;

const MAX_SNAP_ENTITIES: usize = 100_000;
const MAX_SNAP_PRIMITIVES: usize = 100_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct Snap3 {
    pub point: Vec3,
    pub projected: Vec2,
    pub screen: Pos2,
    pub mode: u32,
}

#[derive(Clone, Debug)]
pub(super) struct Snap3Index {
    vertices: Vec<Vec3>,
    centers: Vec<Vec3>,
    edges: Vec<(Vec3, Vec3)>,
}

impl Snap3Index {
    pub fn build(d: &Drawing, space: &Space) -> Option<Self> {
        let store = d.space(space)?;
        if store.len() > MAX_SNAP_ENTITIES {
            return None;
        }
        let mut out = Self { vertices: Vec::new(), centers: Vec::new(), edges: Vec::new() };
        for entity in store.iter() {
            if !d.is_visible(entity) {
                continue;
            }
            match &entity.kind {
                EntityKind::Line(line) => {
                    out.vertices.extend([line.a, line.b]);
                    out.edges.push((line.a, line.b));
                }
                EntityKind::Point(point) => out.vertices.push(point.p),
                EntityKind::Polyline3d(poly) => {
                    if poly.points.len() > MAX_SNAP_PRIMITIVES.saturating_sub(out.vertices.len() + out.edges.len() * 2) / 2 {
                        return None;
                    }
                    out.vertices.extend(poly.points.iter().copied());
                    out.edges.extend(poly.points.windows(2).map(|w| (w[0], w[1])));
                    if poly.closed && poly.points.len() > 2 {
                        out.edges.push((*poly.points.last()?, poly.points[0]));
                    }
                }
                EntityKind::Face3d(face) => {
                    let mut corners = face.corners.to_vec();
                    if corners[2] == corners[3] {
                        corners.pop();
                    }
                    out.vertices.extend(corners.iter().copied());
                    for i in 0..corners.len() {
                        out.edges.push((corners[i], corners[(i + 1) % corners.len()]));
                    }
                    if !corners.is_empty() {
                        out.centers.push(corners.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) * (1.0 / corners.len() as f64));
                    }
                }
                EntityKind::Solid(solid) | EntityKind::Trace(solid) => {
                    let corners = solid.corners;
                    out.vertices.extend(corners);
                    for i in 0..4 {
                        out.edges.push((corners[i], corners[(i + 1) % 4]));
                    }
                    out.centers.push(corners.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) * 0.25);
                }
                _ => {}
            }
            if out.vertices.len() + out.edges.len() * 2 + out.centers.len() > MAX_SNAP_PRIMITIVES {
                return None;
            }
        }
        Some(out)
    }
}

/// Intersect the camera ray through a projected canvas point with the active UCS XY plane.
/// `projected` is the camera-projected coordinate from the canvas transform. A parallel ray is
/// refused so a point is never silently flattened onto WCS Z=0.
#[cfg(test)]
pub(super) fn point_on_ucs_plane(projected: Vec2, view: View3d, frame: Frame) -> Option<Vec3> {
    if !projected.is_finite() {
        return None;
    }
    let (sa, ca) = view.azimuth.sin_cos();
    let (se, ce) = view.elevation.sin_cos();
    let right = Vec3::new(-sa, ca, 0.0);
    let up = Vec3::new(-se * ca, -se * sa, ce);
    let toward = Vec3::new(ce * ca, ce * sa, se);
    let ray_origin = right * projected.x + up * projected.y;
    intersect_plane(ray_origin, toward, frame)
}

pub(super) fn intersect_plane(ray_origin: Vec3, ray_direction: Vec3, frame: Frame) -> Option<Vec3> {
    if !ray_origin.is_finite() || !ray_direction.is_finite() {
        return None;
    }
    let denominator = frame.z.dot(ray_direction);
    if !denominator.is_finite() || denominator.abs() <= 1e-10 {
        return None;
    }
    let distance = frame.z.dot(frame.origin - ray_origin) / denominator;
    let point = ray_origin + ray_direction * distance;
    (distance.is_finite() && point.is_finite()).then_some(point)
}

/// Best running 3D object snap near the pointer. Vertices, segment midpoints, face centers and
/// nearest points on straight 3D edges keep their actual WCS elevation. The bounded scan covers
/// explicitly 3D primitives and returns no snap for oversized spaces.
pub(super) fn snap3d(
    index: &Snap3Index,
    pointer: Pos2,
    xf: &Xf,
    projector: cadcraft_render::view3d::Projector3d,
    perspective: bool,
    osmode: u32,
    aperture: f32,
) -> Option<Snap3> {
    if osmode & mode::OFF != 0 || osmode & (mode::END | mode::MID | mode::CEN | mode::NEA) == 0 || !pointer.x.is_finite() || !pointer.y.is_finite() {
        return None;
    }
    let mut best: Option<(f32, u8, Snap3)> = None;
    if osmode & mode::END != 0 {
        for p in &index.vertices {
            consider(*p, mode::END, pointer, aperture, xf, projector, &mut best);
        }
    }
    if osmode & mode::CEN != 0 {
        for center in &index.centers {
            consider(*center, mode::CEN, pointer, aperture, xf, projector, &mut best);
        }
    }
    for (a, b) in &index.edges {
        let (pa, fa) = projector.project(*a);
        let (pb, fb) = projector.project(*b);
        if !(pa.is_finite() && pb.is_finite()) {
            continue;
        }
        if osmode & mode::MID != 0 {
            consider((*a + *b) * 0.5, mode::MID, pointer, aperture, xf, projector, &mut best);
        }
        if osmode & mode::NEA != 0 {
            let sa = xf.to_screen(pa);
            let sb = xf.to_screen(pb);
            let delta = sb - sa;
            let length2 = delta.length_sq();
            if length2 > 1e-12 {
                let screen_t = ((pointer - sa).dot(delta) / length2).clamp(0.0, 1.0) as f64;
                let t = if perspective {
                    let denominator = (1.0 - screen_t) * fb + screen_t * fa;
                    if !denominator.is_finite() || denominator.abs() <= 1e-12 {
                        continue;
                    }
                    screen_t * fa / denominator
                } else {
                    screen_t
                };
                consider(*a + (*b - *a) * t, mode::NEA, pointer, aperture, xf, projector, &mut best);
            }
        }
    }
    best.map(|(_, _, snap)| snap)
}

fn consider(
    point: Vec3,
    snap_mode: u32,
    pointer: Pos2,
    aperture: f32,
    xf: &Xf,
    projector: cadcraft_render::view3d::Projector3d,
    best: &mut Option<(f32, u8, Snap3)>,
) {
    if !point.is_finite() {
        return;
    }
    let projected = projector.project(point).0;
    if !projected.is_finite() {
        return;
    }
    let screen = xf.to_screen(projected);
    let distance2 = pointer.distance_sq(screen);
    if !distance2.is_finite() || distance2 > aperture * aperture {
        return;
    }
    let priority = match snap_mode {
        mode::END => 0,
        mode::MID => 1,
        mode::CEN => 2,
        _ => 3,
    };
    if best.as_ref().is_some_and(|(d, p, _)| distance2 > *d || (distance2 == *d && priority >= *p)) {
        return;
    }
    *best = Some((distance2, priority, Snap3 { point, projected, screen, mode: snap_mode }));
}

pub(super) fn draw_snap3_marker(painter: &egui::Painter, snap: Option<Snap3>) {
    let Some(snap) = snap else { return };
    let color = crate::theme::Tokens::get().snap;
    let stroke = Stroke::new(2.0, color);
    let r = 5.0;
    match snap.mode {
        mode::END => {
            painter.rect_stroke(egui::Rect::from_center_size(snap.screen, egui::vec2(r * 2.0, r * 2.0)), 0.0, stroke, egui::StrokeKind::Middle);
        }
        mode::MID => {
            painter.add(egui::Shape::closed_line(
                vec![snap.screen + egui::vec2(0.0, -r), snap.screen + egui::vec2(r, r), snap.screen + egui::vec2(-r, r)],
                stroke,
            ));
        }
        mode::CEN => {
            painter.circle_stroke(snap.screen, r, stroke);
        }
        _ => {
            painter.line_segment([snap.screen + egui::vec2(-r, -r), snap.screen + egui::vec2(r, r)], stroke);
            painter.line_segment([snap.screen + egui::vec2(r, -r), snap.screen + egui::vec2(-r, r)], stroke);
        }
    }
    let label = match snap.mode {
        mode::END => "3D Endpoint",
        mode::MID => "3D Midpoint",
        mode::CEN => "Face Center",
        _ => "Nearest Edge",
    };
    painter.text(snap.screen + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, label, crate::theme::small(), Color32::BLACK);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, EntityKind, Line};

    #[test]
    fn orthographic_ray_intersects_tilted_ucs_and_refuses_parallel_plane() {
        let view = View3d { azimuth: -0.6, elevation: 0.5, orbiting: false };
        let frame = Frame::from_axes(Vec3::new(3.0, 4.0, 2.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0)).unwrap();
        let p = frame.to_world(Vec3::new(2.0, -1.0, 0.0));
        let (projected, _) = cadcraft_render::view3d::Camera3d { azimuth: view.azimuth, elevation: view.elevation }.project(p);
        let hit = point_on_ucs_plane(projected, view, frame).unwrap();
        assert!((hit - p).len() < 1e-9);

        let vertical_view = View3d { azimuth: 0.0, elevation: 0.0, orbiting: false };
        // With a horizontal camera ray, a WCS XY plane is parallel to the ray. A vertical
        // plane would be perpendicular here and correctly produce an intersection.
        assert!(point_on_ucs_plane(Vec2::ZERO, vertical_view, Frame::WORLD).is_none());
    }

    #[test]
    fn snap3d_returns_the_actual_elevated_endpoint_and_nearest_edge_point() {
        let mut drawing = Drawing::default();
        let space = Space::Model;
        let a = Vec3::new(0.0, 0.0, 12.0);
        let b = Vec3::new(10.0, 0.0, 22.0);
        drawing.add(&space, Common::default(), EntityKind::Line(Line { a, b })).unwrap();
        let xf = Xf { rect: egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 200.0)), center: Vec2::ZERO, scale: 10.0 };
        let view = View3d { azimuth: -std::f64::consts::FRAC_PI_2, elevation: std::f64::consts::FRAC_PI_2, orbiting: false };
        let projector = cadcraft_render::view3d::Projector3d::new(
            &drawing,
            cadcraft_render::view3d::Camera3d { azimuth: view.azimuth, elevation: view.elevation },
            cadcraft_render::Render3dOptions::default(),
        );
        let index = Snap3Index::build(&drawing, &space).unwrap();
        let endpoint_screen = xf.to_screen(projector.project(a).0);
        let endpoint = snap3d(&index, endpoint_screen, &xf, projector, false, mode::END, 12.0).unwrap();
        assert_eq!(endpoint.point, a);
        let nearest_screen = xf.to_screen(projector.project(Vec3::new(4.0, 0.7, 16.0)).0);
        let nearest = snap3d(&index, nearest_screen, &xf, projector, false, mode::NEA, 12.0).unwrap();
        // Pointer positions and the screen interpolation parameter are f32. Bound the
        // world-space error by that precision rather than requiring sub-pixel f64 accuracy.
        let tolerance = f64::from(f32::EPSILON) * (b - a).len();
        assert!((nearest.point - Vec3::new(4.0, 0.0, 16.0)).len() < tolerance, "nearest 3D edge snap returned {:?}", nearest.point);
    }

    #[test]
    fn projector_ray_recovers_points_in_perspective_and_orthographic_views() {
        let drawing = Drawing::default();
        let camera = cadcraft_render::view3d::Camera3d { azimuth: -0.4, elevation: 0.55 };
        let frame = Frame::WORLD;
        let p = Vec3::new(0.3, 0.2, 0.0);
        for projection in [cadcraft_render::Projection3d::Perspective, cadcraft_render::Projection3d::Orthographic] {
            let options = cadcraft_render::Render3dOptions { projection, ..Default::default() };
            let projector = cadcraft_render::view3d::Projector3d::new(&drawing, camera, options);
            let projected = projector.project(p).0;
            let (origin, direction) = projector.ray(projected).unwrap();
            assert!((intersect_plane(origin, direction, frame).unwrap() - p).len() < 1e-8);
        }
    }
}
