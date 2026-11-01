//! Bounded orthographic depth buffer for shaded model-space geometry.
use crate::{
    raster::View,
    view3d::{Camera3d, Projector3d, Render3dOptions, VisualStyle3d},
};
use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2, Vec3};

#[derive(Clone, Copy, Debug)]
struct Vertex {
    p: Vec2,
    z: f64,
}
#[derive(Clone, Debug)]
struct Triangle {
    v: [Vertex; 3],
    color: Rgb,
    handle: Handle,
}
#[derive(Clone, Debug)]
struct Segment {
    v: [Vertex; 2],
    color: Rgb,
    handle: Handle,
}
/// Geometry projected into an orthographic camera, retaining depth for rasterization and picks.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    triangles: Vec<Triangle>,
    segments: Vec<Segment>,
}
/// Pixel colours and visible entity handles. Empty pixels are transparent.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub handles: Vec<Option<Handle>>,
}

fn shade_color(base: Rgb, normal: Vec3, center: Vec3, projector: Projector3d, options: Render3dOptions) -> Rgb {
    if options.style == VisualStyle3d::Flat {
        return base;
    }
    let light = options.light_direction.normalized();
    let diffuse = normal.dot(light).max(0.0);
    let mut specular = 0.0;
    if options.style == VisualStyle3d::Realistic {
        let halfway = (light + projector.view_direction(center)).normalized();
        specular = options.specular * normal.dot(halfway).max(0.0).powf(options.shininess);
    }
    let factor = (options.ambient + options.light_intensity * diffuse).clamp(0.0, 4.0);
    let channel = |v: u8| (f64::from(v) * factor + 255.0 * specular).round().clamp(0.0, 255.0) as u8;
    Rgb(channel(base.0), channel(base.1), channel(base.2))
}

impl Image {
    pub fn pick(&self, x: u32, y: u32) -> Option<Handle> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.handles.get((y as usize).checked_mul(self.width as usize)?.checked_add(x as usize)?).copied().flatten()
    }
}
impl Scene {
    pub fn new(d: &Drawing, camera: Camera3d) -> Self {
        Self::new_with_options(d, camera, Render3dOptions::default())
    }

    pub fn new_with_options(d: &Drawing, camera: Camera3d, options: Render3dOptions) -> Self {
        Self::new_with_render_options(d, camera, options, &crate::Options::default())
    }

