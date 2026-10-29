use super::{
    Accept, EntityKind, Input, Interactive, Outcome, PlineAsk, PolyVertex, Prompt, Result, Session, Step, Value, Vec2, added, arc_to_bulge, bad,
    bool_or, bulge_to_arc, distinct_points, f64_or, json, line, lwpoly, number, point_value, points_param, size_param, snap,
};

// ---------------- JSON forms ----------------

pub(super) fn run_line(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("line", "`points` [[x,y],...] (2 or more) is required"))?;
    if pts.len() < 2 {
        return Err(bad("line", "need at least 2 points"));
    }
    // A repeated point would make a zero-length line.
    let pts = distinct_points(&pts);
    if pts.len() < 2 {
        return Err(bad("line", "need at least 2 different points"));
    }
    let mut hs = Vec::new();
    for w in pts.windows(2) {
        if let [a, b] = w {
            hs.push(s.add_entity(line(*a, *b))?.hex());
        }
    }
    if bool_or(p, "closed", false)
        && pts.len() > 2
        && let (Some(a), Some(b)) = (pts.last(), pts.first())
        && !a.near(*b, 1e-12)
    {
        hs.push(s.add_entity(line(*a, *b))?.hex());
    }
    Ok(json!({ "handles": hs }))
}

fn vertex_value(v: &Value) -> Option<PolyVertex> {
    if let Some(p) = point_value(v) {
        return Some(PolyVertex::new(p));
    }
    let p = point_value(v.get("p")?)?;
    Some(PolyVertex { p, bulge: f64_or(v, "bulge", 0.0), start_width: f64_or(v, "startWidth", 0.0), end_width: f64_or(v, "endWidth", 0.0) })
}

pub(super) fn run_pline(s: &mut Session, p: &Value) -> Result<Value> {
    let raw = p.get("vertices").or_else(|| p.get("points")).and_then(Value::as_array);
    for v in raw.into_iter().flatten() {
        size_param("pline", v, "startWidth", true)?;
        size_param("pline", v, "endWidth", true)?;
    }
    let vs: Vec<PolyVertex> = raw.map(|a| a.iter().filter_map(vertex_value).collect()).unwrap_or_default();
    if vs.len() < 2 {
        return Err(bad("pline", "`vertices` needs at least 2 points"));
    }
    let width = size_param("pline", p, "width", true)?.unwrap_or(0.0);
    let mut k = lwpoly(vs, bool_or(p, "closed", false));
    if let EntityKind::LwPolyline(pl) = &mut k {
        pl.const_width = width;
    }
    added(s.add_entity(k)?)
}

// ---------------- interactive ----------------

#[derive(Default)]
pub(super) struct LineM {
    pts: Vec<Vec2>,
    handles: Vec<cadcraft_doc::Handle>,
    /// A first point picked as a deferred tangent/perpendicular, resolved by the next point.
    first: Option<snap::Deferred>,
}

impl LineM {
    /// Draw the first segment from the deferred first point to `end`.
    fn resolve_first(&mut self, s: &mut Session, first: snap::Deferred, end: snap::LineEnd) -> Result<Step> {
        let Some((a, b)) = snap::resolve_deferred(snap::LineEnd::Deferred(first), end) else {
            let what = if first.mode == snap::mode::TAN { "tangent" } else { "perpendicular" };
            return Err(crate::EngineError::Other(format!("No {what} line through that point; pick another point.")));
        };
        self.handles.push(s.add_entity(line(a, b))?);
        self.pts = vec![a, b];
        self.first = None;
        s.last_point = b;
        Ok(Step::Continue)
    }
}

