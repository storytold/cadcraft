//! 3D projected ViewCube geometry, compass ring, and hit-testing (faces, edges, corners).
//!
//! Renders an AutoCAD-style interactive 3D ViewCube that rotates live with [`cadcraft_render::Orbit3d`]
//! (`yaw` and `pitch`), tilts the compass ring (`N`, `E`, `S`, `W`) in 3D space, highlights hovered
//! sub-regions (6 faces, 12 edges, 8 corners), and computes target `(yaw_deg, pitch_deg)` camera
//! orientations when any face, edge, or corner is clicked.

use cadcraft_geom::Vec3;
use cadcraft_render::Orbit3d;
use egui::{Color32, Pos2, Shape, Stroke, epaint::PathShape, pos2, vec2};

/// A clickable sub-region on the 3D ViewCube surface (6 faces, 12 edges, 8 corners), identified by
/// its outward normal sign triple `(nx, ny, nz)` where each component is in `{-1, 0, 1}` (not all zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CubeZone {
    pub nx: i8,
    pub ny: i8,
    pub nz: i8,
}

impl CubeZone {
    /// Construct a zone from signs `(-1..=1)`.
    pub const fn new(nx: i8, ny: i8, nz: i8) -> Self {
        Self { nx, ny, nz }
    }

    /// Number of non-zero components: `1` = face, `2` = edge, `3` = corner.
    pub const fn rank(self) -> u8 {
        (self.nx != 0) as u8 + (self.ny != 0) as u8 + (self.nz != 0) as u8
    }

    /// Outward direction vector in world space.
    pub fn direction(self) -> Vec3 {
        Vec3::new(f64::from(self.nx), f64::from(self.ny), f64::from(self.nz)).normalized()
    }

    /// Target `(yaw_deg, pitch_deg)` for `3dorbit` that aligns the camera to look from `self.direction()`
    /// toward the origin.
    pub fn target_angles_deg(self, _current_yaw_deg: f64) -> (f64, f64) {
        let dir = self.direction();
        // Camera looks along:
        // dir.x = -sin(yaw) * sin(pitch)
        // dir.y = -cos(yaw) * sin(pitch)
        // dir.z =  cos(pitch)
        let pitch_rad = dir.z.clamp(-1.0, 1.0).acos();
        let pitch_deg = pitch_rad.to_degrees();
        let xy_len = dir.x.hypot(dir.y);
        let yaw_deg = if xy_len < 1e-6 {
            // Looking straight at Top (0,0,1) or Bottom (0,0,-1): keep +X pointing right (`yaw = 0°`).
            0.0
        } else {
            (-dir.x).atan2(-dir.y).to_degrees()
        };
        (yaw_deg, pitch_deg)
    }

    /// Human-readable tooltip describing the hovered face, edge, or corner.
    pub fn tooltip(self) -> String {
        match (self.nx, self.ny, self.nz) {
            (0, 0, 1) => "Top view (click to align, drag to orbit)".to_string(),
            (0, 0, -1) => "Bottom view (click to align, drag to orbit)".to_string(),
            (0, -1, 0) => "Front view (click to align, drag to orbit)".to_string(),
            (0, 1, 0) => "Back view (click to align, drag to orbit)".to_string(),
            (-1, 0, 0) => "Left view (click to align, drag to orbit)".to_string(),
            (1, 0, 0) => "Right view (click to align, drag to orbit)".to_string(),
            _ => {
                let mut parts = Vec::with_capacity(3);
                if self.nz > 0 {
                    parts.push("Top");
                } else if self.nz < 0 {
                    parts.push("Bottom");
                }
                if self.ny < 0 {
                    parts.push("Front");
                } else if self.ny > 0 {
                    parts.push("Back");
                }
                if self.nx < 0 {
                    parts.push("Left");
                } else if self.nx > 0 {
                    parts.push("Right");
                }
                format!("{} view (click to align, drag to orbit)", parts.join("-"))
            }
        }
    }
}

