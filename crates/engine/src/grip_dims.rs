//! Grip dimensional input: while one grip is stretched, Dynamic Input shows the dimensions the
//! stretch changes (a line's length and angle, a polyline segment's, a circle's radius) as value
//! fields, and a typed value places the grip.
//!
//! Which dimensions show follows DYNDIVIS (0: the first, 1: the first two, 2: all of DYNDIGRIP's
//! bits: 1 resulting length, 2 length change, 4 absolute angle, 8 angle change, 16 radius). Each
//! dimension is measured from an anchor that stays put (the line's other end, the neighbouring
//! vertex, the circle's centre). Typed values lock: one locked value holds the grip to a circle or
//! a ray around its anchor, two pin it to their crossing.
//!
//! The UI reads [`grip_dims`] and sends the resolved point as a plain point input, so the edit
//! itself is the GRIP command's.

use cadcraft_doc::EntityKind;
use cadcraft_geom::{TAU, Vec2};
use serde::Serialize;

use crate::Session;

/// What a dimension measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GripDimKind {
    /// The resulting length from the anchor (DYNDIGRIP 1).
    Length,
    /// The change in that length (DYNDIGRIP 2).
    LengthChange,
    /// The absolute angle from the anchor (DYNDIGRIP 4).
    Angle,
    /// The change in that angle (DYNDIGRIP 8).
    AngleChange,
    /// A circle's radius (DYNDIGRIP 16).
    Radius,
}

impl GripDimKind {
    /// The DYNDIGRIP bit that shows it.
    pub fn bit(self) -> u32 {
        match self {
            GripDimKind::Length => 1,
            GripDimKind::LengthChange => 2,
            GripDimKind::Angle => 4,
            GripDimKind::AngleChange => 8,
            GripDimKind::Radius => 16,
        }
    }
    /// The value is an angle (radians), not a length.
    pub fn is_angle(self) -> bool {
        matches!(self, GripDimKind::Angle | GripDimKind::AngleChange)
    }
}

/// One dimension of a grip being stretched.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GripDim {
    pub kind: GripDimKind,
    /// The point it is measured from, which the stretch doesn't move.
    pub anchor: Vec2,
    /// The length or angle before the stretch (what the change dimensions are measured against).
    pub original: f64,
}

/// Where the grip may go once a value is locked.
#[derive(Clone, Copy, Debug)]
enum Locus {
    Circle(Vec2, f64),
    Ray(Vec2, Vec2),
}

/// An angle in [0, 2π).
fn wrap(a: f64) -> f64 {
    let r = a.rem_euclid(TAU);
    if r >= TAU { 0.0 } else { r }
}

/// An angle in (-π, π].
fn wrap_signed(a: f64) -> f64 {
    let r = wrap(a);
    if r > std::f64::consts::PI { r - TAU } else { r }
}

impl GripDim {
    fn length(kind: GripDimKind, anchor: Vec2, grip: Vec2) -> Self {
        GripDim { kind, anchor, original: anchor.dist(grip) }
    }
    fn angle(kind: GripDimKind, anchor: Vec2, grip: Vec2) -> Self {
        GripDim { kind, anchor, original: wrap(anchor.angle_to(grip)) }
    }

    /// The value it reads with the grip at `p` (radians for angles).
    pub fn value(&self, p: Vec2) -> f64 {
        let (len, ang) = (self.anchor.dist(p), wrap(self.anchor.angle_to(p)));
        match self.kind {
            GripDimKind::Length | GripDimKind::Radius => len,
            GripDimKind::LengthChange => len - self.original,
            GripDimKind::Angle => ang,
            GripDimKind::AngleChange => wrap_signed(ang - self.original),
        }
    }

    /// Where the grip may go once this dimension is locked at `v`; `None` for a length that isn't
    /// positive.
    fn locus(&self, v: f64) -> Option<Locus> {
        let circle = |r: f64| (r.is_finite() && r > 1e-12).then_some(Locus::Circle(self.anchor, r));
        let ray = |a: f64| a.is_finite().then(|| Locus::Ray(self.anchor, Vec2::from_angle(a)));
        match self.kind {
            GripDimKind::Length | GripDimKind::Radius => circle(v),
            GripDimKind::LengthChange => circle(self.original + v),
            GripDimKind::Angle => ray(v),
            GripDimKind::AngleChange => ray(self.original + v),
        }
    }

    /// Read a typed value: a distance, or an angle (absolute in the drawing's angle settings, or
    /// a signed change in its angle units).
    pub fn parse(&self, s: &Session, text: &str) -> Option<f64> {
        let au = s.angle_settings();
        let v = match self.kind {
            GripDimKind::Angle => au.direction(text),
            GripDimKind::AngleChange => au.rotation(text),
            _ => crate::units::parse_distance(text),
        };
        v.filter(|v| v.is_finite())
    }
}

