//! Projection and surface options for the model-space 3D view.
use crate::{DPrim, DisplayList, Kind, Options};
use cadcraft_doc::{Drawing, EntityKind, Space};
use cadcraft_geom::{Bounds2, Mat3, Vec2, Vec3};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Projection3d {
    Orthographic,
    Perspective,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VisualStyle3d {
    Shaded,
    Flat,
    Realistic,
    Wireframe,
}

/// Session-level projection, lighting and material controls for model-space rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Render3dOptions {
    pub projection: Projection3d,
    pub field_of_view: f64,
    pub style: VisualStyle3d,
    /// Direction from a surface toward the directional light.
    pub light_direction: Vec3,
    pub light_intensity: f64,
    pub ambient: f64,
    pub specular: f64,
    pub shininess: f64,
    /// Optional material override; `None` keeps each entity's resolved drawing color.
    pub material_color: Option<cadcraft_color::Rgb>,
}

impl Default for Render3dOptions {
    fn default() -> Self {
        Self {
            projection: Projection3d::Orthographic,
            field_of_view: 50.0,
            style: VisualStyle3d::Shaded,
            light_direction: Vec3::new(0.4, -0.5, 0.75).normalized(),
            light_intensity: 0.65,
            ambient: 0.35,
            specular: 0.18,
            shininess: 32.0,
            material_color: None,
        }
    }
}

impl Render3dOptions {
    pub fn fingerprint(self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.projection.hash(&mut h);
        self.style.hash(&mut h);
        self.field_of_view.to_bits().hash(&mut h);
        self.light_direction.x.to_bits().hash(&mut h);
        self.light_direction.y.to_bits().hash(&mut h);
        self.light_direction.z.to_bits().hash(&mut h);
        self.light_intensity.to_bits().hash(&mut h);
        self.ambient.to_bits().hash(&mut h);
        self.specular.to_bits().hash(&mut h);
        self.shininess.to_bits().hash(&mut h);
        self.material_color.hash(&mut h);
        h.finish()
    }

    pub fn is_valid(self) -> bool {
        let light_length = self.light_direction.len();
        self.field_of_view.is_finite()
            && (10.0..=120.0).contains(&self.field_of_view)
            && self.light_direction.is_finite()
            && light_length.is_finite()
            && light_length > 1e-12
            && self.light_intensity.is_finite()
            && (0.0..=4.0).contains(&self.light_intensity)
            && self.ambient.is_finite()
            && (0.0..=1.0).contains(&self.ambient)
            && self.specular.is_finite()
            && (0.0..=1.0).contains(&self.specular)
            && self.shininess.is_finite()
            && (1.0..=256.0).contains(&self.shininess)
    }
}

/// Camera angles in radians. Azimuth is measured around Z and elevation above XY.
#[derive(Clone, Copy, Debug)]
pub struct Camera3d {
    pub azimuth: f64,
    pub elevation: f64,
}
impl Camera3d {
    /// Project a world point into view coordinates and signed depth (larger is nearer).
    pub fn project(self, p: Vec3) -> (Vec2, f64) {
        let (sa, ca) = self.azimuth.sin_cos();
        let (se, ce) = self.elevation.sin_cos();
        let right = Vec3::new(-sa, ca, 0.0);
        let up = Vec3::new(-se * ca, -se * sa, ce);
        let toward = Vec3::new(ce * ca, ce * sa, se);
        (Vec2::new(p.dot(right), p.dot(up)), p.dot(toward))
    }
}

/// A camera bound to drawing extents. Perspective uses a conservative auto-distance so all
/// finite drawing geometry remains in front of the camera.
#[derive(Clone, Copy, Debug)]
pub struct Projector3d {
    camera: Camera3d,
    options: Render3dOptions,
    target: Vec3,
    distance: f64,
}

impl Projector3d {
    pub fn new(d: &Drawing, camera: Camera3d, options: Render3dOptions) -> Self {
        let list = crate::build(d, &Space::Model, &crate::Options::default());
        Self::with_planar_bounds(d, camera, options, list.bounds)
    }

