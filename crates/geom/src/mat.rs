use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3};

/// A 2D affine transform `[a c e; b d f; 0 0 1]` (column vectors, like SVG/PDF).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat3 {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Mat3 {
    fn default() -> Self {
        Mat3::IDENTITY
    }
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(v: Vec2) -> Mat3 {
        Mat3 { e: v.x, f: v.y, ..Mat3::IDENTITY }
    }
    pub fn scale(sx: f64, sy: f64) -> Mat3 {
        Mat3 { a: sx, d: sy, ..Mat3::IDENTITY }
    }
    pub fn rotate(angle: f64) -> Mat3 {
        let (s, c) = angle.sin_cos();
        Mat3 { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }
    /// Scale about `base`.
    pub fn scale_about(base: Vec2, s: f64) -> Mat3 {
        Mat3::translate(base).then_before(Mat3::scale(s, s)).then_before(Mat3::translate(-base))
    }
    pub fn rotate_about(base: Vec2, angle: f64) -> Mat3 {
        Mat3::translate(base).then_before(Mat3::rotate(angle)).then_before(Mat3::translate(-base))
    }
    /// Reflection across the line through `p1`, `p2`.
    pub fn mirror(p1: Vec2, p2: Vec2) -> Mat3 {
        let d = (p2 - p1).normalized();
        if d == Vec2::ZERO {
            return Mat3::IDENTITY;
        }
        let (c2, s2) = (d.x * d.x - d.y * d.y, 2.0 * d.x * d.y);
        let r = Mat3 { a: c2, b: s2, c: s2, d: -c2, e: 0.0, f: 0.0 };
        Mat3::translate(p1).then_before(r).then_before(Mat3::translate(-p1))
    }
    /// `self * o`: apply `o` first, then `self`.
    pub fn then_before(self, o: Mat3) -> Mat3 {
        Mat3 {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
    /// Apply `self` first, then `o`.
    pub fn then(self, o: Mat3) -> Mat3 {
        o.then_before(self)
    }
    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }
    pub fn apply_vec(&self, v: Vec2) -> Vec2 {
        Vec2::new(self.a * v.x + self.c * v.y, self.b * v.x + self.d * v.y)
    }
    pub fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    pub fn inverse(&self) -> Option<Mat3> {
        let det = self.det();
        if det.abs() < 1e-300 || !det.is_finite() {
            return None;
        }
        let id = 1.0 / det;
        let a = self.d * id;
        let b = -self.b * id;
        let c = -self.c * id;
        let d = self.a * id;
        let e = -(a * self.e + c * self.f);
        let f = -(b * self.e + d * self.f);
        Some(Mat3 { a, b, c, d, e, f })
    }
    /// Uniform scale factor (geometric mean of the axis scales).
    pub fn scale_factor(&self) -> f64 {
        self.det().abs().sqrt()
    }
    /// Rotation angle of the transformed X axis.
    pub fn rotation(&self) -> f64 {
        crate::norm_angle(self.b.atan2(self.a))
    }
    pub fn is_mirroring(&self) -> bool {
        self.det() < 0.0
    }
}

/// A 4x4 transform, row-major, for 3D views and block transforms with elevation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat4 {
    pub m: [[f64; 4]; 4],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.m[i][k] * o.m[k][j]).sum();
            }
        }
        Mat4 { m: r }
    }
    pub fn apply(&self, p: Vec3) -> Vec3 {
        let m = &self.m;
        let w = m[3][0] * p.x + m[3][1] * p.y + m[3][2] * p.z + m[3][3];
        let w = if w.abs() < 1e-300 { 1.0 } else { w };
        Vec3::new(
            (m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3]) / w,
            (m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3]) / w,
            (m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3]) / w,
        )
    }
    pub fn from_mat3(t: &Mat3) -> Mat4 {
        Mat4 { m: [[t.a, t.c, 0.0, t.e], [t.b, t.d, 0.0, t.f], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }
    /// WCS → view coordinates (AutoCAD's DCS) of a parallel view looking from `target + dir`
    /// towards `target`: x right, y up, z towards the viewer. The axes are the arbitrary-axis
    /// frame of `dir` ([`Mat4::ocs`]) turned by `twist` (radians, counterclockwise), so a plan
    /// view (`dir` = +Z, no twist) of target 0 is the identity.
    pub fn view(dir: Vec3, target: Vec3, twist: f64) -> Mat4 {
        let f = Mat4::ocs(dir);
        let (s, c) = (-twist).sin_cos();
        // Rows of the OCS frame's transpose (WCS → OCS), turned in the view plane.
        let ax = Vec3::new(f.m[0][0], f.m[1][0], f.m[2][0]);
        let ay = Vec3::new(f.m[0][1], f.m[1][1], f.m[2][1]);
        let az = Vec3::new(f.m[0][2], f.m[1][2], f.m[2][2]);
        let x = ax * c - ay * s;
        let y = ax * s + ay * c;
        let row = |v: Vec3| [v.x, v.y, v.z, -(v.x * target.x + v.y * target.y + v.z * target.z)];
        Mat4 { m: [row(x), row(y), row(az), [0.0, 0.0, 0.0, 1.0]] }
    }
    /// The 2D affine map of this transform on the plane `z = elevation`: (x, y) on the plane →
    /// the transformed (x, y). Planar geometry drawn through it needs no 3D path.
    pub fn plane(&self, elevation: f64) -> Mat3 {
        let m = &self.m;
        Mat3 { a: m[0][0], b: m[1][0], c: m[0][1], d: m[1][1], e: m[0][2] * elevation + m[0][3], f: m[1][2] * elevation + m[1][3] }
    }
    /// Whether this is a plan view: x and y kept (up to a translation), z ignored.
    pub fn is_plan(&self) -> bool {
        let m = &self.m;
        (m[0][0] - 1.0).abs() < 1e-12
            && (m[1][1] - 1.0).abs() < 1e-12
            && m[0][1].abs() < 1e-12
            && m[1][0].abs() < 1e-12
            && m[0][2].abs() < 1e-12
            && m[1][2].abs() < 1e-12
    }
    /// The "arbitrary axis algorithm" frame for an extrusion direction (DXF OCS → WCS).
    pub fn ocs(normal: Vec3) -> Mat4 {
        let n = normal.normalized();
        if n == Vec3::ZERO || (n.x.abs() < 1e-12 && n.y.abs() < 1e-12 && n.z > 0.0) {
            return Mat4::IDENTITY;
        }
        let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { Vec3::new(0.0, 1.0, 0.0).cross(n) } else { Vec3::Z.cross(n) }.normalized();
        let ay = n.cross(ax).normalized();
        Mat4 { m: [[ax.x, ay.x, n.x, 0.0], [ax.y, ay.y, n.y, 0.0], [ax.z, ay.z, n.z, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }
}
