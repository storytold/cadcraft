use super::common::{DimText, add_dim, assoc_enabled, dim, hex_param, pick_at, segment_at, with_assoc};
use super::{
    Accept, AssocSnap, DimAssoc, DimKind, EntityKind, Handle, Input, Interactive, Prompt, Result, Segment, Session, Step, Value, Vec2, bad, json,
    point_req, targets, v3,
};

/// Two-line angular dimension (type 2) between two lines, attached to them.
fn angular_from_lines(s: &Session, h1: Handle, h2: Handle, at: Vec2) -> Option<EntityKind> {
    let d = s.doc().ok()?;
    let line = |h: Handle| match &d.entity(h)?.kind {
        EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
        _ => None,
    };
    let ((a1, a2), (b1, b2)) = (line(h1)?, line(h2)?);
    let (v, _, _) = cadcraft_geom::line_line_infinite(a1, a2, b1, b2)?;
    // Use the leg ends on the side of the picked arc location (the quadrant the user meant).
    let pick = |p: Vec2, q: Vec2, snap_p: AssocSnap, snap_q: AssocSnap| {
        let dir = if (p - v).len() > (q - v).len() { (p, snap_p) } else { (q, snap_q) };
        let other = if dir.1 == AssocSnap::Start { (q, AssocSnap::End) } else { (p, AssocSnap::Start) };
        let side = |x: Vec2| (x - v).normalized().dot((at - v).normalized());
        if side(other.0) > side(dir.0) && other.0.dist(v) > 1e-9 { other } else { dir }
    };
    let (p1, s1) = pick(a1, a2, AssocSnap::Start, AssocSnap::End);
    let (p2, s2) = pick(b1, b2, AssocSnap::Start, AssocSnap::End);
    // A two-line angular dimension (type 2): line 1 is p13 -> p14, line 2 is defpt -> p15, p16 is on the arc.
    // The picked leg end goes last in each pair, so the renderer measures between p14 and p15.
    let (o1, t1) = if s1 == AssocSnap::Start { (a2, AssocSnap::End) } else { (a1, AssocSnap::Start) };
    let (o2, t2) = if s2 == AssocSnap::Start { (b2, AssocSnap::End) } else { (b1, AssocSnap::Start) };
    let mut k = dim(s, DimKind::Angular, o2, o1, p1, p2, "");
    if let EntityKind::Dimension(d) = &mut k {
        d.p16 = v3(at);
    }
    let mut assoc = Vec::new();
    if assoc_enabled(s) {
        assoc = vec![
            DimAssoc { point: "p13".into(), handle: h1, snap: t1 },
            DimAssoc { point: "p14".into(), handle: h1, snap: s1 },
            DimAssoc { point: "defpt".into(), handle: h2, snap: t2 },
            DimAssoc { point: "p15".into(), handle: h2, snap: s2 },
        ];
    }
    Some(with_assoc(k, assoc))
}

pub(super) fn run_angular(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(ls) = p.get("lines").and_then(Value::as_array) {
        let hs: Vec<Handle> = ls.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))).collect();
        let (Some(h1), Some(h2)) = (hs.first(), hs.get(1)) else { return Err(bad("dimangular", "`lines` needs two line handles")) };
        let at = point_req("dimangular", p, "at")?;
        let k = angular_from_lines(s, *h1, *h2, at).ok_or_else(|| bad("dimangular", "objects must be two non-parallel lines"))?;
        return Ok(json!({ "handle": add_dim(s, k)?.hex() }));
    }
    if let Some(h) = hex_param(p, "arc") {
        let at = point_req("dimangular", p, "at")?;
        let a = match s.doc()?.entity(h).map(|e| e.kind.clone()) {
            Some(EntityKind::Arc(a)) => a,
            _ => return Err(bad("dimangular", "object is not an arc")),
        };
        let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
        let k = dim(s, DimKind::Angular3P, at, g.start_point(), g.end_point(), g.center, "");
        return Ok(json!({ "handle": add_dim(s, k)?.hex() }));
    }
    let v = point_req("dimangular", p, "vertex")?;
    let a = point_req("dimangular", p, "p1")?;
    let b = point_req("dimangular", p, "p2")?;
    let at = point_req("dimangular", p, "at")?;
    if v.near(a, 1e-12) || v.near(b, 1e-12) {
        return Err(bad("dimangular", "`p1` and `p2` must differ from `vertex`"));
    }
    let k = dim(s, DimKind::Angular3P, at, a, b, v, "");
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

