//! XCLIP: the clipping boundary of one block or xref reference.
//!
//! The boundary is kept in the referenced block's coordinates, so it moves, rotates and scales
//! with its reference; the block definition and other references to it are untouched. In a file
//! it is the reference's `ACAD_FILTER` / `SPATIAL` extension-dictionary entry (a SPATIAL_FILTER
//! object, DXF Reference), read and written by the io crate.

use cadcraft_geom::Vec2;
use serde::{Deserialize, Serialize};

/// Most boundary vertices a clip keeps (hostile files and parameters are capped to this).
pub const MAX_CLIP_VERTICES: usize = 4096;

/// A reference's clipping boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipBoundary {
    /// Vertices in the block's coordinates: two opposite corners for a rectangle, three or more
    /// for a closed polygon (not repeating the first vertex).
    pub points: Vec<Vec2>,
    /// Whether clipping is on (XCLIP ON/OFF); an off boundary is kept but nothing is clipped.
    #[serde(default = "on")]
    pub enabled: bool,
    /// Inverted clip: what is inside the boundary is hidden, what is outside shows.
    #[serde(default)]
    pub inverted: bool,
}

fn on() -> bool {
    true
}

impl ClipBoundary {
    /// A boundary from `points` (block coordinates), on and not inverted.
    pub fn new(points: Vec<Vec2>) -> Self {
        ClipBoundary { points, enabled: true, inverted: false }
    }

    /// The closed outline in block coordinates: a rectangle's four corners or the polygon's
    /// vertices. Empty when the boundary has fewer than two vertices, more than
    /// [`MAX_CLIP_VERTICES`] or a non-finite one.
    pub fn polygon(&self) -> Vec<Vec2> {
        if self.points.len() > MAX_CLIP_VERTICES || !self.points.iter().all(|p| p.is_finite()) {
            return Vec::new();
        }
        match self.points.as_slice() {
            [a, b] => vec![
                Vec2::new(a.x.min(b.x), a.y.min(b.y)),
                Vec2::new(a.x.max(b.x), a.y.min(b.y)),
                Vec2::new(a.x.max(b.x), a.y.max(b.y)),
                Vec2::new(a.x.min(b.x), a.y.max(b.y)),
            ],
            [_, _, _, ..] => self.points.clone(),
            _ => Vec::new(),
        }
    }

    /// Whether the boundary is a rectangle given by two corners.
    pub fn is_rectangle(&self) -> bool {
        self.points.len() == 2
    }

    /// Whether drawing the reference is clipped by it now (on, with a usable outline).
    pub fn active(&self) -> bool {
        self.enabled && self.polygon().len() >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_and_polygon_outlines() {
        let r = ClipBoundary::new(vec![Vec2::new(2.0, 3.0), Vec2::new(0.0, 0.0)]);
        assert_eq!(r.polygon(), vec![Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 3.0), Vec2::new(0.0, 3.0)]);
        assert!(r.active() && r.is_rectangle());
        let bad = ClipBoundary::new(vec![Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0)]);
        assert!(bad.polygon().is_empty() && !bad.active());
        let one = ClipBoundary::new(vec![Vec2::ZERO]);
        assert!(!one.active());
        let off = ClipBoundary { enabled: false, ..r };
        assert!(!off.active());
    }
}
