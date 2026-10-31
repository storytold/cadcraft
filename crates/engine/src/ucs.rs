//! The user coordinate system (UCS): the coordinate system points are typed in, coordinates are
//! shown in and directions are measured in.
//!
//! Model space keeps its current UCS in the drawing header (UCSORG, UCSXDIR, UCSYDIR, UCSNAME,
//! as a DXF file does), each layout its paper-space UCS ([`cadcraft_doc::Layout::ucs`]). The
//! drawing itself stays in world coordinates: a UCS changes how points are read and shown,
//! never the stored geometry, so it costs nothing while drawing or regenerating.
//!
//! CADCraft draws in the world XY plane. A UCS whose XY plane is that plane (any origin, any
//! rotation about Z) is used in full; one tilted out of it is kept and saved as it is, while
//! points are read in the World UCS ([`Session::input_ucs`]).

use cadcraft_doc::{Drawing, HVal, Space, Ucs};
use cadcraft_geom::{Vec2, Vec3};

use crate::{Result, Session};

/// How many earlier UCSs UCS Previous goes back through, in each space.
pub const MAX_PREVIOUS: usize = 10;

/// How close to the World UCS, or to the world XY plane, counts as on it.
const TOL: f64 = 1e-9;

/// A coordinate system: its origin and unit X and Y axes, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UcsFrame {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
}

impl Default for UcsFrame {
    fn default() -> Self {
        Self::WORLD
    }
}

impl UcsFrame {
    pub const WORLD: UcsFrame = UcsFrame { origin: Vec3::ZERO, x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0) };

    /// A frame from an origin and X and Y directions: X is normalised and Y made square to it in
    /// their plane. `None` for non-finite values or directions with no length or along each other.
    pub fn new(origin: Vec3, x: Vec3, y: Vec3) -> Option<UcsFrame> {
        if !(origin.is_finite() && x.is_finite() && y.is_finite()) {
            return None;
        }
        let xl = x.len();
        if !(xl > TOL && xl.is_finite()) {
            return None;
        }
        let x = x * (1.0 / xl);
        let yo = y.len();
        if !(yo > TOL && yo.is_finite()) {
            return None;
        }
        // What is left of Y square to X: next to nothing means the two lie along each other.
        let y = y - x * x.dot(y);
        let yl = y.len();
        if !yl.is_finite() || yl <= 1e-9 * yo {
            return None;
        }
        Some(UcsFrame { origin, x, y: y * (1.0 / yl) })
    }

    /// A UCS table record's frame (`None`: unusable axes).
    pub fn of(u: &Ucs) -> Option<UcsFrame> {
        UcsFrame::new(u.origin, u.x_axis, u.y_axis)
    }

    /// This frame as a UCS table record named `name`.
    pub fn record(&self, name: &str) -> Ucs {
        Ucs { name: name.to_string(), origin: self.origin, x_axis: self.x, y_axis: self.y }
    }

    pub fn z(&self) -> Vec3 {
        self.x.cross(self.y)
    }

    /// The World UCS (to within rounding).
    pub fn is_world(&self) -> bool {
        self.origin.len() <= TOL * (1.0 + self.origin.len()) && self.is_planar() && (self.x - Vec3::new(1.0, 0.0, 0.0)).len() <= TOL
    }

    /// Its XY plane is the world XY plane with Z up (any origin height, any rotation about Z).
    pub fn is_planar(&self) -> bool {
        self.x.z.abs() <= TOL && self.y.z.abs() <= TOL && self.z().z > 0.0
    }

    /// The direction of its X axis in the world XY plane, radians counterclockwise from +X.
    pub fn rotation(&self) -> f64 {
        self.x.xy().angle()
    }

    /// A point of the UCS XY plane, in world coordinates (planar frames; a tilted frame's XY
    /// plane seen from above).
    pub fn to_wcs(&self, p: Vec2) -> Vec2 {
        self.origin.xy() + self.vec_to_wcs(p)
    }

    /// A world point in UCS coordinates (planar frames).
    pub fn to_ucs(&self, p: Vec2) -> Vec2 {
        self.vec_to_ucs(p - self.origin.xy())
    }

    /// A displacement in the UCS XY plane, in world coordinates.
    pub fn vec_to_wcs(&self, v: Vec2) -> Vec2 {
        self.x.xy() * v.x + self.y.xy() * v.y
    }

    /// A world displacement in UCS coordinates.
    pub fn vec_to_ucs(&self, v: Vec2) -> Vec2 {
        Vec2::new(v.dot(self.x.xy()), v.dot(self.y.xy()))
    }

    /// A UCS point in world coordinates, in 3D.
    pub fn to_wcs3(&self, p: Vec3) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z() * p.z
    }

    /// A world point in UCS coordinates, in 3D.
    pub fn to_ucs3(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z()))
    }

    /// The same axes at another origin.
    pub fn with_origin(&self, origin: Vec3) -> UcsFrame {
        UcsFrame { origin, ..*self }
    }

    /// Turned by `angle` radians about its own Z axis (counterclockwise looking down Z).
    pub fn rotated_z(&self, angle: f64) -> UcsFrame {
        let (s, c) = angle.sin_cos();
        UcsFrame { origin: self.origin, x: self.x * c + self.y * s, y: self.y * c - self.x * s }
    }

    /// UCS from points (world coordinates): a new origin; with a point on the positive X axis,
    /// X points at it; with a point on the positive-Y half of the XY plane too, that fixes Y.
    /// Without the third point Y stays square to X and to this frame's Z. `None` when the
    /// points don't fix an axis (on top of each other, or all three on a line).
    pub fn from_points(&self, origin: Vec3, x_point: Option<Vec3>, xy_point: Option<Vec3>) -> Option<UcsFrame> {
        let Some(xp) = x_point else { return origin.is_finite().then(|| self.with_origin(origin)) };
        let x = xp - origin;
        match xy_point {
            Some(yp) => UcsFrame::new(origin, x, yp - origin),
            None => {
                let xl = x.len();
                if !(xl > TOL && xl.is_finite()) {
                    return None;
                }
                let y = self.z().cross(x * (1.0 / xl));
                UcsFrame::new(origin, x, y)
            }
        }
    }
}

