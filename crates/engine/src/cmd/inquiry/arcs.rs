//! The Arc option of AREA's and DIST's point sequences (and of MEASUREGEOM's Distance, Area and
//! Volume): arc segments, asked for as PLINE's arc mode asks for them. Each point carries the
//! bulge of the segment that starts at it; the last point's bulge is the closing segment's,
//! which only CLose sets.

use cadcraft_doc::EntityKind;
use cadcraft_geom::{PolyVertex, Polyline, Vec2, arc_to_bulge, bulge_to_arc};

use super::super::helpers::lwpoly;
use super::super::machines::number;
use super::super::pline_opts::{Outcome, PlineAsk};

use crate::{Accept, EngineError, Input, Prompt, Result, Session};

/// Arc mode's keywords: PLINE's arc mode without Halfwidth and Width.
const KEYWORDS: [&str; 8] = ["Angle", "CEnter", "CLose", "Direction", "Line", "Radius", "Second pt", "Undo"];

/// Arc mode of a point sequence.
#[derive(Default)]
pub(super) struct Arcs {
    /// The Arc option turned arc mode on; Line turns it off.
    pub(super) on: bool,
    /// An arc option waiting for its answers (Angle, CEnter, Direction, Radius, Second pt).
    ask: Option<PlineAsk>,
}

/// The polyline through `pts`, each segment with its bulge in `bulges` (straight when missing).
pub(super) fn polyline(pts: &[Vec2], bulges: &[f64], closed: bool) -> Polyline {
    let vertices = pts.iter().enumerate().map(|(i, p)| PolyVertex::with_bulge(*p, bulges.get(i).copied().unwrap_or(0.0))).collect();
    Polyline { vertices, closed }
}

/// Whether any segment of the sequence is an arc.
pub(super) fn has_arcs(bulges: &[f64]) -> bool {
    bulges.iter().any(|b| *b != 0.0)
}

/// The direction the sequence runs in at its last point (+X before the second point): along the
/// last line segment, or along the tangent at the end of the last arc segment.
pub(super) fn tangent(pts: &[Vec2], bulges: &[f64]) -> Vec2 {
    let Some(i) = pts.len().checked_sub(2) else { return Vec2::X };
    let (Some(prev), Some(last)) = (pts.get(i).copied(), pts.last().copied()) else { return Vec2::X };
    let d = match bulge_to_arc(prev, last, bulges.get(i).copied().unwrap_or(0.0)) {
        Some((a, ccw)) => {
            let r = (last - a.center).perp().normalized();
            if ccw { r } else { -r }
        }
        None => (last - prev).normalized(),
    };
    if d == Vec2::ZERO { Vec2::X } else { d }
}

/// The bulge of an arc from the last point to `p` that continues the sequence's direction; an
/// error when `p` lies straight behind the last point, where no such arc exists.
fn tangent_bulge(pts: &[Vec2], bulges: &[f64], p: Vec2) -> Result<f64> {
    let Some(last) = pts.last().copied() else { return Ok(0.0) };
    let dir = tangent(pts, bulges);
    let chord = p - last;
    // The included angle is twice the angle between the tangent and the chord.
    let theta = dir.cross(chord).atan2(dir.dot(chord));
    let b = arc_to_bulge(2.0 * theta);
    if theta.abs() < std::f64::consts::PI - 1e-6 && b.is_finite() { Ok(b) } else { Err(EngineError::Other("Invalid arc endpoint.".into())) }
}

/// Add the segment from the last point to `end` with bulge `bulge` (the first point when there is none).
pub(super) fn push(pts: &mut Vec<Vec2>, bulges: &mut Vec<f64>, end: Vec2, bulge: f64) {
    bulges.resize(pts.len(), 0.0);
    if let Some(b) = bulges.last_mut() {
        *b = bulge;
    }
    pts.push(end);
    bulges.push(0.0);
}

impl Arcs {
    /// An arc option is waiting for its answers.
    pub(super) fn asking(&self) -> bool {
        self.ask.is_some()
    }

    /// The question being answered takes the arc's end point, so a picked point is the end of the segment.
    pub(super) fn asks_end_point(&self) -> bool {
        matches!(
            self.ask,
            Some(PlineAsk::AngleEnd(_) | PlineAsk::CenterEnd(_) | PlineAsk::DirectionEnd(_) | PlineAsk::RadiusEnd(_) | PlineAsk::SecondEnd(_))
        )
    }

