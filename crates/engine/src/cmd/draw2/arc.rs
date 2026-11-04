use super::{
    Accept, Arc, Circle, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, added, arc, bad, circle, curves, line,
    norm_angle, number, other, point_param, point_req, point_value,
};

// =====================================================================================
// Generic point/value sequences (arc and circle variants)
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArcV {
    /// The ARC command itself: three points, with options switching to the other variants.
    ThreeP,
    /// Start, end, then center.
    Sec,
    Sce,
    Sca,
    Scl,
    Sea,
    Sed,
    Ser,
    Cse,
    Csa,
    Csl,
    CircleCd,
    Circle2p,
    Circle3p,
}

/// What a step asks for.
#[derive(Clone, Copy, Debug)]
enum Ask {
    Point,
    /// Absolute angle from value `base` to the picked point, or a typed angle.
    Angle(usize),
    /// CCW angle at value `center` from the ray to value `from` to the ray to the picked point.
    Sweep(usize, usize),
    /// Distance from value `base`, or a typed distance (may be negative).
    Dist(usize),
}

#[derive(Clone, Copy, Debug)]
enum Val {
    P(Vec2),
    N(f64),
}

impl Val {
    fn p(self) -> Option<Vec2> {
        if let Val::P(p) = self { Some(p) } else { None }
    }
    fn n(self) -> Option<f64> {
        if let Val::N(n) = self { Some(n) } else { None }
    }
}

impl ArcV {
    fn name(self) -> &'static str {
        match self {
            ArcV::CircleCd | ArcV::Circle2p | ArcV::Circle3p => "CIRCLE",
            _ => "ARC",
        }
    }
    fn steps(self) -> &'static [(&'static str, Ask)] {
        const S: &str = "Specify start point of arc";
        const C: &str = "Specify center point of arc";
        const E: &str = "Specify end point of arc";
        const A: &str = "Specify included angle";
        const L: &str = "Specify length of chord";
        match self {
            ArcV::ThreeP => &[(S, Ask::Point), ("Specify second point of arc", Ask::Point), (E, Ask::Point)],
            ArcV::Sec => &[(S, Ask::Point), (E, Ask::Point), (C, Ask::Point)],
            ArcV::Sce => &[(S, Ask::Point), (C, Ask::Point), (E, Ask::Point)],
            ArcV::Sca => &[(S, Ask::Point), (C, Ask::Point), (A, Ask::Sweep(1, 0))],
            ArcV::Scl => &[(S, Ask::Point), (C, Ask::Point), (L, Ask::Dist(0))],
            ArcV::Sea => &[(S, Ask::Point), (E, Ask::Point), (A, Ask::Angle(0))],
            ArcV::Sed => &[(S, Ask::Point), (E, Ask::Point), ("Specify tangent direction for the start point of arc", Ask::Angle(0))],
            ArcV::Ser => &[(S, Ask::Point), (E, Ask::Point), ("Specify radius of arc", Ask::Dist(1))],
            ArcV::Cse => &[(C, Ask::Point), (S, Ask::Point), ("Specify end point of arc (hold Ctrl to switch direction)", Ask::Point)],
            ArcV::Csa => &[(C, Ask::Point), (S, Ask::Point), (A, Ask::Sweep(0, 1))],
            ArcV::Csl => &[(C, Ask::Point), (S, Ask::Point), (L, Ask::Dist(1))],
            ArcV::CircleCd => &[("Specify center point for circle", Ask::Point), ("Specify diameter of circle", Ask::Dist(0))],
            ArcV::Circle2p => {
                &[("Specify first end point of circle's diameter", Ask::Point), ("Specify second end point of circle's diameter", Ask::Point)]
            }
            ArcV::Circle3p => &[
                ("Specify first point on circle", Ask::Point),
                ("Specify second point on circle", Ask::Point),
                ("Specify third point on circle", Ask::Point),
            ],
        }
    }
    fn build(self, v: &[Val], ctrl: bool) -> Option<EntityKind> {
        let p = |i: usize| v.get(i).and_then(|x| x.p());
        let n = |i: usize| v.get(i).and_then(|x| x.n());
        let a = match self {
            ArcV::ThreeP => Arc::from_3_points(p(0)?, p(1)?, p(2)?),
            ArcV::Sec => Some(Arc::from_start_center_end(p(0)?, p(2)?, p(1)?)),
            ArcV::Sce => Some(Arc::from_start_center_end(p(0)?, p(1)?, p(2)?)),
            ArcV::Sca => curves::arc_sca(p(0)?, p(1)?, n(2)?),
            ArcV::Scl => curves::arc_scl(p(0)?, p(1)?, n(2)?),
            ArcV::Sea => curves::arc_sea(p(0)?, p(1)?, n(2)?),
            ArcV::Sed => curves::arc_sed(p(0)?, p(1)?, Vec2::from_angle(n(2)?)),
            ArcV::Ser => curves::arc_ser(p(0)?, p(1)?, n(2)?),
            // Center, Start, End. Ctrl keeps the two endpoints on the circle and walks the
            // other way around: swapping the start/end angles is the mirror arc through the
            // same two endpoints. (Continue below flips differently, by negating the start
            // tangent; both are "reverse", but only here is the chord fixed by the picks.)
            ArcV::Cse => {
                let a = Arc::from_start_center_end(p(1)?, p(0)?, p(2)?);
                Some(if ctrl { Arc::new(a.center, a.radius, a.end, a.start) } else { a })
            }
            ArcV::Csa => curves::arc_sca(p(1)?, p(0)?, n(2)?),
            ArcV::Csl => curves::arc_scl(p(1)?, p(0)?, n(2)?),
            ArcV::CircleCd => {
                let d = n(1)?.abs();
                let c = p(0)?;
                return (d > 1e-12 && d.is_finite()).then(|| circle(c, d / 2.0));
            }
            ArcV::Circle2p => {
                let c = Circle::from_2_points(p(0)?, p(1)?);
                return (c.radius > 1e-12).then(|| circle(c.center, c.radius));
            }
            ArcV::Circle3p => {
                let c = Circle::from_3_points(p(0)?, p(1)?, p(2)?)?;
                return Some(circle(c.center, c.radius));
            }
        }?;
        (a.radius > 1e-12 && a.radius.is_finite() && a.center.is_finite()).then(|| arc(&a))
    }
}

