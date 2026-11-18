//! The prompt sequences of the solid commands. They gather values and call the same builders
//! and `run` functions as the JSON forms; the running command records the single undo step.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Mesh3, Vec2, Vec3};
use serde_json::{Value, json};

use super::super::helpers::{circle, rect_vertices};
use super::super::machines::{SelOutcome, SelectPhase, number};
use super::{
    build_box, build_cone, build_cylinder, build_pyramid, build_sphere, build_torus, faces_of, run_extrude, run_loft, run_revolve, run_sweep,
};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Most triangles a preview draws.
const PREVIEW_FACES: usize = 3000;

fn err(msg: &str) -> EngineError {
    EngineError::Other(msg.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cmd) enum Kind {
    Box,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Pyramid,
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    /// The first point (a corner, or the centre of the base).
    First,
    /// The other corner of the box base.
    Corner,
    /// A positive distance (radius) or, for `signed`, any non-zero number (height).
    Dist(&'static str, bool),
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Box => "BOX",
            Kind::Cylinder => "CYLINDER",
            Kind::Cone => "CONE",
            Kind::Sphere => "SPHERE",
            Kind::Torus => "TORUS",
            Kind::Pyramid => "PYRAMID",
        }
    }
    fn slots(self) -> &'static [Slot] {
        match self {
            Kind::Box => &[Slot::First, Slot::Corner, Slot::Dist("Specify height", true)],
            Kind::Cylinder => &[Slot::First, Slot::Dist("Specify base radius", false), Slot::Dist("Specify height", true)],
            Kind::Cone => &[Slot::First, Slot::Dist("Specify base radius", false), Slot::Dist("Specify height", true)],
            Kind::Pyramid => &[Slot::First, Slot::Dist("Specify base radius (to the corners)", false), Slot::Dist("Specify height", true)],
            Kind::Sphere => &[Slot::First, Slot::Dist("Specify radius", false)],
            Kind::Torus => &[Slot::First, Slot::Dist("Specify torus radius", false), Slot::Dist("Specify tube radius", false)],
        }
    }
    fn first_prompt(self) -> &'static str {
        match self {
            Kind::Box => "Specify first corner of box",
            Kind::Cylinder | Kind::Cone | Kind::Pyramid => "Specify center point of base",
            Kind::Sphere | Kind::Torus => "Specify center point",
        }
    }
}

#[derive(Clone, Copy)]
enum Val {
    P(Vec3),
    N(f64),
}

/// BOX, CYLINDER, CONE, SPHERE, TORUS and PYRAMID.
pub(in crate::cmd) struct PrimM {
    kind: Kind,
    vals: Vec<Val>,
    sides: usize,
    ask_sides: bool,
}

impl PrimM {
    pub(in crate::cmd) fn new(kind: Kind) -> Self {
        PrimM { kind, vals: Vec::new(), sides: 4, ask_sides: false }
    }
    fn slot(&self) -> Option<Slot> {
        self.kind.slots().get(self.vals.len()).copied()
    }
    fn first(&self) -> Option<Vec3> {
        match self.vals.first() {
            Some(Val::P(p)) => Some(*p),
            _ => None,
        }
    }
    fn num(&self, i: usize) -> Option<f64> {
        match self.vals.get(i) {
            Some(Val::N(v)) => Some(*v),
            _ => None,
        }
    }
    /// The JSON form of the values so far, with `last` as the value still being asked.
    fn params(&self, last: Option<Val>) -> Option<Value> {
        let mut vals = self.vals.clone();
        vals.extend(last);
        let p0 = match vals.first() {
            Some(Val::P(p)) => *p,
            _ => return None,
        };
        let n = |i: usize| match vals.get(i) {
            Some(Val::N(v)) => Some(*v),
            _ => None,
        };
        let c = json!([p0.x, p0.y, p0.z]);
        match self.kind {
            Kind::Box => {
                let Some(Val::P(q)) = vals.get(1) else { return None };
                Some(json!({ "p1": c, "p2": [q.x, q.y, q.z], "height": n(2)? }))
            }
            Kind::Cylinder | Kind::Cone => Some(json!({ "center": c, "radius": n(1)?, "height": n(2)? })),
            Kind::Pyramid => Some(json!({ "center": c, "radius": n(1)?, "height": n(2)?, "sides": self.sides })),
            Kind::Sphere => Some(json!({ "center": c, "radius": n(1)? })),
            Kind::Torus => Some(json!({ "center": c, "radius": n(1)?, "tubeRadius": n(2)? })),
        }
    }
    fn build(&self, p: &Value) -> Result<Mesh3> {
        match self.kind {
            Kind::Box => build_box(p),
            Kind::Cylinder => build_cylinder(p),
            Kind::Cone => build_cone(p),
            Kind::Sphere => build_sphere(p),
            Kind::Torus => build_torus(p),
            Kind::Pyramid => build_pyramid(p),
        }
    }
    /// Take one value; once all are in, make the solid.
    fn push(&mut self, s: &mut Session, v: Val) -> Result<Step> {
        self.vals.push(v);
        if self.slot().is_some() {
            return Ok(Step::Continue);
        }
        let done = self.params(None).ok_or_else(|| err("Incomplete values."));
        let made = done.and_then(|p| {
            let spec = super::super::find_command(&self.kind.name().to_ascii_lowercase()).ok_or_else(|| err("Unknown command."))?;
            (spec.run)(s, &p)
        });
        match made {
            Ok(_) => Ok(Step::Done),
            Err(e) => {
                // Ask the last value again.
                self.vals.pop();
                Err(e)
            }
        }
    }
}

