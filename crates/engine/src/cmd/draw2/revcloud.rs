use super::{
    Accept, Chain, Circle, Ellipse, EntityKind, Handle, Input, Interactive, Line, MAX_GEN, PolyVertex, Polyline, Prompt, Result, Session, Step,
    Value, Vec2, added, bad, bool_or, curves, lwpoly, number, other, point_param, points_param, rect_vertices, shoelace, str_param,
};

// =====================================================================================
// REVCLOUD
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloudKind {
    Rect,
    Poly,
    Free,
}

fn default_arc_len(s: &Session) -> f64 {
    let d = s.doc().ok();
    let metric = d.is_some_and(|d| d.header.i64("MEASUREMENT", 0) == 1);
    d.map(|d| d.header.f64("REVCLOUDARCLEN", if metric { 15.0 } else { 0.5 })).unwrap_or(0.5).max(1e-6)
}

/// Bulged closed polyline vertices for a revision cloud around `boundary`.
pub(crate) fn cloud_vertices(boundary: &[Vec2], arc_len: f64, per_edge: bool, reverse: bool) -> Option<Vec<PolyVertex>> {
    let mut pts: Vec<Vec2> = Vec::new();
    for p in boundary {
        if p.is_finite() && pts.last().is_none_or(|l| !l.near(*p, 1e-9)) {
            pts.push(*p);
        }
    }
    while pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-9)) {
        pts.pop();
    }
    if pts.len() < 3 || shoelace(&pts).abs() < 1e-12 {
        return None;
    }
    if shoelace(&pts) < 0.0 {
        pts.reverse();
    }
    let n = pts.len();
    let perim: f64 = (0..n).filter_map(|i| Some(pts.get(i)?.dist(*pts.get((i + 1) % n)?))).sum();
    if !(arc_len.is_finite() && arc_len > 0.0) {
        return None;
    }
    let arc_len = arc_len.max(perim / MAX_GEN as f64);
    let mut out: Vec<Vec2> = Vec::new();
    if per_edge {
        for i in 0..n {
            let (Some(a), Some(b)) = (pts.get(i).copied(), pts.get((i + 1) % n).copied()) else { continue };
            let k = ((a.dist(b) / arc_len).round() as usize).clamp(1, MAX_GEN);
            for j in 0..k {
                out.push(a.lerp(b, j as f64 / k as f64));
            }
        }
    } else {
        let chain = Chain {
            segs: (0..n).filter_map(|i| Some(cadcraft_geom::Segment::Line(Line::new(*pts.get(i)?, *pts.get((i + 1) % n)?)))).collect(),
            closed: true,
        };
        let k = ((perim / arc_len).round() as usize).clamp(3, MAX_GEN);
        for j in 0..k {
            out.extend(chain.at_length(perim * j as f64 / k as f64).map(|x| x.0));
        }
    }
    let b = if reverse { -0.5 } else { 0.5 };
    Some(out.into_iter().map(|p| PolyVertex::with_bulge(p, b)).collect())
}

fn object_outline(s: &Session, h: Handle) -> Result<Vec<Vec2>> {
    let e = curves::entity(s, h)?;
    let tol = 1e-3;
    let pts = match &e.kind {
        EntityKind::LwPolyline(p) if p.closed => Polyline { vertices: p.vertices.clone(), closed: true }.tessellate(tol),
        EntityKind::Circle(c) => {
            let mut v = Vec::new();
            Circle::new(c.center.xy(), c.radius).tessellate(c.radius * 1e-3, &mut v);
            v
        }
        EntityKind::Ellipse(el) => {
            let mut v = Vec::new();
            Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end }
                .tessellate(el.major.xy().len() * 1e-3, &mut v);
            v
        }
        EntityKind::Spline(sp) if sp.closed => sp.tessellate(tol),
        _ => return Err(other("Select a closed polyline, circle, ellipse or closed spline.")),
    };
    Ok(pts)
}

fn cloud_entity(vs: Vec<PolyVertex>, calligraphy: bool, arc_len: f64) -> EntityKind {
    let vs = if calligraphy { vs.into_iter().map(|v| PolyVertex { start_width: 0.0, end_width: arc_len * 0.15, ..v }).collect() } else { vs };
    lwpoly(vs, true)
}

pub(super) fn run_revcloud(s: &mut Session, p: &Value) -> Result<Value> {
    let arc_len = p.get("arcLength").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or_else(|| default_arc_len(s));
    let reverse = bool_or(p, "reverse", false);
    let calligraphy = str_param(p, "style").is_some_and(|x| x.eq_ignore_ascii_case("calligraphy"));
    let (pts, per_edge, src) = if let (Some(a), Some(b)) = (point_param(p, "p1"), point_param(p, "p2")) {
        (rect_vertices(a, b).into_iter().map(|v| v.p).collect(), true, None)
    } else if let Some(pts) = points_param(p, "points") {
        (pts, true, None)
    } else if let Some(pts) = points_param(p, "freehand") {
        (pts, false, None)
    } else if let Some(h) = curves::handle_param(p, "handle") {
        (object_outline(s, h)?, false, Some(h))
    } else {
        return Err(bad("revcloud", "give {p1, p2}, {points}, {freehand} or {handle}"));
    };
    if pts.len() > MAX_GEN {
        return Err(bad("revcloud", "too many points"));
    }
    let vs = cloud_vertices(&pts, arc_len, per_edge, reverse).ok_or_else(|| bad("revcloud", "the boundary needs an area"))?;
    let h = s.add_entity(cloud_entity(vs, calligraphy, arc_len))?;
    if let Some(src) = src {
        let common = curves::entity(s, src)?.common;
        let d = s.doc_mut()?;
        d.modify_entity(h, |e| e.common = common)?;
        d.remove_entity(src);
    }
    added(h)
}