/// The current UCS of `space` in `d` and its name (empty when it isn't a named UCS). A missing
/// or unusable one is the World UCS.
pub fn current(d: &Drawing, space: &Space) -> (UcsFrame, String) {
    match space {
        Space::Model => {
            let h = &d.header;
            let frame = match (h.point("UCSORG"), h.point("UCSXDIR"), h.point("UCSYDIR")) {
                (Some(o), Some(x), Some(y)) => UcsFrame::new(o, x, y),
                (o, None, None) => o.filter(|o| o.is_finite()).map(|o| UcsFrame::WORLD.with_origin(o)),
                _ => None,
            };
            match frame {
                Some(f) => (f, h.str("UCSNAME", "").trim().to_string()),
                None => (UcsFrame::WORLD, String::new()),
            }
        }
        Space::Paper(name) => match d.layout(name).and_then(|l| l.ucs.as_ref()).and_then(|u| Some((UcsFrame::of(u)?, u.name.trim().to_string()))) {
            Some(x) => x,
            None => (UcsFrame::WORLD, String::new()),
        },
    }
}

/// Make `frame` (named `name`, empty for an unnamed one) the current UCS of `space`.
pub fn set_current(d: &mut Drawing, space: &Space, frame: &UcsFrame, name: &str) {
    match space {
        Space::Model => {
            let h = &mut d.header;
            h.set("UCSORG", HVal::Point(frame.origin));
            h.set("UCSXDIR", HVal::Point(frame.x));
            h.set("UCSYDIR", HVal::Point(frame.y));
            h.set_str("UCSNAME", name);
        }
        Space::Paper(layout) => {
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(layout)) {
                l.ucs = (!(frame.is_world() && name.is_empty())).then(|| frame.record(name));
            }
        }
    }
}

/// `space`'s UCS Previous history, kept in the drawing (so undo restores it) and, for a layout,
/// with the layout (so it follows a rename and goes with a delete). `None`: no such layout.
fn previous_mut<'a>(d: &'a mut Drawing, space: &Space) -> Option<&'a mut Vec<Ucs>> {
    match space {
        Space::Model => Some(&mut d.ucs_previous),
        Space::Paper(layout) => d.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(layout)).map(|l| &mut l.ucs_previous),
    }
}

