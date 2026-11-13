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

    /// Apply a 2D plan-view `twist` (radians counterclockwise) when in Top plan view.
    pub fn with_twist(self, twist: f64) -> Self {
        if self.is_top_plan() && twist != 0.0 && twist.is_finite() {
            self.roll_in_plane(-twist.to_degrees())
        } else {
            self
        }
    }

    /// True when the camera's viewing direction is nearly parallel to the world XY plane (`|z.z| <= 1e-2`),
    /// such as in Front, Back, Left or Right views.
    pub fn is_edge_on_xy(&self) -> bool {
        self.z.z.abs() <= 1e-2
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

    /// Camera preset for any of the 26 ViewCube zones (faces, edges, corners) or standard aliases
    /// (ported from MATCAD `applyViewToNamedView.m`).
    ///
    /// `name` is trimmed, matched case-insensitively, and has `-` and spaces normalized to `_`.
    /// Returns `None` if `name` is not recognized.
    ///
    /// Accepted preset names and aliases:
    /// - **Faces (6):** `"top"` (`"plan"`), `"bottom"`, `"front"` (`"south"`, `"s"`),
    ///   `"back"` (`"north"`, `"n"`), `"left"` (`"west"`, `"w"`), `"right"` (`"east"`, `"e"`).
    /// - **Edges (12):** `"front_top"` (`"top_front"`), `"front_bottom"` (`"bottom_front"`),
    ///   `"front_right"`, `"front_left"`, `"back_top"` (`"top_back"`),
    ///   `"back_bottom"` (`"bottom_back"`), `"back_right"`, `"back_left"`,
    ///   `"right_top"` (`"top_right"`), `"right_bottom"` (`"bottom_right"`),
    ///   `"left_top"` (`"top_left"`), `"left_bottom"` (`"bottom_left"`).
    /// - **Corners / Isometrics (8):**
    ///   `"top_front_right"` (`"se_isometric"`, `"iso_se"`, `"seiso"`, `"home"`, `"iso"`),
    ///   `"top_front_left"` (`"sw_isometric"`, `"iso_sw"`, `"swiso"`),
    ///   `"top_back_right"` (`"ne_isometric"`, `"iso_ne"`, `"neiso"`),
    ///   `"top_back_left"` (`"nw_isometric"`, `"iso_nw"`, `"nwiso"`),
    ///   `"bottom_front_right"`, `"bottom_front_left"`,
    ///   `"bottom_back_right"`, `"bottom_back_left"`.
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

    /// Neighbor orthogonal view when clicking one of the 4 directional triangles around the
    /// ViewCube in an orthogonal main view (ported from MATCAD `start_viewcube.m`).
    ///
    /// Accepted `dir` values (exact lowercase): `"top"`, `"bottom"`, `"left"`, `"right"`.
    ///
    /// Transitions by `self.view_name()`:
    /// - `"Top"`: `"top" -> "back"`, `"bottom" -> "front"`, `"left" -> "left"`, `"right" -> "right"`
    /// - `"Bottom"`: `"top" -> "back"`, `"bottom" -> "front"`, `"left" -> "right"`, `"right" -> "left"`
    /// - `"Front"`: `"top" -> "top"`, `"bottom" -> "bottom"`, `"left" -> "left"`, `"right" -> "right"`
    /// - `"Back"`: `"top" -> "top"`, `"bottom" -> "bottom"`, `"left" -> "right"`, `"right" -> "left"`
    /// - `"Left"`: `"top" -> "top"`, `"bottom" -> "bottom"`, `"left" -> "back"`, `"right" -> "front"`
    /// - `"Right"`: `"top" -> "top"`, `"bottom" -> "bottom"`, `"left" -> "front"`, `"right" -> "back"`
    ///
    /// For non-orthogonal views or any unrecognized `dir`, falls back to `"top"` ([`Camera3D::TOP`]).
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
    ///
    /// Because callers evaluate `cam0.orbit(daz_deg, del_deg, clamp)` from the camera state `cam0`
    /// captured at drag start, the yaw sign is locked to `self` (`cam0`) for the entire drag
    /// stroke and only flips when a new drag is started on the negative Z side of the XY plane
    /// (`self.z.z > 1e-6`, e.g. in Bottom view), keeping screen-space rotation direction consistent
    /// with Top view without ever flipping mid-stroke.
    pub fn orbit(&self, daz_deg: f64, del_deg: f64, clamp: bool) -> Self {
        let world_up = Vec3::Z;
        let yaw_sign = if self.z.z > 1e-6 { 1.0 } else { -1.0 };
        let daz = rad(yaw_sign * daz_deg);
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
    /// returning `None` in edge-on views (`|z.z| <= 1e-2`) where rays are parallel to the XY plane.
    pub fn try_unproject_to_xy(&self, uv: Vec2, center: Vec2) -> Option<Vec2> {
        if self.is_edge_on_xy() {
            return None;
        }
        let target = Vec3::new(center.x, center.y, 0.0);
        let ray_o = target + self.x * uv.x + self.y * uv.y - self.z * 100.0;
        intersect_ray_plane(ray_o, self.z, target, Vec3::Z).map(Vec3::xy)
    }

    /// Intersect a camera ray through camera-plane offset `uv` with the world XY plane (`Z = 0`),
    /// ported from MATCAD `intersectRayPlane.m`. In edge-on views where rays are parallel to XY,
    /// folds the out-of-plane vertical component along the camera's XY depth axis so neither screen
    /// axis is discarded.
    pub fn unproject_to_xy(&self, uv: Vec2, center: Vec2) -> Vec2 {
        if let Some(hit) = self.try_unproject_to_xy(uv, center) {
            return hit;
        }
        // Edge-on view (Front/Back/Left/Right): preserve both screen axes by mapping the Z
        // component onto the camera's viewing depth axis in XY.
        let d3 = self.x * uv.x + self.y * uv.y;
        let z_xy = Vec2::new(self.z.x, self.z.y);
        let depth_dir = if z_xy.len() > 1e-12 {
            z_xy / z_xy.len()
        } else {
            Vec2::new(-self.x.y, self.x.x).normalized()
        };
        center + Vec2::new(d3.x, d3.y) + depth_dir * d3.z
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
    /// Four 3D corner vertices of the quad on its cube side plane (`[-scale, scale]`).
    pub verts: [Vec3; 4],
    /// Unit outward normal of the cube side (`±X`, `±Y`, or `±Z`).
    pub normal: Vec3,
    /// ViewCube part name accepted by [`Camera3D::preset`] (6 faces, 12 edges, or 8 corners).
    pub part: &'static str,
    /// `true` for the central quad (`i == 1 && j == 1`) of each of the 6 main faces; `false` for bevel edges/corners.
    pub is_main_face: bool,
}

/// Build the 54 external quads of the 4x4x4 beveled cube (ported from MATCAD `buildBeveledCubeFaces.m`).
///
/// # Parameters
/// - `scale`: Half-extent of the cube in ViewCube/world units; generated vertex coordinates span
///   `[-scale, scale]` along each axis.
/// - `ew`: Normalized edge/bevel strip width fraction relative to `scale`, clamped internally to
///   `[0.05, 0.45]`, placing the inner grid cuts at `±(1.0 - ew) * scale`.
///
/// # Returns
/// A `Vec<CubeQuad>` with 54 quads (`6` sides × `3×3` grid) partitioned across 26 ViewCube parts
/// (6 main faces appearing once, 12 edges appearing twice, and 8 corners appearing three times).
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
            "front" => "front_right",
            "back" | "right" => "back_right",
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
        let scale = 1.0;
        let quads = build_beveled_cube(scale, 0.22);
        assert_eq!(quads.len(), 54);
        assert_eq!(quads.iter().filter(|q| q.is_main_face).count(), 6);

        let mut distinct: Vec<&'static str> = quads.iter().map(|q| q.part).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 26, "expected 26 distinct ViewCube parts: {distinct:?}");

        for q in &quads {
            assert!(Camera3D::preset(q.part).is_some(), "missing preset for {}", q.part);
            let geometric = (q.verts[1] - q.verts[0]).cross(q.verts[2] - q.verts[0]).normalized();
            assert!(
                geometric.dot(q.normal).abs() > 0.999_999,
                "quad {} geometry not perpendicular to normal {:?}",
                q.part,
                q.normal
            );
            for v in q.verts {
                assert!((v.dot(q.normal) - scale).abs() < 1e-12, "quad {} off face plane: v={v:?}", q.part);
            }
        }

        let face_normals = [
            ("top", Vec3::new(0.0, 0.0, 1.0)),
            ("bottom", Vec3::new(0.0, 0.0, -1.0)),
            ("front", Vec3::new(0.0, -1.0, 0.0)),
            ("back", Vec3::new(0.0, 1.0, 0.0)),
            ("right", Vec3::new(1.0, 0.0, 0.0)),
            ("left", Vec3::new(-1.0, 0.0, 0.0)),
        ];
        for (face, normal) in face_normals {
            let side: Vec<&CubeQuad> = quads.iter().filter(|q| (q.normal - normal).len() < 1e-12).collect();
            assert_eq!(side.len(), 9, "{face}: expected 9 quads");
            let main: Vec<&&CubeQuad> = side.iter().filter(|q| q.is_main_face).collect();
            assert_eq!(main.len(), 1, "{face}: expected 1 main face");
            assert_eq!(main[0].part, face);
        }

        // 6 faces appear once, 12 edges appear twice, 8 corners appear three times.
        let mut counts = std::collections::BTreeMap::new();
        for q in &quads {
            *counts.entry(q.part).or_insert(0usize) += 1;
        }
        assert_eq!(counts.values().filter(|&&c| c == 1).count(), 6);
        assert_eq!(counts.values().filter(|&&c| c == 2).count(), 12);
        assert_eq!(counts.values().filter(|&&c| c == 3).count(), 8);

        // `ew` clamps to [0.05, 0.45]: clamping extremes match boundary values.
        assert_eq!(build_beveled_cube(2.0, 0.01)[0].verts, build_beveled_cube(2.0, 0.05)[0].verts);
        assert_eq!(build_beveled_cube(2.0, 0.90)[0].verts, build_beveled_cube(2.0, 0.45)[0].verts);
    }

    #[test]
    fn bottom_view_orbit_matches_top_view_screen_direction_on_new_stroke() {
        let top0 = Camera3D::TOP;
        let bot0 = Camera3D::preset("bottom").unwrap_or_default();
        assert!(bot0.z.z > 0.999);

        // Starting a new drag in Top vs Bottom with the same horizontal mouse motion (daz = 30°)
        // rotates the camera's local screen frame (x0, y0) in the exact same screen direction.
        let top1 = top0.orbit(30.0, 0.0, true);
        let bot1 = bot0.orbit(30.0, 0.0, true);
        assert!((top1.y.dot(top0.x) - bot1.y.dot(bot0.x)).abs() < 1e-12);
        assert!((top1.y.dot(top0.y) - bot1.y.dot(bot0.y)).abs() < 1e-12);
    }

    #[test]
    fn edge_on_unproject_preserves_depth_axis() {
        let center = Vec2::new(15.0, -8.0);
        let uv = Vec2::new(12.0, 25.0);
        for name in ["front", "back", "left", "right", "front_left", "front_right"] {
            let c = Camera3D::preset(name).unwrap_or_default();
            assert!(c.try_unproject_to_xy(uv, center).is_none(), "{name} should be edge-on");
            assert!(c.unproject_to_xy(Vec2::ZERO, center).near(center, EPS));

            let up = c.unproject_to_xy(uv, center);
            let down = c.unproject_to_xy(Vec2::new(uv.x, -uv.y), center);
            assert!((up - down).len() > 49.0, "{name} discarded vertical offset: up={up:?}, down={down:?}");
            assert!(((up - center).len() - uv.len()).abs() < 1e-9, "{name} should map uv isometrically: up={up:?}");

            // Rolled edge-on views must also map both screen axes without collapsing either.
            let rolled = c.roll_in_plane(90.0);
            let rp = rolled.unproject_to_xy(uv, center);
            assert!(((rp - center).len() - uv.len()).abs() < 1e-9, "{name} rolled lost norm: rp={rp:?}");
        }
    }

    fn assert_orthonormal(c: &Camera3D, require_horizontal_x: bool, label: &str) {
        assert!((c.x.len() - 1.0).abs() < 1e-12, "{label}: |x| != 1: {:?}", c.x);
        assert!((c.y.len() - 1.0).abs() < 1e-12, "{label}: |y| != 1: {:?}", c.y);
        assert!((c.z.len() - 1.0).abs() < 1e-12, "{label}: |z| != 1: {:?}", c.z);
        assert!((c.up.len() - 1.0).abs() < 1e-12, "{label}: |up| != 1: {:?}", c.up);
        assert!((c.up - c.y).len() < 1e-12, "{label}: up != y: up={:?}, y={:?}", c.up, c.y);
        assert!(c.x.dot(c.y).abs() < 1e-12, "{label}: x·y != 0: {}", c.x.dot(c.y));
        assert!(c.y.dot(c.z).abs() < 1e-12, "{label}: y·z != 0: {}", c.y.dot(c.z));
        assert!(c.z.dot(c.x).abs() < 1e-12, "{label}: z·x != 0: {}", c.z.dot(c.x));
        assert!((c.x.cross(c.z) - c.y).len() < 1e-12, "{label}: handedness x×z != y");
        if require_horizontal_x {
            assert!(c.x.z.abs() < 1e-12, "{label}: x.z != 0: {}", c.x.z);
        }
    }

    fn assert_vec3_near(actual: Vec3, expected: Vec3, tol: f64, label: &str) {
        assert!(
            (actual - expected).len() < tol,
            "{label}: got {actual:?}, want {expected:?} (tol {tol})"
        );
    }

    fn assert_cam_near(a: &Camera3D, b: &Camera3D, tol: f64, label: &str) {
        assert_vec3_near(a.x, b.x, tol, &format!("{label}.x"));
        assert_vec3_near(a.y, b.y, tol, &format!("{label}.y"));
        assert_vec3_near(a.z, b.z, tol, &format!("{label}.z"));
        assert_vec3_near(a.up, b.up, tol, &format!("{label}.up"));
    }

    #[test]
    fn preset_covers_all_26_zones_aliases_and_unknown_names() {
        let d = 0.5_f64.sqrt();
        let h = 0.5_f64;
        let canonical_26 = [
            // 6 main faces
            ("top", Vec3::new(0.0, 0.0, -1.0), "Top"),
            ("bottom", Vec3::new(0.0, 0.0, 1.0), "Bottom"),
            ("front", Vec3::new(0.0, 1.0, 0.0), "Front"),
            ("back", Vec3::new(0.0, -1.0, 0.0), "Back"),
            ("left", Vec3::new(1.0, 0.0, 0.0), "Left"),
            ("right", Vec3::new(-1.0, 0.0, 0.0), "Right"),
            // 12 edges
            ("front_top", Vec3::new(0.0, d, -d), "3D"),
            ("front_bottom", Vec3::new(0.0, d, d), "3D"),
            ("front_right", Vec3::new(-d, d, 0.0), "3D"),
            ("front_left", Vec3::new(d, d, 0.0), "3D"),
            ("back_top", Vec3::new(0.0, -d, -d), "3D"),
            ("back_bottom", Vec3::new(0.0, -d, d), "3D"),
            ("back_right", Vec3::new(-d, -d, 0.0), "3D"),
            ("back_left", Vec3::new(d, -d, 0.0), "3D"),
            ("right_top", Vec3::new(-d, 0.0, -d), "3D"),
            ("right_bottom", Vec3::new(-d, 0.0, d), "3D"),
            ("left_top", Vec3::new(d, 0.0, -d), "3D"),
            ("left_bottom", Vec3::new(d, 0.0, d), "3D"),
            // 8 corners
            ("top_front_right", Vec3::new(-h, h, -d), "3D"),
            ("top_front_left", Vec3::new(h, h, -d), "3D"),
            ("top_back_right", Vec3::new(-h, -h, -d), "3D"),
            ("top_back_left", Vec3::new(h, -h, -d), "3D"),
            ("bottom_front_right", Vec3::new(-h, h, d), "3D"),
            ("bottom_front_left", Vec3::new(h, h, d), "3D"),
            ("bottom_back_right", Vec3::new(-h, -h, d), "3D"),
            ("bottom_back_left", Vec3::new(h, -h, d), "3D"),
        ];

        assert_eq!(canonical_26.len(), 26);
        for (name, expected_z, expected_view) in canonical_26 {
            let Some(cam) = Camera3D::preset(name) else {
                assert!(false, "missing preset for {name}");
                return;
            };
            assert_orthonormal(&cam, true, name);
            assert_vec3_near(cam.z, expected_z, 1e-12, name);
            assert_eq!(cam.view_name(), expected_view, "view_name mismatch for {name}");
        }

        let aliases = [
            ("plan", "top"),
            ("south", "front"),
            ("s", "front"),
            ("north", "back"),
            ("n", "back"),
            ("west", "left"),
            ("w", "left"),
            ("east", "right"),
            ("e", "right"),
            ("top_front", "front_top"),
            ("top_back", "back_top"),
            ("bottom_front", "front_bottom"),
            ("bottom_back", "back_bottom"),
            ("top_right", "right_top"),
            ("bottom_right", "right_bottom"),
            ("top_left", "left_top"),
            ("bottom_left", "left_bottom"),
            ("se_isometric", "top_front_right"),
            ("iso_se", "top_front_right"),
            ("seiso", "top_front_right"),
            ("home", "top_front_right"),
            ("iso", "top_front_right"),
            ("sw_isometric", "top_front_left"),
            ("iso_sw", "top_front_left"),
            ("swiso", "top_front_left"),
            ("ne_isometric", "top_back_right"),
            ("iso_ne", "top_back_right"),
            ("neiso", "top_back_right"),
            ("nw_isometric", "top_back_left"),
            ("iso_nw", "top_back_left"),
            ("nwiso", "top_back_left"),
            ("  SW-Isometric ", "top_front_left"),
            ("Front Top", "front_top"),
        ];
        for (alias, canonical) in aliases {
            let (Some(a), Some(b)) = (Camera3D::preset(alias), Camera3D::preset(canonical)) else {
                assert!(false, "alias {alias} -> {canonical} failed to resolve");
                return;
            };
            assert_cam_near(&a, &b, 1e-12, &format!("{alias} == {canonical}"));
        }

        for unknown in ["", "   ", "unknown", "center", "top_bottom", "side"] {
            assert!(Camera3D::preset(unknown).is_none(), "expected None for {unknown:?}");
        }
    }

    #[test]
    fn orthogonal_neighbor_covers_all_24_mappings_and_fallbacks() {
        let mappings = [
            ("top", "top", "back"),
            ("top", "bottom", "front"),
            ("top", "left", "left"),
            ("top", "right", "right"),
            ("bottom", "top", "back"),
            ("bottom", "bottom", "front"),
            ("bottom", "left", "right"),
            ("bottom", "right", "left"),
            ("front", "top", "top"),
            ("front", "bottom", "bottom"),
            ("front", "left", "left"),
            ("front", "right", "right"),
            ("back", "top", "top"),
            ("back", "bottom", "bottom"),
            ("back", "left", "right"),
            ("back", "right", "left"),
            ("left", "top", "top"),
            ("left", "bottom", "bottom"),
            ("left", "left", "back"),
            ("left", "right", "front"),
            ("right", "top", "top"),
            ("right", "bottom", "bottom"),
            ("right", "left", "front"),
            ("right", "right", "back"),
        ];
        assert_eq!(mappings.len(), 24);
        for (start, dir, expected) in mappings {
            let (Some(cam), Some(want)) = (Camera3D::preset(start), Camera3D::preset(expected)) else {
                assert!(false, "preset lookup failed for {start} -> {expected}");
                return;
            };
            let got = cam.orthogonal_neighbor(dir);
            assert_eq!(got.view_name(), want.view_name(), "({start}, {dir}) view_name");
            assert_cam_near(&got, &want, 1e-12, &format!("({start}, {dir})"));
        }

        // Fallback cases: unknown direction or non-orthogonal starting view return Top.
        for (start, dir) in [("front", "diagonal"), ("top", ""), ("sw_isometric", "left"), ("front_top", "top")] {
            let Some(cam) = Camera3D::preset(start) else {
                assert!(false, "missing preset {start}");
                return;
            };
            let got = cam.orthogonal_neighbor(dir);
            assert_eq!(got.view_name(), "Top", "fallback for ({start}, {dir})");
            assert_cam_near(&got, &Camera3D::TOP, 1e-12, &format!("fallback ({start}, {dir})"));
        }
    }

    #[test]
    fn orbit_clamps_to_principal_axes_and_preserves_orthonormality() {
        for start in ["top", "front", "bottom", "sw_isometric"] {
            for clamp in [true, false] {
                let Some(mut c) = Camera3D::preset(start) else {
                    assert!(false, "missing preset {start}");
                    return;
                };
                for (daz, del) in [(27.0, 18.0), (-53.0, 31.0), (112.0, -49.0), (-77.0, 62.0)] {
                    c = c.orbit(daz, del, clamp);
                    assert_orthonormal(&c, true, &format!("{start} clamp={clamp}"));
                }
            }
        }

        // Clamping within 2° snaps cleanly to all 6 principal axes.
        let near_axis_cases = [
            ("front", 0.0, -89.0, "Top", Vec3::new(0.0, 0.0, -1.0)),
            ("front", 0.0, 89.0, "Bottom", Vec3::new(0.0, 0.0, 1.0)),
            ("front_top", 0.0, 44.0, "Front", Vec3::new(0.0, 1.0, 0.0)),
            ("back_top", 0.0, 44.0, "Back", Vec3::new(0.0, -1.0, 0.0)),
            ("left_top", 0.0, 44.0, "Left", Vec3::new(1.0, 0.0, 0.0)),
            ("right_top", 0.0, 44.0, "Right", Vec3::new(-1.0, 0.0, 0.0)),
        ];
        for (start, daz, del, expected_name, expected_z) in near_axis_cases {
            let Some(base) = Camera3D::preset(start) else {
                assert!(false, "missing preset {start}");
                return;
            };
            let snapped = base.orbit(daz, del, true);
            assert_orthonormal(&snapped, true, &format!("snap {expected_name}"));
            assert_eq!(snapped.view_name(), expected_name, "{start} -> {expected_name}");
            assert_vec3_near(snapped.z, expected_z, 1e-12, expected_name);
        }

        // Secondary Top/Bottom XY snap within 2° restores canonical plan axes.
        let snapped_top = Camera3D::TOP.orbit(1.5, 0.0, true);
        assert!(snapped_top.is_top_plan(), "small Top yaw should snap back to plan");

        // Outside 2° does not snap to a principal axis or plan.
        let front = Camera3D::preset("front").unwrap_or_default();
        assert_eq!(front.orbit(0.0, 5.0, true).view_name(), "3D");
        assert!(!Camera3D::TOP.orbit(5.0, 0.0, true).is_top_plan());
    }

    #[test]
    fn orbit_unclamped_skips_snap_and_pitches_past_poles() {
        let front = Camera3D::preset("front").unwrap_or_default();
        let front_top = Camera3D::preset("front_top").unwrap_or_default();

        // With clamp=false, <2° off-axis orbits do not snap to Top or Front exact axes.
        let near_top = front.orbit(0.0, -89.0, false);
        assert_orthonormal(&near_top, true, "unclamped near_top");
        assert!((near_top.z - Vec3::new(0.0, 0.0, -1.0)).len() > 0.01, "z={:?}", near_top.z);

        let near_front = front_top.orbit(0.0, 44.0, false);
        assert_orthonormal(&near_front, true, "unclamped near_front");
        assert!((near_front.z - Vec3::new(0.0, 1.0, 0.0)).len() > 0.01, "z={:?}", near_front.z);

        // Pitch past the Top pole (-Z side) and Bottom pole (+Z side) without clamping.
        let over_north = front.orbit(0.0, -120.0, false);
        assert_orthonormal(&over_north, true, "over north pole");
        assert!(over_north.z.y < 0.0 && over_north.z.z < 0.0, "unexpected z={:?}", over_north.z);
        assert!(over_north.y.z < 0.0, "expected inverted y past pole: {:?}", over_north.y);

        let over_south = front.orbit(0.0, 120.0, false);
        assert_orthonormal(&over_south, true, "over south pole");
        assert!(over_south.z.y < 0.0 && over_south.z.z > 0.0, "unexpected z={:?}", over_south.z);
        assert!(over_south.y.z < 0.0, "expected inverted y past south pole: {:?}", over_south.y);
    }

    #[test]
    fn from_dir_up_handles_degenerate_inputs_and_fallbacks() {
        // Zero, sub-EPS, and NaN viewing directions all fall back to Camera3D::TOP.
        for bad_z in [Vec3::ZERO, Vec3::new(1e-12, 0.0, 0.0), Vec3::new(f64::NAN, 0.0, 0.0)] {
            assert_eq!(Camera3D::from_dir_up(bad_z, Vec3::Z), Camera3D::TOP);
        }

        // Degenerate or collinear `up` triggers automatic fallback axes while preserving `z`.
        let front = Camera3D::preset("front").unwrap_or_default();
        let zero_up_front = Camera3D::from_dir_up(Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO);
        assert_orthonormal(&zero_up_front, false, "zero_up_front");
        assert_cam_near(&zero_up_front, &front, 1e-12, "zero_up_front");

        let zero_up_top = Camera3D::from_dir_up(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO);
        assert_cam_near(&zero_up_top, &Camera3D::TOP, 1e-12, "zero_up_top");

        let collinear_front = Camera3D::from_dir_up(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert_orthonormal(&collinear_front, false, "collinear_front");
        assert_cam_near(&collinear_front, &front, 1e-12, "collinear_front");

        let collinear_top = Camera3D::from_dir_up(Vec3::new(0.0, 0.0, -1.0), Vec3::Z);
        assert_cam_near(&collinear_top, &Camera3D::TOP, 1e-12, "collinear_top");

        // Non-orthogonal inputs yield a normalized orthonormal camera aligned with `z`.
        let raw_z = Vec3::new(1.0, 1.0, 0.0);
        let cam = Camera3D::from_dir_up(raw_z, Vec3::new(1.0, 0.0, 1.0));
        assert_orthonormal(&cam, false, "non_orthogonal");
        assert_vec3_near(cam.z, raw_z.normalized(), 1e-12, "non_orthogonal.z");
    }

    #[test]
    fn roll_in_plane_preserves_view_dir_and_is_invertible() {
        for name in ["top", "front", "right", "sw_isometric"] {
            let Some(base) = Camera3D::preset(name) else {
                assert!(false, "missing preset {name}");
                return;
            };
            for angle in [0.0, 30.0, -45.0, 90.0, 180.0] {
                let rolled = base.roll_in_plane(angle);
                assert_orthonormal(&rolled, false, &format!("{name} roll {angle}"));
                assert_vec3_near(rolled.z, base.z, 1e-12, &format!("{name} roll {angle} preserves z"));

                let expected_y = rotate_vec_around_axis(base.y, base.z, -rad(angle)).normalized();
                assert_vec3_near(rolled.y, expected_y, 1e-12, &format!("{name} roll {angle} y"));

                let restored = rolled.roll_in_plane(-angle);
                assert_cam_near(&restored, &base, 1e-12, &format!("{name} roll {angle} roundtrip"));
            }

            let nan_rolled = base.roll_in_plane(f64::NAN);
            assert_cam_near(&nan_rolled, &base, 1e-12, &format!("{name} NaN roll"));
        }
    }

    #[test]
    fn roll_world_z_steps_to_next_multiple_in_both_directions() {
        let front = Camera3D::preset("front").unwrap_or_default();

        // Cardinal 90° steps cycle Front -> Left -> Back -> Right -> Front (+), and reverse (-).
        let mut pos = front;
        for expected in ["Left", "Back", "Right", "Front"] {
            pos = pos.roll_world_z(90.0);
            assert_orthonormal(&pos, true, expected);
            assert_eq!(pos.view_name(), expected);
        }
        let mut neg = front;
        for expected in ["Right", "Back", "Left", "Front"] {
            neg = neg.roll_world_z(-90.0);
            assert_orthonormal(&neg, true, expected);
            assert_eq!(neg.view_name(), expected);
        }

        // Small angles (< 90°) enforce the 90° minimum step in both directions.
        assert_eq!(front.roll_world_z(30.0).view_name(), "Left");
        assert_eq!(front.roll_world_z(-30.0).view_name(), "Right");
        assert_eq!(front.roll_world_z(180.0).view_name(), "Back");
        assert_eq!(front.roll_world_z(-180.0).view_name(), "Back");

        // Intermediate headings snap strictly forward (+) or backward (-) to the next 90° multiple.
        let front_left = Camera3D::preset("front_left").unwrap_or_default(); // compass = +45°
        assert_eq!(front_left.roll_world_z(10.0).view_name(), "Left");
        assert_eq!(front_left.roll_world_z(90.0).view_name(), "Left");
        assert_eq!(front_left.roll_world_z(-10.0).view_name(), "Front");
        assert_eq!(front_left.roll_world_z(-90.0).view_name(), "Front");

        let front_right = Camera3D::preset("front_right").unwrap_or_default(); // compass = -45°
        assert_eq!(front_right.roll_world_z(10.0).view_name(), "Front");
        assert_eq!(front_right.roll_world_z(-10.0).view_name(), "Right");

        let skewed = front.orbit(20.0, 0.0, true); // compass = +20°
        assert_eq!(skewed.roll_world_z(90.0).view_name(), "Left");
        assert_eq!(skewed.roll_world_z(-90.0).view_name(), "Front");
    }

    #[test]
    fn compass_dir_xy_tracks_horizon_and_pole_without_singularity() {
        for name in ["front", "back", "left", "right"] {
            let Some(cam) = Camera3D::preset(name) else {
                assert!(false, "missing preset {name}");
                return;
            };
            assert!(cam.compass_dir_xy().near(cam.z.xy().normalized(), 1e-12), "{name}");
        }

        // At Top/Bottom poles (z.xy == 0), compass follows y.xy smoothly even when rolled in-plane.
        let top_rolled = Camera3D::TOP.roll_in_plane(90.0);
        assert!(Camera3D::TOP.compass_dir_xy().near(Vec2::Y, 1e-12));
        assert!(top_rolled.compass_dir_xy().near(top_rolled.y.xy().normalized(), 1e-12));
        let bottom = Camera3D::preset("bottom").unwrap_or_default();
        assert!(bottom.compass_dir_xy().near(bottom.y.xy().normalized(), 1e-12));

        // Degenerate camera with zero XY projections in both `y` and `z` falls back to Vec2::Y.
        let degenerate = Camera3D { x: Vec3::Z, y: Vec3::Z, z: Vec3::Z, up: Vec3::Z };
        assert_eq!(degenerate.compass_dir_xy(), Vec2::Y);
    }

    #[test]
    fn intersect_ray_plane_and_behind_camera_projection_cases() {
        // Parallel rays and degenerate inputs return None.
        assert!(intersect_ray_plane(Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z).is_none());
        assert!(intersect_ray_plane(Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0), Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).is_none());
        assert!(intersect_ray_plane(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::Z).is_none());
        assert!(intersect_ray_plane(Vec3::ZERO, Vec3::Z, Vec3::ZERO, Vec3::ZERO).is_none());
        assert!(intersect_ray_plane(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0), Vec3::ZERO, Vec3::Z).is_none());
        assert!(intersect_ray_plane(Vec3::new(f64::INFINITY, 0.0, 1.0), Vec3::Z, Vec3::ZERO, Vec3::Z).is_none());

        // Oblique hit in front of ray origin (t > 0) and geometric hit behind ray origin (t < 0).
        let Some(hit_front) = intersect_ray_plane(Vec3::new(1.0, 2.0, 4.0), Vec3::new(1.0, -1.0, -2.0), Vec3::ZERO, Vec3::Z) else {
            assert!(false, "expected oblique ray-plane hit");
            return;
        };
        assert_vec3_near(hit_front, Vec3::new(3.0, 0.0, 0.0), 1e-12, "oblique hit");

        let Some(hit_behind) = intersect_ray_plane(Vec3::new(4.0, -3.0, 5.0), Vec3::Z, Vec3::ZERO, Vec3::Z) else {
            assert!(false, "expected behind-origin line-plane hit");
            return;
        };
        assert_vec3_near(hit_behind, Vec3::new(4.0, -3.0, 0.0), 1e-12, "behind-origin hit");

        // Orthographic project_point agrees with project_xy on Z=0 and is invariant along `z` (in front or behind camera).
        let cam = Camera3D::TOP.orbit(37.0, -52.0, true);
        let center = Vec2::new(-8.0, 14.0);
        let p2 = Vec2::new(6.5, -2.25);
        let uv0 = cam.project_xy(p2, center);
        let uv3 = cam.project_point(Vec3::new(p2.x, p2.y, 0.0), center);
        assert!(uv0.near(uv3, 1e-12));

        let target = Vec3::new(center.x, center.y, 0.0);
        for depth in [-500.0, -100.0, 0.0, 250.0] {
            let shifted = target + cam.x * uv0.x + cam.y * uv0.y + cam.z * depth;
            assert!(cam.project_point(shifted, center).near(uv0, 1e-12), "depth {depth} shifted uv");
        }
    }

    #[test]
    fn projection_roundtrips_and_edge_on_fallback_cases() {
        let center = Vec2::new(-12.5, 7.25);
        let p = Vec2::new(19.0, -21.5);

        // Non-isometric 3D view and Bottom view both round-trip cleanly via try_unproject_to_xy and unproject_to_xy.
        let oblique = Camera3D::TOP.orbit(37.0, -52.0, true);
        assert_eq!(oblique.view_name(), "3D");
        let bottom = Camera3D::preset("bottom").unwrap_or_default();
        let shallow = Camera3D::preset("front").unwrap_or_default().orbit(0.0, -1.5, false); // |z.z| > 1e-2
        assert!(!shallow.is_edge_on_xy());

        for (label, cam) in [("bottom", bottom), ("oblique_3d", oblique), ("shallow_non_edge", shallow)] {
            let uv = cam.project_xy(p, center);
            let Some(try_back) = cam.try_unproject_to_xy(uv, center) else {
                assert!(false, "{label}: try_unproject_to_xy returned None");
                return;
            };
            assert!(try_back.near(p, 1e-8), "{label}: try_back={try_back:?}, want={p:?}");
            assert!(cam.unproject_to_xy(uv, center).near(p, 1e-8), "{label}: unproject mismatch");
        }

        // Front and near-edge-on views (|z.z| <= 1e-2) return None from try_unproject_to_xy and use the fallback.
        let front = Camera3D::preset("front").unwrap_or_default();
        let near_edge = front.orbit(0.0, -0.3, false);
        assert!(near_edge.is_edge_on_xy());
        assert!(near_edge.try_unproject_to_xy(Vec2::new(4.0, 5.0), center).is_none());

        let front_uv = front.project_xy(p, center);
        assert!(front_uv.near(Vec2::new(p.x - center.x, 0.0), 1e-12));
        assert!(front.try_unproject_to_xy(front_uv, center).is_none());
        let front_back = front.unproject_to_xy(front_uv, center);
        assert!(front_back.near(Vec2::new(p.x, center.y), 1e-12), "front_back={front_back:?}");
        // In-plane vertical motion in Front maps cleanly between screen v and world Z in project_point.
        let screen_uv = Vec2::new(14.0, -9.5);
        assert!(front.project_point(Vec3::new(center.x + screen_uv.x, center.y, screen_uv.y), center).near(screen_uv, 1e-12));
    }

    #[test]
    fn name_from_grid_maps_every_face_edge_and_corner_cell() {
        let grid_table: [(&str, [[&str; 3]; 3]); 6] = [
            (
                "top",
                [
                    ["top_front_left", "front_top", "top_front_right"],
                    ["left_top", "top", "right_top"],
                    ["top_back_left", "back_top", "top_back_right"],
                ],
            ),
            (
                "bottom",
                [
                    ["bottom_front_left", "front_bottom", "bottom_front_right"],
                    ["left_bottom", "bottom", "right_bottom"],
                    ["bottom_back_left", "back_bottom", "bottom_back_right"],
                ],
            ),
            (
                "front",
                [
                    ["bottom_front_left", "front_bottom", "bottom_front_right"],
                    ["front_left", "front", "front_right"],
                    ["top_front_left", "front_top", "top_front_right"],
                ],
            ),
            (
                "back",
                [
                    ["bottom_back_left", "back_bottom", "bottom_back_right"],
                    ["back_left", "back", "back_right"],
                    ["top_back_left", "back_top", "top_back_right"],
                ],
            ),
            (
                "right",
                [
                    ["bottom_front_right", "right_bottom", "bottom_back_right"],
                    ["front_right", "right", "back_right"],
                    ["top_front_right", "right_top", "top_back_right"],
                ],
            ),
            (
                "left",
                [
                    ["bottom_front_left", "left_bottom", "bottom_back_left"],
                    ["front_left", "left", "back_left"],
                    ["top_front_left", "left_top", "top_back_left"],
                ],
            ),
        ];

        for (face, rows_by_j) in grid_table {
            for (j, row) in rows_by_j.iter().enumerate() {
                for (i, &expected) in row.iter().enumerate() {
                    assert_eq!(name_from_grid(i, j, face), expected, "({face}, i={i}, j={j})");
                }
            }
        }

        let axes_expected = [
            ("top", Vec3::new(0.0, 0.0, 1.0)),
            ("bottom", Vec3::new(0.0, 0.0, -1.0)),
            ("front", Vec3::new(0.0, -1.0, 0.0)),
            ("back", Vec3::new(0.0, 1.0, 0.0)),
            ("right", Vec3::new(1.0, 0.0, 0.0)),
            ("left", Vec3::new(-1.0, 0.0, 0.0)),
        ];
        for (face, normal) in axes_expected {
            let (u, v) = face_text_axes(face);
            assert!((u.len() - 1.0).abs() < 1e-12 && (v.len() - 1.0).abs() < 1e-12, "{face} length");
            assert!(u.dot(v).abs() < 1e-12, "{face} orthogonal");
            assert_vec3_near(u.cross(v), normal, 1e-12, &format!("{face} outward normal"));
        }
        assert_eq!(face_text_axes("unknown"), face_text_axes("top"));
    }
}
