//! 3D camera, orbit/roll/ray-plane math and beveled ViewCube geometry (ported from MATCAD).

use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3, rad};

/// Rotate vector `v` around unit `axis` by `angle_rad` radians (Rodrigues' formula).
pub fn rotate_vec_around_axis(v: Vec3, axis: Vec3, angle_rad: f64) -> Vec3 {
    let k = axis.normalized();
    if k == Vec3::ZERO || !angle_rad.is_finite() {
        return v;
    }
    let (s, c) = angle_rad.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// Intersect ray `ray_o + t * ray_d` with plane `(X - plane_pt) · plane_n = 0`.
pub fn intersect_ray_plane(ray_o: Vec3, ray_d: Vec3, plane_pt: Vec3, plane_n: Vec3) -> Option<Vec3> {
    let den = ray_d.dot(plane_n);
    if !den.is_finite() || den.abs() < 1e-12 {
        return None;
    }
    let t = (plane_pt - ray_o).dot(plane_n) / den;
    t.is_finite().then(|| ray_o + ray_d * t)
}

/// Orthonormal 3D camera frame (`x` right, `y` up, `z` forward from camera toward target).
///
/// In the default Top plan view, `x = (1, 0, 0)`, `y = (0, 1, 0)`, `z = (0, 0, -1)`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera3D {
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
    pub up: Vec3,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self::TOP
    }
}