pub(super) struct RevcloudM {
    kind: CloudKind,
    pts: Vec<Vec2>,
    arc_len: f64,
    asking_arc: bool,
    object: bool,
    calligraphy: bool,
    asking_style: bool,
}

impl RevcloudM {
    pub(super) fn new(s: &Session, kind: CloudKind) -> Self {
        RevcloudM { kind, pts: Vec::new(), arc_len: default_arc_len(s), asking_arc: false, object: false, calligraphy: false, asking_style: false }
    }
    fn boundary(&self, extra: Option<Vec2>) -> Vec<Vec2> {
        let mut pts = self.pts.clone();
        pts.extend(extra);
        if self.kind == CloudKind::Rect
            && let (Some(a), Some(b)) = (pts.first().copied(), pts.get(1).copied())
        {
            return rect_vertices(a, b).into_iter().map(|v| v.p).collect();
        }
        pts
    }
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let b = self.boundary(None);
        match cloud_vertices(&b, self.arc_len, self.kind != CloudKind::Free, false) {
            Some(vs) => {
                s.add_entity(cloud_entity(vs, self.calligraphy, self.arc_len))?;
                s.echo("Revision cloud finished.");
                Ok(Step::Done)
            }
            None => Err(other("The revision cloud needs at least three points enclosing an area.")),
        }
    }
}

impl Interactive for RevcloudM {
    fn name(&self) -> &'static str {
        "REVCLOUD"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let t = match self.kind {
            CloudKind::Rect => "Rectangular",
            CloudKind::Poly => "Polygonal",
            CloudKind::Free => "Freehand",
        };
        s.echo(format!("Minimum arc length: {0:.4}   Maximum arc length: {0:.4}   Style: Normal   Type: {t}", self.arc_len));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_arc {
            return Prompt::new("Specify minimum length of arc", Accept::NUMBER).default(format!("{:.4}", self.arc_len));
        }
        if self.asking_style {
            return Prompt::new("Select arc style", curves::KW).kw(&["Normal", "Calligraphy"]).default("Normal");
        }
        if self.object {
            return Prompt::new("Select object", Accept::POINT);
        }
        let first_kw: &[&str] = &["Arc length", "Object", "Rectangular", "Polygonal", "Freehand", "Style"];
        match (self.kind, self.pts.len()) {
            (CloudKind::Rect, 0) => Prompt::new("Specify first corner point", Accept::POINT).kw(first_kw).default("Object"),
            (CloudKind::Rect, _) => {
                Prompt { corner: true, ..Prompt::new("Specify opposite corner", Accept::POINT).base_opt(self.pts.first().copied()) }
            }
            (CloudKind::Poly, 0) => Prompt::new("Specify start point", Accept::POINT).kw(first_kw).default("Object"),
            (CloudKind::Poly, 1) => Prompt::new("Specify next point", Accept::POINT).base_opt(self.pts.last().copied()),
            (CloudKind::Poly, _) => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            (CloudKind::Free, 0) => Prompt::new("Specify first point", Accept::POINT).kw(first_kw).default("Object"),
            (CloudKind::Free, _) => Prompt::new("Guide crosshairs along cloud path", Accept::POINT).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_arc {
            if let Input::Text(t) = &i {
                let v = number(t).filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive value."))?;
                self.arc_len = v;
                s.doc_mut()?.header.set_f64("REVCLOUDARCLEN", v);
            }
            self.asking_arc = false;
            return Ok(Step::Continue);
        }
        if self.asking_style {
            if let Input::Keyword(k) | Input::Text(k) = &i {
                self.calligraphy = k.to_ascii_lowercase().starts_with('c');
            }
            self.asking_style = false;
            return Ok(Step::Continue);
        }
        if self.object {
            let Input::Point(p) = i else { return if i == Input::Enter { Ok(Step::Cancel) } else { Ok(Step::Continue) } };
            let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
            let pts = object_outline(s, h)?;
            let vs = cloud_vertices(&pts, self.arc_len, false, false).ok_or_else(|| other("The object has no area."))?;
            let nh = s.add_entity(cloud_entity(vs, self.calligraphy, self.arc_len))?;
            let common = curves::entity(s, h)?.common;
            let d = s.doc_mut()?;
            d.modify_entity(nh, |e| e.common = common)?;
            d.remove_entity(h);
            s.echo("Revision cloud finished.");
            return Ok(Step::Done);
        }
        match i {
            Input::Keyword(k) => {
                match k.as_str() {
                    "Arc length" => self.asking_arc = true,
                    "Object" => self.object = true,
                    "Rectangular" => self.kind = CloudKind::Rect,
                    "Polygonal" => self.kind = CloudKind::Poly,
                    "Freehand" => self.kind = CloudKind::Free,
                    "Style" => self.asking_style = true,
                    "Undo" => {
                        self.pts.pop();
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                self.pts.push(p);
                if self.kind == CloudKind::Rect && self.pts.len() == 2 {
                    return self.finish(s);
                }
                if self.pts.len() > MAX_GEN {
                    return self.finish(s);
                }
                Ok(Step::Continue)
            }
            Input::Enter if self.pts.is_empty() => {
                self.object = true;
                Ok(Step::Continue)
            }
            Input::Enter => self.finish(s),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() || self.object {
            return Vec::new();
        }
        let b = self.boundary(Some(c));
        match cloud_vertices(&b, self.arc_len, self.kind != CloudKind::Free, false) {
            Some(vs) if vs.len() < 2000 => vec![lwpoly(vs, true)],
            _ => {
                let mut pts = self.pts.clone();
                pts.push(c);
                vec![lwpoly(pts.into_iter().map(PolyVertex::new).collect(), false)]
            }
        }
    }
}
