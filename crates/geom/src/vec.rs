use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// A 2D point or vector in drawing units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const X: Vec2 = Vec2 { x: 1.0, y: 0.0 };
    pub const Y: Vec2 = Vec2 { x: 0.0, y: 1.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
    /// Unit vector at `angle` radians.
    pub fn from_angle(angle: f64) -> Self {
        Vec2::new(angle.cos(), angle.sin())
    }
    pub fn polar(center: Vec2, radius: f64, angle: f64) -> Self {
        center + Vec2::from_angle(angle) * radius
    }
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// z of the 3D cross product.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn len2(self) -> f64 {
        self.dot(self)
    }
    pub fn dist(self, o: Vec2) -> f64 {
        (self - o).len()
    }
    pub fn angle(self) -> f64 {
        crate::norm_angle(self.y.atan2(self.x))
    }
    pub fn angle_to(self, o: Vec2) -> f64 {
        (o - self).angle()
    }
    /// Unit vector, or zero for a zero-length vector.
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l > crate::EPS { self / l } else { Vec2::ZERO }
    }
    /// Rotated 90° counter-clockwise.
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
    pub fn rotate(self, a: f64) -> Vec2 {
        let (s, c) = a.sin_cos();
        Vec2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    pub fn rotate_about(self, center: Vec2, a: f64) -> Vec2 {
        center + (self - center).rotate(a)
    }
    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
    pub fn mid(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.midpoint(o.x), self.y.midpoint(o.y))
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn near(self, o: Vec2, tol: f64) -> bool {
        // Squaring very large finite values can overflow on both sides and make
        // unrelated points compare equal (infinity <= infinity).
        tol.is_finite() && tol >= 0.0 && self.dist(o) <= tol
    }
    pub fn to3(self, z: f64) -> Vec3 {
        Vec3::new(self.x, self.y, z)
    }
    pub fn min(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.min(o.x), self.y.min(o.y))
    }
    pub fn max(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.max(o.x), self.y.max(o.y))
    }
    /// Mirror across the infinite line through `a` and `b`.
    pub fn mirror(self, a: Vec2, b: Vec2) -> Vec2 {
        let d = (b - a).normalized();
        if d == Vec2::ZERO {
            return self;
        }
        let v = self - a;
        let proj = d * v.dot(d);
        a + proj * 2.0 - v
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Div<f64> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f64) -> Vec2 {
        if s == 0.0 { Vec2::ZERO } else { Vec2::new(self.x / s, self.y / s) }
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// A 3D point or vector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const Z: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn len(self) -> f64 {
        // Avoid overflowing the squared components of large finite vectors.
        self.x.hypot(self.y).hypot(self.z)
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.len();
        if l > crate::EPS { self * (1.0 / l) } else { Vec3::ZERO }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midpoint_handles_finite_coordinate_extremes() {
        for (a, b, expected) in [
            (Vec2::new(-1e308, 1e308), Vec2::new(1e308, -1e308), Vec2::ZERO),
            (Vec2::new(1e308, -1e308), Vec2::new(1.6e308, -1.6e308), Vec2::new(1.3e308, -1.3e308)),
            (Vec2::new(f64::from_bits(1), 0.0), Vec2::new(f64::from_bits(1), 0.0), Vec2::new(f64::from_bits(1), 0.0)),
        ] {
            assert_eq!(a.mid(b), expected);
            assert_eq!(b.mid(a), expected);
        }
    }

    #[test]
    fn near_uses_finite_nonnegative_tolerance_without_squaring() {
        let origin = Vec2::ZERO;
        let distant = Vec2::new(1e200, 0.0);
        assert!(!distant.near(origin, 1e100), "overflowed squares must not compare equal");
        assert!(distant.near(origin, 1.1e200));
        assert!(origin.near(origin, 0.0));
        assert!(!origin.near(origin, -1.0));
        assert!(!origin.near(origin, f64::INFINITY));
    }

    #[test]
    fn large_finite_vec3_has_finite_length_and_unit_direction() {
        let v = Vec3::new(3e200, 4e200, 0.0);
        assert!((v.len() / 1e200 - 5.0).abs() < 1e-12);
        let unit = v.normalized();
        assert!((unit.x - 0.6).abs() < 1e-12);
        assert!((unit.y - 0.8).abs() < 1e-12);
        assert!((unit.len() - 1.0).abs() < 1e-12);
    }
}