impl Interactive for LineM {
    fn name(&self) -> &'static str {
        "LINE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.pts.len() {
            0 if self.first.is_some() => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).deferred(),
            0 => Prompt::new("Specify first point", Accept::POINT).deferred(),
            1 | 2 => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify next point", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.pts.is_empty()
            && let Some(first) = self.first
        {
            match i {
                Input::Point(p) => {
                    // A deferred TAN first point with a typed `@d<a` next point: resolve
                    // exactly from the direction and distance the user gave.
                    if let Some((dist, ang)) = s.last_polar.take()
                        && let Some(t) = snap::tangent_point_in_direction(&first.curve, ang, first.at)
                    {
                        let e = t + Vec2::new(ang.cos(), ang.sin()) * dist;
                        self.handles.push(s.add_entity(line(t, e))?);
                        self.pts = vec![t, e];
                        s.last_point = e;
                        self.first = None;
                        return Ok(Step::Continue);
                    }
                    return self.resolve_first(s, first, snap::LineEnd::Point(p));
                }
                Input::Deferred(d) => return self.resolve_first(s, first, snap::LineEnd::Deferred(d)),
                _ => {}
            }
        }
        match i {
            Input::Deferred(d) if self.pts.is_empty() => {
                self.first = Some(d);
                Ok(Step::Continue)
            }
            // A deferred pick with a known last point (not offered by the prompt, but harmless).
            Input::Deferred(d) => match self.pts.last().copied() {
                Some(last) => match snap::resolve_deferred(snap::LineEnd::Point(last), snap::LineEnd::Deferred(d)) {
                    Some((_, p)) => self.input(s, Input::Point(p)),
                    None => Err(crate::EngineError::Other("No tangent or perpendicular from the last point.".into())),
                },
                None => Ok(Step::Continue),
            },
            Input::Point(p) => {
                if let Some(last) = self.pts.last().copied() {
                    if last.near(p, 1e-12) {
                        return Ok(Step::Continue);
                    }
                    self.handles.push(s.add_entity(line(last, p))?);
                }
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                if self.first.take().is_some() {
                    return Ok(Step::Continue);
                }
                if let Some(h) = self.handles.pop() {
                    s.doc_mut()?.remove_entity(h);
                }
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Close" => {
                if let (Some(a), Some(b)) = (self.pts.last().copied(), self.pts.first().copied()) {
                    s.add_entity(line(a, b))?;
                }
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err(crate::EngineError::Other("Point or option keyword required.".into())),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(first) = self.first {
            // Over a second curve, show the tangent-to-tangent line the click would draw.
            let end = s.cursor_deferred.map_or(snap::LineEnd::Point(c), snap::LineEnd::Deferred);
            return snap::resolve_deferred(snap::LineEnd::Deferred(first), end).map(|(a, b)| vec![line(a, b)]).unwrap_or_default();
        }
        self.pts.last().map(|l| vec![line(*l, c)]).unwrap_or_default()
    }
}

#[derive(Default)]
pub(super) struct PlineM {
    verts: Vec<PolyVertex>,
    arc_mode: bool,
    /// Total width at the start and end of the next segment (Width / Halfwidth answers).
    start_w: f64,
    end_w: f64,
    handle: Option<cadcraft_doc::Handle>,
    /// 1 = asking the starting width, 2 = the ending width.
    asking_width: u8,
    /// The widths are being asked as half-widths (centre line to edge).
    half: bool,
    /// An option asking further questions (arc mode's Angle, CEnter…, line mode's Length).
    ask: Option<PlineAsk>,
}

impl PlineM {
    fn entity(&self, closed: bool) -> EntityKind {
        let mut k = lwpoly(self.verts.clone(), closed);
        if let EntityKind::LwPolyline(pl) = &mut k {
            // Every segment the same uniform width: store it as the constant width, as before.
            let segs = if closed { pl.vertices.len() } else { pl.vertices.len().saturating_sub(1) };
            let w = pl.vertices.first().map_or(0.0, |v| v.start_width);
            if pl.vertices.iter().take(segs).all(|v| v.start_width == w && v.end_width == w) {
                pl.const_width = w;
                for v in &mut pl.vertices {
                    (v.start_width, v.end_width) = (0.0, 0.0);
                }
            }
        }
        k
    }
    /// The next segment, starting at the last vertex, takes the requested widths (#104); the
    /// segments after it are uniform at the ending width, as in AutoCAD.
    fn begin_segment(&mut self) {
        if let Some(last) = self.verts.last_mut() {
            (last.start_width, last.end_width) = (self.start_w, self.end_w);
            self.start_w = self.end_w;
        }
    }
    fn sync(&mut self, s: &mut Session, closed: bool) -> Result<()> {
        if self.verts.len() < 2 {
            if let Some(h) = self.handle.take() {
                s.doc_mut()?.remove_entity(h);
            }
            return Ok(());
        }
        let k = self.entity(closed);
        match self.handle {
            Some(h) => {
                s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
            }
            None => self.handle = Some(s.add_entity(k)?),
        }
        Ok(())
    }
    /// The direction the polyline runs in at its last vertex (+X before the second vertex).
    fn tangent(&self) -> Vec2 {
        let n = self.verts.len();
        let Some(last) = self.verts.last() else { return Vec2::X };
        if n >= 2 {
            let prev = self.verts.get(n - 2).copied().unwrap_or_default();
            match bulge_to_arc(prev.p, last.p, prev.bulge) {
                Some((a, ccw)) => {
                    let r = (last.p - a.center).perp().normalized();
                    if ccw { r } else { -r }
                }
                None => (last.p - prev.p).normalized(),
            }
        } else {
            Vec2::X
        }
    }
    /// Bulge for a tangent-continuing arc from the last vertex to `p`.
    fn tangent_bulge(&self, p: Vec2) -> f64 {
        let Some(last) = self.verts.last() else { return 0.0 };
        let dir = self.tangent();
        let chord = p - last.p;
        // Included angle = 2 × angle between tangent and chord.
        let ang = dir.cross(chord).atan2(dir.dot(chord));
        arc_to_bulge(2.0 * ang)
    }
}

