use super::{
    Accept, EntityKind, Handle, Input, Interactive, Polyline, Prompt, RayLine, Result, Session, Step, Value, Vec2, added, bad, bool_or, curves,
    f64_req, number, other, point_param, point_req, v3,
};

// =====================================================================================
// XLINE with Hor/Ver/Ang/Bisect/Offset
// =====================================================================================

fn add_xline(s: &mut Session, base: Vec2, dir: Vec2) -> Result<Handle> {
    let d = dir.normalized();
    if d == Vec2::ZERO || !base.is_finite() {
        return Err(other("Invalid direction."));
    }
    s.add_entity(EntityKind::XLine(RayLine { base: v3(base), dir: v3(d) }))
}

/// A line-like object for XLINE Offset/Reference: (base, direction). For a polyline, the
/// segment nearest `pick` (the first without one), which must be straight.
fn linear_of(s: &Session, h: Handle, pick: Option<Vec2>) -> Result<(Vec2, Vec2)> {
    let e = curves::entity(s, h)?;
    match &e.kind {
        EntityKind::Line(l) => Ok((l.a.xy(), (l.b.xy() - l.a.xy()).normalized())),
        EntityKind::XLine(r) | EntityKind::Ray(r) => Ok((r.base.xy(), r.dir.xy().normalized())),
        EntityKind::LwPolyline(p) => {
            let segs = Polyline { vertices: p.vertices.clone(), closed: p.closed }.segments();
            let seg = match pick {
                Some(q) => segs.iter().min_by(|a, b| a.closest(q).dist(q).total_cmp(&b.closest(q).dist(q))),
                None => segs.first(),
            };
            match seg {
                Some(cadcraft_geom::Segment::Line(l)) => Ok((l.a, l.dir())),
                _ => Err(other("Select a line object.")),
            }
        }
        _ => Err(other("Select a line object.")),
    }
}

fn bisect_dir(v: Vec2, a: Vec2, b: Vec2) -> Option<Vec2> {
    let u1 = (a - v).normalized();
    let u2 = (b - v).normalized();
    if u1 == Vec2::ZERO || u2 == Vec2::ZERO {
        return None;
    }
    let s = u1 + u2;
    if s.len() < 1e-12 { Some(u1.perp()) } else { Some(s.normalized()) }
}

fn offset_xline(base: Vec2, dir: Vec2, dist: f64, side: Vec2) -> (Vec2, Vec2) {
    let n = dir.perp();
    let sgn = if n.dot(side - base) >= 0.0 { 1.0 } else { -1.0 };
    (base + n * (dist * sgn), dir)
}