pub(super) fn run_arclen(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("dimarc", "`handle` of an arc is required"))?;
    let at = point_req("dimarc", p, "at")?;
    let a = match s.doc()?.entity(h).map(|e| e.kind.clone()) {
        Some(EntityKind::Arc(a)) => a,
        _ => return Err(bad("dimarc", "object is not an arc")),
    };
    let ga = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
    let k = dim(s, DimKind::ArcLength, at, ga.start_point(), ga.end_point(), ga.center, "");
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

#[derive(Default)]
pub(super) struct AngularM {
    handles: Vec<Handle>,
    first: Option<(Vec2, Vec2)>,
    second: Option<(Vec2, Vec2)>,
    vertex_mode: Vec<Vec2>,
    vertex: bool,
    arc: Option<(Vec2, Vec2, Vec2)>,
    text: DimText,
    /// Quadrant: a point inside the angle to measure; the arc location then only sets the radius.
    quadrant: Option<Vec2>,
    asking_quadrant: bool,
}

impl AngularM {
    /// (vertex, p1, p2) of the angle being dimensioned.
    fn geometry(&self) -> Option<(Vec2, Vec2, Vec2)> {
        if let Some(a) = self.arc {
            return Some(a);
        }
        if let [v, a, b] = self.vertex_mode.as_slice() {
            return Some((*v, *a, *b));
        }
        let ((a1, a2), (b1, b2)) = (self.first?, self.second?);
        let (v, _, _) = cadcraft_geom::line_line_infinite(a1, a2, b1, b2)?;
        let far = |p: Vec2, q: Vec2| if p.dist(v) > q.dist(v) { p } else { q };
        Some((v, far(a1, a2), far(b1, b2)))
    }
    /// The arc location `at`, turned into the locked quadrant (same radius) when one is set.
    fn located(&self, v: Vec2, at: Vec2) -> Vec2 {
        match self.quadrant {
            Some(q) if q.dist(v) > 1e-12 && at.dist(v) > 1e-12 => v + (q - v).normalized() * at.dist(v),
            _ => at,
        }
    }
}