impl Interactive for PlineM {
    fn name(&self) -> &'static str {
        "PLINE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.asking_width > 0 {
            let unit = if self.half { "half-width" } else { "width" };
            let (which, w) = if self.asking_width == 1 { ("starting", self.start_w) } else { ("ending", self.end_w) };
            let shown = if self.half { w / 2.0 } else { w };
            return Prompt::new(format!("Specify {which} {unit}"), Accept::NUMBER).default(format!("{shown:.4}"));
        }
        let base = self.verts.last().map(|v| v.p);
        if let (Some(a), Some(st)) = (self.ask, base) {
            return a.prompt(st, self.tangent(), s);
        }
        match (self.verts.len(), self.arc_mode) {
            (0, _) => Prompt::new("Specify start point", Accept::POINT),
            (n, false) => {
                let mut kw = vec!["Arc", "Halfwidth", "Length", "Undo", "Width"];
                if n >= 2 {
                    kw.insert(1, "Close");
                }
                Prompt::new("Specify next point", Accept::POINT).kw(&kw).base_opt(base)
            }
            (_, true) => Prompt::new("Specify endpoint of arc", Accept::POINT)
                .kw(&["Angle", "CEnter", "CLose", "Direction", "Halfwidth", "Line", "Radius", "Second pt", "Undo", "Width"])
                .base_opt(base),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_width > 0 {
            match i {
                Input::Text(t) => {
                    let w = number(&t).filter(|w| w.is_finite()).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
                    // Halfwidth is centre line to edge: the total width is twice the answer (#83).
                    let w = w.max(0.0) * if self.half { 2.0 } else { 1.0 };
                    if self.asking_width == 1 {
                        self.start_w = w;
                    } else {
                        self.end_w = w;
                    }
                }
                Input::Enter => {}
                _ => return Ok(Step::Continue),
            }
            if self.asking_width == 1 {
                // The ending width defaults to the starting width.
                self.end_w = self.start_w;
            }
            self.asking_width = if self.asking_width == 1 { 2 } else { 0 };
            return Ok(Step::Continue);
        }
        if let (Some(a), Some(st)) = (self.ask, self.verts.last().map(|v| v.p)) {
            match a.input(s, i, st, self.tangent())? {
                Outcome::Ask(next) => self.ask = Some(next),
                Outcome::Back => self.ask = None,
                Outcome::Segment(end, bulge) => {
                    self.ask = None;
                    self.begin_segment();
                    if let Some(last) = self.verts.last_mut() {
                        last.bulge = bulge;
                    }
                    self.verts.push(PolyVertex::new(end));
                    self.sync(s, false)?;
                }
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => {
                if self.verts.is_empty() {
                    s.echo(format!("Current line-width is {:.4}", self.start_w));
                }
                self.begin_segment();
                if self.arc_mode && !self.verts.is_empty() {
                    let b = self.tangent_bulge(p);
                    if let Some(last) = self.verts.last_mut() {
                        last.bulge = b;
                    }
                }
                self.verts.push(PolyVertex::new(p));
                self.sync(s, false)?;
                Ok(Step::Continue)
            }
            Input::Keyword(k) => match k.as_str() {
                "Arc" => {
                    self.arc_mode = true;
                    Ok(Step::Continue)
                }
                "Line" => {
                    self.arc_mode = false;
                    Ok(Step::Continue)
                }
                "Close" | "CLose" => {
                    if self.arc_mode
                        && let Some(first) = self.verts.first().map(|v| v.p)
                    {
                        let b = self.tangent_bulge(first);
                        if let Some(last) = self.verts.last_mut() {
                            last.bulge = b;
                        }
                    }
                    // The closing segment starts at the last vertex.
                    self.begin_segment();
                    self.sync(s, true)?;
                    Ok(Step::Done)
                }
                "Undo" => {
                    self.verts.pop();
                    if let Some(l) = self.verts.last_mut() {
                        l.bulge = 0.0;
                        (l.start_width, l.end_width) = (0.0, 0.0);
                    }
                    self.sync(s, false)?;
                    Ok(Step::Continue)
                }
                "Width" | "Halfwidth" => {
                    self.half = k == "Halfwidth";
                    self.asking_width = 1;
                    Ok(Step::Continue)
                }
                k => {
                    match PlineAsk::from_keyword(k, self.arc_mode) {
                        Some(a) if !self.verts.is_empty() => self.ask = Some(a),
                        _ => return Err(crate::EngineError::Other("Point or option keyword required.".into())),
                    }
                    Ok(Step::Continue)
                }
            },
            Input::Enter => Ok(Step::Done),
            _ => Err(crate::EngineError::Other("Point or option keyword required.".into())),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let Some(last) = self.verts.last() else { return Vec::new() };
        if let Some(a) = self.ask {
            return a
                .preview(last.p, self.tangent(), c)
                .map(|(e, b)| vec![lwpoly(vec![PolyVertex::with_bulge(last.p, b), PolyVertex::new(e)], false)])
                .unwrap_or_default();
        }
        if self.arc_mode {
            let b = self.tangent_bulge(c);
            vec![lwpoly(vec![PolyVertex::with_bulge(last.p, b), PolyVertex::new(c)], false)]
        } else {
            vec![line(last.p, c)]
        }
    }
}