impl ArcV {
    /// Options at step `i` and the variant each one switches to (the values so far carry over).
    fn alts(self, i: usize) -> &'static [(&'static str, ArcV)] {
        match (self, i) {
            (ArcV::ThreeP, 0) => &[("Center", ArcV::Cse)],
            (ArcV::ThreeP, 1) => &[("Center", ArcV::Sce), ("End", ArcV::Sec)],
            (ArcV::Sce, 2) => &[("Angle", ArcV::Sca), ("chord Length", ArcV::Scl)],
            (ArcV::Cse, 2) => &[("Angle", ArcV::Csa), ("chord Length", ArcV::Csl)],
            (ArcV::Sec, 2) => &[("Angle", ArcV::Sea), ("Direction", ArcV::Sed), ("Radius", ArcV::Ser)],
            _ => &[],
        }
    }
}

pub(super) fn seq(v: ArcV) -> Box<dyn Interactive> {
    Box::new(SeqM { v, vals: Vec::new(), cont: None })
}

/// The full ARC prompt sequence (all options; Enter at the first prompt continues the last
/// line or arc).
pub(crate) fn arc_machine() -> Box<dyn Interactive> {
    seq(ArcV::ThreeP)
}

struct SeqM {
    v: ArcV,
    vals: Vec<Val>,
    /// ARC continuation: start point and tangent.
    cont: Option<(Vec2, Vec2)>,
}

impl SeqM {
    fn ask(&self) -> Option<(&'static str, Ask)> {
        self.v.steps().get(self.vals.len()).copied()
    }
    fn value_from_point(&self, ask: Ask, q: Vec2) -> Option<Val> {
        let pt = |i: usize| self.vals.get(i).and_then(|x| x.p());
        Some(match ask {
            Ask::Point => Val::P(q),
            Ask::Angle(b) => Val::N(pt(b)?.angle_to(q)),
            Ask::Sweep(c, f) => {
                let c = pt(c)?;
                Val::N(norm_angle(c.angle_to(q) - c.angle_to(pt(f)?)))
            }
            Ask::Dist(b) => Val::N(pt(b)?.dist(q)),
        })
    }
    fn base(&self) -> Option<Vec2> {
        match self.ask()?.1 {
            Ask::Point => self.vals.last().and_then(|v| v.p()),
            Ask::Angle(b) | Ask::Dist(b) => self.vals.get(b).and_then(|v| v.p()),
            Ask::Sweep(c, _) => self.vals.get(c).and_then(|v| v.p()),
        }
    }
}

