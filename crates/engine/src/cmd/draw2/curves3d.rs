use super::{
    Accept, EntityKind, Handle, Helix, HelixConstrain, Input, Interactive, MAX_GEN, Polyline3d, Prompt, Result, Session, Spline, Step, Value, Vec2,
    Vec3, added, bad, bool_or, circle, curves, f64_or, f64_req, line, number, other, point_req, points_param, v3,
};

// =====================================================================================
// 3DPOLY
// =====================================================================================

fn point3_value(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    let z = a.get(2).and_then(Value::as_f64).unwrap_or(0.0);
    let p = Vec3::new(x, y, z);
    p.is_finite().then_some(p)
}

pub(super) fn run_3dpoly(s: &mut Session, p: &Value) -> Result<Value> {
    let pts: Vec<Vec3> = p.get("points").and_then(Value::as_array).map(|a| a.iter().filter_map(point3_value).collect()).unwrap_or_default();
    if pts.len() < 2 || pts.len() > MAX_GEN {
        return Err(bad("3dpoly", "`points` needs 2 or more [x,y,z] points"));
    }
    added(s.add_entity(EntityKind::Polyline3d(Polyline3d { points: pts, closed: bool_or(p, "closed", false) }))?)
}

#[derive(Default)]
pub(super) struct Poly3dM {
    pts: Vec<Vec2>,
    handle: Option<Handle>,
}

impl Poly3dM {
    fn sync(&mut self, s: &mut Session, closed: bool) -> Result<()> {
        let k = EntityKind::Polyline3d(Polyline3d { points: self.pts.iter().map(|p| v3(*p)).collect(), closed });
        match (self.handle, self.pts.len() >= 2) {
            (Some(h), true) => s.doc_mut()?.modify_entity(h, |e| e.kind = k)?,
            (None, true) => self.handle = Some(s.add_entity(k)?),
            (Some(h), false) => {
                s.doc_mut()?.remove_entity(h);
                self.handle = None;
            }
            (None, false) => {}
        }
        Ok(())
    }
}

