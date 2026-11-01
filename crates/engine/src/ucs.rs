//! Validated user-coordinate frames and the active frame for each drawing.

use cadcraft_doc::Ucs;
use cadcraft_geom::Vec3;

/// Orthonormal WCS basis for a user coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl Frame {
    pub const WORLD: Self = Self { origin: Vec3::ZERO, x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0), z: Vec3::new(0.0, 0.0, 1.0) };

    /// Construct a right-handed orthonormal frame from an origin and two directions. The Y
    /// direction is projected onto the plane normal to X, as in a three-point UCS.
    pub fn from_axes(origin: Vec3, x_axis: Vec3, y_axis: Vec3) -> Option<Self> {
        if !bounded(origin) || !bounded(x_axis) || !bounded(y_axis) {
            return None;
        }
        let x = x_axis.normalized();
        if x == Vec3::ZERO {
            return None;
        }
        let y0 = y_axis - x * x.dot(y_axis);
        let y = y0.normalized();
        if y == Vec3::ZERO {
            return None;
        }
        let z = x.cross(y).normalized();
        if z == Vec3::ZERO {
            return None;
        }
        Some(Self { origin, x, y: z.cross(x).normalized(), z })
    }

    pub fn from_record(record: &Ucs) -> Option<Self> {
        Self::from_axes(record.origin, record.x_axis, record.y_axis)
    }

    pub fn to_record(self, name: impl Into<String>) -> Ucs {
        Ucs { name: name.into(), origin: self.origin, x_axis: self.x, y_axis: self.y }
    }

    /// Convert a local UCS coordinate to WCS.
    pub fn to_world(self, p: Vec3) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }

    /// Convert a WCS coordinate to local UCS coordinates.
    pub fn from_world(self, p: Vec3) -> Vec3 {
        let q = p - self.origin;
        Vec3::new(q.dot(self.x), q.dot(self.y), q.dot(self.z))
    }

    /// Convert a local UCS displacement to WCS without applying the origin.
    pub fn vector_to_world(self, v: Vec3) -> Vec3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::WORLD
    }
}

impl crate::Session {
    /// The active drawing's coordinate frame.
    pub fn active_ucs(&self) -> Frame {
        self.state().map_or(Frame::WORLD, |state| state.active_ucs)
    }

    /// Change the active drawing's UCS while retaining its previous frame.
    pub fn set_active_ucs(&mut self, frame: Frame) -> crate::Result<()> {
        let state = self.state_mut()?;
        if state.active_ucs != frame {
            state.previous_ucs = state.active_ucs;
            state.active_ucs = frame;
        }
        Ok(())
    }

    /// Restore the UCS that was active before the most recent UCS change.
    pub fn previous_ucs(&mut self) -> crate::Result<()> {
        let state = self.state_mut()?;
        std::mem::swap(&mut state.active_ucs, &mut state.previous_ucs);
        Ok(())
    }
}

fn bounded(v: Vec3) -> bool {
    const LIMIT: f64 = 1.0e100;
    v.is_finite() && v.x.abs() <= LIMIT && v.y.abs() <= LIMIT && v.z.abs() <= LIMIT
}

/// Parse `x,y,z` or relative `@dx,dy,dz` local UCS coordinates. Returns the local point,
/// relative to `last_local` when prefixed with `@`.
pub fn parse_point3(text: &str, last_local: Vec3) -> Option<Vec3> {
    let t = text.trim();
    let (relative, body) = match t.strip_prefix('@') {
        Some(body) => (true, body),
        None => (false, t.strip_prefix('#').unwrap_or(t)),
    };
    let mut values = body.split(',').map(|v| v.trim().parse::<f64>().ok());
    let (Some(Some(x)), Some(Some(y)), Some(Some(z))) = (values.next(), values.next(), values.next()) else { return None };
    if values.next().is_some() || ![x, y, z].iter().all(|v| v.is_finite()) {
        return None;
    }
    let p = Vec3::new(x, y, z);
    let p = if relative { last_local + p } else { p };
    bounded(p).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_frame_round_trips_points_and_vectors() {
        let f = Frame::from_axes(Vec3::new(4.0, -2.0, 8.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(-1.0, 1.0, 3.0)).unwrap();
        let p = Vec3::new(3.0, -5.0, 2.0);
        assert!((f.from_world(f.to_world(p)) - p).len() < 1e-9);
        assert!((f.from_world(f.origin + f.vector_to_world(p)) - p).len() < 1e-9);
    }

    #[test]
    fn xyz_parser_requires_three_finite_components() {
        let last = Vec3::new(10.0, 20.0, 30.0);
        assert_eq!(parse_point3("1,2,3", last), Some(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(parse_point3("@1,-2,3", last), Some(Vec3::new(11.0, 18.0, 33.0)));
        assert_eq!(parse_point3("#1,2,3", last), Some(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(parse_point3("1,2", last), None);
        assert_eq!(parse_point3("1,2,NaN", last), None);
    }
}