/// Definition of one of the 6 primary cube faces in local cube coordinates `[-1, 1]^3`.
#[derive(Clone, Copy)]
struct FaceDef {
    normal: (i8, i8, i8),
    /// Local `+u` tangent axis (rightward when looking at the face).
    u_axis: Vec3,
    /// Local `+v` tangent axis (upward when looking at the face).
    v_axis: Vec3,
    /// Unlocalized face key (`"TOP"`, `"BOTTOM"`, `"FRONT"`, `"BACK"`, `"LEFT"`, `"RIGHT"`).
    key: &'static str,
}

const FACES: [FaceDef; 6] = [
    FaceDef { normal: (0, 0, 1), u_axis: Vec3::new(1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, 1.0, 0.0), key: "TOP" },
    FaceDef { normal: (0, 0, -1), u_axis: Vec3::new(1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, -1.0, 0.0), key: "BOTTOM" },
    FaceDef { normal: (0, -1, 0), u_axis: Vec3::new(1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "FRONT" },
    FaceDef { normal: (0, 1, 0), u_axis: Vec3::new(-1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "BACK" },
    FaceDef { normal: (-1, 0, 0), u_axis: Vec3::new(0.0, -1.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "LEFT" },
    FaceDef { normal: (1, 0, 0), u_axis: Vec3::new(0.0, 1.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "RIGHT" },
];

impl FaceDef {
    /// Outward unit normal vector of this cube face.
    fn n_vec(self) -> Vec3 {
        Vec3::new(f64::from(self.normal.0), f64::from(self.normal.1), f64::from(self.normal.2))
    }

    /// Map surface parameters `(u, v)` in `[-1, 1]^2` to a 3D point on the unit cube `[-1, 1]^3`.
    fn point(self, u: f64, v: f64) -> Vec3 {
        self.n_vec() + self.u_axis * u + self.v_axis * v
    }

    /// Compute the [`CubeZone`] for a point `(u, v)` in `[-1, 1]^2` on this face.
    /// The outer margin (`|u| > C` or `|v| > C`) selects the adjacent edge or corner.
    fn zone_at(self, u: f64, v: f64) -> CubeZone {
        const BAND: f64 = 0.56;
        let du = if u < -BAND {
            -1.0
        } else if u > BAND {
            1.0
        } else {
            0.0
        };
        let dv = if v < -BAND {
            -1.0
        } else if v > BAND {
            1.0
        } else {
            0.0
        };
        let raw = self.n_vec() + self.u_axis * du + self.v_axis * dv;
        CubeZone::new(raw.x.round() as i8, raw.y.round() as i8, raw.z.round() as i8)
    }
}

/// Camera projection helper for the ViewCube widget.
#[derive(Clone, Copy)]
pub struct CubeProjector {
    pub center: Pos2,
    pub half_size: f32,
    pub orbit: Orbit3d,
}

impl CubeProjector {
    /// Create a projector centered at `center` with cube half-extent `half_size` in screen pixels.
    pub fn new(center: Pos2, half_size: f32, orbit: Orbit3d) -> Self {
        Self { center, half_size, orbit }
    }

    /// Outward camera direction vector in world space (pointing from target toward the viewer).
    pub fn view_dir(&self) -> Vec3 {
        let (sy, cy) = self.orbit.yaw.sin_cos();
        let (sp, cp) = self.orbit.pitch.sin_cos();
        Vec3::new(-sy * sp, -cy * sp, cp)
    }

    /// Project a 3D point in unit-cube coordinates (`[-1, 1]^3`) to screen coordinates (`Pos2`)
    /// and depth (`z_cam`, positive toward viewer).
    pub fn project_unit(&self, p: Vec3) -> (Pos2, f64) {
        let v = self.orbit.project_vec(p);
        let depth = p.dot(self.view_dir());
        let s = f64::from(self.half_size);
        let screen = pos2(self.center.x + (v.x * s) as f32, self.center.y - (v.y * s) as f32);
        (screen, depth)
    }

    /// Project a 3D point on the compass base plane (`z = -1.0` in unit-cube space when tilted, or
    /// `z = 0.0` in 2D plan view) at pixel radius `r` around `self.center`.
    pub fn project_ring_point(&self, world_xy: Vec3, radius: f32) -> Pos2 {
        let (sy, cy) = self.orbit.yaw.sin_cos();
        let (sp, cp) = self.orbit.pitch.sin_cos();
        let eff_cp = if cp >= -0.15 { cp.max(0.28) } else { cp.min(-0.28) };
        // In 3D views the compass sits on the bottom plane of the cube (`z = -1.0`), so its center
        // shifts down by `sp * half_size` and the cube stands on top of the compass disc.
        let z_plane = -1.0_f64;
        let x1 = world_xy.x * cy - world_xy.y * sy;
        let y_ring = (world_xy.x * sy + world_xy.y * cy) * eff_cp;
        let y_z = z_plane * sp * f64::from(self.half_size);
        let r = f64::from(radius);
        pos2(self.center.x + (x1 * r) as f32, self.center.y - (y_ring * r + y_z) as f32)
    }

    /// Determine which [`CubeZone`] (face, edge, or corner) is under screen position `pos`.
    pub fn zone_at(&self, pos: Pos2) -> Option<CubeZone> {
        let view_dir = self.view_dir();
        let mut best: Option<(f64, CubeZone)> = None;
        for face in FACES {
            let ndot = face.n_vec().dot(view_dir);
            if ndot <= 1e-3 {
                continue;
            }
            let (p00, _) = self.project_unit(face.point(-1.0, -1.0));
            let (p10, _) = self.project_unit(face.point(1.0, -1.0));
            let (p01, _) = self.project_unit(face.point(-1.0, 1.0));
            if let Some((u, v)) = inverse_parallelogram(pos, p00, p10, p01)
                && (-1.0..=1.0).contains(&u)
                && (-1.0..=1.0).contains(&v)
            {
                let p3d = face.point(u, v);
                let depth = p3d.dot(view_dir);
                if best.is_none_or(|(best_d, _)| depth > best_d) {
                    best = Some((depth, face.zone_at(u, v)));
                }
            }
        }
        best.map(|(_, z)| z)
    }

    /// Check whether `pos` lies on the tilted 3D compass ring at `ring_radius`.
    pub fn on_compass_ring(&self, pos: Pos2, ring_radius: f32) -> bool {
        const STEPS: usize = 48;
        for i in 0..STEPS {
            let a0 = (i as f64) * std::f64::consts::TAU / (STEPS as f64);
            let a1 = ((i + 1) as f64) * std::f64::consts::TAU / (STEPS as f64);
            let p0 = self.project_ring_point(Vec3::new(a0.cos(), a0.sin(), 0.0), ring_radius);
            let p1 = self.project_ring_point(Vec3::new(a1.cos(), a1.sin(), 0.0), ring_radius);
            if dist_to_segment(pos, p0, p1) <= 9.0 {
                return true;
            }
        }
        false
    }

    /// Paint the 3D tilted compass ring (`N`, `E`, `S`, `W`), visible cube faces, dashed sub-zone
    /// boundaries, hovered face/edge/corner highlight, and face labels.
    pub fn paint(&self, painter: &egui::Painter, ring_radius: f32, hovered_ring: bool, hovered_zone: Option<CubeZone>) {
        self.paint_compass_ring(painter, ring_radius, hovered_ring);
        self.paint_cube(painter, hovered_zone);
    }

    /// Paint the 3D compass disc and its localized cardinal labels (`N`, `E`, `S`, `W`).
    fn paint_compass_ring(&self, painter: &egui::Painter, ring_radius: f32, hovered_ring: bool) {
        const STEPS: usize = 64;
        let ring_col = if hovered_ring { Color32::from_rgb(0x5c, 0x67, 0x7c) } else { Color32::from_rgb(0x46, 0x4e, 0x5a) };
        let rim_col = Color32::from_rgb(0x64, 0x6d, 0x7a);
        let r_inner = ring_radius - 5.5;
        let r_outer = ring_radius + 5.5;

        // Render the compass ring as a true 3D flat annulus on the base plane (`z = -1.0`), so its
        // thickness foreshortens naturally at the front/back and stays wider at the left/right sides.
        let mut ring_mesh = egui::Mesh::default();
        let mut pts_inner = Vec::with_capacity(STEPS);
        let mut pts_outer = Vec::with_capacity(STEPS);
        for i in 0..STEPS {
            let a = (i as f64) * std::f64::consts::TAU / (STEPS as f64);
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            let pi = self.project_ring_point(dir, r_inner);
            let po = self.project_ring_point(dir, r_outer);
            pts_inner.push(pi);
            pts_outer.push(po);
            ring_mesh.colored_vertex(pi, ring_col);
            ring_mesh.colored_vertex(po, ring_col);
        }
        for i in 0..STEPS {
            let next = (i + 1) % STEPS;
            let i0 = (2 * i) as u32;
            let i1 = (2 * i + 1) as u32;
            let n0 = (2 * next) as u32;
            let n1 = (2 * next + 1) as u32;
            ring_mesh.add_triangle(i0, i1, n1);
            ring_mesh.add_triangle(i0, n1, n0);
        }
        painter.add(Shape::mesh(ring_mesh));
        painter.add(Shape::Path(PathShape::closed_line(pts_inner, Stroke::new(1.0, rim_col))));
        painter.add(Shape::Path(PathShape::closed_line(pts_outer, Stroke::new(1.0, rim_col))));

        // Render the cardinal letters (`N`, `E`/`O`, `S`/`Z`, `W`) as 3D plane-transformed
        // font TextShapes on the compass plane right on the ring, supporting all 17 languages.
        let lc = Color32::from_rgb(0xe2, 0xe6, 0xec);
        let right_dir = self.project_ring_point(Vec3::new(1.0, 0.0, 0.0), 1.0) - self.project_ring_point(Vec3::ZERO, 1.0);
        let down_dir = self.project_ring_point(Vec3::new(0.0, -1.0, 0.0), 1.0) - self.project_ring_point(Vec3::ZERO, 1.0);
        for (dir, label) in [
            (Vec3::new(0.0, 1.0, 0.0), crate::tl!("N")),
            (Vec3::new(0.0, -1.0, 0.0), crate::tl!("S")),
            (Vec3::new(1.0, 0.0, 0.0), crate::tl!("E")),
            (Vec3::new(-1.0, 0.0, 0.0), crate::tl!("W")),
        ] {
            let pos = self.project_ring_point(dir, ring_radius);
            paint_plane_text(painter, pos, label, 15.0, right_dir, down_dir, 24.0, lc);
        }
    }

    /// Paint the visible faces of the 3D ViewCube, subdivision lines, hover highlights, and face labels.
    fn paint_cube(&self, painter: &egui::Painter, hovered_zone: Option<CubeZone>) {
        let view_dir = self.view_dir();
        let light_dir = (view_dir + Vec3::new(-0.25, -0.15, 0.45)).normalized();

        let mut visible: Vec<(f64, FaceDef)> = FACES
            .into_iter()
            .filter_map(|f| {
                let ndot = f.n_vec().dot(view_dir);
                (ndot > 1e-3).then_some((ndot, f))
            })
            .collect();
        // Painter's algorithm: back-to-front among the up to 3 visible faces.
        visible.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        for (ndot, face) in visible {
            let p00 = self.project_unit(face.point(-1.0, -1.0)).0;
            let p10 = self.project_unit(face.point(1.0, -1.0)).0;
            let p11 = self.project_unit(face.point(1.0, 1.0)).0;
            let p01 = self.project_unit(face.point(-1.0, 1.0)).0;

            let shade = (0.52 + 0.36 * face.n_vec().dot(light_dir).clamp(0.0, 1.0)) as f32;
            let base_r = (182.0 * shade).round() as u8;
            let base_g = (187.0 * shade).round() as u8;
            let base_b = (195.0 * shade).round() as u8;
            let face_zone = CubeZone::new(face.normal.0, face.normal.1, face.normal.2);
            let fill = if hovered_zone == Some(face_zone) {
                Color32::from_rgb(base_r.saturating_add(28), base_g.saturating_add(30), base_b.saturating_add(34))
            } else {
                Color32::from_rgb(base_r, base_g, base_b)
            };

            painter.add(Shape::convex_polygon(
                vec![p00, p10, p11, p01],
                fill,
                Stroke::new(1.1, Color32::from_rgb(0x4b, 0x50, 0x58)),
            ));

            // Draw AutoCAD-style dashed subdivision lines along the edges/corners of visible faces.
            self.paint_face_subdivisions(painter, face);

            // Highlight hovered edge or corner sub-patch on this face.
            if let Some(hz) = hovered_zone
                && hz.rank() >= 2
            {
                self.paint_zone_highlight_on_face(painter, face, hz);
            }

            // Render the localized face label using 3D plane-transformed TextShapes so it is
            // crisp, bold, skewed with the face parallelogram, upside-down from behind, and
            // supports all 17 interface languages (including Greek, Cyrillic, CJK, Hangul, Arabic).
            if ndot > 0.16 {
                self.paint_face_label(painter, face, ndot);
            }
        }
    }

    /// Paint the localized, 3D plane-transformed label centered on `face`.
    fn paint_face_label(&self, painter: &egui::Painter, face: FaceDef, _ndot: f64) {
        let label = localized_face_label(face.key).to_uppercase();
        let center_2d = self.project_unit(face.point(0.0, 0.0)).0;
        // Screen-space tangent vectors corresponding to one unit in `u` (right) and `-v` (down) on the face.
        let right = (self.project_unit(face.point(1.0, 0.0)).0 - center_2d) / self.half_size;
        let down = (self.project_unit(face.point(0.0, -1.0)).0 - center_2d) / self.half_size;
        let max_width = self.half_size * 1.55;
        paint_plane_text(painter, center_2d, &label, 13.0, right, down, max_width, Color32::from_rgb(0x1f, 0x24, 0x2b));
    }

    /// Paint the dashed inner edge/corner subdivision lines on `face`.
    fn paint_face_subdivisions(&self, painter: &egui::Painter, face: FaceDef) {
        const B: f64 = 0.56;
        const SEGMENTS: usize = 7;
        let st = Stroke::new(0.85, Color32::from_rgba_unmultiplied(0x38, 0x3d, 0x45, 135));
        for c in [-B, B] {
            // Dashed line along v from -1 to 1 at u = c, and along u from -1 to 1 at v = c.
            for k in (0..SEGMENTS).step_by(2) {
                let t0 = -1.0 + 2.0 * (k as f64) / (SEGMENTS as f64);
                let t1 = -1.0 + 2.0 * ((k + 1) as f64) / (SEGMENTS as f64);
                let a = self.project_unit(face.point(c, t0)).0;
                let b = self.project_unit(face.point(c, t1)).0;
                painter.line_segment([a, b], st);
                let c0 = self.project_unit(face.point(t0, c)).0;
                let c1 = self.project_unit(face.point(t1, c)).0;
                painter.line_segment([c0, c1], st);
            }
        }
    }

    /// Highlight the sub-patch of `face` belonging to the hovered edge or corner `zone`.
    fn paint_zone_highlight_on_face(&self, painter: &egui::Painter, face: FaceDef, zone: CubeZone) {
        const B: f64 = 0.56;
        let zv = Vec3::new(f64::from(zone.nx), f64::from(zone.ny), f64::from(zone.nz));
        // Only highlight if this face is one of the faces participating in `zone`.
        if (face.n_vec().dot(zv) - 1.0).abs() > 1e-4 {
            return;
        }
        let du = face.u_axis.dot(zv).round() as i8;
        let dv = face.v_axis.dot(zv).round() as i8;
        let (u0, u1) = match du {
            -1 => (-1.0, -B),
            0 => (-B, B),
            1 => (B, 1.0),
            _ => return,
        };
        let (v0, v1) = match dv {
            -1 => (-1.0, -B),
            0 => (-B, B),
            1 => (B, 1.0),
            _ => return,
        };
        let q00 = self.project_unit(face.point(u0, v0)).0;
        let q10 = self.project_unit(face.point(u1, v0)).0;
        let q11 = self.project_unit(face.point(u1, v1)).0;
        let q01 = self.project_unit(face.point(u0, v1)).0;
        painter.add(Shape::convex_polygon(
            vec![q00, q10, q11, q01],
            Color32::from_rgba_unmultiplied(0x6e, 0xa8, 0xfe, 155),
            Stroke::new(1.0, Color32::from_rgb(0x3b, 0x82, 0xf6)),
        ));
    }
}

/// Solve for `(u, v)` in `[-1, 1]^2` given a 2D point `p` inside the projected parallelogram
/// with corners `p00 = (-1, -1)`, `p10 = (1, -1)`, and `p01 = (-1, 1)`.
fn inverse_parallelogram(p: Pos2, p00: Pos2, p10: Pos2, p01: Pos2) -> Option<(f64, f64)> {
    let eu = p10 - p00;
    let ev = p01 - p00;
    let det = f64::from(eu.x) * f64::from(ev.y) - f64::from(eu.y) * f64::from(ev.x);
    if det.abs() < 1e-4 {
        return None;
    }
    let rel = p - p00;
    let s = (f64::from(rel.x) * f64::from(ev.y) - f64::from(rel.y) * f64::from(ev.x)) / det;
    let t = (f64::from(eu.x) * f64::from(rel.y) - f64::from(eu.y) * f64::from(rel.x)) / det;
    Some((s * 2.0 - 1.0, t * 2.0 - 1.0))
}

/// Euclidean distance from `p` to the 2D line segment `a..=b`.
fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= 1e-6 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + vec2(ab.x * t, ab.y * t))
}

/// Transform the existing font galley in the 3D face/compass plane while keeping it inside a
/// [`egui::epaint::TextShape`] so `egui` populates and normalizes the font-atlas UVs at end-of-frame
/// for every Unicode script (Latin, Greek, Cyrillic, Arabic, Hangul, CJK).
#[allow(clippy::too_many_arguments)]
fn paint_plane_text(
    painter: &egui::Painter,
    at: Pos2,
    label: &str,
    font_size: f32,
    right: egui::Vec2,
    down: egui::Vec2,
    max_width: f32,
    ink: Color32,
) {
    let base_galley = painter.layout_no_wrap(label.to_owned(), egui::FontId::proportional(font_size), ink);
    if base_galley.is_empty() {
        return;
    }
    let center = base_galley.rect.center().to_vec2();
    let fit = (max_width / base_galley.size().x.max(1.0)).min(1.0);
    // Two slight horizontal offsets (`-0.35`, `0.35`) produce a crisp bold face label.
    for dx_bold in [-0.35_f32, 0.35_f32] {
        let mut galley = (*base_galley).clone();
        let mut bounds = egui::Rect::NOTHING;
        for placed in &mut galley.rows {
            let row = std::sync::Arc::make_mut(&mut placed.row);
            for v in &mut row.visuals.mesh.vertices {
                let q = (v.pos.to_vec2() + placed.pos.to_vec2() - center) * fit + vec2(dx_bold, 0.0);
                v.pos = (right * q.x + down * q.y).to_pos2();
            }
            placed.pos = Pos2::ZERO;
            row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
            bounds = bounds.union(row.visuals.mesh_bounds);
        }
        galley.mesh_bounds = bounds;
        galley.rect = bounds;
        painter.add(Shape::Text(egui::epaint::TextShape::new(at, std::sync::Arc::new(galley), ink)));
    }
}

/// Localized ViewCube face label across all 18 interface languages (`TOP`, `BOTTOM`, `FRONT`,
/// `BACK`, `LEFT`, `RIGHT`). Uses catalog entries when present and built-in fallback translations
/// if a catalog omits one of the 3D face names.
fn localized_face_label(key: &'static str) -> &'static str {
    match key {
        "TOP" => crate::tl!("TOP"),
        "LEFT" => {
            let s = crate::tl!("Left");
            if s != "Left" { s } else { face_fallback("LEFT") }
        }
        "RIGHT" => {
            let s = crate::tl!("Right");
            if s != "Right" { s } else { face_fallback("RIGHT") }
        }
        "FRONT" => {
            let s = crate::tl!("Front");
            if s != "Front" { s } else { face_fallback("FRONT") }
        }
        "BACK" => {
            let s = crate::tl!("Back");
            if s != "Back" { s } else { face_fallback("BACK") }
        }
        "BOTTOM" => {
            let s = crate::tl!("Bottom");
            if s != "Bottom" { s } else { face_fallback("BOTTOM") }
        }
        other => other,
    }
}

/// Built-in localized fallback table for the 6 ViewCube face labels keyed by the active language's
/// `"TOP"` translation.
fn face_fallback(key: &'static str) -> &'static str {
    let lang = crate::i18n::t("TOP");
    match (lang, key) {
        // Dutch (`TOP` -> `BOVEN`)
        ("BOVEN", "FRONT") => "VOOR",
        ("BOVEN", "BACK") => "ACHTER",
        ("BOVEN", "BOTTOM") => "ONDER",
        ("BOVEN", "LEFT") => "LINKS",
        ("BOVEN", "RIGHT") => "RECHTS",
        // German (`TOP` -> `OBEN`)
        ("OBEN", "FRONT") => "VORNE",
        ("OBEN", "BACK") => "HINTEN",
        ("OBEN", "BOTTOM") => "UNTEN",
        // French (`TOP` -> `HAUT`)
        ("HAUT", "FRONT") => "AVANT",
        ("HAUT", "BACK") => "ARRIÈRE",
        ("HAUT", "BOTTOM") => "BAS",
        // Spanish (`TOP` -> `SUP.`)
        ("SUP.", "FRONT") => "FRENTE",
        ("SUP.", "BACK") => "POST.",
        ("SUP.", "BOTTOM") => "INF.",
        ("SUP.", "LEFT") => "IZQ.",
        ("SUP.", "RIGHT") => "DER.",
        // Brazilian Portuguese (`TOP` -> `SUPERIOR`)
        ("SUPERIOR", "FRONT") => "FRENTE",
        ("SUPERIOR", "BACK") => "POSTERIOR",
        ("SUPERIOR", "BOTTOM") => "INFERIOR",
        // Italian (`TOP` -> `ALTO`)
        ("ALTO", "FRONT") => "FRONTE",
        ("ALTO", "BACK") => "RETRO",
        ("ALTO", "BOTTOM") => "BASSO",
        // Swedish (`TOP` -> `OVAN`)
        ("OVAN", "FRONT") => "FRAM",
        ("OVAN", "BACK") => "BAK",
        ("OVAN", "BOTTOM") => "UNDER",
        // Czech (`TOP` -> `SHORA`)
        ("SHORA", "FRONT") => "VPŘEDU",
        ("SHORA", "BACK") => "VZADU",
        ("SHORA", "BOTTOM") => "ZDOLA",
        // Hungarian (`TOP` -> `FELÜL`)
        ("FELÜL", "FRONT") => "ELÖL",
        ("FELÜL", "BACK") => "HÁTUL",
        ("FELÜL", "BOTTOM") => "ALUL",
        // Turkish (`TOP` -> `ÜST`)
        ("ÜST", "FRONT") => "ÖN",
        ("ÜST", "BACK") => "ARKA",
        ("ÜST", "BOTTOM") => "ALT",
        // Greek (`TOP` -> `ΚΑΤΟΨΗ`)
        ("ΚΑΤΟΨΗ", "FRONT") => "ΠΡΟΣΟΨΗ",
        ("ΚΑΤΟΨΗ", "BACK") => "ΠΙΣΩ",
        ("ΚΑΤΟΨΗ", "BOTTOM") => "ΚΑΤΩ",
        // Ukrainian (`TOP` -> `ЗВЕРХУ`)
        ("ЗВЕРХУ", "FRONT") => "СПЕРЕДУ",
        ("ЗВЕРХУ", "BACK") => "ЗЗАДУ",
        ("ЗВЕРХУ", "BOTTOM") => "ЗНИЗУ",
        // Russian (`TOP` -> `СВЕРХУ`)
        ("СВЕРХУ", "FRONT") => "СПЕРЕДИ",
        ("СВЕРХУ", "BACK") => "СЗАДИ",
        ("СВЕРХУ", "BOTTOM") => "СНИЗУ",
        // Chinese (`TOP` -> `俯视`)
        ("俯视", "FRONT") => "前视",
        ("俯视", "BACK") => "后视",
        ("俯视", "BOTTOM") => "仰视",
        // Korean (`TOP` -> `평면`)
        ("평면", "FRONT") => "정면",
        ("평면", "BACK") => "배면",
        ("평면", "BOTTOM") => "저면",
        // Arabic (`TOP` -> `أعلى`)
        ("أعلى", "FRONT") => "أمام",
        ("أعلى", "BACK") => "خلف",
        ("أعلى", "BOTTOM") => "أسفل",
        _ => key,
    }
}