    /// A typed radius or chord length multiplied by `scale`, for a sequence measured in another
    /// space's units than the one its points are in; any other input as it is.
    pub(super) fn scale_typed_size(&self, i: Input, scale: f64) -> Input {
        let size = matches!(self.ask, Some(PlineAsk::Radius | PlineAsk::AngleRadius(_) | PlineAsk::CenterLength(_)));
        match i {
            Input::Text(t) if size => match number(&t).map(|v| v * scale).filter(|v| v.is_finite()) {
                Some(v) => Input::Text(v.to_string()),
                None => Input::Text(t),
            },
            i => i,
        }
    }

    /// The arc-mode prompt, or the question of the arc option being answered; `None` in line mode
    /// and before the first point.
    pub(super) fn prompt(&self, s: &Session, pts: &[Vec2], bulges: &[f64], prefix: &str) -> Option<Prompt> {
        let st = pts.last().copied()?;
        if let Some(a) = self.ask {
            return Some(a.prompt(st, tangent(pts, bulges), s));
        }
        self.on.then(|| Prompt::new(format!("{prefix}Specify endpoint of arc"), Accept::POINT).kw(&KEYWORDS).base(st))
    }

    /// Feed an answer to the arc option being answered; true when it added a segment.
    pub(super) fn answer(&mut self, s: &Session, i: Input, pts: &mut Vec<Vec2>, bulges: &mut Vec<f64>) -> Result<bool> {
        let (Some(a), Some(st)) = (self.ask, pts.last().copied()) else {
            self.ask = None;
            return Ok(false);
        };
        match a.input(s, i, st, tangent(pts, bulges))? {
            Outcome::Ask(next) => self.ask = Some(next),
            Outcome::Back => self.ask = None,
            Outcome::Segment(end, bulge) => {
                self.ask = None;
                push(pts, bulges, end, bulge);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Arc, Line, and arc mode's Angle, CEnter, Direction, Radius and Second pt; false for any
    /// other keyword.
    pub(super) fn keyword(&mut self, k: &str, pts: &[Vec2]) -> bool {
        match k {
            "Arc" if !pts.is_empty() => self.on = true,
            "Line" if self.on => self.on = false,
            _ => match PlineAsk::from_keyword(k, true) {
                Some(a) if self.on && !pts.is_empty() => self.ask = Some(a),
                _ => return false,
            },
        }
        true
    }

    /// A point at the next-point prompt: in arc mode, the end of an arc that continues the
    /// sequence's direction; otherwise the end of a line segment.
    pub(super) fn point(&self, pts: &mut Vec<Vec2>, bulges: &mut Vec<f64>, p: Vec2) -> Result<()> {
        let bulge = if self.on && !pts.is_empty() { tangent_bulge(pts, bulges, p)? } else { 0.0 };
        push(pts, bulges, p, bulge);
        Ok(())
    }

    /// CLose: the closing segment becomes an arc from the last point to the first that continues
    /// the sequence's direction.
    pub(super) fn close(&self, pts: &[Vec2], bulges: &mut Vec<f64>) -> Result<()> {
        let Some(first) = pts.first().copied().filter(|_| pts.len() >= 2) else {
            return Err(EngineError::Other("CLose needs at least two points.".into()));
        };
        let bulge = tangent_bulge(pts, bulges, first)?;
        bulges.resize(pts.len(), 0.0);
        if let Some(b) = bulges.last_mut() {
            *b = bulge;
        }
        Ok(())
    }

    /// Undo: remove the last point and the segment to it. Removing the first point leaves arc mode.
    pub(super) fn undo(&mut self, pts: &mut Vec<Vec2>, bulges: &mut Vec<f64>) {
        pts.pop();
        bulges.truncate(pts.len());
        if let Some(b) = bulges.last_mut() {
            *b = 0.0;
        }
        if pts.is_empty() {
            *self = Arcs::default();
        }
    }

    /// The sequence with the segment to the cursor `c` that a click would add.
    pub(super) fn preview(&self, pts: &[Vec2], bulges: &[f64], c: Vec2, closed: bool) -> Vec<EntityKind> {
        let Some(st) = pts.last().copied() else { return Vec::new() };
        let next = match self.ask {
            Some(a) => a.preview(st, tangent(pts, bulges), c),
            None if self.on => tangent_bulge(pts, bulges, c).ok().map(|b| (c, b)),
            None => Some((c, 0.0)),
        };
        let mut g = polyline(pts, bulges, closed);
        if let Some((end, bulge)) = next {
            if let Some(v) = g.vertices.last_mut() {
                v.bulge = bulge;
            }
            g.vertices.push(PolyVertex::new(end));
        }
        vec![lwpoly(g.vertices, closed)]
    }
}
