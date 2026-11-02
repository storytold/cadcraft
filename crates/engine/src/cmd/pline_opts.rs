//! PLINE options that ask further questions before a segment is drawn: arc mode's Angle,
//! CEnter, Direction, Radius and Second pt, and line mode's Length. Each ends in the next
//! segment from the last vertex `st`: its end point and bulge.

use cadcraft_geom::{Arc, TAU, Vec2, arc_to_bulge};

use super::curves::{arc_sca, arc_scl, arc_sea, arc_sed, arc_ser};
use super::machines::number;
use crate::{Accept, EngineError, Input, Prompt, Result, Session};

/// A PLINE option waiting for its answers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlineAsk {
    /// Angle: the included angle.
    Angle,
    /// Angle given: the end point, or [CEnter/Radius].
    AngleEnd(f64),
    AngleCenter(f64),
    AngleRadius(f64),
    /// Included angle and radius given: the direction of the chord.
    Chord(f64, f64),
    /// CEnter: the center point.
    Center,
    /// Center given: the end point, or [Angle/Length].
    CenterEnd(Vec2),
    CenterAngle(Vec2),
    CenterLength(Vec2),
    /// Direction: the tangent direction at the start of the arc.
    Direction,
    DirectionEnd(Vec2),
    /// Radius: the radius.
    Radius,
    /// Radius given: the end point, or [Angle].
    RadiusEnd(f64),
    RadiusAngle(f64),
    /// Second pt: the second point of a three-point arc.
    Second,
    SecondEnd(Vec2),
    /// Line mode's Length: a line segment this long, continuing the last segment.
    Length,
}

/// What an answer leads to.
pub(crate) enum Outcome {
    Ask(PlineAsk),
    /// The next segment: end point and bulge.
    Segment(Vec2, f64),
    /// Back to the arc (or line) prompt without a segment.
    Back,
}

fn other(m: &str) -> EngineError {
    EngineError::Other(m.into())
}

/// The polyline segment an arc starting at `st` makes: its end point and bulge. A whole circle
/// can't be one segment.
fn arc_segment(st: Vec2, a: &Arc) -> Option<(Vec2, f64)> {
    let sweep = a.sweep();
    if !(1e-9..TAU - 1e-9).contains(&sweep) {
        return None;
    }
    let tol = 1e-6 * a.radius.max(1.0);
    if a.start_point().near(st, tol) {
        Some((a.end_point(), arc_to_bulge(sweep)))
    } else if a.end_point().near(st, tol) {
        Some((a.start_point(), arc_to_bulge(-sweep)))
    } else {
        None
    }
}

impl PlineAsk {
    /// The option an arc-mode (or line-mode) keyword opens.
    pub(crate) fn from_keyword(k: &str, arc_mode: bool) -> Option<PlineAsk> {
        Some(match (k, arc_mode) {
            ("Angle", true) => PlineAsk::Angle,
            ("CEnter", true) => PlineAsk::Center,
            ("Direction", true) => PlineAsk::Direction,
            ("Radius", true) => PlineAsk::Radius,
            ("Second pt", true) => PlineAsk::Second,
            ("Length", false) => PlineAsk::Length,
            _ => return None,
        })
    }

    pub(crate) fn prompt(self, st: Vec2, tangent: Vec2, s: &Session) -> Prompt {
        let end = |kw: &[&str]| Prompt::new("Specify endpoint of arc", Accept::POINT).kw(kw).base(st);
        match self {
            PlineAsk::Angle | PlineAsk::CenterAngle(_) | PlineAsk::RadiusAngle(_) => Prompt::new("Specify included angle", Accept::NUMBER),
            PlineAsk::AngleEnd(_) => end(&["CEnter", "Radius"]),
            PlineAsk::AngleCenter(_) | PlineAsk::Center => Prompt::new("Specify center point of arc", Accept::POINT).base(st),
            PlineAsk::AngleRadius(_) | PlineAsk::Radius => Prompt::new("Specify radius of arc", Accept::POINT_OR_NUMBER).base(st).distance(),
            PlineAsk::Chord(a, _) => {
                let d = shown_direction(s, chord_default(tangent, a));
                Prompt::new("Specify direction of chord for arc", Accept::POINT_OR_NUMBER).base(st).default(d)
            }
            PlineAsk::CenterEnd(_) => end(&["Angle", "Length"]),
            PlineAsk::CenterLength(_) => Prompt::new("Specify length of chord", Accept::POINT_OR_NUMBER).base(st).distance(),
            PlineAsk::Direction => Prompt::new("Specify the tangent direction for the start point of arc", Accept::POINT_OR_NUMBER).base(st),
            PlineAsk::DirectionEnd(_) => Prompt::new("Specify endpoint of the arc", Accept::POINT).base(st),
            PlineAsk::RadiusEnd(_) => end(&["Angle"]),
            PlineAsk::Second => Prompt::new("Specify second point on arc", Accept::POINT).base(st),
            PlineAsk::SecondEnd(_) => Prompt::new("Specify end point of arc", Accept::POINT).base(st),
            PlineAsk::Length => Prompt::new("Specify length of line", Accept::POINT_OR_NUMBER).base(st).distance(),
        }
    }