impl Interactive for AngularM {
    fn name(&self) -> &'static str {
        "DIMANGULAR"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(p) = self.text.prompt() {
            return p;
        }
        if self.asking_quadrant {
            return Prompt::new("Specify quadrant", Accept::POINT);
        }
        if self.geometry().is_some() {
            return Prompt::new("Specify dimension arc line location", Accept::POINT).kw(&["Mtext", "Text", "Angle", "Quadrant"]);
        }
        if self.vertex {
            return Prompt::new(
                ["Specify angle vertex", "Specify first angle endpoint", "Specify second angle endpoint"]
                    .get(self.vertex_mode.len())
                    .copied()
                    .unwrap_or(""),
                Accept::POINT,
            );
        }
        match self.first {
            None => Prompt::new("Select arc, circle, line", Accept::POINT).default("specify vertex"),
            Some(_) => Prompt::new("Select second line", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.text.answer(s, &i) {
            return Ok(Step::Continue);
        }
        if self.asking_quadrant {
            if let Input::Point(q) = i {
                self.quadrant = Some(q);
            }
            self.asking_quadrant = false;
            return Ok(Step::Continue);
        }
        if let Some((v, a, b)) = self.geometry() {
            return match i {
                Input::Point(at) => {
                    let at = self.located(v, at);
                    let k = match (self.handles.as_slice(), self.arc) {
                        ([h1, h2], None) => angular_from_lines(s, *h1, *h2, at),
                        _ => None,
                    };
                    let k = k.unwrap_or_else(|| dim(s, DimKind::Angular3P, at, a, b, v, ""));
                    add_dim(s, self.text.apply(k))?;
                    Ok(Step::Done)
                }
                Input::Keyword(k) => {
                    if k == "Quadrant" {
                        self.asking_quadrant = true;
                    } else {
                        self.text.keyword(&k);
                    }
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            };
        }
        if self.vertex {
            match i {
                Input::Point(p) => self.vertex_mode.push(p),
                Input::Enter => return Ok(Step::Cancel),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Enter if self.first.is_none() => {
                self.vertex = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let Some(h) = pick_at(s, p) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                let circle = s
                    .doc()?
                    .entity(h)
                    .and_then(|e| if let EntityKind::Circle(c) = &e.kind { Some(cadcraft_geom::Circle::new(c.center.xy(), c.radius)) } else { None });
                if let Some(g) = circle.filter(|_| self.first.is_none()) {
                    // The center is the vertex and the pick the first endpoint; ask for the second.
                    self.vertex = true;
                    self.vertex_mode = vec![g.center, g.closest(p)];
                    return Ok(Step::Continue);
                }
                match segment_at(s, h, p) {
                    Some(Segment::Arc { arc: g, .. }) if self.first.is_none() => {
                        self.arc = Some((g.center, g.start_point(), g.end_point()));
                    }
                    Some(Segment::Line(l)) => {
                        self.handles.push(h);
                        let seg = (l.a, l.b);
                        if self.first.is_none() {
                            self.first = Some(seg);
                        } else {
                            self.second = Some(seg);
                        }
                    }
                    _ => s.echo("Select an arc, circle or line."),
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.geometry().map(|(v, a, b)| vec![self.text.apply(dim(s, DimKind::Angular3P, self.located(v, c), a, b, v, ""))]).unwrap_or_default()
    }
}

#[derive(Default)]
pub(super) struct ArcLenM {
    arc: Option<cadcraft_geom::Arc>,
    text: DimText,
    /// Partial: the points picked so far on the arc (`None` when not choosing a part).
    partial: Option<Vec<Vec2>>,
}

/// The part of `arc` between the points nearest `p` and `q` (in the arc's direction).
fn arc_part(arc: &cadcraft_geom::Arc, p: Vec2, q: Vec2) -> Option<cadcraft_geom::Arc> {
    let sweep = arc.sweep();
    // Offset along the arc from its start, clamped to the nearer end when the point is off the arc.
    let along = |x: Vec2| {
        let t = cadcraft_geom::ccw_sweep(arc.start, arc.center.angle_to(x));
        if t <= sweep {
            t
        } else if t - sweep < std::f64::consts::TAU - t {
            sweep
        } else {
            0.0
        }
    };
    let (t1, t2) = (along(p), along(q));
    let (lo, hi) = (t1.min(t2), t1.max(t2));
    (hi - lo > 1e-9).then(|| cadcraft_geom::Arc::new(arc.center, arc.radius, arc.start + lo, arc.start + hi))
}

impl Interactive for ArcLenM {
    fn name(&self) -> &'static str {
        "DIMARC"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(p) = self.text.prompt() {
            return p;
        }
        if let Some(picked) = &self.partial {
            return match picked.first() {
                None => Prompt::new("Specify first point for arc length dimension", Accept::POINT),
                Some(&f) => Prompt::new("Specify second point for arc length dimension", Accept::POINT).base(f),
            };
        }
        match self.arc {
            None => Prompt::new("Select arc or polyline arc segment", Accept::POINT),
            Some(_) => Prompt::new("Specify arc length dimension location", Accept::POINT).kw(&["Mtext", "Text", "Angle", "Partial"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.text.answer(s, &i) {
            return Ok(Step::Continue);
        }
        if let Some(picked) = self.partial.take() {
            match (i, picked.first().copied(), self.arc) {
                (Input::Point(p), None, _) => self.partial = Some(vec![p]),
                (Input::Point(q), Some(p), Some(g)) => match arc_part(&g, p, q) {
                    Some(part) => self.arc = Some(part),
                    None => s.echo("The two points must be different."),
                },
                (Input::Enter, ..) => {}
                _ => self.partial = Some(picked),
            }
            return Ok(Step::Continue);
        }
        match (self.arc, i) {
            (None, Input::Point(p)) => {
                if let Some(EntityKind::Arc(a)) = pick_at(s, p).and_then(|h| s.doc().ok()?.entity(h).map(|e| e.kind.clone())) {
                    self.arc = Some(cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end));
                } else {
                    s.echo("Select an arc.");
                }
                Ok(Step::Continue)
            }
            (Some(g), Input::Point(at)) => {
                add_dim(s, self.text.apply(dim(s, DimKind::ArcLength, at, g.start_point(), g.end_point(), g.center, "")))?;
                Ok(Step::Done)
            }
            (Some(_), Input::Keyword(k)) => {
                if k == "Partial" {
                    self.partial = Some(Vec::new());
                } else {
                    self.text.keyword(&k);
                }
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.partial.is_some() {
            return Vec::new();
        }
        self.arc.map(|g| vec![self.text.apply(dim(s, DimKind::ArcLength, c, g.start_point(), g.end_point(), g.center, ""))]).unwrap_or_default()
    }
}
