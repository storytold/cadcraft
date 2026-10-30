use super::{
    Accept, Ellipse, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, added, bad, f64_or, f64_req, line, number,
    point_req, v3,
};

pub(super) fn run_ellipse(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("isocircle").is_some() {
        return crate::cmd::settings::iso::run_isocircle(s, p);
    }
    let c = point_req("ellipse", p, "center")?;
    let m = point_req("ellipse", p, "major")?;
    let ratio = f64_req("ellipse", p, "ratio")?;
    if !(ratio > 0.0 && ratio <= 1.0) || m.len() < 1e-12 {
        return Err(bad("ellipse", "ratio must be in (0, 1] and major non-zero"));
    }
    let start = f64_or(p, "start", 0.0).to_radians();
    let end = f64_or(p, "end", 360.0).to_radians();
    added(s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse { center: v3(c), major: v3(m), ratio, start, end }))?)
}

/// ELLIPSE prompt machine starting in axis-end or center mode, optionally for an elliptical arc.
pub(crate) fn ellipse_machine(center: bool, arc: bool) -> Box<dyn Interactive> {
    Box::new(EllipseM { center_mode: center, arc_mode: arc, ..Default::default() })
}

#[derive(Default)]
pub(super) struct EllipseM {
    center_mode: bool,
    arc_mode: bool,
    pts: Vec<Vec2>,
    /// The `Rotation` option: the next value is the rotation around the major axis.
    rotation: bool,
    ellipse: Option<Ellipse>,
    /// The `Parameter` option: arc start/end values are ellipse parameters, not true angles.
    param: bool,
    start: Option<f64>,
    /// The `Included angle` option: the next value is the arc's angle from its start angle.
    included: bool,
    /// The Isocircle option (isometric snap style) in progress.
    iso: Option<crate::cmd::settings::iso::IsoCircle>,
}

impl EllipseM {
    fn axis(&self) -> Option<(Vec2, Vec2)> {
        // (center, major vector): towards the first axis endpoint, as AutoCAD stores it.
        match (self.center_mode, self.pts.as_slice()) {
            (true, [c, e, ..]) => Some((*c, *e - *c)),
            (false, [a, b, ..]) => Some((a.mid(*b), *a - a.mid(*b))),
            _ => None,
        }
    }
    /// The other half axis reaches as far as `other` is from the center (AutoCAD measures the
    /// picked point's distance from the center, not from the first axis). Longer than the
    /// first half axis, it becomes the major axis: the first axis turned 90° counterclockwise.
    fn build(&self, other: Vec2) -> Option<Ellipse> {
        let (c, m) = self.axis()?;
        let ml = m.len();
        if ml < 1e-12 {
            return None;
        }
        let u = m / ml;
        let minor = (other - c).len();
        let (major, ratio) = if minor > ml { (u.perp() * minor, ml / minor) } else { (m, minor / ml) };
        (ratio > 1e-9).then_some(Ellipse::full(c, major, ratio.min(1.0)))
    }
    /// The ellipse seen as a circle on the first axis rotated by `a` around it: ratio = cos(a),
    /// for rotations up to 89.4°.
    fn rotated(&self, a: f64) -> Option<Ellipse> {
        let (c, m) = self.axis()?;
        let ratio = a.cos().abs();
        (m.len() >= 1e-12 && ratio >= 89.4f64.to_radians().cos()).then_some(Ellipse::full(c, m, ratio.min(1.0)))
    }
    /// The full ellipse is known: draw it, or go on to the arc angles.
    fn place(&mut self, s: &mut Session, e: Ellipse) -> Result<Step> {
        if self.arc_mode {
            self.ellipse = Some(e);
            return Ok(Step::Continue);
        }
        s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse {
            center: v3(e.center),
            major: v3(e.major),
            ratio: e.ratio,
            start: 0.0,
            end: cadcraft_geom::TAU,
        }))?;
        Ok(Step::Done)
    }
}