impl Interactive for PrimM {
    fn name(&self) -> &'static str {
        self.kind.name()
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.ask_sides {
            return Prompt::new("Enter number of sides", Accept::NUMBER).default(self.sides.to_string());
        }
        let first = self.first();
        match self.slot() {
            Some(Slot::First) | None => {
                let p = Prompt::new(self.kind.first_prompt(), Accept::POINT).point3d();
                if self.kind == Kind::Pyramid { p.kw(&["Sides"]) } else { p }
            }
            Some(Slot::Corner) => Prompt::new("Specify other corner", Accept::POINT).point3d().corner(first.unwrap_or(Vec3::ZERO).xy()),
            Some(Slot::Dist(msg, _)) => {
                let p = Prompt::new(msg, Accept::POINT_OR_NUMBER).point3d().base_opt(first.map(Vec3::xy)).distance();
                if self.kind == Kind::Pyramid && self.vals.len() == 1 { p.kw(&["Sides"]) } else { p }
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.ask_sides {
            return match i {
                Input::Enter => {
                    self.ask_sides = false;
                    Ok(Step::Continue)
                }
                Input::Text(t) => {
                    let n = number(&t)
                        .filter(|n| n.fract() == 0.0 && (3.0..=256.0).contains(n))
                        .ok_or_else(|| err("Requires an integer from 3 to 256."))?;
                    self.sides = n as usize;
                    self.ask_sides = false;
                    Ok(Step::Continue)
                }
                Input::Cancel => Ok(Step::Cancel),
                _ => Err(err("Requires an integer from 3 to 256.")),
            };
        }
        match (self.slot(), i) {
            (_, Input::Cancel | Input::Enter) => Ok(Step::Cancel),
            (_, Input::Keyword(k)) if k == "Sides" && self.kind == Kind::Pyramid => {
                self.ask_sides = true;
                Ok(Step::Continue)
            }
            (Some(Slot::First), Input::Point(p)) => self.push(s, Val::P(Vec3::new(p.x, p.y, 0.0))),
            (Some(Slot::First), Input::Point3(p)) => self.push(s, Val::P(p)),
            (Some(Slot::Corner), Input::Point(p)) => {
                let p = Vec3::new(p.x, p.y, 0.0);
                if self.first().is_some_and(|f| (f - p).len() <= 1e-12) {
                    return Err(err("The corners must differ."));
                }
                self.push(s, Val::P(p))
            }
            (Some(Slot::Corner), Input::Point3(p)) => {
                if self.first().is_some_and(|f| (f - p).len() <= 1e-12) {
                    return Err(err("The corners must differ."));
                }
                self.push(s, Val::P(p))
            }
            (Some(Slot::Dist(_, signed)), Input::Point(p)) => {
                let q = Vec3::new(p.x, p.y, 0.0);
                let d = self.first().map_or(0.0, |f| (f - q).len());
                if d <= 0.0 || (!signed && d <= 1e-12) {
                    return Err(err("Value must be positive and nonzero."));
                }
                self.push(s, Val::N(d))
            }
            (Some(Slot::Dist(_, signed)), Input::Point3(p)) => {
                let d = self.first().map_or(0.0, |f| (f - p).len());
                if d <= 0.0 || (!signed && d <= 1e-12) {
                    return Err(err("Value must be positive and nonzero."));
                }
                self.push(s, Val::N(d))
            }
            (Some(Slot::Dist(_, signed)), Input::Text(t)) => {
                let v = number(&t).filter(|v| v.is_finite()).ok_or_else(|| err("Requires numeric distance or second point."))?;
                if v == 0.0 || (!signed && v < 0.0) {
                    return Err(err("Value must be positive and nonzero."));
                }
                self.push(s, Val::N(v))
            }
            (Some(Slot::First | Slot::Corner), Input::Text(_)) => Err(err("Point or option keyword required.")),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let _ = s;
        let Some(first3) = self.first() else { return Vec::new() };
        let first = first3.xy();
        let to_cursor = first.dist(cursor);
        match self.slot() {
            Some(Slot::Corner) => vec![super::super::helpers::lwpoly(rect_vertices(first, cursor), true)],
            Some(Slot::Dist(_, false)) if self.vals.len() == 1 => vec![circle(first, to_cursor)],
            Some(Slot::Dist(_, _)) => {
                let Some(p) = self.params(Some(Val::N(to_cursor))) else { return Vec::new() };
                match self.build(&p) {
                    Ok(m) if m.triangles.len() <= PREVIEW_FACES => faces_of(&m).into_iter().map(EntityKind::Face3d).collect(),
                    _ => Vec::new(),
                }
            }
            _ => {
                let _ = self.num(0);
                Vec::new()
            }
        }
    }
}

// ---------------- commands that start from existing objects ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cmd) enum Op {
    Extrude,
    Revolve,
    Sweep,
    Loft,
}

#[derive(PartialEq)]
enum Stage {
    Select,
    /// A single-object pick for the sweep path.
    Path,
    /// A number: the extrusion height or the loft spacing.
    Number,
    /// A point on the revolve axis.
    Axis,
}

/// EXTRUDE, REVOLVE, SWEEP and LOFT: pick the profiles, then the one extra value each needs.
pub(in crate::cmd) struct ObjM {
    op: Op,
    stage: Stage,
    sel: SelectPhase,
    path_sel: SelectPhase,
    picked: Vec<Handle>,
}

impl ObjM {
    pub(in crate::cmd) fn new(op: Op) -> Self {
        ObjM { op, stage: Stage::Select, sel: SelectPhase::default(), path_sel: SelectPhase::single(), picked: Vec::new() }
    }
    fn handles(&self) -> Vec<String> {
        self.picked.iter().map(|h| h.hex()).collect()
    }
    /// The profiles are chosen: ask for what the command needs next.
    fn chosen(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        self.picked = hs;
        match self.op {
            Op::Extrude | Op::Loft => self.stage = Stage::Number,
            Op::Revolve => self.stage = Stage::Axis,
            Op::Sweep => self.stage = Stage::Path,
        }
        let _ = s;
        Ok(Step::Continue)
    }
    fn finish(&mut self, s: &mut Session, p: Value) -> Result<Step> {
        let run = match self.op {
            Op::Extrude => run_extrude(s, &p),
            Op::Revolve => run_revolve(s, &p),
            Op::Sweep => run_sweep(s, &p),
            Op::Loft => run_loft(s, &p),
        };
        run.map(|_| Step::Done)
    }
}

impl Interactive for ObjM {
    fn name(&self) -> &'static str {
        match self.op {
            Op::Extrude => "EXTRUDE",
            Op::Revolve => "REVOLVE",
            Op::Sweep => "SWEEP",
            Op::Loft => "LOFT",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.chosen(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (&self.stage, self.op) {
            (Stage::Select, Op::Loft) => Prompt::new("Select cross-section profiles in order (closed polylines or circles)", Accept::SELECT),
            (Stage::Select, _) => Prompt::new("Select closed profiles (polylines, circles, ellipses)", Accept::SELECT),
            (Stage::Path, _) => Prompt::new("Select sweep path (line, polyline or arc)", Accept::SELECT),
            (Stage::Number, Op::Loft) => Prompt::new("Specify spacing between sections", Accept::NUMBER),
            (Stage::Number, _) => Prompt::new("Specify height of extrusion (negative extrudes down)", Accept::NUMBER),
            (Stage::Axis, _) => Prompt::new("Specify a point on the vertical axis of revolution (Enter = X 0)", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        match self.stage {
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Cancel),
                SelOutcome::Done(hs) => self.chosen(s, hs),
            },
            Stage::Path => match self.path_sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Cancel),
                SelOutcome::Done(hs) => {
                    let path = hs.first().map(|h| h.hex()).ok_or_else(|| err("Nothing selected."))?;
                    self.finish(s, json!({ "handles": self.handles(), "pathHandle": path }))
                }
            },
            Stage::Number => match i {
                Input::Text(t) => {
                    let v = number(&t).filter(|v| v.is_finite() && *v != 0.0).ok_or_else(|| err("Requires a nonzero number."))?;
                    let key = if self.op == Op::Loft { "spacing" } else { "height" };
                    self.finish(s, json!({ "handles": self.handles(), key: v }))
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Err(err("Requires a nonzero number.")),
            },
            Stage::Axis => match i {
                Input::Point(p) => self.finish(s, json!({ "handles": self.handles(), "axisX": p.x })),
                Input::Enter => self.finish(s, json!({ "handles": self.handles() })),
                _ => Err(err("Point required.")),
            },
        }
    }
}
