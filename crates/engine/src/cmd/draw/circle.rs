use super::{
    Accept, Arc, Circle, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, added, arc, bad, circle, f64_req, line, number,
    point_param, size_param,
};

pub(super) fn run_circle(s: &mut Session, p: &Value) -> Result<Value> {
    let c = if let Some(center) = point_param(p, "center") {
        // Checked before `Circle::new`, which would turn a negative radius positive.
        let r = match size_param("circle", p, "radius", false)? {
            Some(r) => r,
            None => size_param("circle", p, "diameter", false)?.ok_or_else(|| bad("circle", "`radius` or `diameter` is required"))? / 2.0,
        };
        Circle::new(center, r)
    } else if let (Some(a), Some(b)) = (point_param(p, "p1"), point_param(p, "p2")) {
        match point_param(p, "p3") {
            Some(c3) => Circle::from_3_points(a, b, c3).ok_or_else(|| bad("circle", "points are collinear"))?,
            None => Circle::from_2_points(a, b),
        }
    } else {
        return Err(bad("circle", "give {center, radius} or {p1, p2[, p3]}"));
    };
    if !(c.radius > 1e-12 && c.radius.is_finite()) {
        return Err(bad("circle", "radius must be positive"));
    }
    added(s.add_entity(circle(c.center, c.radius))?)
}

pub(super) fn run_arc(s: &mut Session, p: &Value) -> Result<Value> {
    let a = if let (Some(a), Some(b), Some(c)) = (point_param(p, "p1"), point_param(p, "p2"), point_param(p, "p3")) {
        Arc::from_3_points(a, b, c).ok_or_else(|| bad("arc", "points are collinear"))?
    } else if let (Some(st), Some(c), Some(e)) = (point_param(p, "start"), point_param(p, "center"), point_param(p, "end")) {
        Arc::from_start_center_end(st, c, e)
    } else if let Some(c) = point_param(p, "center") {
        let r = f64_req("arc", p, "radius")?;
        // Checked before `Arc::new`, which would turn a negative radius positive (zero is
        // refused below).
        if r < 0.0 {
            return Err(bad("arc", "`radius` must be positive"));
        }
        Arc::new(c, r, f64_req("arc", p, "start")?.to_radians(), f64_req("arc", p, "end")?.to_radians())
    } else {
        return Err(bad("arc", "give {p1,p2,p3}, {start,center,end} or {center,radius,start,end}"));
    };
    // Zero or non-finite radius (e.g. start == center): refuse, like the interactive ARC and the
    // arc.* variants, before anything is added (#85).
    let a = crate::cmd::curves::finite_arc(a).ok_or_else(|| bad("arc", "these values do not define an arc"))?;
    added(s.add_entity(arc(&a))?)
}

#[derive(Default)]
pub(super) struct CircleM {
    mode: u8, // 0 center-radius, 1 = 3P, 2 = 2P, 3 = TTR
    pts: Vec<Vec2>,
    diameter: bool,
    /// The `Ttr` option hands over to the tangent-circle machine.
    delegate: Option<Box<dyn Interactive>>,
}