    /// The arc (or line) the prompt would give for the point `p`.
    fn segment_at(self, st: Vec2, tangent: Vec2, p: Vec2) -> Option<(Vec2, f64)> {
        let a = match self {
            PlineAsk::AngleEnd(a) => arc_sea(st, p, a),
            PlineAsk::AngleCenter(a) => arc_sca(st, p, a),
            PlineAsk::CenterEnd(c) => (!p.near(c, 1e-12) && st.dist(c) > 1e-12).then(|| Arc::from_start_center_end(st, c, p)),
            PlineAsk::DirectionEnd(d) => arc_sed(st, p, d),
            PlineAsk::RadiusEnd(r) => arc_ser(st, p, r),
            PlineAsk::SecondEnd(m) => Arc::from_3_points(st, m, p),
            PlineAsk::Chord(a, r) => return self.chord_segment(st, a, r, st.angle_to(p)),
            PlineAsk::Length => return Some((st + tangent * st.dist(p), 0.0)),
            _ => None,
        }?;
        arc_segment(st, &a)
    }

    /// The arc with included angle `a` and radius `r` whose chord runs in direction `dir`.
    fn chord_segment(self, st: Vec2, a: f64, r: f64, dir: f64) -> Option<(Vec2, f64)> {
        let chord = 2.0 * r * (a.abs() / 2.0).sin();
        let en = st + Vec2::from_angle(dir) * chord;
        arc_sea(st, en, a).and_then(|arc| arc_segment(st, &arc))
    }

    /// Rubber-band segment for the cursor `c`.
    pub(crate) fn preview(self, st: Vec2, tangent: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
        self.segment_at(st, tangent, c)
    }