impl Interactive for Poly3dM {
    fn name(&self) -> &'static str {
        "3DPOLY"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.pts.len() {
            0 => Prompt::new("Specify start point of polyline", Accept::POINT),
            1 | 2 => Prompt::new("Specify endpoint of line", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify endpoint of line", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                if self.pts.last().is_none_or(|l| !l.near(p, 1e-12)) && self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                    self.sync(s, false)?;
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                self.pts.pop();
                self.sync(s, false)?;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Close" => {
                self.sync(s, true)?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.pts.last().map(|l| vec![line(*l, c)]).unwrap_or_default()
    }
}

// =====================================================================================
// HELIX (3D polyline approximation)
// =====================================================================================

/// A helix entity (base centre `c`, radii, height, turns, twist, start angle).
fn helix_kind(c: Vec2, rb: f64, rt: f64, height: f64, turns: f64, ccw: bool, a0: f64) -> EntityKind {
    let h = Helix { center: c.to3(0.0), base_radius: rb, top_radius: rt, height, turns, start_angle: a0, ccw, constrain: HelixConstrain::Height };
    EntityKind::Helix(h.sanitized())
}

pub(super) fn run_helix(s: &mut Session, p: &Value) -> Result<Value> {
    let c = point_req("helix", p, "center")?;
    let rb = f64_req("helix", p, "baseRadius")?;
    let rt = f64_or(p, "topRadius", rb);
    let turns = f64_or(p, "turns", 3.0);
    if !(rb > 0.0 && rt >= 0.0 && turns > 0.0 && turns <= 500.0) {
        return Err(bad("helix", "radii must be positive and turns in (0, 500]"));
    }
    let height = f64_or(p, "height", 1.0);
    if height.abs() > 1e12 || rb > 1e12 || rt > 1e12 {
        return Err(bad("helix", "sizes are out of range"));
    }
    added(s.add_entity(helix_kind(c, rb, rt, height, turns, bool_or(p, "ccw", true), f64_or(p, "startAngle", 0.0).to_radians()))?)
}

#[derive(Default)]
pub(super) struct HelixM {
    center: Option<Vec2>,
    rb: Option<f64>,
    rt: Option<f64>,
    turns: Option<f64>,
    cw: bool,
    diameter: bool,
    asking: u8, // 0 main, 1 turns, 2 twist, 3 turn height
    start: Option<f64>,
}

impl HelixM {
    fn turns(&self) -> f64 {
        self.turns.unwrap_or(3.0)
    }
    fn create(&self, s: &mut Session, height: f64) -> Result<Step> {
        let (Some(c), Some(rb)) = (self.center, self.rb) else { return Ok(Step::Cancel) };
        let rt = self.rt.unwrap_or(rb);
        if !height.is_finite() || height.abs() > 1e12 {
            return Err(other("Requires a distance."));
        }
        s.add_entity(helix_kind(c, rb, rt, height, self.turns(), !self.cw, self.start.unwrap_or(0.0)))?;
        Ok(Step::Done)
    }
}

impl Interactive for HelixM {
    fn name(&self) -> &'static str {
        "HELIX"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Number of turns = {:.4}     Twist=CCW", self.turns()));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.asking {
            1 => return Prompt::new("Enter number of turns", Accept::NUMBER).default(format!("{:.4}", self.turns())),
            2 => return Prompt::new("Enter twist direction of helix", curves::KW).kw(&["CW", "CCW"]).default("CCW"),
            3 => return Prompt::new("Specify distance between turns", Accept::NUMBER),
            _ => {}
        }
        match (self.center, self.rb, self.rt) {
            (None, ..) => Prompt::new("Specify center point of base", Accept::POINT),
            (Some(c), None, _) => {
                Prompt::new(if self.diameter { "Specify base diameter" } else { "Specify base radius or" }, Accept::POINT_OR_NUMBER)
                    .kw(if self.diameter { &[] } else { &["Diameter"] })
                    .default("1.0000")
                    .base(c)
            }
            (Some(c), Some(rb), None) => {
                Prompt::new(if self.diameter { "Specify top diameter" } else { "Specify top radius or" }, Accept::POINT_OR_NUMBER)
                    .kw(if self.diameter { &[] } else { &["Diameter"] })
                    .default(format!("{rb:.4}"))
                    .base(c)
            }
            (Some(c), Some(_), Some(_)) => Prompt::new("Specify helix height or", Accept::POINT_OR_NUMBER)
                .kw(&["Axis endpoint", "Turns", "turn Height", "tWist"])
                .default("1.0000")
                .base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.asking {
            1 => {
                if let Input::Text(t) = &i {
                    let v = number(t).filter(|v| *v > 0.0 && *v <= 500.0).ok_or_else(|| other("Turns must be in (0, 500]."))?;
                    self.turns = Some(v);
                }
                self.asking = 0;
                return Ok(Step::Continue);
            }
            2 => {
                if let Input::Keyword(k) | Input::Text(k) = &i {
                    self.cw = k.eq_ignore_ascii_case("cw");
                }
                self.asking = 0;
                return Ok(Step::Continue);
            }
            3 => {
                self.asking = 0;
                if let Input::Text(t) = &i {
                    let v = number(t).ok_or_else(|| other("Requires a distance."))?;
                    return self.create(s, v * self.turns());
                }
                return Ok(Step::Continue);
            }
            _ => {}
        }
        let num = |i: &Input, base: Option<Vec2>| -> Option<f64> {
            match i {
                Input::Point(p) => base.map(|b| b.dist(*p)),
                Input::Text(t) => number(t),
                _ => None,
            }
        };
        match (self.center, self.rb, self.rt, &i) {
            (None, _, _, Input::Point(p)) => {
                self.center = Some(*p);
                Ok(Step::Continue)
            }
            (Some(_), None | Some(_), _, Input::Keyword(k)) if k == "Diameter" => {
                self.diameter = true;
                Ok(Step::Continue)
            }
            (Some(c), None, _, _) => {
                let v = match &i {
                    Input::Enter => Some(1.0),
                    _ => num(&i, Some(c)),
                };
                let v = v.filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive distance."))?;
                if let Input::Point(p) = &i {
                    self.start = Some(c.angle_to(*p));
                }
                self.rb = Some(if self.diameter { v / 2.0 } else { v });
                self.diameter = false;
                Ok(Step::Continue)
            }
            (Some(c), Some(rb), None, _) => {
                let v = match &i {
                    Input::Enter => Some(if self.diameter { rb * 2.0 } else { rb }),
                    _ => num(&i, Some(c)),
                };
                let v = v.filter(|v| *v >= 0.0).ok_or_else(|| other("Requires a distance."))?;
                self.rt = Some(if self.diameter { v / 2.0 } else { v });
                Ok(Step::Continue)
            }
            (Some(c), Some(_), Some(_), _) => match &i {
                Input::Keyword(k) => {
                    self.asking = match k.as_str() {
                        "Turns" => 1,
                        "tWist" => 2,
                        "turn Height" => 3,
                        _ => 0,
                    };
                    Ok(Step::Continue)
                }
                Input::Enter => self.create(s, 1.0),
                _ => {
                    let h = num(&i, Some(c)).ok_or_else(|| other("Requires a distance."))?;
                    self.create(s, h)
                }
            },
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.center, self.rb, self.rt) {
            (Some(ct), None, _) => vec![circle(ct, ct.dist(c))],
            (Some(ct), Some(rb), None) => {
                vec![helix_kind(ct, rb, ct.dist(c), 0.0, self.turns(), !self.cw, self.start.unwrap_or(0.0))]
            }
            _ => Vec::new(),
        }
    }
}

