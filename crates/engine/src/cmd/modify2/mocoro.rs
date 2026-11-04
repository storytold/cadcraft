//! MOCORO (Express Tools Move/Copy/Rotate): select objects and a base point once, then Move,
//! Copy, Rotate, Scale, Base and Undo at one option prompt until eXit.
//!
//! Copy leaves the selected objects where they are and adds copies: the options after it act on
//! the originals, from the same base point. Move carries the base point along, so a later Rotate
//! or Scale turns about the same spot on the objects. The whole command is one undo step; Undo
//! inside it steps back one option at a time (Base included), up to the last 1000.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::other;
use crate::cmd::curves::KW;
use crate::cmd::helpers::line;
use crate::cmd::machines::{SelOutcome, SelectPhase, number};
use crate::cmd::modify::transform_entities;
use crate::cmd::{bad, point_req, point_value, targets};
use crate::prompt::Drag;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most options one JSON call runs.
const MAX_OPS: usize = 10_000;
/// Most options Undo can step back through; the oldest are dropped past it.
const MAX_HISTORY: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Move(Vec2),
    Copy(Vec2),
    /// Radians, counterclockwise.
    Rotate(f64),
    Scale(f64),
    Base(Vec2),
}

fn positive_factor(f: f64) -> Option<f64> {
    (f.is_finite() && f > 1e-12).then_some(f)
}

/// One `ops` entry: an object with exactly one of move, copy, rotate (degrees), scale or base.
fn parse_op(i: usize, v: &Value) -> Result<Op> {
    let err = |m: &str| bad("mocoro", format!("`ops[{i}]`: {m}"));
    let Some(o) = v.as_object().filter(|o| o.len() == 1) else {
        return Err(err("one of {move|copy|base: [x,y]} | {rotate: degrees} | {scale: factor}"));
    };
    let Some((k, v)) = o.iter().next() else { return Err(err("empty")) };
    let point = || point_value(v).ok_or_else(|| err("a point [x, y] is required"));
    match k.as_str() {
        "move" => Ok(Op::Move(point()?)),
        "copy" => Ok(Op::Copy(point()?)),
        "base" => Ok(Op::Base(point()?)),
        "rotate" => v.as_f64().filter(|a| a.is_finite()).map(|a| Op::Rotate(a.to_radians())).ok_or_else(|| err("`rotate` is an angle in degrees")),
        "scale" => v.as_f64().and_then(positive_factor).map(Op::Scale).ok_or_else(|| err("`scale` is a factor above zero")),
        _ => Err(err("unknown option (move, copy, rotate, scale, base)")),
    }
}

/// How many of `objs` are still in the drawing, and how many of those are on a locked layer
/// (they don't move, rotate or scale; copies are made).
fn counts(s: &Session, objs: &[Handle]) -> Result<(usize, usize)> {
    let d = s.doc()?;
    let live: Vec<_> = objs.iter().filter_map(|h| d.entity(*h)).collect();
    let locked = live.iter().filter(|e| d.layer(&e.common.layer).is_some_and(|l| l.locked)).count();
    Ok((live.len(), locked))
}

fn locked_message(n: usize) -> String {
    if n == 1 { "1 object was on a locked layer.".into() } else { format!("{n} objects were on a locked layer.") }
}

/// Apply one option to the objects; returns the copies made. The drawing is not touched when
/// nothing would change (objects gone or all locked), so no empty undo step is recorded.
fn apply(s: &mut Session, objs: &[Handle], base: &mut Vec2, op: Op) -> Result<Vec<Handle>> {
    let b = *base;
    let (live, locked) = counts(s, objs)?;
    let m = match op {
        Op::Base(p) => {
            *base = p;
            return Ok(Vec::new());
        }
        Op::Copy(p) if live > 0 => return transform_entities(s, objs, &Mat3::translate(p - b), true),
        Op::Copy(_) => return Ok(Vec::new()),
        Op::Move(p) => Mat3::translate(p - b),
        Op::Rotate(a) => Mat3::rotate_about(b, a),
        Op::Scale(f) => Mat3::scale_about(b, f),
    };
    if locked > 0 {
        s.echo(locked_message(locked));
    }
    if locked < live {
        transform_entities(s, objs, &m, false)?;
        if let Op::Move(p) = op {
            *base = p;
        }
    }
    Ok(Vec::new())
}

