use super::{
    Accept, Circle, Handle, Input, Interactive, Prompt, Result, Session, Step, TanObj, Value, Vec2, added, bad, circle, curves, f64_req, number,
    other, point_param, point_req,
};

pub(super) fn run_circle_variant(s: &mut Session, p: &Value) -> Result<Value> {
    let c = if let (Some(c), Some(d)) = (point_param(p, "center"), p.get("diameter").and_then(Value::as_f64)) {
        Circle::new(c, d / 2.0)
    } else if let (Some(a), Some(b)) = (point_param(p, "p1"), point_param(p, "p2")) {
        match point_param(p, "p3") {
            Some(c3) => Circle::from_3_points(a, b, c3).ok_or_else(|| bad("circle", "points are collinear"))?,
            None => Circle::from_2_points(a, b),
        }
    } else {
        return Err(bad("circle", "give {center, diameter}, {p1, p2} or {p1, p2, p3}"));
    };
    if !(c.radius > 1e-12 && c.radius.is_finite()) {
        return Err(bad("circle", "radius must be positive"));
    }
    let h = s.add_entity(circle(c.center, c.radius))?;
    s.doc_mut()?.header.set_f64("CIRCLERAD", c.radius);
    added(h)
}

// ---------------- tangent circles ----------------

fn tan_obj(s: &Session, h: Handle, pick: Vec2) -> Result<TanObj> {
    let e = curves::entity(s, h)?;
    TanObj::from_kind(&e.kind, pick).ok_or_else(|| other("Select a line, circle, arc or polyline segment."))
}

fn pick_pair(cmd: &str, p: &Value, hk: &str, pk: &str) -> Result<(Handle, Vec2)> {
    let h = curves::handle_param(p, hk).ok_or_else(|| bad(cmd, format!("`{hk}` is required")))?;
    let q = point_req(cmd, p, pk)?;
    Ok((h, q))
}

pub(super) fn run_circle_ttr(s: &mut Session, p: &Value) -> Result<Value> {
    let (h1, p1) = pick_pair("circle.ttr", p, "h1", "p1")?;
    let (h2, p2) = pick_pair("circle.ttr", p, "h2", "p2")?;
    let r = f64_req("circle.ttr", p, "radius")?;
    let c = curves::ttr(tan_obj(s, h1, p1)?, p1, tan_obj(s, h2, p2)?, p2, r).ok_or_else(|| other("Circle does not exist."))?;
    let h = s.add_entity(circle(c.center, c.radius))?;
    s.doc_mut()?.header.set_f64("CIRCLERAD", c.radius);
    added(h)
}

pub(super) fn run_circle_ttt(s: &mut Session, p: &Value) -> Result<Value> {
    let (h1, p1) = pick_pair("circle.ttt", p, "h1", "p1")?;
    let (h2, p2) = pick_pair("circle.ttt", p, "h2", "p2")?;
    let (h3, p3) = pick_pair("circle.ttt", p, "h3", "p3")?;
    let objs = [(tan_obj(s, h1, p1)?, p1), (tan_obj(s, h2, p2)?, p2), (tan_obj(s, h3, p3)?, p3)];
    let c = curves::ttt(objs).ok_or_else(|| other("Circle does not exist."))?;
    added(s.add_entity(circle(c.center, c.radius))?)
}

/// TTR (two picks + radius) or TTT (three picks). Also used by CIRCLE's `Ttr` option.
pub(crate) struct TanCircleM {
    radius_mode: bool,
    picks: Vec<(TanObj, Vec2)>,
}

impl TanCircleM {
    pub(crate) fn new(radius_mode: bool) -> Self {
        TanCircleM { radius_mode, picks: Vec::new() }
    }
}

impl Interactive for TanCircleM {
    fn name(&self) -> &'static str {
        "CIRCLE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let n = self.picks.len();
        if self.radius_mode {
            match n {
                0 => Prompt::new("Specify point on object for first tangent of circle", Accept::POINT),
                1 => Prompt::new("Specify point on object for second tangent of circle", Accept::POINT),
                _ => {
                    let r = s.doc().map(|d| d.header.f64("CIRCLERAD", 0.0)).unwrap_or(0.0);
                    let p = Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).base_opt(self.picks.last().map(|x| x.1)).distance();
                    if r > 0.0 { p.default(format!("{r:.4}")) } else { p }
                }
            }
        } else {
            let m = ["Specify first point on circle: _tan to", "Specify second point on circle: _tan to", "Specify third point on circle: _tan to"];
            Prompt::new(m.get(n).copied().unwrap_or(""), Accept::POINT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let need = if self.radius_mode { 2 } else { 3 };
        if self.picks.len() < need {
            return match i {
                Input::Point(p) => {
                    let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                    let o = tan_obj(s, h, p)?;
                    self.picks.push((o, p));
                    if !self.radius_mode && self.picks.len() == 3 {
                        let (Some(a), Some(b), Some(c)) = (self.picks.first(), self.picks.get(1), self.picks.get(2)) else {
                            return Ok(Step::Cancel);
                        };
                        let c = curves::ttt([*a, *b, *c]).ok_or_else(|| {
                            self.picks.clear();
                            other("Circle does not exist.")
                        })?;
                        s.add_entity(circle(c.center, c.radius))?;
                        return Ok(Step::Done);
                    }
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Err(other("Select an object.")),
            };
        }
        let r = match i {
            Input::Point(p) => self.picks.last().map(|x| x.1.dist(p)).unwrap_or(0.0),
            Input::Text(t) => number(&t).ok_or_else(|| other("Requires numeric distance or second point."))?,
            Input::Enter => s.doc()?.header.f64("CIRCLERAD", 0.0),
            _ => return Ok(Step::Continue),
        };
        if r.is_nan() || r <= 0.0 {
            return Err(other("Value must be positive and nonzero."));
        }
        let ((o1, p1), (o2, p2)) = match (self.picks.first(), self.picks.get(1)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return Ok(Step::Cancel),
        };
        match curves::ttr(o1, p1, o2, p2, r) {
            Some(c) => {
                s.add_entity(circle(c.center, c.radius))?;
                s.doc_mut()?.header.set_f64("CIRCLERAD", r);
                Ok(Step::Done)
            }
            None => {
                s.echo("Circle does not exist.");
                Ok(Step::Done)
            }
        }
    }
}