pub(crate) fn run_xline2(s: &mut Session, p: &Value) -> Result<Value> {
    if let (Some(v), Some(a), Some(b)) = (point_param(p, "vertex"), point_param(p, "start"), point_param(p, "end")) {
        let d = bisect_dir(v, a, b).ok_or_else(|| bad("xline", "degenerate angle"))?;
        return added(add_xline(s, v, d)?);
    }
    if let Some(h) = curves::handle_param(p, "handle") {
        let (base, dir) = linear_of(s, h, None)?;
        let (nb, nd) = if let Some(t) = point_param(p, "through") {
            (t, dir)
        } else {
            let dist = f64_req("xline", p, "distance")?;
            let side = point_req("xline", p, "side")?;
            offset_xline(base, dir, dist, side)
        };
        return added(add_xline(s, nb, nd)?);
    }
    let b = point_param(p, "base").or_else(|| point_param(p, "through")).ok_or_else(|| bad("xline", "`base` is required"))?;
    let dir = if bool_or(p, "hor", false) {
        Vec2::X
    } else if bool_or(p, "ver", false) {
        Vec2::Y
    } else if let (Some(t), Some(_)) = (point_param(p, "through"), point_param(p, "base")) {
        t - b
    } else {
        Vec2::from_angle(f64_req("xline", p, "angle")?.to_radians())
    };
    if dir.len() < 1e-12 {
        return Err(bad("xline", "through point equals base"));
    }
    added(add_xline(s, b, dir)?)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum XMode {
    #[default]
    Two,
    Fixed(f64),
    AngAsk,
    RefSelect,
    RefAngle(f64),
    Bisect,
    OffDist,
    OffSelect,
    OffSide,
}

#[derive(Default)]
pub(crate) struct XlineM2 {
    mode: XMode,
    base: Option<Vec2>,
    bis: Vec<Vec2>,
    offset: Option<f64>, // None = Through
    line: Option<(Vec2, Vec2)>,
}

impl Interactive for XlineM2 {
    fn name(&self) -> &'static str {
        "XLINE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.mode {
            XMode::Two => match self.base {
                None => Prompt::new("Specify a point", Accept::POINT).kw(&["Hor", "Ver", "Ang", "Bisect", "Offset"]),
                Some(b) => Prompt::new("Specify through point", Accept::POINT).base(b),
            },
            XMode::Fixed(_) => Prompt::new("Specify through point", Accept::POINT),
            XMode::AngAsk => Prompt::new("Enter angle of xline (0) or", Accept::NUMBER).kw(&["Reference"]),
            XMode::RefSelect => Prompt::new("Select a line object", Accept::POINT),
            XMode::RefAngle(_) => Prompt::new("Enter angle of xline", Accept::NUMBER).default("0"),
            XMode::Bisect => match self.bis.len() {
                0 => Prompt::new("Specify angle vertex point", Accept::POINT),
                1 => Prompt::new("Specify angle start point", Accept::POINT).base_opt(self.bis.first().copied()),
                _ => Prompt::new("Specify angle end point", Accept::POINT).base_opt(self.bis.first().copied()),
            },
            XMode::OffDist => {
                let d = s.doc().map(|d| d.header.f64("OFFSETDIST", 1.0)).unwrap_or(1.0);
                Prompt::new("Specify offset distance or", Accept::NUMBER).kw(&["Through"]).default(if d < 0.0 {
                    "Through".into()
                } else {
                    format!("{d:.4}")
                })
            }
            XMode::OffSelect => Prompt::new("Select a line object", Accept::POINT),
            XMode::OffSide => Prompt::new(if self.offset.is_some() { "Specify side to offset" } else { "Specify through point" }, Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.mode, i) {
            (_, Input::Enter) if matches!(self.mode, XMode::OffDist) => {
                let d = s.doc()?.header.f64("OFFSETDIST", 1.0);
                self.offset = (d >= 0.0).then_some(d);
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            (XMode::Two, Input::Keyword(k)) => {
                self.mode = match k.as_str() {
                    "Hor" => XMode::Fixed(0.0),
                    "Ver" => XMode::Fixed(std::f64::consts::FRAC_PI_2),
                    "Ang" => XMode::AngAsk,
                    "Bisect" => XMode::Bisect,
                    _ => XMode::OffDist,
                };
                Ok(Step::Continue)
            }
            (XMode::Two, Input::Point(p)) => {
                match self.base {
                    None => self.base = Some(p),
                    Some(b) => {
                        if !b.near(p, 1e-12) {
                            add_xline(s, b, p - b)?;
                        }
                    }
                }
                Ok(Step::Continue)
            }
            (XMode::Fixed(a), Input::Point(p)) => {
                add_xline(s, p, Vec2::from_angle(a))?;
                Ok(Step::Continue)
            }
            (XMode::AngAsk, Input::Keyword(_)) => {
                self.mode = XMode::RefSelect;
                Ok(Step::Continue)
            }
            (XMode::AngAsk, Input::Text(t)) => {
                let a = s.angle_settings().direction(&t).ok_or_else(|| other("Requires a valid angle."))?;
                self.mode = XMode::Fixed(a);
                Ok(Step::Continue)
            }
            (XMode::RefSelect, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                let (_, d) = linear_of(s, h, Some(p))?;
                self.mode = XMode::RefAngle(d.angle());
                Ok(Step::Continue)
            }
            (XMode::RefAngle(base), Input::Text(t)) => {
                let a = s.angle_settings().rotation(&t).ok_or_else(|| other("Requires a valid angle."))?;
                self.mode = XMode::Fixed(base + a);
                Ok(Step::Continue)
            }
            (XMode::Bisect, Input::Point(p)) => {
                if self.bis.len() < 2 {
                    self.bis.push(p);
                } else if let (Some(v), Some(a)) = (self.bis.first().copied(), self.bis.get(1).copied()) {
                    let d = bisect_dir(v, a, p).ok_or_else(|| other("Invalid angle."))?;
                    add_xline(s, v, d)?;
                }
                Ok(Step::Continue)
            }
            (XMode::OffDist, Input::Keyword(_)) => {
                self.offset = None;
                s.doc_mut()?.header.set_f64("OFFSETDIST", -1.0);
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            (XMode::OffDist, Input::Text(t)) => {
                let d = number(&t).filter(|d| *d >= 0.0).ok_or_else(|| other("Requires a non-negative distance."))?;
                self.offset = Some(d);
                s.doc_mut()?.header.set_f64("OFFSETDIST", d);
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            (XMode::OffSelect, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                self.line = Some(linear_of(s, h, Some(p))?);
                self.mode = XMode::OffSide;
                Ok(Step::Continue)
            }
            (XMode::OffSide, Input::Point(p)) => {
                let (b, d) = self.line.ok_or_else(|| other("No line selected."))?;
                let (nb, nd) = match self.offset {
                    Some(dist) => offset_xline(b, d, dist, p),
                    None => (p, d),
                };
                add_xline(s, nb, nd)?;
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            _ => Err(other("Invalid input.")),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let xl = |b: Vec2, d: Vec2| {
            let d = d.normalized();
            (d != Vec2::ZERO).then(|| EntityKind::XLine(RayLine { base: v3(b), dir: v3(d) }))
        };
        match self.mode {
            XMode::Two => self.base.and_then(|b| xl(b, c - b)).into_iter().collect(),
            XMode::Fixed(a) => xl(c, Vec2::from_angle(a)).into_iter().collect(),
            XMode::Bisect => match self.bis.as_slice() {
                [v, a] => bisect_dir(*v, *a, c).and_then(|d| xl(*v, d)).into_iter().collect(),
                _ => Vec::new(),
            },
            XMode::OffSide => self
                .line
                .and_then(|(b, d)| {
                    let (nb, nd) = match self.offset {
                        Some(dist) => offset_xline(b, d, dist, c),
                        None => (c, d),
                    };
                    xl(nb, nd)
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }
}