    /// Build with the same tessellation settings as the display list so perspective target and
    /// screen coordinates stay aligned for every curve detail level.
    pub fn new_with_render_options(d: &Drawing, camera: Camera3d, options: Render3dOptions, render_options: &crate::Options) -> Self {
        let mut scene = Self::default();
        if !camera.azimuth.is_finite() || !camera.elevation.is_finite() || !options.is_valid() {
            return scene;
        }
        // Build once, then reuse its bounds for the camera and its ordinary planar primitives.
        let list = crate::build(d, &cadcraft_doc::Space::Model, render_options);
        let projector = Projector3d::with_planar_bounds(d, camera, options, list.bounds);
        let project = |p| {
            let (p, z) = projector.project(p);
            Vertex { p, z }
        };
        // Existing planar drawing entities live at Z=0 in model space. Include their
        // tessellated primitives in the same depth test as explicit 3D geometry.
        let handles: std::collections::HashSet<_> = d
            .model
            .iter()
            .filter(|e| matches!(&e.kind, EntityKind::Face3d(_) | EntityKind::Polyline3d(_) | EntityKind::Line(_) | EntityKind::Point(_)))
            .map(|e| e.handle)
            .collect();
        for prim in &list.prims {
            if handles.contains(&prim.handle) {
                continue;
            }
            let points = list.points(prim);
            let color = prim.display_rgb(Rgb(33, 40, 48));
            let p3 = |p: Vec2| project(Vec3::new(p.x, p.y, 0.0));
            match prim.kind {
                crate::Kind::Tris => {
                    for tri in points.chunks_exact(3) {
                        if let [a, b, c] = tri {
                            let v = [p3(*a), p3(*b), p3(*c)];
                            if v.iter().all(|p| p.p.is_finite() && p.z.is_finite()) {
                                scene.triangles.push(Triangle { v, color, handle: prim.handle });
                            }
                        }
                    }
                }
                crate::Kind::Polyline => {
                    for pair in points.windows(2) {
                        if let [a, b] = pair {
                            let v = [p3(*a), p3(*b)];
                            if v.iter().all(|p| p.p.is_finite() && p.z.is_finite()) {
                                scene.segments.push(Segment { v, color, handle: prim.handle });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for e in d.model.iter() {
            if d.is_hidden_object(e.handle) {
                continue;
            }
            let ctx = crate::pipeline::top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]);
            let (ink, _, _, _, visible, _) = crate::props::resolve(&ctx, e, false);
            if !visible {
                continue;
            }
            match &e.kind {
                EntityKind::Face3d(f) if f.corners.iter().all(|p| p.is_finite()) => {
                    for inds in [[0, 1, 2], [0, 2, 3]] {
                        let [a, b, c] = inds.map(|i| f.corners[i]);
                        let n = (b - a).cross(c - a).normalized();
                        if n == Vec3::ZERO {
                            continue;
                        }
                        let center = (a + b + c) * (1.0 / 3.0);
                        let base = options.material_color.unwrap_or(ink.rgb);
                        let color = shade_color(base, n, center, projector, options);
                        let v = [a, b, c].map(project);
                        if v.iter().all(|p| p.p.is_finite() && p.z.is_finite()) && options.style != VisualStyle3d::Wireframe {
                            scene.triangles.push(Triangle { v, color, handle: e.handle });
                        }
                    }
                    if options.style == VisualStyle3d::Wireframe {
                        scene.add_wireframe(f, e.handle, projector, options, ink.rgb);
                    }
                }
                EntityKind::Line(l) if l.a.is_finite() && l.b.is_finite() => {
                    let v = [project(l.a), project(l.b)];
                    if v.iter().all(|p| p.p.is_finite() && p.z.is_finite()) {
                        scene.segments.push(Segment { v, color: ink.rgb, handle: e.handle });
                    }
                }
                EntityKind::Point(p) if p.p.is_finite() => {
                    let point = project(p.p);
                    if point.p.is_finite() && point.z.is_finite() {
                        scene.segments.push(Segment { v: [point, point], color: ink.rgb, handle: e.handle });
                    }
                }
                EntityKind::Polyline3d(p) => {
                    let mut add = |a: Vec3, b: Vec3| {
                        if a.is_finite() && b.is_finite() {
                            let v = [project(a), project(b)];
                            if v.iter().all(|p| p.p.is_finite() && p.z.is_finite()) {
                                scene.segments.push(Segment { v, color: ink.rgb, handle: e.handle });
                            }
                        }
                    };
                    for pair in p.points.windows(2) {
                        if let [a, b] = pair {
                            add(*a, *b);
                        }
                    }
                    if p.closed
                        && let (Some(a), Some(b)) = (p.points.last(), p.points.first())
                    {
                        add(*a, *b);
                    }
                }
                _ => {}
            }
        }
        scene
    }
    fn add_wireframe(&mut self, face: &cadcraft_doc::Face3d, handle: Handle, projector: Projector3d, options: Render3dOptions, ink: Rgb) {
        let project = |p| {
            let (p, z) = projector.project(p);
            Vertex { p, z }
        };
        for (a, b, bit) in [(0usize, 1usize, 1u8), (1, 2, 2), (2, 3, 4), (3, 0, 8)] {
            if face.hidden_edges & bit != 0 {
                continue;
            }
            let (Some(a), Some(b)) = (face.corners.get(a).copied(), face.corners.get(b).copied()) else { continue };
            if a == b {
                continue;
            }
            let v = [project(a), project(b)];
            if v.iter().all(|p| p.p.is_finite() && p.z.is_finite()) {
                self.segments.push(Segment { v, color: options.material_color.unwrap_or(ink), handle });
            }
        }
    }
    /// Render with a per-pixel depth test. Dimensions are limited to bound memory usage.
    pub fn render(&self, view: &View) -> Option<Image> {
        let (w, h) = (view.width, view.height);
        if w == 0 || h == 0 || w > 2048 || h > 2048 || !view.scale.is_finite() || view.scale <= 0.0 {
            return None;
        }
        let count = (w as usize).checked_mul(h as usize)?;
        let mut out = Image { width: w, height: h, rgba: vec![0; count.checked_mul(4)?], handles: vec![None; count] };
        let mut depth = vec![f64::NEG_INFINITY; count];
        let mut put = |x: u32, y: u32, z: f64, color: Rgb, handle: Handle| {
            let i = y as usize * w as usize + x as usize;
            if let (Some(old), Some(pixel), Some(hit)) = (depth.get_mut(i), out.rgba.get_mut(i * 4..i * 4 + 4), out.handles.get_mut(i))
                && z >= *old
            {
                *old = z;
                pixel.copy_from_slice(&[color.0, color.1, color.2, 255]);
                *hit = Some(handle);
            }
        };
        for tri in &self.triangles {
            let v = tri.v.map(|p| {
                let (x, y) = view.to_screen(p.p);
                (f64::from(x), f64::from(y), p.z)
            });
            if !v.iter().all(|p| p.0.is_finite() && p.1.is_finite() && p.2.is_finite()) {
                continue;
            }
            let edge = |a: (f64, f64, f64), b: (f64, f64, f64), x: f64, y: f64| (x - a.0) * (b.1 - a.1) - (y - a.1) * (b.0 - a.0);
            let area = edge(v[0], v[1], v[2].0, v[2].1);
            if area.abs() < 1e-12 {
                continue;
            }
            let minx = v.iter().map(|p| p.0).fold(f64::INFINITY, f64::min).floor().clamp(0.0, f64::from(w - 1)) as u32;
            let maxx = v.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max).ceil().clamp(0.0, f64::from(w - 1)) as u32;
            let miny = v.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).floor().clamp(0.0, f64::from(h - 1)) as u32;
            let maxy = v.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).ceil().clamp(0.0, f64::from(h - 1)) as u32;
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (x0, y0) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    let a = edge(v[1], v[2], x0, y0) / area;
                    let b = edge(v[2], v[0], x0, y0) / area;
                    let c = 1.0 - a - b;
                    if a >= -1e-10 && b >= -1e-10 && c >= -1e-10 {
                        put(x, y, a * v[0].2 + b * v[1].2 + c * v[2].2, tri.color, tri.handle);
                    }
                }
            }
        }
        for line in &self.segments {
            let (ax, ay) = view.to_screen(line.v[0].p);
            let (bx, by) = view.to_screen(line.v[1].p);
            if ![ax, ay, bx, by].iter().all(|n| n.is_finite()) {
                continue;
            }
            // Clip before stepping, avoiding unbounded loops on hostile coordinates.
            let rect = cadcraft_geom::Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(f64::from(w - 1), f64::from(h - 1)));
            let a = Vec2::new(f64::from(ax), f64::from(ay));
            let b = Vec2::new(f64::from(bx), f64::from(by));
            let Some((start, end)) = crate::clip::clip_segment(a, b, &rect) else { continue };
            let delta = b - a;
            let len2 = delta.dot(delta);
            let param = |p: Vec2| if len2 > 0.0 { (p - a).dot(delta) / len2 } else { 0.0 };
            let (t0, t1) = (param(start), param(end));
            let steps = (end.x - start.x).abs().max((end.y - start.y).abs()).ceil() as u32;
            for i in 0..=steps {
                let k = if steps == 0 { 0.0 } else { f64::from(i) / f64::from(steps) };
                let p = start + (end - start) * k;
                let t = t0 + (t1 - t0) * k;
                put(p.x.round() as u32, p.y.round() as u32, line.v[0].z + (line.v[1].z - line.v[0].z) * t, line.color, line.handle);
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(x: f64, y: f64, z: f64) -> Vertex {
        Vertex { p: Vec2::new(x, y), z }
    }
    #[test]
    fn intersecting_faces_and_hidden_line_pick_the_visible_surface() {
        let points = [(-1.0, -1.0), (1.0, -1.0), (0.0, 1.0)];
        let one = Triangle { v: points.map(|(x, y)| v(x, y, x)), color: Rgb(220, 40, 40), handle: Handle(1) };
        let two = Triangle { v: points.map(|(x, y)| v(x, y, -x)), color: Rgb(40, 100, 220), handle: Handle(2) };
        let line = Segment { v: [v(-0.8, 0.0, -2.0), v(0.8, 0.0, -2.0)], color: Rgb(0, 255, 0), handle: Handle(3) };
        let scene = Scene { triangles: vec![one, two], segments: vec![line] };
        let view = View { center: Vec2::ZERO, scale: 100.0, width: 240, height: 240 };
        let image = scene.render(&view).expect("image missing");
        assert_eq!(image.pick(95, 120), Some(Handle(2)));
        assert_eq!(image.pick(145, 120), Some(Handle(1)));
        assert_eq!(image.pick(120, 120), Some(Handle(1)));
    }
    #[test]
    fn intersecting_boxes_render_after_orbit() -> Result<(), cadcraft_doc::DocError> {
        let mut d = Drawing::new_imperial();
        let faces = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
        for (origin, color) in [(Vec3::new(-0.8, -0.8, -0.8), Rgb(240, 90, 60)), (Vec3::new(0.0, 0.0, 0.0), Rgb(60, 150, 240))] {
            let v = [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(2.0, 2.0, 0.0),
                Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(0.0, 0.0, 2.0),
                Vec3::new(2.0, 0.0, 2.0),
                Vec3::new(2.0, 2.0, 2.0),
                Vec3::new(0.0, 2.0, 2.0),
            ]
            .map(|p| p + origin);
            let common = cadcraft_doc::Common { color: cadcraft_color::Color::True(color), ..Default::default() };
            for f in faces {
                d.add(
                    &cadcraft_doc::Space::Model,
                    common.clone(),
                    EntityKind::Face3d(cadcraft_doc::Face3d { corners: f.map(|i| v[i]), hidden_edges: 0 }),
                )?;
            }
        }
        let view = View { center: Vec2::new(0.0, 0.3), scale: 100.0, width: 600, height: 480 };
        let camera = Camera3d { azimuth: -0.8, elevation: 0.5 };
        let rendered = Scene::new(&d, camera).render(&view);
        assert!(rendered.is_some());
        let Some(image) = rendered else { return Ok(()) };
        assert!(image.handles.iter().flatten().count() > 1000);
        let rotated = Scene::new(&d, Camera3d { azimuth: 0.3, elevation: 0.5 }).render(&view);
        assert!(rotated.is_some_and(|other| other.rgba != image.rgba));
        if let Ok(path) = std::env::var("CADCRAFT_DEPTH_DEMO")
            && let Some(size) = tiny_skia::IntSize::from_wh(image.width, image.height)
            && let Some(pm) = tiny_skia::Pixmap::from_vec(image.rgba, size)
        {
            let _ = pm.save_png(path);
        }
        Ok(())
    }
}