    /// Reuse the ordinary renderer's already-computed planar bounds when constructing a
    /// projector; this avoids a second tessellation pass in the 3D display and depth renderers.
    pub fn with_planar_bounds(d: &Drawing, camera: Camera3d, options: Render3dOptions, planar: Bounds2) -> Self {
        let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut add = |p: Vec3| {
            if p.is_finite() {
                lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            }
        };
        // Include ordinary planar geometry through the same tessellation bounds as the renderer.
        if !planar.is_empty() {
            add(Vec3::new(planar.min.x, planar.min.y, 0.0));
            add(Vec3::new(planar.max.x, planar.max.y, 0.0));
        }
        for e in d.model.iter() {
            match &e.kind {
                EntityKind::Face3d(f) => f.corners.iter().copied().for_each(&mut add),
                EntityKind::Line(l) => {
                    add(l.a);
                    add(l.b);
                }
                EntityKind::Point(p) => add(p.p),
                EntityKind::Polyline3d(p) => p.points.iter().copied().for_each(&mut add),
                _ => {}
            }
        }
        let finite_bounds = lo.is_finite() && hi.is_finite();
        let target = if finite_bounds { lo * 0.5 + hi * 0.5 } else { Vec3::ZERO };
        let radius = if finite_bounds { (hi - lo).len() * 0.5 } else { 1.0 }.max(1e-6);
        let half_fov = (options.field_of_view.clamp(10.0, 120.0).to_radians() * 0.5).sin().max(1e-6);
        Self {
            camera,
            options,
            target,
            distance: if options.projection == Projection3d::Perspective { radius / half_fov * 1.1 } else { f64::INFINITY },
        }
    }

    /// Rebind camera angles while retaining the drawing-dependent target and perspective fit.
    /// The target and distance depend on geometry/options, not azimuth or elevation.
    pub fn with_camera(mut self, camera: Camera3d) -> Self {
        self.camera = camera;
        self
    }

    pub fn project(self, p: Vec3) -> (Vec2, f64) {
        if self.options.projection == Projection3d::Orthographic {
            return self.camera.project(p);
        }
        let (mut xy, depth) = self.camera.project(p - self.target);
        let denominator = self.distance - depth;
        if !denominator.is_finite() || denominator <= self.distance * 1e-9 {
            return (Vec2::new(f64::NAN, f64::NAN), depth);
        }
        // Reciprocal camera depth is linear across a projected triangle. Returning the same
        // value used to scale the projected point lets the depth buffer interpolate correctly
        // in perspective mode, while retaining the old linear camera depth in orthographic mode.
        let factor = self.distance / denominator;
        xy = xy * factor;
        (xy, factor)
    }

    pub fn eye(self) -> Vec3 {
        let (se, ce) = self.camera.elevation.sin_cos();
        let (sa, ca) = self.camera.azimuth.sin_cos();
        self.target + Vec3::new(ce * ca, ce * sa, se) * self.distance
    }

    pub fn view_direction(self, p: Vec3) -> Vec3 {
        if self.options.projection == Projection3d::Perspective {
            (self.eye() - p).normalized()
        } else {
            let (se, ce) = self.camera.elevation.sin_cos();
            let (sa, ca) = self.camera.azimuth.sin_cos();
            Vec3::new(ce * ca, ce * sa, se)
        }
    }

    /// Return the world-space ray through a projected view point. Perspective rays begin at the
    /// camera eye; orthographic rays begin on the camera depth-zero plane. This is the inverse of
    /// [`Projector3d::project`] for the same camera fit and is suitable for model-space picking.
    pub fn ray(self, projected: Vec2) -> Option<(Vec3, Vec3)> {
        if !projected.is_finite() || !self.options.is_valid() || !self.camera.azimuth.is_finite() || !self.camera.elevation.is_finite() {
            return None;
        }
        let (sa, ca) = self.camera.azimuth.sin_cos();
        let (se, ce) = self.camera.elevation.sin_cos();
        let right = Vec3::new(-sa, ca, 0.0);
        let up = Vec3::new(-se * ca, -se * sa, ce);
        let toward = Vec3::new(ce * ca, ce * sa, se);
        let (origin, direction) = if self.options.projection == Projection3d::Perspective {
            let eye = self.eye();
            (eye, right * projected.x + up * projected.y - toward * self.distance)
        } else {
            (right * projected.x + up * projected.y, toward)
        };
        let length = direction.len();
        if !origin.is_finite() || !direction.is_finite() || !length.is_finite() || length <= 1e-12 {
            return None;
        }
        Some((origin, direction * (1.0 / length)))
    }
}