/// The named UCS `name` of the drawing's UCS table, if it has one with usable axes.
pub fn named(d: &Drawing, name: &str) -> Option<(UcsFrame, String)> {
    let u = d.ucss.iter().find(|u| u.name.eq_ignore_ascii_case(name.trim()))?;
    Some((UcsFrame::of(u)?, u.name.clone()))
}

impl Session {
    /// The current UCS of the space commands act on, and its name.
    pub fn ucs_named(&self) -> (UcsFrame, String) {
        self.doc().map(|d| current(d, &self.space())).unwrap_or_default()
    }

    /// The current UCS of the space commands act on.
    pub fn ucs(&self) -> UcsFrame {
        self.ucs_named().0
    }

    /// The UCS points are typed in and shown in: the current one, or the World UCS while the
    /// current one is tilted out of the XY plane (CADCraft draws in that plane).
    pub fn input_ucs(&self) -> UcsFrame {
        let u = self.ucs();
        if u.is_planar() { u } else { UcsFrame::WORLD }
    }

    /// A typed point as the user means it: in the current UCS (`*` for world coordinates), with
    /// the drawing's angle settings; relative forms from `last`. A world point.
    pub fn parse_typed_point(&self, text: &str, last: Vec2) -> Option<Vec2> {
        crate::prompt::parse_point_in(text, last, &self.angle_settings(), &self.input_ucs())
    }

    /// A world point in the coordinates shown to the user (the current UCS).
    pub fn ucs_point(&self, p: Vec2) -> Vec2 {
        self.input_ucs().to_ucs(p)
    }

    /// A world direction (radians from world +X) measured from the current UCS X axis.
    pub fn ucs_angle(&self, a: f64) -> f64 {
        cadcraft_geom::norm_angle(a - self.input_ucs().rotation())
    }

    /// Make `frame` the current UCS (named `name`; empty for an unnamed one), remembering the one
    /// it replaces for UCS Previous. Part of the running command's undo step.
    pub fn set_ucs(&mut self, frame: UcsFrame, name: &str) -> Result<()> {
        let space = self.space();
        let (old, old_name) = self.ucs_named();
        if old == frame && old_name.eq_ignore_ascii_case(name) {
            return Ok(());
        }
        let d = self.doc_mut()?;
        set_current(d, &space, &frame, name);
        if let Some(h) = previous_mut(d, &space) {
            h.push(old.record(&old_name));
            // At most MAX_PREVIOUS: drop the oldest.
            if h.len() > MAX_PREVIOUS {
                h.remove(0);
            }
        }
        Ok(())
    }