impl Locus {
    /// The closest point of the locus to `p`.
    fn closest(self, p: Vec2) -> Vec2 {
        match self {
            Locus::Circle(c, r) => {
                let d = p - c;
                let l = d.len();
                c + if l > 1e-12 { d * (r / l) } else { Vec2::X * r }
            }
            Locus::Ray(o, dir) => o + dir * (p - o).dot(dir).max(0.0),
        }
    }

    /// The points where two loci cross.
    fn crossings(self, other: Locus) -> Vec<Vec2> {
        match (self, other) {
            (Locus::Circle(c, r), Locus::Ray(o, d)) | (Locus::Ray(o, d), Locus::Circle(c, r)) => {
                // |o + t d - c| = r, t >= 0.
                let f = o - c;
                let b = f.dot(d);
                let disc = b * b - (f.dot(f) - r * r);
                if disc < 0.0 {
                    return Vec::new();
                }
                let s = disc.sqrt();
                [-b - s, -b + s].into_iter().filter(|t| *t >= -1e-9).map(|t| o + d * t).collect()
            }
            (Locus::Circle(c1, r1), Locus::Circle(c2, r2)) => {
                let d = c2 - c1;
                let l = d.len();
                if l < 1e-12 || l > r1 + r2 + 1e-9 || l < (r1 - r2).abs() - 1e-9 {
                    return Vec::new();
                }
                let a = (r1 * r1 - r2 * r2 + l * l) / (2.0 * l);
                let h = (r1 * r1 - a * a).max(0.0).sqrt();
                let m = c1 + d * (a / l);
                let n = d.perp() * (h / l);
                vec![m + n, m - n]
            }
            (Locus::Ray(o1, d1), Locus::Ray(o2, d2)) => {
                let den = d1.cross(d2);
                if den.abs() < 1e-12 {
                    return Vec::new();
                }
                let w = o2 - o1;
                let (t, u) = (w.cross(d2) / den, w.cross(d1) / den);
                if t >= -1e-9 && u >= -1e-9 { vec![o1 + d1 * t] } else { Vec::new() }
            }
        }
    }
}

/// The dimensions to show for the grip the current prompt stretches, in cycle order (Tab), as
/// DYNDIVIS and DYNDIGRIP ask; empty when no single grip is being stretched or Dynamic Input
/// is off.
pub fn grip_dims(s: &Session) -> Vec<GripDim> {
    if !s.settings.dynmode {
        return Vec::new();
    }
    let Some((h, i)) = s.current_prompt().and_then(|p| p.grip) else { return Vec::new() };
    let Ok(d) = s.doc() else { return Vec::new() };
    let Some(e) = d.entity(h) else { return Vec::new() };
    let all = dims_of(&d.anno_shown_in(e, s.anno_view()), i);
    let st = &s.settings;
    match st.dyndivis {
        0 => all.into_iter().take(1).collect(),
        1 => all.into_iter().take(2).collect(),
        _ => all.into_iter().filter(|d| st.dyndigrip & d.kind.bit() != 0).collect(),
    }
}

/// Every dimension a grip's stretch changes, in cycle order.
fn dims_of(k: &EntityKind, grip: usize) -> Vec<GripDim> {
    use GripDimKind::*;
    let grips = k.grips();
    let Some(&g) = grips.get(grip) else { return Vec::new() };
    let mut anchors: Vec<Vec2> = Vec::new();
    match k {
        // An end of a line: measured from the other end (the middle grip moves the whole line).
        EntityKind::Line(l) => match grip {
            0 => anchors.push(l.b.xy()),
            2 => anchors.push(l.a.xy()),
            _ => {}
        },
        // A quadrant of a circle: its radius.
        EntityKind::Circle(c) if grip > 0 => {
            let o = c.center.xy();
            return vec![GripDim::length(Radius, o, g), GripDim::length(LengthChange, o, g)];
        }
        // A vertex of a polyline: the straight segments on either side.
        EntityKind::LwPolyline(p) if grip.is_multiple_of(2) => {
            let n = p.vertices.len();
            let k = grip / 2;
            let straight = |seg: usize| p.vertices.get(seg).is_some_and(|v| v.bulge.abs() < 1e-12);
            let prev = if k > 0 {
                Some(k - 1)
            } else if p.closed && n > 2 {
                Some(n - 1)
            } else {
                None
            };
            let next = if k + 1 < n {
                Some(k + 1)
            } else if p.closed && n > 2 {
                Some(0)
            } else {
                None
            };
            if let Some(j) = prev.filter(|j| straight(*j))
                && let Some(v) = p.vertices.get(j)
            {
                anchors.push(v.p);
            }
            if let Some(j) = next.filter(|_| straight(k))
                && let Some(v) = p.vertices.get(j)
            {
                anchors.push(v.p);
            }
        }
        _ => {}
    }
    let anchors: Vec<Vec2> = anchors.into_iter().filter(|a| a.is_finite() && a.dist(g) > 1e-12).collect();
    let mut out: Vec<GripDim> = anchors.iter().map(|a| GripDim::length(Length, *a, g)).collect();
    out.extend(anchors.iter().map(|a| GripDim::length(LengthChange, *a, g)));
    // The angle of the first segment only (the others follow from the lengths).
    if let Some(a) = anchors.first() {
        out.push(GripDim::angle(Angle, *a, g));
        out.push(GripDim::angle(AngleChange, *a, g));
    }
    out
}

