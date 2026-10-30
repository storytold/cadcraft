use super::{
    Accept, EntityKind, Input, Interactive, Prompt, Result, Session, Spline, Step, Value, Vec2, added, bad, bool_or, distinct_points, find_command,
    points_param,
};

pub(super) fn run_spline(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("object").is_some() {
        return crate::cmd::spline_opts::run_object(s, p);
    }
    let sp = if let Some(f) = points_param(p, "fit") {
        if distinct_points(&f).len() < 2 {
            return Err(bad("spline", "need 2+ different fit points"));
        }
        Spline::fit_with(&f, bool_or(p, "closed", false), crate::cmd::spline_opts::json_options(s, p)?)
    } else if let Some(c) = points_param(p, "control") {
        if c.len() < 2 || c.len() > crate::cmd::curves::MAX_GEN {
            return Err(bad("spline", "need 2+ control points"));
        }
        // Degree 1..=10 as at the Degree prompt, `spline.cv` and DXF input (tessellation grows
        // with its square).
        Spline::from_control(c, p.get("degree").and_then(Value::as_u64).unwrap_or(3).clamp(1, 10) as usize)
    } else {
        return Err(bad("spline", "`fit` or `control` points are required"));
    };
    added(s.add_entity(EntityKind::Spline(sp))?)
}

#[derive(Default)]
pub(super) struct SplineM {
    pts: Vec<Vec2>,
    asking_method: bool,
    /// `Method` > `CV` hands over to the control-vertex machine (`spline.cv`).
    delegate: Option<Box<dyn Interactive>>,
    /// Knots, end tangents and fit tolerance chosen so far.
    opts: cadcraft_geom::FitOptions,
    /// An option asking a further question (Knots, Tangency, toLerance, Object).
    ask: Option<crate::cmd::spline_opts::SplineAsk>,
}

impl SplineM {
    fn make(&self, closed: bool) -> Spline {
        Spline::fit_with(&self.pts, closed, self.opts)
    }
}

impl Interactive for SplineM {
    fn name(&self) -> &'static str {
        "SPLINE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.opts.knots = crate::cmd::spline_opts::current_knots(s);
        s.echo(format!("Current settings: Method=Fit   Knots={}", self.opts.knots.name()));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if let Some(d) = &self.delegate {
            return d.prompt(s);
        }
        if let Some(a) = &self.ask {
            return a.prompt(&self.opts, &self.pts);
        }
        if self.asking_method {
            return Prompt::new("Enter spline creation method", crate::cmd::curves::KW).kw(&["Fit", "CV"]).default("Fit");
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first point", Accept::POINT).kw(&["Method", "Knots", "Object"]),
            1 => Prompt::new("Enter next point", Accept::POINT).kw(&["start Tangency", "toLerance"]).base_opt(self.pts.last().copied()),
            2 => Prompt::new("Enter next point", Accept::POINT).kw(&["end Tangency", "toLerance", "Undo"]).base_opt(self.pts.last().copied()),
            _ => {
                Prompt::new("Enter next point", Accept::POINT).kw(&["end Tangency", "toLerance", "Undo", "Close"]).base_opt(self.pts.last().copied())
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(d) = &mut self.delegate {
            return d.input(s, i);
        }
        if let Some(a) = &mut self.ask {
            use crate::cmd::spline_opts::Outcome;
            return match a.input(s, i, &mut self.opts, &self.pts)? {
                Outcome::Stay => Ok(Step::Continue),
                Outcome::Back => {
                    self.ask = None;
                    Ok(Step::Continue)
                }
                Outcome::Finish => {
                    self.ask = None;
                    if self.pts.len() >= 2 {
                        s.add_entity(EntityKind::Spline(self.make(false)))?;
                    }
                    Ok(Step::Done)
                }
                Outcome::Done => Ok(Step::Done),
            };
        }
        if self.asking_method {
            match i {
                Input::Keyword(k) if k == "CV" => {
                    let Some(factory) = find_command("spline.cv").and_then(|c| c.interactive) else {
                        return Err(crate::EngineError::Other("CV: not available yet".into()));
                    };
                    let mut m = factory(s)?;
                    self.asking_method = false;
                    let step = m.begin(s)?;
                    self.delegate = Some(m);
                    return Ok(step);
                }
                Input::Keyword(_) | Input::Enter => self.asking_method = false,
                Input::Text(_) => return Err(crate::EngineError::Other("Invalid option keyword.".into())),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Close" => {
                // A smooth closed (periodic) curve, not a repeat of the first point.
                s.add_entity(EntityKind::Spline(self.make(true)))?;
                Ok(Step::Done)
            }
            Input::Keyword(k) if k == "Method" => {
                self.asking_method = true;
                Ok(Step::Continue)
            }
            Input::Keyword(k) => {
                // Knots, Object, start/end Tangency, toLerance ask their own question.
                self.ask = Some(
                    crate::cmd::spline_opts::SplineAsk::from_keyword(&k)
                        .ok_or_else(|| crate::EngineError::Other("Invalid option keyword.".into()))?,
                );
                Ok(Step::Continue)
            }
            Input::Enter => {
                if self.pts.len() >= 2 {
                    s.add_entity(EntityKind::Spline(self.make(false)))?;
                }
                Ok(Step::Done)
            }
            Input::Text(_) => Err(crate::EngineError::Other("Point or option keyword required.".into())),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(d) = &self.delegate {
            return d.preview(s, c);
        }
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut opts = self.opts;
        let mut pts = self.pts.clone();
        match self.ask {
            // Rubber-band the tangent being picked.
            Some(crate::cmd::spline_opts::SplineAsk::StartTangent) => {
                opts.start_tangent = self.pts.first().map(|f| c - *f).filter(|d| d.len() > 1e-12)
            }
            Some(crate::cmd::spline_opts::SplineAsk::EndTangent) => opts.end_tangent = self.pts.last().map(|l| c - *l).filter(|d| d.len() > 1e-12),
            Some(_) => {}
            None => pts.push(c),
        }
        if pts.len() < 2 {
            return Vec::new();
        }
        vec![EntityKind::Spline(Spline::fit_with(&pts, false, opts))]
    }
}