impl Camera3D {
    pub const TOP: Camera3D = Camera3D {
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 1.0, 0.0),
        z: Vec3::new(0.0, 0.0, -1.0),
        up: Vec3::new(0.0, 1.0, 0.0),
    };

    /// Build a stabilized orthonormal camera frame from forward vector `z` (camera -> target)
    /// and desired `up` vector (ported from MATCAD `getCamParams.m`).
    pub fn from_dir_up(z: Vec3, up: Vec3) -> Self {
        let cam_z = z.normalized();
        if cam_z == Vec3::ZERO {
            return Self::TOP;
        }
        let mut cam_up = up.normalized();
        if cam_up == Vec3::ZERO {
            cam_up = if cam_z.z.abs() > 0.99 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::Z };
        }
        let mut cam_x = cam_z.cross(cam_up).normalized();
        if cam_x == Vec3::ZERO {
            let fallback = if cam_z.z.abs() > 0.9 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::Z };
            cam_x = cam_z.cross(fallback).normalized();
            if cam_x == Vec3::ZERO {
                return Self::TOP;
            }
        }
        let cam_y = cam_x.cross(cam_z).normalized();
        if cam_y == Vec3::ZERO {
            return Self::TOP;
        }
        Camera3D { x: cam_x, y: cam_y, z: cam_z, up: cam_y }
    }

    /// True when the camera is in the unrotated 2D Top plan view.
    pub fn is_top_plan(&self) -> bool {
        (self.z.x.abs() < 1e-6)
            && (self.z.y.abs() < 1e-6)
            && (self.z.z + 1.0).abs() < 1e-6
            && (self.x.x - 1.0).abs() < 1e-6
            && (self.y.y - 1.0).abs() < 1e-6
    }

    /// True when looking along one of the 6 principal world axes (+/-X, +/-Y, +/-Z).
    pub fn is_orthogonal(&self) -> bool {
        self.z.x.abs().max(self.z.y.abs()).max(self.z.z.abs()) > 0.999
    }

    /// Human-readable view classification name (`Top`, `Front`, `SW Isometric`, `3D`, etc.).
    pub fn view_name(&self) -> &'static str {
        let (ax, ay, az) = (self.z.x.abs(), self.z.y.abs(), self.z.z.abs());
        if az > 0.999 {
            return if self.z.z < 0.0 { "Top" } else { "Bottom" };
        }
        if ay > 0.999 {
            return if self.z.y > 0.0 { "Front" } else { "Back" };
        }
        if ax > 0.999 {
            return if self.z.x < 0.0 { "Right" } else { "Left" };
        }
        let iso = 1.0 / 3.0_f64.sqrt();
        if (ax - iso).abs() < 0.02 && (ay - iso).abs() < 0.02 && (az - iso).abs() < 0.02 && self.z.z < 0.0 {
            return match (self.z.x > 0.0, self.z.y > 0.0) {
                (true, true) => "SW Isometric",
                (false, true) => "SE Isometric",
                (false, false) => "NE Isometric",
                (true, false) => "NW Isometric",
            };
        }
        "3D"
    }

    /// Camera preset for any of the 26 ViewCube zones (faces, edges, corners) or standard names
    /// (ported from MATCAD `applyViewToNamedView.m`).
    pub fn preset(name: &str) -> Option<Self> {
        let n = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        // Matches MATCAD applyViewToNamedView.m: rotate canonical Top camera by (az, 90 - el).
        let (az_deg, el_deg): (f64, f64) = match n.as_str() {
            "top" | "plan" => (0.0, 90.0),
            "bottom" => (180.0, -90.0),
            "front" | "south" | "s" => (0.0, 0.0),
            "back" | "north" | "n" => (180.0, 0.0),
            "left" | "west" | "w" => (90.0, 0.0),
            "right" | "east" | "e" => (-90.0, 0.0),
            // 12 edges
            "front_top" | "top_front" => (0.0, 45.0),
            "front_bottom" | "bottom_front" => (0.0, -45.0),
            "front_right" => (-45.0, 0.0),
            "front_left" => (45.0, 0.0),
            "back_top" | "top_back" => (180.0, 45.0),
            "back_bottom" | "bottom_back" => (180.0, -45.0),
            "back_right" => (-135.0, 0.0),
            "back_left" => (135.0, 0.0),
            "right_top" | "top_right" => (-90.0, 45.0),
            "right_bottom" | "bottom_right" => (-90.0, -45.0),
            "left_top" | "top_left" => (90.0, 45.0),
            "left_bottom" | "bottom_left" => (90.0, -45.0),
            // 8 corners / isometrics
            "top_front_right" | "se_isometric" | "iso_se" | "seiso" | "home" | "iso" => (-45.0, 45.0),
            "top_front_left" | "sw_isometric" | "iso_sw" | "swiso" => (45.0, 45.0),
            "top_back_right" | "ne_isometric" | "iso_ne" | "neiso" => (-135.0, 45.0),
            "top_back_left" | "nw_isometric" | "iso_nw" | "nwiso" => (135.0, 45.0),
            "bottom_front_right" => (-45.0, -45.0),
            "bottom_front_left" => (45.0, -45.0),
            "bottom_back_right" => (-135.0, -45.0),
            "bottom_back_left" => (135.0, -45.0),
            _ => return None,
        };
        Some(Self::TOP.orbit(az_deg, 90.0 - el_deg, true))
    }

    /// Neighbor orthogonal view when clicking one of the 4 directional triangles (`top`, `bottom`, `left`, `right`)
    /// around the ViewCube in an orthogonal main view (ported from MATCAD `start_viewcube.m`).
    pub fn orthogonal_neighbor(&self, dir: &'static str) -> Self {
        let target = match (self.view_name(), dir) {
            ("Top", "top") => "back",
            ("Top", "bottom") => "front",
            ("Top", "left") => "left",
            ("Top", "right") => "right",
            ("Bottom", "top") => "back",
            ("Bottom", "bottom") => "front",
            ("Bottom", "left") => "right",
            ("Bottom", "right") => "left",
            ("Front", "top") => "top",
            ("Front", "bottom") => "bottom",
            ("Front", "left") => "left",
            ("Front", "right") => "right",
            ("Back", "top") => "top",
            ("Back", "bottom") => "bottom",
            ("Back", "left") => "right",
            ("Back", "right") => "left",
            ("Left", "top") => "top",
            ("Left", "bottom") => "bottom",
            ("Left", "left") => "back",
            ("Left", "right") => "front",
            ("Right", "top") => "top",
            ("Right", "bottom") => "bottom",
            ("Right", "left") => "front",
            ("Right", "right") => "back",
            _ => "top",
        };
        Self::preset(target).unwrap_or(*self)
    }

    /// 3D orbit rotation by `daz_deg` (yaw about world Z) and `del_deg` (pitch about camera X),
    /// with optional 2° axis clamping (ported from MATCAD `rotating3D.m`).
    ///
    /// Keeps `x` strictly in the world XY plane (`x.z == 0`) so turntable rotation never
    /// introduces camera roll / gimbal ("gumball") twist.
    pub fn orbit(&self, daz_deg: f64, del_deg: f64, clamp: bool) -> Self {
        let world_up = Vec3::Z;
        let daz = rad(-daz_deg);
        let del = rad(del_deg);

        // 1) YAW about worldUp = (0, 0, 1).
        // Enforce x_yaw.z == 0 so the camera right axis stays strictly horizontal (no gumball roll).
        let x_rot = rotate_vec_around_axis(self.x, world_up, daz);
        let mut x_yaw = Vec3::new(x_rot.x, x_rot.y, 0.0).normalized();
        if x_yaw == Vec3::ZERO {
            x_yaw = Vec3::new(1.0, 0.0, 0.0);
        }
        let y_yaw = rotate_vec_around_axis(self.y, world_up, daz).normalized();
        let z_yaw = rotate_vec_around_axis(self.z, world_up, daz).normalized();

        // 2) PITCH about camera-right axis x_yaw.
        let mut y_pitch = rotate_vec_around_axis(y_yaw, x_yaw, del).normalized();
        let mut z_pitch = rotate_vec_around_axis(z_yaw, x_yaw, del).normalized();

        // 3) Clamping to principal axes within 2° (MATCAD rotating3D.m lines 66-127).
        if clamp {
            let cos_tol = rad(2.0).cos();
            let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Z];
            for axis in axes {
                let d = z_pitch.dot(axis);
                if d.abs() > cos_tol {
                    z_pitch = axis * d.signum();
                    y_pitch = (y_pitch - z_pitch * y_pitch.dot(z_pitch)).normalized();
                    if z_pitch.z.abs() > 0.999 {
                        // Top or Bottom view: optionally clamp camera Y to world X or Y within 2°.
                        y_pitch = Vec3::new(y_pitch.x, y_pitch.y, 0.0).normalized();
                        for xy_axis in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)] {
                            let dxy = y_pitch.dot(xy_axis);
                            if dxy.abs() > cos_tol {
                                y_pitch = xy_axis * dxy.signum();
                                break;
                            }
                        }
                        x_yaw = z_pitch.cross(y_pitch).normalized();
                    } else {
                        // Horizontal orthogonal view (Front, Back, Left, Right): stabilize camera Y to +/-Z
                        // so x_yaw remains strictly in the world XY plane.
                        let sign_z = if y_pitch.z >= 0.0 { 1.0 } else { -1.0 };
                        y_pitch = Vec3::new(0.0, 0.0, sign_z);
                        x_yaw = z_pitch.cross(y_pitch).normalized();
                    }
                    break;
                }
            }
        }

        // Re-orthogonalize around the horizontal x_yaw so (x_yaw, y_pitch, z_pitch) is exact.
        z_pitch = (z_pitch - x_yaw * z_pitch.dot(x_yaw)).normalized();
        if z_pitch == Vec3::ZERO {
            return *self;
        }
        y_pitch = x_yaw.cross(z_pitch).normalized();
        if y_pitch == Vec3::ZERO {
            return *self;
        }
        Camera3D { x: x_yaw, y: y_pitch, z: z_pitch, up: y_pitch }
    }

    /// Roll the camera in-plane around its viewing axis `z` by `angle_deg` (positive = CCW, negative = CW).
    pub fn roll_in_plane(&self, angle_deg: f64) -> Self {
        let a = rad(angle_deg);
        let y_new = rotate_vec_around_axis(self.y, self.z, -a).normalized();
        Self::from_dir_up(self.z, y_new)
    }

    /// Roll around world Z to the next cardinal multiple of `step_deg` (ported from MATCAD `rollViewCubeCamera.m`).
    pub fn roll_world_z(&self, d_roll_deg: f64) -> Self {
        let dir_xy = self.compass_dir_xy();
        let compass_deg = dir_xy.x.atan2(dir_xy.y).to_degrees();
        let step = d_roll_deg.abs().max(90.0);
        let target = if d_roll_deg < 0.0 {
            ((compass_deg / step - 1e-4).floor()) * step
        } else {
            ((compass_deg / step + 1e-4).ceil()) * step
        };
        let mut d_angle = target - compass_deg;
        if d_angle.abs() < 1e-3 {
            d_angle = if d_roll_deg < 0.0 { -step } else { step };
        }
        self.orbit(d_angle, 0.0, true)
    }

    /// Pole-singularity-free compass North vector in world XY (ported from MATCAD `synchronize_viewcube.m`).
    ///
    /// Returns the 2D unit vector `dirXY` used as `ringAxes.CameraUpVector` in MATCAD.
    pub fn compass_dir_xy(&self) -> Vec2 {
        let d1 = Vec2::new(self.y.x, self.y.y);
        let d2 = Vec2::new(self.z.x, self.z.y);
        let v = if d1.len2() >= d2.len2() { d1 } else { d2 };
        let n = v.normalized();
        if n == Vec2::ZERO { Vec2::Y } else { n }
    }

    /// Project a 3D world point `p` relative to `(center.x, center.y, 0)` onto the camera's `(u, v)` plane.
    pub fn project_point(&self, p: Vec3, center: Vec2) -> Vec2 {
        let d = Vec3::new(p.x - center.x, p.y - center.y, p.z);
        Vec2::new(d.dot(self.x), d.dot(self.y))
    }

    /// Project a 2D point `(p.x, p.y, 0)` onto the camera's `(u, v)` plane.
    pub fn project_xy(&self, p: Vec2, center: Vec2) -> Vec2 {
        let dx = p.x - center.x;
        let dy = p.y - center.y;
        Vec2::new(dx * self.x.x + dy * self.x.y, dx * self.y.x + dy * self.y.y)
    }

    /// Intersect a camera ray through camera-plane offset `uv` with the world XY plane (`Z = 0`),
    /// ported from MATCAD `intersectRayPlane.m`. Falls back to in-plane projection when viewing edge-on.
    pub fn unproject_to_xy(&self, uv: Vec2, center: Vec2) -> Vec2 {
        let target = Vec3::new(center.x, center.y, 0.0);
        let ray_o = target + self.x * uv.x + self.y * uv.y - self.z * 100.0;
        if self.z.z.abs() > 1e-2
            && let Some(hit) = intersect_ray_plane(ray_o, self.z, target, Vec3::Z)
        {
            return hit.xy();
        }
        // Edge-on view (Front/Back/Left/Right): map directly onto the active horizontal/vertical axes.
        let p = target + self.x * uv.x + self.y * uv.y;
        Vec2::new(p.x, p.y)
    }
}