impl Interactive for EllipseM {
    fn name(&self) -> &'static str {
        "ELLIPSE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if let Some(e) = &self.ellipse {
            if self.included {
                return Prompt::new("Specify included angle for arc", Accept::POINT_OR_NUMBER).default("180").base(e.center);
            }
            let (msg, kw): (&str, &[&str]) = match (self.start.is_none(), self.param) {
                (true, false) => ("Specify start angle", &["Parameter"]),
                (false, false) => ("Specify end angle", &["Parameter", "Included angle"]),
                (true, true) => ("Specify start parameter", &["Angle"]),
                (false, true) => ("Specify end parameter", &["Angle"]),
            };
            return Prompt::new(msg, Accept::POINT_OR_NUMBER).kw(kw).base(e.center);
        }
        if let Some(iso) = &self.iso {
            return iso.prompt();
        }
        match (self.center_mode, self.pts.len()) {
            (false, 0) if s.settings.isodraft => Prompt::new("Specify axis endpoint of ellipse", Accept::POINT).kw(&["Arc", "Center", "Isocircle"]),
            (false, 0) => Prompt::new("Specify axis endpoint of ellipse", Accept::POINT).kw(&["Arc", "Center"]),
            (false, 1) => Prompt::new("Specify other endpoint of axis", Accept::POINT).base_opt(self.pts.first().copied()),
            (true, 0) => Prompt::new("Specify center of ellipse", Accept::POINT),
            (true, 1) => Prompt::new("Specify endpoint of axis", Accept::POINT).base_opt(self.pts.first().copied()),
            _ if self.rotation => Prompt::new("Specify rotation around major axis", Accept::POINT_OR_NUMBER).base_opt(self.axis().map(|a| a.0)),
            _ => Prompt::new("Specify distance to other axis", Accept::POINT_OR_NUMBER).kw(&["Rotation"]).base_opt(self.axis().map(|a| a.0)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(e) = self.ellipse {
            let ge = Ellipse { start: 0.0, end: cadcraft_geom::TAU, ..e };
            if self.included {
                let incl = match i {
                    Input::Point(p) => (p - e.center).angle(),
                    Input::Text(t) => s.angle_settings().amount(&t).ok_or_else(|| crate::EngineError::Other("Requires an angle.".into()))?,
                    Input::Enter => std::f64::consts::PI,
                    _ => return Ok(Step::Continue),
                };
                // A true angle added to the start angle (AutoCAD: start 214.659°, included
                // 232.716° ends the arc at 87.375° from the major axis).
                let st = self.start.unwrap_or(0.0);
                let end = ge.param_of(e.center + Vec2::from_angle((ge.at_param(st) - e.center).angle() + incl));
                s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse {
                    center: v3(e.center),
                    major: v3(e.major),
                    ratio: e.ratio,
                    start: st,
                    end,
                }))?;
                return Ok(Step::Done);
            }
            let ang = match i {
                Input::Keyword(k) => {
                    match k.as_str() {
                        "Parameter" => self.param = true,
                        "Angle" => self.param = false,
                        "Included angle" if self.start.is_some() && !self.param => self.included = true,
                        _ => {}
                    }
                    return Ok(Step::Continue);
                }
                // A parameter is the angle from the major axis taken as the parameter itself.
                Input::Point(p) if self.param => cadcraft_geom::norm_angle((p - e.center).angle() - e.major.angle()),
                Input::Point(p) => ge.param_of(p),
                Input::Text(t) => {
                    let a = s.angle_settings().amount(&t).ok_or_else(|| crate::EngineError::Other("Requires an angle.".into()))?;
                    if self.param {
                        cadcraft_geom::norm_angle(a)
                    } else {
                        // A typed angle is a true angle from the major axis; convert it to an ellipse parameter like a picked point.
                        ge.param_of(e.center + Vec2::from_angle(a + e.major.angle()))
                    }
                }
                _ => return Ok(Step::Continue),
            };
            match self.start {
                None => {
                    self.start = Some(ang);
                    return Ok(Step::Continue);
                }
                Some(st) => {
                    s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse {
                        center: v3(e.center),
                        major: v3(e.major),
                        ratio: e.ratio,
                        start: st,
                        end: ang,
                    }))?;
                    return Ok(Step::Done);
                }
            }
        }
        if let Some(iso) = &mut self.iso {
            return match iso.input(s, i)? {
                Some(e) => self.place(s, e),
                None => Ok(Step::Continue),
            };
        }
        match i {
            Input::Keyword(k) if k == "Isocircle" && self.pts.is_empty() => self.iso = Some(Default::default()),
            Input::Keyword(k) if k == "Center" => self.center_mode = true,
            Input::Keyword(k) if k == "Arc" => self.arc_mode = true,
            Input::Keyword(k) if k == "Rotation" && self.pts.len() == 2 => self.rotation = true,
            Input::Point(p) if self.pts.len() < 2 => self.pts.push(p),
            Input::Point(p) if self.rotation => {
                if let Some(e) = self.axis().and_then(|(c, _)| self.rotated((p - c).angle())) {
                    return self.place(s, e);
                }
            }
            Input::Point(p) => {
                if let Some(e) = self.build(p) {
                    return self.place(s, e);
                }
            }
            Input::Text(t) if self.pts.len() == 2 && self.rotation => {
                let e = s
                    .angle_settings()
                    .amount(&t)
                    .and_then(|a| self.rotated(a))
                    .ok_or_else(|| crate::EngineError::Other("Requires an angle between 0 and 89.4 degrees.".into()))?;
                return self.place(s, e);
            }
            Input::Text(t) if self.pts.len() == 2 => {
                let d = number(&t).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
                if let Some((c, m)) = self.axis()
                    && let Some(e) = self.build(c + m.normalized().perp() * d)
                {
                    return self.place(s, e);
                }
            }
            Input::Enter => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(iso) = &self.iso {
            return iso.preview(s, c).map(|e| crate::cmd::settings::iso::ellipse_entity(&e)).into_iter().collect();
        }
        if self.pts.len() < 2 {
            return self.pts.first().map(|a| vec![line(*a, c)]).unwrap_or_default();
        }
        let e = if self.rotation { self.axis().and_then(|(ctr, _)| self.rotated((c - ctr).angle())) } else { self.build(c) };
        e.map(|e| {
            vec![EntityKind::Ellipse(cadcraft_doc::Ellipse {
                center: v3(e.center),
                major: v3(e.major),
                ratio: e.ratio,
                start: 0.0,
                end: cadcraft_geom::TAU,
            })]
        })
        .unwrap_or_default()
    }
}