impl Interactive for SeqM {
    fn name(&self) -> &'static str {
        self.v.name()
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some((st, _)) = self.cont {
            return Prompt::new("Specify end point of arc (hold Ctrl to switch direction)", Accept::POINT).base(st);
        }
        let kws: Vec<&str> = self.v.alts(self.vals.len()).iter().map(|a| a.0).collect();
        let p = match self.ask() {
            Some((m, Ask::Point)) => Prompt::new(m, Accept::POINT).base_opt(self.base()),
            Some((m, _)) => Prompt::new(m, Accept::POINT_OR_NUMBER).base_opt(self.base()),
            None => Prompt::new("", Accept::default()),
        };
        p.kw(&kws)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some((st, dir)) = self.cont {
            // Ctrl flips the arc: negating the start tangent makes `arc_sed` pick the mirror
            // arc (same two points and center, opposite sweep). `ctrl_down` comes from the
            // UI's per-frame write on `Session`; the `Input` enum itself stays free of
            // modifiers so it remains UI-agnostic.
            let dir = if s.ctrl_down { -dir } else { dir };
            return match i {
                Input::Point(e) => {
                    let a = curves::arc_sed(st, e, dir).ok_or_else(|| other("Invalid end point."))?;
                    s.add_entity(arc(&a))?;
                    Ok(Step::Done)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Err(other("Point required.")),
            };
        }
        if let Input::Keyword(k) = &i
            && let Some((_, v)) = self.v.alts(self.vals.len()).iter().find(|a| a.0 == k)
        {
            self.v = *v;
            return Ok(Step::Continue);
        }
        if i == Input::Enter && self.v == ArcV::ThreeP && self.vals.is_empty() {
            self.cont = curves::last_curve_end(s);
            if self.cont.is_none() {
                s.echo("No line or arc to continue.");
                return Ok(Step::Cancel);
            }
            return Ok(Step::Continue);
        }
        let Some((_, ask)) = self.ask() else { return Ok(Step::Done) };
        let val = match (&i, ask) {
            (Input::Point(q), _) => self.value_from_point(ask, *q),
            (Input::Text(t), Ask::Angle(_) | Ask::Sweep(..)) => {
                // The tangent direction is a direction; included angles are sizes.
                let au = s.angle_settings();
                let a = if self.v == ArcV::Sed { au.direction(t) } else { au.amount(t) };
                Some(Val::N(a.ok_or_else(|| other("Requires a valid angle."))?))
            }
            (Input::Text(t), Ask::Dist(_)) => Some(Val::N(number(t).ok_or_else(|| other("Requires numeric distance or a point."))?)),
            (Input::Enter, _) => return Ok(Step::Cancel),
            _ => return Err(other("Point or value required.")),
        };
        let Some(val) = val else { return Ok(Step::Continue) };
        self.vals.push(val);
        if self.vals.len() < self.v.steps().len() {
            return Ok(Step::Continue);
        }
        match self.v.build(&self.vals, s.ctrl_down) {
            Some(k) => {
                if let EntityKind::Circle(c) = &k {
                    let r = c.radius;
                    s.add_entity(k)?;
                    s.doc_mut()?.header.set_f64("CIRCLERAD", r);
                } else {
                    s.add_entity(k)?;
                }
                Ok(Step::Done)
            }
            None => {
                self.vals.pop();
                Err(other("Invalid arc or circle for these values."))
            }
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some((st, d)) = self.cont {
            let d = if s.ctrl_down { -d } else { d };
            return curves::arc_sed(st, c, d).map(|a| vec![arc(&a)]).unwrap_or_default();
        }
        let Some((_, ask)) = self.ask() else { return Vec::new() };
        let Some(v) = self.value_from_point(ask, c) else { return Vec::new() };
        let mut vals = self.vals.clone();
        vals.push(v);
        let mut out: Vec<EntityKind> = self.base().map(|b| vec![line(b, c)]).unwrap_or_default();
        if vals.len() == self.v.steps().len() {
            out.extend(self.v.build(&vals, s.ctrl_down));
        }
        out
    }
}

fn direction_param(p: &Value) -> Option<f64> {
    match p.get("direction") {
        Some(v) if v.is_array() || v.is_object() => point_value(v).filter(|d| d.len() > 1e-12).map(Vec2::angle),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(f64::to_radians),
        None => None,
    }
}

fn arc_from_params(p: &Value) -> Result<EntityKind> {
    let st = point_param(p, "start");
    let c = point_param(p, "center");
    let e = point_param(p, "end");
    let ang = p.get("angle").and_then(Value::as_f64).filter(|v| v.is_finite()).map(f64::to_radians);
    let len = p.get("length").and_then(Value::as_f64).filter(|v| v.is_finite());
    let rad = p.get("radius").and_then(Value::as_f64).filter(|v| v.is_finite());
    let dir = direction_param(p);
    let a = match (st, c, e) {
        (Some(st), Some(c), Some(e)) => Some(Arc::from_start_center_end(st, c, e)),
        (Some(st), Some(c), None) => match (ang, len) {
            (Some(a), _) => curves::arc_sca(st, c, a),
            (None, Some(l)) => curves::arc_scl(st, c, l),
            _ => return Err(bad("arc", "start+center needs `end`, `angle` or `length`")),
        },
        (Some(st), None, Some(e)) => match (ang, dir, rad) {
            (Some(a), _, _) => curves::arc_sea(st, e, a),
            (None, Some(d), _) => curves::arc_sed(st, e, Vec2::from_angle(d)),
            (None, None, Some(r)) => curves::arc_ser(st, e, r),
            _ => return Err(bad("arc", "start+end needs `angle`, `direction` or `radius`")),
        },
        _ => return Err(bad("arc", "give start with center and/or end (see params)")),
    };
    let a = a.filter(|a| a.radius > 1e-12 && a.radius.is_finite()).ok_or_else(|| bad("arc", "these values do not define an arc"))?;
    Ok(arc(&a))
}

pub(super) fn run_arc_variant(s: &mut Session, p: &Value) -> Result<Value> {
    let k = arc_from_params(p)?;
    added(s.add_entity(k)?)
}

pub(super) fn run_arc_continue(s: &mut Session, p: &Value) -> Result<Value> {
    let e = point_req("arc.continue", p, "end")?;
    let (st, dir) = match (point_param(p, "start"), direction_param(p)) {
        (Some(st), Some(d)) => (st, Vec2::from_angle(d)),
        _ => curves::last_curve_end(s).ok_or_else(|| bad("arc.continue", "no line or arc to continue from"))?,
    };
    let a = curves::arc_sed(st, e, dir).ok_or_else(|| bad("arc.continue", "end point is on the tangent line"))?;
    let h = s.add_entity(arc(&a))?;
    // The next continuation starts where this arc was drawn to, not at its stored CCW end.
    s.last_point = e;
    added(h)
}

#[derive(Default)]
pub(super) struct ArcContinueM {
    from: Option<(Vec2, Vec2)>,
}

impl Interactive for ArcContinueM {
    fn name(&self) -> &'static str {
        "ARC"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.from = curves::last_curve_end(s);
        if self.from.is_none() {
            s.echo("No line or arc to continue.");
            return Ok(Step::Cancel);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify end point of arc (hold Ctrl to switch direction)", Accept::POINT).base_opt(self.from.map(|f| f.0))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some((st, dir)) = self.from else { return Ok(Step::Cancel) };
        // Ctrl flips the direction, exactly as in `SeqM::input`.
        let dir = if s.ctrl_down { -dir } else { dir };
        match i {
            Input::Point(e) => {
                let a = curves::arc_sed(st, e, dir).ok_or_else(|| other("Invalid end point."))?;
                s.add_entity(arc(&a))?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Err(other("Point required.")),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.from
            .and_then(|(st, dir)| {
                let dir = if s.ctrl_down { -dir } else { dir };
                curves::arc_sed(st, c, dir)
            })
            .map(|a| vec![arc(&a)])
            .unwrap_or_default()
    }
}
