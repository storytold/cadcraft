//! SOLID: filled triangles and quadrilaterals (2D solids), chained as AutoCAD 2024 chains them.
//!
//! The prompts are "Specify first point", "Specify second point", "Specify third point" and
//! "Specify fourth point or <exit>"; Enter there makes a triangle (the fourth corner repeats the
//! third). Then "Specify third point" and "Specify fourth point or <exit>" come again for the next
//! solid, whose first two corners are the previous one's last two, until Enter at "Specify third
//! point". Enter there right after the second point makes nothing.

use cadcraft_doc::Solid;

use super::{Accept, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, bad, json, points_param, v3};

/// A 2D solid from its corners in AutoCAD's order (the fourth is drawn opposite the first).
fn solid(c: [Vec2; 4]) -> EntityKind {
    EntityKind::Solid(Solid { corners: c.map(v3) })
}

/// `{points}`: the first solid takes three or four points, each further one two more, its first
/// two corners being the previous one's last two; a solid given three points is a triangle.
pub(super) fn run_solid(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("solid", "`points` is required"))?;
    let mut it = pts.into_iter();
    let (Some(mut a), Some(mut b), Some(_)) = (it.next(), it.next(), it.clone().next()) else {
        return Err(bad("solid", "`points` needs at least three points"));
    };
    let mut hs = Vec::new();
    while let Some(c) = it.next() {
        let d = it.next().unwrap_or(c);
        hs.push(s.add_entity(solid([a, b, c, d]))?.hex());
        (a, b) = (c, d);
    }
    Ok(json!({ "handles": hs }))
}

#[derive(Default)]
pub(super) struct SolidM {
    /// The corners of the solid being drawn (the first two from the previous one after it).
    pts: Vec<Vec2>,
}

impl Interactive for SolidM {
    fn name(&self) -> &'static str {
        "SOLID"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let last = self.pts.last().copied();
        match self.pts.len() {
            0 => Prompt::new("Specify first point", Accept::POINT),
            1 => Prompt::new("Specify second point", Accept::POINT).base_opt(last),
            2 => Prompt::new("Specify third point", Accept::POINT).base_opt(last),
            _ => Prompt::new("Specify fourth point or", Accept::POINT).default("exit").base_opt(last),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (i, self.pts.as_slice()) {
            (Input::Point(p), &[a, b, c]) => self.make(s, [a, b, c, p])?,
            (Input::Point(p), _) => self.pts.push(p),
            (Input::Enter, &[a, b, c]) => self.make(s, [a, b, c, c])?,
            (Input::Enter, _) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        match *self.pts.as_slice() {
            [a, b] => vec![solid([a, b, cursor, cursor])],
            [a, b, c] => vec![solid([a, b, c, cursor])],
            _ => Vec::new(),
        }
    }
}

impl SolidM {
    fn make(&mut self, s: &mut Session, c: [Vec2; 4]) -> Result<()> {
        s.add_entity(solid(c))?;
        self.pts = vec![c[2], c[3]];
        Ok(())
    }
}