/// The grip's point for the cursor at `cursor` with the `locked` dimension values (index into
/// `dims`, value): none locked follows the cursor; one holds it to that dimension's circle or ray;
/// two or more pin it where the first two cross (the crossing nearest the cursor).
pub fn resolve(dims: &[GripDim], locked: &[(usize, f64)], cursor: Vec2) -> Vec2 {
    let loci: Vec<Locus> = locked.iter().filter_map(|(i, v)| dims.get(*i)?.locus(*v)).take(2).collect();
    match loci.as_slice() {
        [] => cursor,
        [a] => a.closest(cursor),
        [a, b, ..] => a
            .crossings(*b)
            .into_iter()
            .filter(|p| p.is_finite())
            .min_by(|p, q| p.dist(cursor).total_cmp(&q.dist(cursor)))
            .unwrap_or_else(|| a.closest(cursor)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.dist(b) < 1e-9
    }

    #[test]
    fn a_line_end_measures_from_the_other_end() {
        let l = EntityKind::Line(cadcraft_doc::Line { a: Vec2::new(0.0, 0.0).to3(0.0), b: Vec2::new(10.0, 0.0).to3(0.0) });
        let d = dims_of(&l, 2);
        let kinds: Vec<_> = d.iter().map(|d| d.kind).collect();
        assert_eq!(kinds, [GripDimKind::Length, GripDimKind::LengthChange, GripDimKind::Angle, GripDimKind::AngleChange]);
        assert!(d.iter().all(|d| d.anchor == Vec2::ZERO));
        let p = Vec2::new(0.0, 12.0);
        assert!((d[0].value(p) - 12.0).abs() < 1e-12);
        assert!((d[1].value(p) - 2.0).abs() < 1e-12);
        assert!((d[2].value(p) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((d[3].value(Vec2::new(0.0, -3.0)) + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        // The middle grip moves the line: nothing to measure.
        assert!(dims_of(&l, 1).is_empty());
        assert!(dims_of(&l, 9).is_empty());
    }

    #[test]
    fn locks_hold_and_pin_the_grip() {
        let o = Vec2::ZERO;
        let g = Vec2::new(10.0, 0.0);
        let d = [GripDim::length(GripDimKind::Length, o, g), GripDim::angle(GripDimKind::Angle, o, g)];
        let c = Vec2::new(3.0, 4.0);
        assert_eq!(resolve(&d, &[], c), c);
        // Length 20: on the circle, toward the cursor.
        assert!(close(resolve(&d, &[(0, 20.0)], c), Vec2::new(12.0, 16.0)));
        // Angle 90°: on the ray.
        assert!(close(resolve(&d, &[(1, std::f64::consts::FRAC_PI_2)], c), Vec2::new(0.0, 4.0)));
        // Both: the point.
        assert!(close(resolve(&d, &[(0, 20.0), (1, std::f64::consts::FRAC_PI_2)], c), Vec2::new(0.0, 20.0)));
        // A length that isn't positive locks nothing.
        assert_eq!(resolve(&d, &[(0, -1.0)], c), c);
        // Two lengths from different anchors: where the circles cross, nearest the cursor.
        let d2 = [GripDim::length(GripDimKind::Length, Vec2::ZERO, g), GripDim::length(GripDimKind::Length, Vec2::new(10.0, 0.0), g)];
        assert!(close(resolve(&d2, &[(0, 5.0_f64.hypot(5.0)), (1, 5.0_f64.hypot(5.0))], Vec2::new(5.0, -1.0)), Vec2::new(5.0, -5.0)));
    }
}
