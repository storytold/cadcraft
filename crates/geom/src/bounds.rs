use serde::{Deserialize, Serialize};

use crate::Vec2;

/// Axis-aligned bounding box. An empty box has `min > max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds2 {
    pub min: Vec2,
    pub max: Vec2,
}

/// `max - min`, clamped to `f64::MAX` when finite endpoints make it overflow.
fn span(min: f64, max: f64) -> f64 {
    let d = max - min;
    if d.is_infinite() && min.is_finite() && max.is_finite() { f64::MAX } else { d }
}

impl Default for Bounds2 {
    fn default() -> Self {
        Bounds2::EMPTY
    }
}

impl Bounds2 {
    pub const EMPTY: Bounds2 = Bounds2 { min: Vec2 { x: f64::INFINITY, y: f64::INFINITY }, max: Vec2 { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY } };

    pub fn new(a: Vec2, b: Vec2) -> Self {
        Bounds2 { min: a.min(b), max: a.max(b) }
    }
    pub fn from_points<I: IntoIterator<Item = Vec2>>(it: I) -> Self {
        let mut b = Bounds2::EMPTY;
        for p in it {
            b.add(p);
        }
        b
    }
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }
    pub fn add(&mut self, p: Vec2) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    pub fn union(&self, o: &Bounds2) -> Bounds2 {
        if o.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *o;
        }
        Bounds2 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn width(&self) -> f64 {
        if self.is_empty() { 0.0 } else { span(self.min.x, self.max.x) }
    }
    pub fn height(&self) -> f64 {
        if self.is_empty() { 0.0 } else { span(self.min.y, self.max.y) }
    }
    pub fn center(&self) -> Vec2 {
        if self.is_empty() {
            return Vec2::ZERO;
        }
        // (max - min) can overflow even when both endpoints and their midpoint
        // are finite. Halve first when the endpoints have opposite signs.
        let mid = |a: f64, b: f64| {
            if a.is_sign_negative() != b.is_sign_negative() { a * 0.5 + b * 0.5 } else { a + (b - a) * 0.5 }
        };
        Vec2::new(mid(self.min.x, self.max.x), mid(self.min.y, self.max.y))
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    /// True when `o` lies entirely inside.
    pub fn contains_box(&self, o: &Bounds2) -> bool {
        !o.is_empty() && self.contains(o.min) && self.contains(o.max)
    }
    pub fn intersects(&self, o: &Bounds2) -> bool {
        !(self.is_empty() || o.is_empty() || o.min.x > self.max.x || o.max.x < self.min.x || o.min.y > self.max.y || o.max.y < self.min.y)
    }
    pub fn expand(&self, d: f64) -> Bounds2 {
        if self.is_empty() {
            return *self;
        }
        Bounds2 { min: self.min - Vec2::new(d, d), max: self.max + Vec2::new(d, d) }
    }
    pub fn corners(&self) -> [Vec2; 4] {
        [self.min, Vec2::new(self.max.x, self.min.y), self.max, Vec2::new(self.min.x, self.max.y)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_stays_finite_for_opposite_large_coordinates() {
        let b = Bounds2::new(Vec2::new(-1e308, -1e308), Vec2::new(1e308, 1e308));
        assert_eq!(b.center(), Vec2::ZERO);
        let b = Bounds2::new(Vec2::new(1e308, 0.0), Vec2::new(1.6e308, 10.0));
        assert!(b.center().x.is_finite());
        assert!((b.center().x / 1e308 - 1.3).abs() < 1e-12);
        assert_eq!(b.center().y, 5.0);
        assert_eq!(Bounds2::EMPTY.center(), Vec2::ZERO);
    }

    #[test]
    fn width_and_height_stay_finite_for_opposite_large_coordinates() {
        let b = Bounds2::new(Vec2::new(-1e308, -1e308), Vec2::new(1e308, 1e308));
        assert_eq!(b.width(), f64::MAX);
        assert_eq!(b.height(), f64::MAX);
        let b = Bounds2::new(Vec2::new(1e308, 0.0), Vec2::new(1.6e308, 10.0));
        assert!((b.width() / 1e308 - 0.6).abs() < 1e-12);
        assert_eq!(b.height(), 10.0);
    }
}