impl Interactive for CircleM {
    fn name(&self) -> &'static str {
        "CIRCLE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if let Some(d) = &self.delegate {
            return d.prompt(s);
        }
        let n = self.pts.len();
        let last_r = s.doc().map(|d| d.header.f64("CIRCLERAD", 0.0)).unwrap_or(0.0);
        match (self.mode, n) {
            (0, 0) => Prompt::new("Specify center point for circle", Accept::POINT).kw(&["3P", "2P", "Ttr (tan tan radius)"]),
            (0, _) if self.diameter => {
                Prompt::new("Specify diameter of circle", Accept::POINT_OR_NUMBER).base_opt(self.pts.first().copied()).distance()
            }
            (0, _) => {
                let p =
                    Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).kw(&["Diameter"]).base_opt(self.pts.first().copied()).distance();
                if last_r > 0.0 { p.default(format!("{last_r:.4}")) } else { p }
            }
            (1, k) => Prompt::new(
                ["Specify first point on circle", "Specify second point on circle", "Specify third point on circle"].get(k).copied().unwrap_or(""),
                Accept::POINT,
            )
            .base_opt(self.pts.last().copied()),
            (2, k) => Prompt::new(
                if k == 0 { "Specify first end point of circle's diameter" } else { "Specify second end point of circle's diameter" },
                Accept::POINT,
            )
            .base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify center point for circle", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(d) = &mut self.delegate {
            return d.input(s, i);
        }
        match i {
            Input::Keyword(k) => {
                self.mode = match k.as_str() {
                    "3P" => 1,
                    "2P" => 2,
                    "Diameter" => {
                        self.diameter = true;
                        self.mode
                    }
                    _ => {
                        self.delegate = Some(Box::new(crate::cmd::draw2::TanCircleM::new(true)));
                        3
                    }
                };
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                if self.mode == 0 && self.pts.first().is_some_and(|c| c.near(p, 1e-12)) {
                    return Err(crate::EngineError::Other("Value must be positive and nonzero.".into()));
                }
                self.pts.push(p);
                let done = match self.mode {
                    0 if self.pts.len() == 2 => {
                        let (Some(c), Some(e)) = (self.pts.first(), self.pts.get(1)) else { return Ok(Step::Continue) };
                        let r = if self.diameter { c.dist(*e) / 2.0 } else { c.dist(*e) };
                        Some(Circle::new(*c, r))
                    }
                    1 if self.pts.len() == 3 => Circle::from_3_points(self.pts[0], self.pts[1], self.pts[2]),
                    2 if self.pts.len() == 2 => Some(Circle::from_2_points(self.pts[0], self.pts[1])),
                    _ => None,
                };
                if let Some(c) = done {
                    finish_circle(s, c)?;
                    return Ok(Step::Done);
                }
                Ok(Step::Continue)
            }
            Input::Text(t) if self.mode == 0 && self.pts.len() == 1 => {
                let v = number(&t).ok_or_else(|| crate::EngineError::Other("Requires numeric distance or second point.".into()))?;
                let r = if self.diameter { v / 2.0 } else { v };
                if r <= 0.0 {
                    return Err(crate::EngineError::Other("Value must be positive and nonzero.".into()));
                }
                finish_circle(s, Circle::new(self.pts[0], r))?;
                Ok(Step::Done)
            }
            Input::Enter if self.mode == 0 && self.pts.len() == 1 => {
                let r = s.doc()?.header.f64("CIRCLERAD", 0.0);
                if r > 0.0 {
                    finish_circle(s, Circle::new(self.pts[0], r))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Cancel),
            // Garbage at a point prompt: report it and ask again.
            Input::Text(_) if self.mode == 0 && self.pts.is_empty() => Err(crate::EngineError::Other("Point or option keyword required.".into())),
            Input::Text(_) => Err(crate::EngineError::Other("Invalid point.".into())),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(d) = &self.delegate {
            return d.preview(s, c);
        }
        match (self.mode, self.pts.as_slice()) {
            (0, [center]) => {
                let r = if self.diameter { center.dist(c) / 2.0 } else { center.dist(c) };
                vec![circle(*center, r), line(*center, c)]
            }
            (1, [a, b]) => Circle::from_3_points(*a, *b, c).map(|ci| vec![circle(ci.center, ci.radius)]).unwrap_or_default(),
            (1, [a]) | (2, [a]) => {
                let ci = Circle::from_2_points(*a, c);
                vec![circle(ci.center, ci.radius)]
            }
            _ => Vec::new(),
        }
    }
}

fn finish_circle(s: &mut Session, c: Circle) -> Result<()> {
    s.add_entity(circle(c.center, c.radius))?;
    s.doc_mut()?.header.set_f64("CIRCLERAD", c.radius);
    Ok(())
}