// =====================================================================================
// SPLINE by control vertices
// =====================================================================================

pub(super) fn run_spline_cv(s: &mut Session, p: &Value) -> Result<Value> {
    let mut c = points_param(p, "control").ok_or_else(|| bad("spline.cv", "`control` points are required"))?;
    if c.len() < 2 || c.len() > MAX_GEN {
        return Err(bad("spline.cv", "need 2+ control points"));
    }
    let closed = bool_or(p, "closed", false);
    if closed && let Some(f) = c.first().copied() {
        c.push(f);
    }
    let deg = p.get("degree").and_then(Value::as_u64).unwrap_or(3).clamp(1, 10) as usize;
    let mut sp = Spline::from_control(c, deg);
    sp.closed = closed;
    added(s.add_entity(EntityKind::Spline(sp))?)
}

#[derive(Default)]
pub(super) struct SplineCvM {
    pts: Vec<Vec2>,
    degree: Option<usize>,
    asking_degree: bool,
}

impl Interactive for SplineCvM {
    fn name(&self) -> &'static str {
        "SPLINE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Current settings: Method=CV   Degree={}", self.degree.unwrap_or(3)));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_degree {
            return Prompt::new("Enter degree of spline", Accept::NUMBER).default(self.degree.unwrap_or(3).to_string());
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first point", Accept::POINT).kw(&["Method", "Degree", "Object"]),
            1 => Prompt::new("Enter next point", Accept::POINT).base_opt(self.pts.last().copied()),
            2 => Prompt::new("Enter next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Enter next point", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_degree {
            if let Input::Text(t) = &i {
                let d = number(t).filter(|d| (1.0..=10.0).contains(d)).ok_or_else(|| other("Degree must be 1..10."))?;
                self.degree = Some(d as usize);
            }
            self.asking_degree = false;
            return Ok(Step::Continue);
        }
        let deg = self.degree.unwrap_or(3);
        match i {
            Input::Point(p) => {
                if self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) => {
                match k.as_str() {
                    "Degree" => self.asking_degree = true,
                    "Undo" => {
                        self.pts.pop();
                    }
                    "Close" => {
                        let mut c = self.pts.clone();
                        c.extend(self.pts.first().copied());
                        let mut sp = Spline::from_control(c, deg);
                        sp.closed = true;
                        s.add_entity(EntityKind::Spline(sp))?;
                        return Ok(Step::Done);
                    }
                    _ => s.echo(format!("{k}: use SPLINE for fit points.")),
                }
                Ok(Step::Continue)
            }
            Input::Enter => {
                if self.pts.len() >= 2 {
                    s.add_entity(EntityKind::Spline(Spline::from_control(self.pts.clone(), deg)))?;
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        pts.push(c);
        vec![EntityKind::Spline(Spline::from_control(pts, self.degree.unwrap_or(3)))]
    }
}
