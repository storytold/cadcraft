use super::{
    Accept, EntityKind, Input, Interactive, Point, PolyVertex, Prompt, RayLine, Result, Session, Step, Value, Vec2, added, bad, json, lwpoly, number,
    point_param, point_req, points_param, size_param, v3,
};

pub(super) fn run_point(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = match point_param(p, "at") {
        Some(a) => vec![a],
        None => points_param(p, "points").ok_or_else(|| bad("point", "`at` or `points` is required"))?,
    };
    let mut hs = Vec::new();
    for q in pts {
        hs.push(s.add_entity(EntityKind::Point(Point { p: v3(q), angle: 0.0 }))?.hex());
    }
    Ok(json!({ "handles": hs }))
}

pub(super) fn run_ray(s: &mut Session, p: &Value) -> Result<Value> {
    let b = point_req("ray", p, "base")?;
    let t = point_req("ray", p, "through")?;
    let dir = (t - b).normalized();
    if dir == Vec2::ZERO {
        return Err(bad("ray", "through point equals base"));
    }
    added(s.add_entity(EntityKind::Ray(RayLine { base: v3(b), dir: v3(dir) }))?)
}

fn donut(center: Vec2, inside: f64, outside: f64) -> EntityKind {
    let r = (inside.abs() + outside.abs()) / 4.0;
    let w = (outside.abs() - inside.abs()).abs() / 2.0;
    let mut k = lwpoly(vec![PolyVertex::with_bulge(center - Vec2::X * r, 1.0), PolyVertex::with_bulge(center + Vec2::X * r, 1.0)], true);
    if let EntityKind::LwPolyline(pl) = &mut k {
        pl.const_width = w;
    }
    k
}

pub(super) fn run_donut(s: &mut Session, p: &Value) -> Result<Value> {
    let c = point_req("donut", p, "center")?;
    let inside = size_param("donut", p, "inside", true)?.unwrap_or(0.5);
    let outside = size_param("donut", p, "outside", false)?.unwrap_or(1.0);
    if inside > outside {
        return Err(bad("donut", "`inside` must not be larger than `outside`"));
    }
    added(s.add_entity(donut(c, inside, outside))?)
}

pub(super) struct PointM {
    pub(super) multiple: bool,
}

impl Interactive for PointM {
    fn name(&self) -> &'static str {
        "POINT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let d = s.doc().ok();
        let _ = d;
        Prompt::new("Specify a point", Accept::POINT)
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let (m, sz) = s.doc().map(|d| (d.header.i64("PDMODE", 0), d.header.f64("PDSIZE", 0.0))).unwrap_or((0, 0.0));
        s.echo(format!("Current point modes:  PDMODE={m}  PDSIZE={sz:.4}"));
        Ok(Step::Continue)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                s.add_entity(EntityKind::Point(Point { p: v3(p), angle: 0.0 }))?;
                Ok(if self.multiple { Step::Continue } else { Step::Done })
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

/// RAY: a start point, then through points until Enter (XLINE is `draw2::XlineM2`).
#[derive(Default)]
pub(super) struct RayM {
    base: Option<Vec2>,
}

impl Interactive for RayM {
    fn name(&self) -> &'static str {
        "RAY"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.base {
            None => Prompt::new("Specify start point", Accept::POINT),
            Some(b) => Prompt::new("Specify through point", Accept::POINT).base(b),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                match self.base {
                    None => self.base = Some(p),
                    Some(b) => {
                        let d = (p - b).normalized();
                        if d != Vec2::ZERO {
                            s.add_entity(EntityKind::Ray(RayLine { base: v3(b), dir: v3(d) }))?;
                        }
                    }
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match self.base {
            Some(b) if !b.near(c, 1e-12) => vec![EntityKind::Ray(RayLine { base: v3(b), dir: v3((c - b).normalized()) })],
            _ => Vec::new(),
        }
    }
}

#[derive(Default)]
pub(super) struct DonutM {
    inside: Option<f64>,
    outside: Option<f64>,
}

impl Interactive for DonutM {
    fn name(&self) -> &'static str {
        "DONUT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let d = s.doc().ok();
        let di = d.map(|d| d.header.f64("DONUTID", 0.5)).unwrap_or(0.5);
        let dout = d.map(|d| d.header.f64("DONUTOD", 1.0)).unwrap_or(1.0);
        match (self.inside, self.outside) {
            (None, _) => Prompt::new("Specify inside diameter of donut", Accept::NUMBER).default(format!("{di:.4}")),
            (Some(_), None) => Prompt::new("Specify outside diameter of donut", Accept::NUMBER).default(format!("{dout:.4}")),
            _ => Prompt::new("Specify center of donut", Accept::POINT).kw(&[]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let hdr = |s: &Session, k: &str, d: f64| s.doc().map(|x| x.header.f64(k, d)).unwrap_or(d);
        match (self.inside, self.outside, i) {
            (None, _, Input::Text(t)) => {
                self.inside = Some(number(&t).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?.abs())
            }
            (None, _, Input::Enter) => self.inside = Some(hdr(s, "DONUTID", 0.5)),
            (Some(_), None, Input::Text(t)) => {
                self.outside = Some(number(&t).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?.abs())
            }
            (Some(_), None, Input::Enter) => self.outside = Some(hdr(s, "DONUTOD", 1.0)),
            (Some(a), Some(b), Input::Point(p)) => {
                s.add_entity(donut(p, a, b))?;
                let d = s.doc_mut()?;
                d.header.set_f64("DONUTID", a);
                d.header.set_f64("DONUTOD", b);
            }
            (Some(_), Some(_), Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.inside, self.outside) {
            (Some(a), Some(b)) => vec![donut(c, a, b)],
            _ => Vec::new(),
        }
    }
}