/// Build a projected list. Face triangles are painted back-to-front by mean depth.
/// This painter ordering handles ordinary non-intersecting solid faces; intersecting faces
/// can require a per-pixel depth buffer and are not split here.
pub fn build_3d(d: &Drawing, space: &Space, opts: &Options, camera: Camera3d) -> DisplayList {
    build_3d_with_options(d, space, opts, camera, Render3dOptions::default())
}

pub fn build_3d_with_options(d: &Drawing, space: &Space, opts: &Options, camera: Camera3d, render: Render3dOptions) -> DisplayList {
    if !camera.azimuth.is_finite() || !camera.elevation.is_finite() || !render.is_valid() || !matches!(space, Space::Model) {
        return crate::build(d, space, opts);
    }
    let mut list = crate::build(d, space, opts);
    let projector = Projector3d::with_planar_bounds(d, camera, render, list.bounds);
    let handles: std::collections::HashSet<_> = d
        .model
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Face3d(_) | EntityKind::Polyline3d(_) | EntityKind::Line(_) | EntityKind::Point(_)))
        .map(|e| e.handle)
        .collect();
    list.prims.retain(|p| !handles.contains(&p.handle));
    for p in list.verts.iter_mut().chain(list.tris.iter_mut()) {
        *p = projector.project(Vec3::new(p.x, p.y, 0.0)).0;
    }
    list.bounds = Bounds2::EMPTY;
    let mut bounds = Bounds2::EMPTY;
    for prim in &list.prims {
        for p in list.points(prim) {
            if p.is_finite() {
                bounds.add(*p);
            }
        }
    }
    list.bounds = bounds;
    let mut faces = Vec::new();
    let mut lines = Vec::new();
    for e in d.model.iter() {
        if d.is_hidden_object(e.handle) {
            continue;
        }
        let ctx = crate::pipeline::top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]);
        let (ink, lw, _, _, visible, _) = crate::props::resolve(&ctx, e, false);
        if !visible {
            continue;
        }
        match &e.kind {
            EntityKind::Face3d(f) if f.corners.iter().all(|p| p.is_finite()) => {
                if render.style == VisualStyle3d::Wireframe {
                    for (a, b, bit) in [(0usize, 1usize, 1u8), (1, 2, 2), (2, 3, 4), (3, 0, 8)] {
                        if f.hidden_edges & bit != 0 || f.corners[a] == f.corners[b] {
                            continue;
                        }
                        let pts = [f.corners[a], f.corners[b]].map(|p| projector.project(p).0);
                        if pts.iter().all(|p| p.is_finite()) {
                            lines.push((e.handle, ink, lw, pts.to_vec()));
                        }
                    }
                    continue;
                }
                for indices in [[0, 1, 2], [0, 2, 3]] {
                    let [a, b, c] = indices.map(|i| f.corners[i]);
                    let n = (b - a).cross(c - a).normalized();
                    if n == Vec3::ZERO {
                        continue;
                    }
                    let projected = [a, b, c].map(|p| projector.project(p));
                    if !projected.iter().all(|(p, z)| p.is_finite() && z.is_finite()) {
                        continue;
                    }
                    let shade = match render.style {
                        VisualStyle3d::Flat => 1.0,
                        _ => render.ambient + render.light_intensity * n.dot(render.light_direction.normalized()).max(0.0),
                    }
                    .clamp(0.0, 4.0);
                    let rgb = render.material_color.unwrap_or(ink.rgb);
                    let component = |v: u8| (f64::from(v) * shade).round() as u8;
                    let color = cadcraft_color::Rgb(component(rgb.0), component(rgb.1), component(rgb.2));
                    faces.push(((projected[0].1 + projected[1].1 + projected[2].1) / 3.0, e.handle, color, projected.map(|p| p.0)));
                }
            }
            EntityKind::Line(l) if l.a.is_finite() && l.b.is_finite() => {
                let pts = [l.a, l.b].map(|p| projector.project(p).0);
                if pts.iter().all(|p| p.is_finite()) {
                    lines.push((e.handle, ink, lw, pts.to_vec()));
                }
            }
            EntityKind::Point(p) if p.p.is_finite() => {
                let q = projector.project(p.p).0;
                if q.is_finite() {
                    lines.push((e.handle, ink, lw, vec![q, q]));
                }
            }
            EntityKind::Polyline3d(p) => {
                let mut pts: Vec<_> = p.points.iter().filter(|p| p.is_finite()).map(|p| projector.project(*p).0).filter(|p| p.is_finite()).collect();
                if p.closed
                    && let Some(first) = pts.first().copied()
                {
                    pts.push(first);
                }
                if pts.len() >= 2 {
                    lines.push((e.handle, ink, lw, pts));
                }
            }
            _ => {}
        }
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, handle, color, pts) in faces {
        let Ok(start) = u32::try_from(list.tris.len()) else { break };
        for p in pts {
            list.bounds.add(p);
            list.tris.push(p);
        }
        list.prims.push(DPrim { handle, color, aci7: false, lw: 0.0, transparency: 0, kind: Kind::Tris, start, len: 3 });
    }
    for (handle, ink, lw, pts) in lines {
        let (Ok(start), Ok(len)) = (u32::try_from(list.verts.len()), u32::try_from(pts.len())) else { break };
        for p in pts {
            list.bounds.add(p);
            list.verts.push(p);
        }
        list.prims.push(DPrim { handle, color: ink.rgb, aci7: ink.aci7, lw, transparency: 0, kind: Kind::Polyline, start, len });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orthographic_projection_uses_height_and_rotation() {
        let camera = Camera3d { azimuth: 0.0, elevation: 0.0 };
        let (xy, depth) = camera.project(Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(xy, Vec2::new(3.0, 4.0));
        assert_eq!(depth, 2.0);
        let rotated = Camera3d { azimuth: std::f64::consts::FRAC_PI_2, elevation: 0.0 };
        let (p, d) = rotated.project(Vec3::new(2.0, 3.0, 4.0));
        assert!((p.x + 2.0).abs() < 1e-12 && (d - 3.0).abs() < 1e-12);
    }

    #[test]
    fn perspective_projection_scales_near_points_and_keeps_them_in_front() {
        let drawing = Drawing::new_imperial();
        let camera = Camera3d { azimuth: 0.0, elevation: 0.0 };
        let options = Render3dOptions { projection: Projection3d::Perspective, ..Default::default() };
        let projector = Projector3d::new(&drawing, camera, options);
        let (near, near_depth) = projector.project(Vec3::new(0.5, 1.0, 0.0));
        let (far, far_depth) = projector.project(Vec3::new(-0.5, 1.0, 0.0));
        assert!(near.is_finite() && far.is_finite());
        assert!(near.x > far.x, "the nearer point has greater perspective magnification");
        assert!(near_depth > far_depth, "perspective depth key is nearer-is-larger");
    }
    #[test]
    fn projected_faces_sort_far_before_near() -> Result<(), cadcraft_doc::DocError> {
        let mut d = Drawing::new_imperial();
        let face = |x| {
            EntityKind::Face3d(cadcraft_doc::Face3d {
                corners: [Vec3::new(x, 0.0, 0.0), Vec3::new(x, 1.0, 0.0), Vec3::new(x, 0.0, 1.0), Vec3::new(x, 0.0, 1.0)],
                hidden_edges: 0,
            })
        };
        let near = d.add(&Space::Model, Default::default(), face(2.0))?;
        let far = d.add(&Space::Model, Default::default(), face(1.0))?;
        let list = build_3d(&d, &Space::Model, &Options::default(), Camera3d { azimuth: 0.0, elevation: 0.0 });
        let handles: Vec<_> = list.prims.iter().filter(|p| p.kind == Kind::Tris).map(|p| p.handle).collect();
        assert_eq!(handles, vec![far, near]);
        assert!((list.bounds.height() - 1.0).abs() < 1e-12);
        let wireframe = build_3d_with_options(
            &d,
            &Space::Model,
            &Options::default(),
            Camera3d { azimuth: 0.0, elevation: 0.0 },
            Render3dOptions { style: VisualStyle3d::Wireframe, ..Default::default() },
        );
        assert!(wireframe.prims.iter().all(|p| p.kind != Kind::Tris));
        assert!(wireframe.prims.iter().any(|p| p.kind == Kind::Polyline));
        Ok(())
    }
}