pub(super) fn run_mocoro(s: &mut Session, p: &Value) -> Result<Value> {
    let objs = targets(s, p)?;
    let mut base = point_req("mocoro", p, "base")?;
    let list = p.get("ops").and_then(Value::as_array).ok_or_else(|| bad("mocoro", "`ops` (array) is required"))?;
    if list.len() > MAX_OPS {
        return Err(bad("mocoro", format!("at most {MAX_OPS} `ops`")));
    }
    // Every option is checked before the drawing changes.
    let ops = list.iter().enumerate().map(|(i, v)| parse_op(i, v)).collect::<Result<Vec<_>>>()?;
    let mut copies = Vec::new();
    for op in ops {
        copies.extend(apply(s, &objs, &mut base, op)?);
    }
    Ok(json!({ "handles": copies.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// The prompt MOCORO is at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Mode {
    #[default]
    Select,
    BasePoint,
    Options,
    Move,
    Copy,
    Rotate,
    Scale,
    Base,
}

#[derive(Default)]
pub(super) struct MocoroM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    base: Vec2,
    mode: Mode,
    /// The drawing and base point before each option that changed either, for Undo.
    history: Vec<(Arc<Drawing>, Vec2)>,
}

impl MocoroM {
    fn run(&mut self, s: &mut Session, op: Op) -> Result<Step> {
        let before = s.state()?.doc.clone();
        let base = self.base;
        apply(s, &self.objs, &mut self.base, op)?;
        if !Arc::ptr_eq(&before, &s.state()?.doc) || self.base != base {
            if self.history.len() >= MAX_HISTORY {
                self.history.remove(0);
            }
            self.history.push((before, base));
        }
        self.mode = Mode::Options;
        Ok(Step::Continue)
    }

    fn undo(&mut self, s: &mut Session) -> Result<Step> {
        match self.history.pop() {
            Some((doc, base)) => {
                // Settings changed meanwhile (a transparent command) stay.
                let header = s.doc()?.header.clone();
                s.state_mut()?.doc = doc;
                if s.doc()?.header != header {
                    s.doc_mut()?.header = header;
                }
                self.base = base;
            }
            None => s.echo("Nothing to undo."),
        }
        Ok(Step::Continue)
    }

    /// The transform under the cursor in an option that drags the objects.
    fn matrix(&self, c: Vec2) -> Option<Mat3> {
        let b = self.base;
        match self.mode {
            Mode::Move | Mode::Copy => Some(Mat3::translate(c - b)),
            Mode::Rotate => Some(Mat3::rotate_about(b, b.angle_to(c))),
            Mode::Scale => positive_factor(b.dist(c)).map(|f| Mat3::scale_about(b, f)),
            _ => None,
        }
    }
}

impl Interactive for MocoroM {
    fn name(&self) -> &'static str {
        "MOCORO"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            self.mode = Mode::BasePoint;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let b = self.base;
        match self.mode {
            Mode::Select => self.sel.prompt(),
            Mode::BasePoint => Prompt::new("Specify base point", Accept::POINT),
            Mode::Options => Prompt::new("Enter an option", KW).kw(&["Move", "Copy", "Rotate", "Scale", "Base", "Undo", "eXit"]).default("eXit"),
            Mode::Move | Mode::Copy => Prompt::new("Specify second point", Accept::POINT).base(b),
            Mode::Rotate => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).base(b),
            Mode::Scale => Prompt::new("Specify scale factor", Accept::POINT_OR_NUMBER).base(b),
            Mode::Base => Prompt::new("Specify base point", Accept::POINT).base(b),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.mode, i) {
            (Mode::Select, i) => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    self.mode = Mode::BasePoint;
                    Ok(Step::Continue)
                }
            },
            (Mode::BasePoint, Input::Point(p)) => {
                self.base = p;
                self.mode = Mode::Options;
                Ok(Step::Continue)
            }
            (Mode::BasePoint, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Mode::Options, Input::Keyword(k)) => {
                self.mode = match k.as_str() {
                    "Move" => Mode::Move,
                    "Copy" => Mode::Copy,
                    "Rotate" => Mode::Rotate,
                    "Scale" => Mode::Scale,
                    "Base" => Mode::Base,
                    "Undo" => return self.undo(s),
                    _ => {
                        s.set_selection(Vec::new());
                        return Ok(Step::Done);
                    }
                };
                Ok(Step::Continue)
            }
            (Mode::Options, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Mode::Options, _) => Err(other("Invalid option keyword.")),
            // Enter in an option goes back to the option prompt without changing anything.
            (Mode::Move | Mode::Copy | Mode::Rotate | Mode::Scale | Mode::Base, Input::Enter) => {
                self.mode = Mode::Options;
                Ok(Step::Continue)
            }
            (Mode::Move, Input::Point(p)) => self.run(s, Op::Move(p)),
            (Mode::Copy, Input::Point(p)) => self.run(s, Op::Copy(p)),
            (Mode::Base, Input::Point(p)) => self.run(s, Op::Base(p)),
            (Mode::Rotate, Input::Point(p)) => self.run(s, Op::Rotate(self.base.angle_to(p))),
            (Mode::Rotate, Input::Text(t)) => {
                let a = s.angle_settings().rotation(&t).filter(|a| a.is_finite()).ok_or_else(|| other("Requires an angle or point."))?;
                self.run(s, Op::Rotate(a))
            }
            (Mode::Scale, Input::Point(p)) => {
                let f = positive_factor(self.base.dist(p)).ok_or_else(|| other("Requires a positive number."))?;
                self.run(s, Op::Scale(f))
            }
            (Mode::Scale, Input::Text(t)) => {
                let f = number(&t).and_then(positive_factor).ok_or_else(|| other("Requires a positive number."))?;
                self.run(s, Op::Scale(f))
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(m), Ok(d)) = (self.matrix(c), s.doc()) else { return Vec::new() };
        let mut out: Vec<EntityKind> = self
            .objs
            .iter()
            .take(500)
            .filter_map(|h| d.entity(*h))
            .map(|e| {
                let mut k = e.kind.clone();
                k.transform(&m);
                k
            })
            .collect();
        out.push(line(self.base, c));
        out
    }
    fn drag(&self, _s: &Session, c: Vec2) -> Option<Drag> {
        let matrix = self.matrix(c)?;
        Some(Drag { handles: self.objs.clone(), matrix, extra: vec![line(self.base, c)], clipboard: false })
    }
}