    pub(crate) fn input(self, s: &Session, i: Input, st: Vec2, tangent: Vec2) -> Result<Outcome> {
        let angles = s.angle_settings();
        let invalid = || other("Invalid arc for these values.");
        // A typed or picked size (distance from the last vertex).
        let size = |i: &Input| -> Option<f64> {
            match i {
                Input::Point(p) => Some(st.dist(*p)),
                Input::Text(t) => number(t),
                _ => None,
            }
        };
        let segment = |r: Option<(Vec2, f64)>| r.map(|(e, b)| Outcome::Segment(e, b)).ok_or_else(invalid);
        if matches!(i, Input::Enter) && !matches!(self, PlineAsk::Chord(..)) {
            return Ok(Outcome::Back);
        }
        Ok(match (self, i) {
            (PlineAsk::Angle | PlineAsk::CenterAngle(_) | PlineAsk::RadiusAngle(_), Input::Text(t)) => {
                let a = angles.rotation(&t).filter(|a| a.is_finite()).ok_or_else(|| other("Requires an angle."))?;
                if a.abs() < 1e-9 || a.abs() >= TAU - 1e-9 {
                    return Err(other("The included angle must be between 0 and 360 degrees."));
                }
                match self {
                    PlineAsk::CenterAngle(c) => segment(arc_sca(st, c, a).and_then(|arc| arc_segment(st, &arc)))?,
                    PlineAsk::RadiusAngle(r) => Outcome::Ask(PlineAsk::Chord(a, r)),
                    _ => Outcome::Ask(PlineAsk::AngleEnd(a)),
                }
            }
            (PlineAsk::AngleEnd(a), Input::Keyword(k)) if k == "CEnter" => Outcome::Ask(PlineAsk::AngleCenter(a)),
            (PlineAsk::AngleEnd(a), Input::Keyword(k)) if k == "Radius" => Outcome::Ask(PlineAsk::AngleRadius(a)),
            (PlineAsk::CenterEnd(c), Input::Keyword(k)) if k == "Angle" => Outcome::Ask(PlineAsk::CenterAngle(c)),
            (PlineAsk::CenterEnd(c), Input::Keyword(k)) if k == "Length" => Outcome::Ask(PlineAsk::CenterLength(c)),
            (PlineAsk::RadiusEnd(r), Input::Keyword(k)) if k == "Angle" => Outcome::Ask(PlineAsk::RadiusAngle(r)),
            (PlineAsk::Center, Input::Point(c)) => {
                if c.near(st, 1e-12) {
                    return Err(invalid());
                }
                Outcome::Ask(PlineAsk::CenterEnd(c))
            }
            (PlineAsk::Second, Input::Point(m)) => {
                if m.near(st, 1e-12) {
                    return Err(invalid());
                }
                Outcome::Ask(PlineAsk::SecondEnd(m))
            }
            (PlineAsk::Direction, i) => {
                let d = match i {
                    Input::Point(p) => p - st,
                    Input::Text(t) => Vec2::from_angle(angles.direction(&t).ok_or_else(|| other("Requires an angle."))?),
                    _ => return Err(other("Requires a direction.")),
                };
                if d.normalized() == Vec2::ZERO {
                    return Err(other("Requires a direction."));
                }
                Outcome::Ask(PlineAsk::DirectionEnd(d.normalized()))
            }
            (PlineAsk::AngleRadius(a), i) => {
                let r = size(&i).filter(|r| r.is_finite() && *r > 1e-12).ok_or_else(|| other("Requires a positive radius."))?;
                Outcome::Ask(PlineAsk::Chord(a, r))
            }
            // A negative radius gives the major arc.
            (PlineAsk::Radius, i) => {
                let r = size(&i).filter(|r| r.is_finite() && r.abs() > 1e-12).ok_or_else(|| other("Requires a nonzero radius."))?;
                Outcome::Ask(PlineAsk::RadiusEnd(r))
            }
            (PlineAsk::CenterLength(c), i) => {
                let l = size(&i).filter(|l| l.is_finite()).ok_or_else(|| other("Requires a distance."))?;
                segment(arc_scl(st, c, l).and_then(|arc| arc_segment(st, &arc)))?
            }
            (PlineAsk::Chord(a, r), i) => {
                let dir = match i {
                    Input::Point(p) if !p.near(st, 1e-12) => st.angle_to(p),
                    Input::Text(t) => angles.direction(&t).ok_or_else(|| other("Requires an angle."))?,
                    Input::Enter => chord_default(tangent, a),
                    _ => return Err(other("Requires a direction.")),
                };
                segment(self.chord_segment(st, a, r, dir))?
            }
            (PlineAsk::Length, i) => {
                let l = size(&i).filter(|l| l.is_finite() && *l > 1e-12).ok_or_else(|| other("Requires a positive length."))?;
                Outcome::Segment(st + tangent * l, 0.0)
            }
            (_, Input::Point(p)) => segment(self.segment_at(st, tangent, p))?,
            _ => return Err(other("Point or option keyword required.")),
        })
    }
}

/// Default chord direction for an arc with included angle `a` that continues the tangent: the
/// tangent turned by half the angle.
fn chord_default(tangent: Vec2, a: f64) -> f64 {
    tangent.angle() + a / 2.0
}

/// `dir` (radians counterclockwise from +X) as the user reads directions: from ANGBASE in the
/// ANGDIR sense, in AUNITS.
fn shown_direction(s: &Session, dir: f64) -> String {
    let au = s.angle_settings();
    let a = (dir - au.angbase) * if au.clockwise { -1.0 } else { 1.0 };
    let prec = s.doc().map(|d| d.header.i64("AUPREC", 0)).unwrap_or(0);
    crate::units::format_angle(a, au.aunits, prec)
}