/// Local `(u_right, v_up)` 3D tangent vectors for a main ViewCube face (ported from MATCAD `buildBeveledCubeFaces.m`).
pub fn face_text_axes(face: &str) -> (Vec3, Vec3) {
    match face {
        "top" => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
        "bottom" => (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
        "front" => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
        "back" => (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
        "right" => (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
        "left" => (Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
        _ => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
    }
}

/// One quad of the 26-part beveled 3D ViewCube (6 faces, 12 edges, 8 corners).
#[derive(Clone, Debug)]
pub struct CubeQuad {
    pub verts: [Vec3; 4],
    pub normal: Vec3,
    pub part: &'static str,
    pub is_main_face: bool,
}

/// Build the 54 external quads of the 4x4x4 beveled cube (ported from MATCAD `buildBeveledCubeFaces.m`).
pub fn build_beveled_cube(scale: f64, ew: f64) -> Vec<CubeQuad> {
    let ew = ew.clamp(0.05, 0.45);
    let c = [-scale, (-1.0 + ew) * scale, (1.0 - ew) * scale, scale];
    let pt = |xi: usize, yi: usize, zi: usize| -> Vec3 {
        Vec3::new(
            c.get(xi).copied().unwrap_or(0.0),
            c.get(yi).copied().unwrap_or(0.0),
            c.get(zi).copied().unwrap_or(0.0),
        )
    };
    let mut out = Vec::with_capacity(54);
    // 6 cube sides, each a 3x3 grid of quads
    for side in 0..6 {
        for j in 0..3 {
            for i in 0..3 {
                let (verts, normal, face_name) = match side {
                    0 => (
                        [pt(i, j, 3), pt(i + 1, j, 3), pt(i + 1, j + 1, 3), pt(i, j + 1, 3)],
                        Vec3::new(0.0, 0.0, 1.0),
                        "top",
                    ),
                    1 => (
                        [pt(i, j + 1, 0), pt(i + 1, j + 1, 0), pt(i + 1, j, 0), pt(i, j, 0)],
                        Vec3::new(0.0, 0.0, -1.0),
                        "bottom",
                    ),
                    2 => (
                        [pt(i + 1, 0, j), pt(i, 0, j), pt(i, 0, j + 1), pt(i + 1, 0, j + 1)],
                        Vec3::new(0.0, -1.0, 0.0),
                        "front",
                    ),
                    3 => {
                        let z = 3 - j;
                        (
                            [pt(i, 3, z), pt(i + 1, 3, z), pt(i + 1, 3, z - 1), pt(i, 3, z - 1)],
                            Vec3::new(0.0, 1.0, 0.0),
                            "back",
                        )
                    }
                    4 => {
                        let z = 3 - j;
                        (
                            [pt(3, i, z), pt(3, i + 1, z), pt(3, i + 1, z - 1), pt(3, i, z - 1)],
                            Vec3::new(1.0, 0.0, 0.0),
                            "right",
                        )
                    }
                    _ => (
                        [pt(0, i + 1, j), pt(0, i, j), pt(0, i, j + 1), pt(0, i + 1, j + 1)],
                        Vec3::new(-1.0, 0.0, 0.0),
                        "left",
                    ),
                };
                let grid_j = if side == 3 || side == 4 { 2 - j } else { j };
                let part = name_from_grid(i, grid_j, face_name);
                let is_main_face = i == 1 && j == 1;
                out.push(CubeQuad { verts, normal, part, is_main_face });
            }
        }
    }
    out
}

fn name_from_grid(i: usize, j: usize, face: &'static str) -> &'static str {
    if i == 1 && j == 1 {
        return face;
    }
    let corner = (i == 0 || i == 2) && (j == 0 || j == 2);
    if corner {
        return match (face, i, j) {
            ("top", 0, 2) | ("back", 0, 2) | ("left", 2, 2) => "top_back_left",
            ("top", 2, 2) | ("back", 2, 2) | ("right", 2, 2) => "top_back_right",
            ("top", 0, 0) | ("front", 0, 2) | ("left", 0, 2) => "top_front_left",
            ("top", 2, 0) | ("front", 2, 2) | ("right", 0, 2) => "top_front_right",
            ("bottom", 0, 2) | ("back", 0, 0) | ("left", 2, 0) => "bottom_back_left",
            ("bottom", 2, 2) | ("back", 2, 0) | ("right", 2, 0) => "bottom_back_right",
            ("bottom", 0, 0) | ("front", 0, 0) | ("left", 0, 0) => "bottom_front_left",
            ("bottom", 2, 0) | ("front", 2, 0) | ("right", 0, 0) => "bottom_front_right",
            _ => face,
        };
    }
    if j == 2 {
        return match face {
            "top" | "back" => "back_top",
            "bottom" => "back_bottom",
            "front" => "front_top",
            "right" => "right_top",
            "left" => "left_top",
            _ => face,
        };
    }
    if j == 0 {
        return match face {
            "top" => "front_top",
            "bottom" | "front" => "front_bottom",
            "back" => "back_bottom",
            "right" => "right_bottom",
            "left" => "left_bottom",
            _ => face,
        };
    }
    if i == 2 {
        return match face {
            "top" => "right_top",
            "bottom" => "right_bottom",
            "front" | "right" => "front_right",
            "back" => "back_right",
            "left" => "back_left",
            _ => face,
        };
    }
    match face {
        "top" => "left_top",
        "bottom" => "left_bottom",
        "front" | "left" => "front_left",
        "back" => "back_left",
        "right" => "front_right",
        _ => face,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EPS;

    #[test]
    fn default_camera_is_top_plan() {
        let c = Camera3D::default();
        assert!(c.is_top_plan());
        assert!(c.is_orthogonal());
        assert_eq!(c.view_name(), "Top");
        let p = Vec2::new(12.5, -7.25);
        let center = Vec2::new(3.0, 4.0);
        let uv = c.project_xy(p, center);
        let back = c.unproject_to_xy(uv, center);
        assert!(back.near(p, EPS));
    }

    #[test]
    fn orbit_never_introduces_horizon_roll() {
        let mut c = Camera3D::TOP;
        for (daz, del) in [(37.0, 28.0), (-64.0, 41.0), (125.0, -33.0), (-89.0, 19.0), (15.0, -25.0)] {
            c = c.orbit(daz, del, true);
            assert!(c.x.z.abs() < 1e-12, "camera right vector must stay in XY plane: {:?}", c.x);
            assert!((c.x.len() - 1.0).abs() < 1e-12);
            assert!((c.y.len() - 1.0).abs() < 1e-12);
            assert!((c.z.len() - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn orbit_and_ray_plane_roundtrip() {
        let c = Camera3D::preset("top_front_right").unwrap_or_default();
        let center = Vec2::new(10.0, 20.0);
        let p = Vec2::new(25.0, -5.0);
        let uv = c.project_xy(p, center);
        let back = c.unproject_to_xy(uv, center);
        assert!(back.near(p, 1e-6));
    }

    #[test]
    fn beveled_cube_has_54_quads_and_26_parts() {
        let quads = build_beveled_cube(1.0, 0.22);
        assert_eq!(quads.len(), 54);
        assert_eq!(quads.iter().filter(|q| q.is_main_face).count(), 6);
        for q in &quads {
            assert!(Camera3D::preset(q.part).is_some(), "missing preset for {}", q.part);
        }
    }
}