    /// Go back to the UCS before the current one in this space; `false` when there is none.
    pub fn ucs_previous(&mut self) -> Result<bool> {
        let space = self.space();
        let d = self.doc_mut()?;
        // An entry with unusable axes (none is ever stored) is skipped, not restored.
        while let Some(u) = previous_mut(d, &space).and_then(|h| h.pop()) {
            if let Some(frame) = UcsFrame::of(&u) {
                set_current(d, &space, &frame, &u.name);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// UCSICON: 1 the icon is shown, 2 at the UCS origin (when that is on screen).
    pub fn ucsicon_bits(&self) -> i64 {
        self.doc().map(|d| d.header.i64("UCSICON", 3)).unwrap_or(3).clamp(0, 3)
    }

    /// Tell the user once per drawing that its current UCS is tilted and points are read in the
    /// World UCS for now.
    pub(crate) fn note_tilted_ucs(&mut self) {
        if self.ucs().is_planar() {
            return;
        }
        let Ok(st) = self.state_mut() else { return };
        if std::mem::replace(&mut st.ucs_tilted_noted, true) {
            return;
        }
        self.echo("The current UCS is tilted out of the XY plane; points are read and shown in the World UCS for now.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec2, b: Vec2) -> bool {
        a.dist(b) < 1e-9
    }

    #[test]
    fn world_is_the_identity() {
        let w = UcsFrame::WORLD;
        assert!(w.is_world() && w.is_planar());
        assert_eq!(w.rotation(), 0.0);
        let p = Vec2::new(3.5, -2.25);
        assert_eq!(w.to_wcs(p), p);
        assert_eq!(w.to_ucs(p), p);
    }

    #[test]
    fn a_rotated_moved_frame_maps_both_ways() {
        let f = UcsFrame::WORLD.with_origin(Vec3::new(100.0, 50.0, 0.0)).rotated_z(30f64.to_radians());
        assert!(f.is_planar() && !f.is_world());
        assert!((f.rotation() - 30f64.to_radians()).abs() < 1e-12);
        // The UCS X axis one unit out is 30° from world +X.
        assert!(near(f.to_wcs(Vec2::new(1.0, 0.0)), Vec2::new(100.0 + 30f64.to_radians().cos(), 50.0 + 30f64.to_radians().sin())));
        for p in [Vec2::new(0.0, 0.0), Vec2::new(12.0, -7.0), Vec2::new(-1e6, 3e5)] {
            assert!(near(f.to_ucs(f.to_wcs(p)), p), "{p:?}");
            let q = Vec3::new(p.x, p.y, 4.0);
            assert!((f.to_ucs3(f.to_wcs3(q)) - q).len() < 1e-9);
        }
        assert!(near(f.vec_to_ucs(f.vec_to_wcs(Vec2::new(2.0, 3.0))), Vec2::new(2.0, 3.0)));
    }

    #[test]
    fn new_squares_and_rejects() {
        let f = UcsFrame::new(Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 5.0, 0.0)).unwrap();
        assert_eq!((f.x, f.y), (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)));
        assert!(UcsFrame::new(Vec3::ZERO, Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0)).is_none());
        assert!(UcsFrame::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0)).is_none());
        assert!(UcsFrame::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)).is_none());
        assert!(UcsFrame::new(Vec3::ZERO, Vec3::new(f64::INFINITY, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)).is_none());
    }

    #[test]
    fn tilted_and_flipped_frames_are_not_planar() {
        let front = UcsFrame::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)).unwrap();
        assert!(!front.is_planar());
        let bottom = UcsFrame::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0)).unwrap();
        assert!(!bottom.is_planar(), "Z down");
        let raised = UcsFrame::WORLD.with_origin(Vec3::new(0.0, 0.0, 25.0));
        assert!(raised.is_planar() && !raised.is_world());
    }

    #[test]
    fn from_points() {
        let w = UcsFrame::WORLD;
        let o = Vec3::new(10.0, 10.0, 0.0);
        assert_eq!(w.from_points(o, None, None), Some(w.with_origin(o)));
        let f = w.from_points(o, Some(Vec3::new(10.0, 20.0, 0.0)), None).unwrap();
        assert!((f.rotation() - std::f64::consts::FRAC_PI_2).abs() < 1e-12 && f.is_planar());
        // A third point on the negative-Y side of the X axis turns the UCS over.
        let g = w.from_points(o, Some(Vec3::new(20.0, 10.0, 0.0)), Some(Vec3::new(15.0, 0.0, 0.0))).unwrap();
        assert!(!g.is_planar());
        assert!(w.from_points(o, Some(o), None).is_none());
        assert!(w.from_points(o, Some(Vec3::new(20.0, 10.0, 0.0)), Some(Vec3::new(30.0, 10.0, 0.0))).is_none());
    }

    #[test]
    fn stored_per_space() {
        let mut d = Drawing::default();
        let paper = Space::Paper(d.layouts[0].name.clone());
        let f = UcsFrame::WORLD.with_origin(Vec3::new(5.0, 5.0, 0.0)).rotated_z(0.5);
        set_current(&mut d, &Space::Model, &f, "SITE");
        let (m, n) = current(&d, &Space::Model);
        assert!((m.origin - f.origin).len() < 1e-12 && (m.x - f.x).len() < 1e-12 && n == "SITE");
        assert_eq!(current(&d, &paper).0, UcsFrame::WORLD, "paper space has its own");
        set_current(&mut d, &paper, &f, "");
        assert!(d.layouts[0].ucs.is_some());
        set_current(&mut d, &paper, &UcsFrame::WORLD, "");
        assert!(d.layouts[0].ucs.is_none());
        // Broken header values are the World UCS.
        d.header.set("UCSXDIR", HVal::Point(Vec3::ZERO));
        assert_eq!(current(&d, &Space::Model).0, UcsFrame::WORLD);
        d.header.set("UCSXDIR", HVal::Point(Vec3::new(f64::NAN, 0.0, 0.0)));
        assert_eq!(current(&d, &Space::Model).0, UcsFrame::WORLD);
    }
}
