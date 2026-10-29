use super::{
    Accept, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, added, bad, bool_or, lwpoly, number, point_req, points_param,
    polygon_from_edge, polygon_typed_radius_point, polygon_vertices, size_param,
};

pub(super) fn run_polygon(s: &mut Session, p: &Value) -> Result<Value> {
    // A `sides` that is not a whole number (e.g. -3) is refused, not replaced by the default 4.
    let n = match p.get("sides") {
        None | Some(Value::Null) => 4,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0),
    };
    if !(3..=1024).contains(&n) {
        return Err(bad("polygon", "sides must be 3..1024"));
    }
    let vs = if let Some(e) = points_param(p, "edge") {
        let (Some(a), Some(b)) = (e.first(), e.get(1)) else { return Err(bad("polygon", "edge needs 2 points")) };
        if a.near(*b, 1e-12) {
            return Err(bad("polygon", "edge endpoints must differ"));
        }
        polygon_from_edge(*a, *b, n)
    } else {
        let c = point_req("polygon", p, "center")?;
        let r = size_param("polygon", p, "radius", false)?.ok_or_else(|| bad("polygon", "`radius` (number) is required"))?;
        let inscribed = bool_or(p, "inscribed", true);
        // Without an angle, the bottom edge is horizontal, as with a typed radius.
        let rp = match p.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()) {
            Some(a) => Vec2::polar(c, r, a.to_radians()),
            None => polygon_typed_radius_point(c, n, r, inscribed),
        };
        polygon_vertices(c, n, rp, inscribed)
    };
    added(s.add_entity(lwpoly(vs, true))?)
}

#[derive(Default)]
pub(super) struct PolygonM {
    sides: Option<usize>,
    center: Option<Vec2>,
    edge: bool,
    edge_pts: Vec<Vec2>,
    inscribed: Option<bool>,
}

impl Interactive for PolygonM {
    fn name(&self) -> &'static str {
        "POLYGON"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let def = s.doc().map(|d| d.header.i64("POLYSIDES", 4)).unwrap_or(4);
        match (self.sides, self.edge, self.center, self.inscribed) {
            (None, _, _, _) => Prompt::new("Enter number of sides", Accept::NUMBER).default(def.to_string()),
            (Some(_), true, _, _) => Prompt::new(
                if self.edge_pts.is_empty() { "Specify first endpoint of edge" } else { "Specify second endpoint of edge" },
                Accept::POINT,
            )
            .base_opt(self.edge_pts.first().copied()),
            (Some(_), false, None, _) => Prompt::new("Specify center of polygon", Accept::POINT).kw(&["Edge"]),
            (Some(_), false, Some(_), None) => Prompt::new("Enter an option", Accept::TEXT)
                .kw(&["Inscribed in circle", "Circumscribed about circle"])
                .default(if s.last_used.polygon_circumscribed { "C" } else { "I" }),
            (Some(_), false, Some(c), Some(_)) => Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).base(c).distance(),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.sides.is_none() {
            let n = match &i {
                Input::Text(t) => t
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (3..=1024).contains(n))
                    .ok_or_else(|| crate::EngineError::Other("Requires an integer between 3 and 1024.".into()))?,
                Input::Enter => s.doc()?.header.i64("POLYSIDES", 4).clamp(3, 1024) as usize,
                _ => return Ok(Step::Continue),
            };
            self.sides = Some(n);
            s.doc_mut()?.header.set_i64("POLYSIDES", n as i64);
            return Ok(Step::Continue);
        }
        let n = self.sides.unwrap_or(4);
        if self.edge {
            if let Input::Point(p) = i {
                if self.edge_pts.first().is_some_and(|a| a.near(p, 1e-12)) {
                    return Err(crate::EngineError::Other("Edge endpoints must differ.".into()));
                }
                self.edge_pts.push(p);
                if self.edge_pts.len() == 2 {
                    s.add_entity(lwpoly(polygon_from_edge(self.edge_pts[0], self.edge_pts[1], n), true))?;
                    return Ok(Step::Done);
                }
            }
            return Ok(Step::Continue);
        }
        match (self.center, self.inscribed, i) {
            (None, _, Input::Keyword(k)) if k == "Edge" => self.edge = true,
            (None, _, Input::Point(p)) => self.center = Some(p),
            (Some(_), None, inp @ (Input::Keyword(_) | Input::Enter | Input::Text(_))) => {
                // Enter takes the option used last time (the prompt's default).
                let ins = match inp {
                    Input::Keyword(k) => k.starts_with('I'),
                    Input::Text(t) => !t.trim().to_ascii_lowercase().starts_with('c'),
                    _ => !s.last_used.polygon_circumscribed,
                };
                self.inscribed = Some(ins);
                s.last_used.polygon_circumscribed = !ins;
            }
            (Some(c), Some(ins), Input::Point(p)) => {
                if c.near(p, 1e-12) {
                    return Err(crate::EngineError::Other("Value must be positive and nonzero.".into()));
                }
                s.add_entity(lwpoly(polygon_vertices(c, n, p, ins), true))?;
                return Ok(Step::Done);
            }
            (Some(c), Some(ins), Input::Text(t)) => {
                let r = number(&t).ok_or_else(|| crate::EngineError::Other("Requires numeric distance or point.".into()))?;
                if !r.is_finite() || r <= 0.0 {
                    return Err(crate::EngineError::Other("Value must be positive and nonzero.".into()));
                }
                let rp = polygon_typed_radius_point(c, n, r, ins);
                s.add_entity(lwpoly(polygon_vertices(c, n, rp, ins), true))?;
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let n = self.sides.unwrap_or(4);
        if self.edge {
            return self.edge_pts.first().map(|a| vec![lwpoly(polygon_from_edge(*a, c, n), true)]).unwrap_or_default();
        }
        match (self.center, self.inscribed) {
            (Some(ctr), Some(ins)) => vec![lwpoly(polygon_vertices(ctr, n, c, ins), true)],
            _ => Vec::new(),
        }
    }
}
