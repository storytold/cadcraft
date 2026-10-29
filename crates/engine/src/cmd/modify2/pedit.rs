use super::{
    Accept, Chain, EntityKind, Handle, Input, Interactive, LwPolyline, MAX_GEN, PolyVertex, Prompt, Result, SelOutcome, SelectPhase, Session, Spline,
    Step, Value, Vec2, as_lwpoly, bad, bool_or, curves, f64_or, f64_req, json, line, number, other, pedit_vertex, point_req, set_kind, str_param,
    targets,
};

/// Fit curve: a tangent-continuous pair of arcs (biarc) per segment through every vertex,
/// following the tangent directions given per vertex (Edit vertex Tangent).
pub(crate) fn fit_vertices(vs: &[PolyVertex], closed: bool, tans: &[Option<Vec2>]) -> Vec<PolyVertex> {
    let pts: Vec<Vec2> = vs.iter().map(|v| v.p).collect();
    let n = pts.len();
    if n < 3 {
        return vs.iter().map(|v| PolyVertex { bulge: 0.0, ..*v }).collect();
    }
    let at = |i: usize| pts.get(i).copied().unwrap_or_default();
    let tangent = |i: usize| -> Vec2 {
        let (prev, next) = if closed {
            (at((i + n - 1) % n), at((i + 1) % n))
        } else if i == 0 {
            (at(0), at(1))
        } else if i + 1 == n {
            (at(n - 2), at(n - 1))
        } else {
            (at(i - 1), at(i + 1))
        };
        let t = (next - prev).normalized();
        if t == Vec2::ZERO { Vec2::X } else { t }
    };
    // Open ends: mirror the neighbouring tangent across the chord for a natural end.
    let tan_at = |i: usize| -> Vec2 {
        if let Some(Some(t)) = tans.get(i) {
            return *t;
        }
        if closed || (i != 0 && i + 1 != n) {
            return tangent(i);
        }
        let (a, b, tb) = if i == 0 { (at(0), at(1), tangent(1)) } else { (at(n - 1), at(n - 2), -tangent(n - 2)) };
        let c = (b - a).normalized();
        let m = c * (2.0 * tb.dot(c)) - tb;
        let m = if i == 0 { m } else { -m };
        if m.len() < 1e-12 { c } else { m.normalized() }
    };
    let count = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(count * 2 + 1);
    for i in 0..count {
        let p0 = at(i);
        let p1 = at((i + 1) % n);
        let t0 = tan_at(i);
        let t1 = tan_at((i + 1) % n);
        let v = p1 - p0;
        let tsum = t0 + t1;
        let denom = 2.0 * (1.0 - t0.dot(t1));
        let d = if denom.abs() < 1e-12 {
            let vt = v.dot(t1);
            if vt.abs() < 1e-12 { v.len() / 2.0 } else { v.len2() / (4.0 * vt) }
        } else {
            let vt = v.dot(tsum);
            (-vt + (vt * vt + denom * v.len2()).max(0.0).sqrt()) / denom
        };
        let pm = (p0 + t0 * d + p1 - t1 * d) / 2.0;
        let b1 = curves::bulge_from_tangent(p0, pm, t0);
        let b2 = -curves::bulge_from_tangent(p1, pm, -t1);
        let w = vs.get(i).copied().unwrap_or_default();
        out.push(PolyVertex { p: p0, bulge: if b1.is_finite() { b1 } else { 0.0 }, start_width: w.start_width, end_width: w.end_width });
        out.push(PolyVertex { p: pm, bulge: if b2.is_finite() { b2 } else { 0.0 }, start_width: w.end_width, end_width: w.end_width });
    }
    if !closed {
        out.push(PolyVertex::new(at(n - 1)));
    }
    out
}

/// Spline curve: a cubic B-spline with the vertices as its frame (control points).
pub(crate) fn spline_vertices(vs: &[PolyVertex], closed: bool, segs: usize) -> Vec<PolyVertex> {
    let pts: Vec<Vec2> = vs.iter().map(|v| v.p).collect();
    let n = pts.len();
    if n < 3 {
        return vs.to_vec();
    }
    let segs = segs.clamp(1, 64).min((MAX_GEN / n).max(1));
    let mut out: Vec<Vec2> = Vec::new();
    if closed {
        // Uniform periodic cubic B-spline.
        let at = |i: usize| pts.get(i % n).copied().unwrap_or_default();
        for i in 0..n {
            let (p0, p1, p2, p3) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
            for k in 0..segs {
                let t = k as f64 / segs as f64;
                let (t2, t3) = (t * t, t * t * t);
                let b0 = (1.0 - t).powi(3) / 6.0;
                let b1 = (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0;
                let b2 = (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0;
                let b3 = t3 / 6.0;
                out.push(p0 * b0 + p1 * b1 + p2 * b2 + p3 * b3);
            }
        }
    } else {
        let sp = Spline::from_control(pts.clone(), 3);
        let (lo, hi) = sp.domain();
        let m = (segs * (n - 1)).min(MAX_GEN);
        for k in 0..=m {
            out.push(sp.eval(lo + (hi - lo) * k as f64 / m as f64));
        }
    }
    out.into_iter().map(PolyVertex::new).collect()
}

/// Join connected lines/arcs/open polylines onto the ends of polyline `h` (kept in place).
pub(crate) fn pedit_join(s: &mut Session, h: Handle, others: &[Handle], fuzz: f64) -> Result<usize> {
    let e = curves::entity(s, h)?;
    let mut base = as_lwpoly(&e.kind).ok_or_else(|| other("Select a polyline."))?;
    if base.closed {
        return Err(other("Cannot join to a closed polyline."));
    }
    let mut chain = Chain::of(&EntityKind::LwPolyline(base.clone())).unwrap_or_default();
    let tol = fuzz.max(1e-6);
    let mut pool: Vec<(Handle, Chain)> = Vec::new();
    for o in others {
        if *o == h {
            continue;
        }
        let Ok(oe) = curves::entity(s, *o) else { continue };
        let ok = matches!(&oe.kind, EntityKind::Line(_) | EntityKind::Arc(_)) || matches!(&oe.kind, EntityKind::LwPolyline(p) if !p.closed);
        if ok && let Some(c) = Chain::of(&oe.kind) {
            pool.push((*o, c));
        }
    }
    let mut joined = Vec::new();
    while let (Some(st), Some(en)) = (chain.start(), chain.end()) {
        let Some(i) = pool
            .iter()
            .position(|(_, c)| c.start().zip(c.end()).is_some_and(|(a, b)| a.near(en, tol) || b.near(en, tol) || a.near(st, tol) || b.near(st, tol)))
        else {
            break;
        };
        let (oh, c) = pool.remove(i);
        let (Some(a), Some(b)) = (c.start(), c.end()) else { continue };
        if a.near(en, tol) {
            chain.segs.extend(c.segs);
        } else if b.near(en, tol) {
            chain.segs.extend(c.reversed().segs);
        } else if b.near(st, tol) {
            let mut v = c.segs;
            v.extend(chain.segs);
            chain.segs = v;
        } else {
            let mut v = c.reversed().segs;
            v.extend(chain.segs);
            chain.segs = v;
        }
        joined.push(oh);
    }
    if joined.is_empty() {
        return Ok(0);
    }
    chain.closed = chain.start().zip(chain.end()).is_some_and(|(a, b)| a.near(b, tol)) && chain.segs.len() > 1;
    base.vertices = chain.vertices();
    base.closed = chain.closed;
    let d = s.doc_mut()?;
    d.modify_entity(h, |e| e.kind = EntityKind::LwPolyline(base))?;
    for o in &joined {
        d.remove_entity(*o);
    }
    Ok(joined.len())
}

fn pedit_apply(s: &mut Session, h: Handle, opt: &str, p: &Value) -> Result<Value> {
    let e = curves::entity(s, h)?;
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let mut pl = match &e.kind {
        EntityKind::LwPolyline(pl) => pl.clone(),
        EntityKind::Line(_) | EntityKind::Arc(_) if bool_or(p, "convert", true) => as_lwpoly(&e.kind).ok_or_else(|| other("Cannot convert."))?,
        _ => return Err(other("Object selected is not a polyline.")),
    };
    match opt {
        "close" | "c" => pl.closed = pl.vertices.len() > 1,
        "open" | "o" => pl.closed = false,
        "width" | "w" => {
            let w = f64_req("pedit", p, "width")?;
            if w < 0.0 {
                return Err(bad("pedit", "width must be non-negative"));
            }
            pl.const_width = w;
            for v in &mut pl.vertices {
                v.start_width = 0.0;
                v.end_width = 0.0;
            }
        }
        "fit" | "f" => pl.vertices = fit_vertices(&pl.vertices, pl.closed, &pedit_vertex::tangent_param(p, pl.vertices.len())?),
        "spline" | "s" => {
            let segs = s.doc()?.header.i64("SPLINESEGS", 8).clamp(1, 64) as usize;
            pl.vertices = spline_vertices(&pl.vertices, pl.closed, segs);
        }
        "decurve" | "d" => pl.vertices.iter_mut().for_each(|v| v.bulge = 0.0),
        "reverse" | "r" => pl.vertices = curves::reverse_vertices(&pl.vertices, pl.closed),
        "ltypegen" | "l" => pl.plinegen = bool_or(p, "on", !pl.plinegen),
        "join" | "j" => {
            set_kind(s, h, EntityKind::LwPolyline(pl))?;
            let others: Vec<Handle> = p
                .get("handles2")
                .or_else(|| p.get("join"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(curves::handle_value).collect())
                .unwrap_or_default();
            let n = pedit_join(s, h, &others, f64_or(p, "fuzz", 0.0))?;
            return Ok(json!({ "handle": h.hex(), "joined": n }));
        }
        "vertex" | "e" => {
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("pedit", "`index` is required"))? as usize;
            let n = pl.vertices.len();
            if i >= n {
                return Err(bad("pedit", "vertex index out of range"));
            }
            match str_param(p, "action").unwrap_or("move") {
                "move" => {
                    let to = point_req("pedit", p, "to")?;
                    if let Some(v) = pl.vertices.get_mut(i) {
                        v.p = to;
                    }
                }
                "insert" => {
                    let to = point_req("pedit", p, "to")?;
                    if n >= MAX_GEN {
                        return Err(bad("pedit", "too many vertices"));
                    }
                    if let Some(v) = pl.vertices.get_mut(i) {
                        v.bulge = 0.0;
                    }
                    pl.vertices.insert(i + 1, PolyVertex::new(to));
                }
                "straighten" => {
                    let j = p.get("index2").and_then(Value::as_u64).map_or(i, |j| j as usize);
                    pedit_vertex::straighten(&mut pl, i, j)?;
                }
                "break" => {
                    let j = p.get("index2").and_then(Value::as_u64).map_or(i, |j| j as usize);
                    let hs = pedit_vertex::break_poly(s, h, &pl, i, j)?;
                    return Ok(json!({ "handle": h.hex(), "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }));
                }
                // The display is always current; tangents only exist within an interactive PEDIT.
                "regen" => {}
                "width" => {
                    let sw = f64_or(p, "startWidth", 0.0).max(0.0);
                    let ew = f64_or(p, "endWidth", sw).max(0.0);
                    if let Some(v) = pl.vertices.get_mut(i) {
                        v.start_width = sw;
                        v.end_width = ew;
                    }
                }
                _ => return Err(bad("pedit", "action must be move, insert, straighten, break, width or regen")),
            }
        }
        _ => return Err(bad("pedit", "unknown option")),
    }
    set_kind(s, h, EntityKind::LwPolyline(pl))?;
    Ok(json!({ "handle": h.hex() }))
}

pub(super) fn run_pedit(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("pedit", "`handle` is required"));
    }
    let opt = str_param(p, "option").ok_or_else(|| bad("pedit", "`option` is required"))?.to_ascii_lowercase();
    let mut out = Vec::new();
    for h in hs {
        out.push(pedit_apply(s, h, &opt, p)?);
    }
    Ok(if out.len() == 1 { out.pop().unwrap_or(Value::Null) } else { json!({ "results": out }) })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PPhase {
    #[default]
    Select,
    Convert,
    Main,
    Width,
    Join,
    Vertex,
    VMove,
    VInsert,
    VWidthStart,
    VWidthEnd,
    /// The second marker of Break / Straighten.
    VMark,
    VTangent,
}

#[derive(Default)]
pub(super) struct PeditM {
    pub(super) phase: PPhase,
    pub(super) h: Option<Handle>,
    pub(super) history: Vec<EntityKind>,
    original: Option<Vec<PolyVertex>>,
    join: SelectPhase,
    pub(super) vertex: usize,
    vwidth: f64,
    /// Break / Straighten: the second marked vertex, and which of the two runs.
    pub(super) mark2: usize,
    pub(super) breaking: bool,
    /// Edit vertex Tangent directions by vertex position, used by Fit.
    pub(super) tangents: Vec<(Vec2, Vec2)>,
    /// Pieces split off by Break, with the history length after it (erased again by Undo).
    pub(super) split_off: Vec<(usize, Vec<Handle>)>,
}

impl PeditM {
    pub(super) fn poly(&self, s: &Session) -> Option<LwPolyline> {
        let e = s.doc().ok()?.entity(self.h?)?.clone();
        match &e.kind {
            EntityKind::LwPolyline(p) => Some(p.clone()),
            _ => None,
        }
    }
    pub(super) fn snapshot(&mut self, s: &Session) {
        if let Some(h) = self.h
            && let Ok(e) = curves::entity(s, h)
        {
            self.history.push(e.kind);
        }
    }
    pub(super) fn apply(&mut self, s: &mut Session, opt: &str, p: Value) -> Result<()> {
        let h = self.h.ok_or_else(|| other("No polyline."))?;
        self.snapshot(s);
        if let Err(e) = pedit_apply(s, h, opt, &p) {
            self.history.pop();
            return Err(e);
        }
        Ok(())
    }
}

impl Interactive for PeditM {
    fn name(&self) -> &'static str {
        "PEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let sel = s.selection();
        if let [h] = sel.as_slice() {
            let k = curves::entity(s, *h)?.kind;
            match k {
                EntityKind::LwPolyline(_) => {
                    self.h = Some(*h);
                    self.original = self.poly(s).map(|p| p.vertices);
                    self.phase = PPhase::Main;
                }
                EntityKind::Line(_) | EntityKind::Arc(_) => {
                    self.h = Some(*h);
                    self.phase = PPhase::Convert;
                }
                _ => {}
            }
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.phase {
            PPhase::Select => Prompt::new("Select polyline or", Accept::POINT).kw(&["Multiple"]),
            PPhase::Convert => {
                Prompt::new("Object selected is not a polyline. Do you want to turn it into one?", curves::KW).kw(&["Yes", "No"]).default("Y")
            }
            PPhase::Main => {
                let closed = self.poly(s).is_some_and(|p| p.closed);
                let first = if closed { "Open" } else { "Close" };
                Prompt::new("Enter an option", curves::KW).kw(&[
                    first,
                    "Join",
                    "Width",
                    "Edit vertex",
                    "Fit",
                    "Spline",
                    "Decurve",
                    "Ltype gen",
                    "Reverse",
                    "Undo",
                ])
            }
            PPhase::Width => Prompt::new("Specify new width for all segments", Accept::NUMBER),
            PPhase::Join => self.join.prompt(),
            PPhase::Vertex => Prompt::new("Enter a vertex editing option", curves::KW)
                .kw(&["Next", "Previous", "Break", "Insert", "Move", "Regen", "Straighten", "Tangent", "Width", "eXit"])
                .default("N"),
            PPhase::VMove => Prompt::new("Specify new location for marked vertex", Accept::POINT),
            PPhase::VInsert => Prompt::new("Specify location for new vertex", Accept::POINT),
            PPhase::VWidthStart => Prompt::new("Specify starting width for next segment", Accept::NUMBER),
            PPhase::VWidthEnd => Prompt::new("Specify ending width for next segment", Accept::NUMBER).default(format!("{:.4}", self.vwidth)),
            PPhase::VMark => Prompt::new("Enter an option", curves::KW).kw(&["Next", "Previous", "Go", "eXit"]).default("N"),
            PPhase::VTangent => Prompt::new("Specify direction of vertex tangent", Accept::POINT_OR_NUMBER)
                .base_opt(self.poly(s).and_then(|p| p.vertices.get(self.vertex).map(|v| v.p))),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let kw = |i: &Input| match i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
            _ => None,
        };
        match self.phase {
            PPhase::Select => match i {
                Input::Point(p) => {
                    let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                    let k = curves::entity(s, h)?.kind;
                    self.h = Some(h);
                    match k {
                        EntityKind::LwPolyline(pl) => {
                            self.original = Some(pl.vertices);
                            self.phase = PPhase::Main;
                        }
                        EntityKind::Line(_) | EntityKind::Arc(_) => self.phase = PPhase::Convert,
                        _ => {
                            self.h = None;
                            return Err(other("Object selected is not a polyline."));
                        }
                    }
                    s.set_selection(vec![h]);
                    Ok(Step::Continue)
                }
                Input::Keyword(_) => Err(other("Select one polyline (Multiple is available through the JSON form).")),
                Input::Enter => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            },
            PPhase::Convert => {
                let no = kw(&i).is_some_and(|k| k.starts_with('n'));
                if no {
                    return Ok(Step::Done);
                }
                let h = self.h.ok_or_else(|| other("No object."))?;
                let k = curves::entity(s, h)?.kind;
                let pl = as_lwpoly(&k).ok_or_else(|| other("Cannot convert."))?;
                self.original = Some(pl.vertices.clone());
                set_kind(s, h, EntityKind::LwPolyline(pl))?;
                self.phase = PPhase::Main;
                Ok(Step::Continue)
            }
            PPhase::Main => {
                let Some(k) = kw(&i) else {
                    if i == Input::Enter {
                        s.set_selection(Vec::new());
                        return Ok(Step::Done);
                    }
                    return Ok(Step::Continue);
                };
                match k.as_str() {
                    "open" | "o" => self.apply(s, "open", json!({}))?,
                    "close" | "c" => self.apply(s, "close", json!({}))?,
                    "join" | "j" => {
                        self.join = SelectPhase::default();
                        self.phase = PPhase::Join;
                    }
                    "width" | "w" => self.phase = PPhase::Width,
                    "edit vertex" | "e" => {
                        self.vertex = 0;
                        self.phase = PPhase::Vertex;
                    }
                    "fit" | "f" => self.apply(s, "fit", json!({ "tangents": self.tangents_json(s) }))?,
                    "spline" | "s" => self.apply(s, "spline", json!({}))?,
                    "decurve" | "d" => {
                        // Within the session, Decurve restores the frame before Fit/Spline.
                        let h = self.h.ok_or_else(|| other("No polyline."))?;
                        self.snapshot(s);
                        match (self.poly(s), self.original.clone()) {
                            (Some(mut pl), Some(orig)) => {
                                pl.vertices = orig.into_iter().map(|v| PolyVertex { bulge: 0.0, ..v }).collect();
                                set_kind(s, h, EntityKind::LwPolyline(pl))?;
                            }
                            _ => {
                                self.history.pop();
                                self.apply(s, "decurve", json!({}))?;
                            }
                        }
                    }
                    "ltype gen" | "l" => self.apply(s, "ltypegen", json!({}))?,
                    "reverse" | "r" => self.apply(s, "reverse", json!({}))?,
                    "undo" | "u" => {
                        self.undo_split(s)?;
                        if let (Some(h), Some(k)) = (self.h, self.history.pop()) {
                            set_kind(s, h, k)?;
                        }
                    }
                    _ => return Err(other("Invalid option keyword.")),
                }
                Ok(Step::Continue)
            }
            PPhase::Width => {
                if let Input::Text(t) = &i {
                    let w = number(t).filter(|w| *w >= 0.0).ok_or_else(|| other("Requires a non-negative width."))?;
                    self.apply(s, "width", json!({ "width": w }))?;
                }
                self.phase = PPhase::Main;
                Ok(Step::Continue)
            }
            PPhase::Join => match self.join.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => {
                    self.phase = PPhase::Main;
                    Ok(Step::Continue)
                }
                SelOutcome::Done(hs) => {
                    let h = self.h.ok_or_else(|| other("No polyline."))?;
                    self.snapshot(s);
                    let n = pedit_join(s, h, &hs, 0.0)?;
                    s.echo(format!("{n} segments added to polyline"));
                    s.set_selection(vec![h]);
                    self.phase = PPhase::Main;
                    Ok(Step::Continue)
                }
            },
            PPhase::Vertex => {
                let n = self.poly(s).map(|p| p.vertices.len()).unwrap_or(0).max(1);
                let k = kw(&i).unwrap_or_else(|| if i == Input::Enter { "n".into() } else { String::new() });
                match k.as_str() {
                    "next" | "n" => self.vertex = (self.vertex + 1) % n,
                    "previous" | "p" => self.vertex = (self.vertex + n - 1) % n,
                    "move" | "m" => self.phase = PPhase::VMove,
                    "insert" | "i" => self.phase = PPhase::VInsert,
                    "straighten" | "s" | "break" | "b" => {
                        self.breaking = k.starts_with('b');
                        self.mark2 = self.vertex;
                        self.phase = PPhase::VMark;
                    }
                    "tangent" | "t" => self.phase = PPhase::VTangent,
                    // The display is always current.
                    "regen" | "r" => {}
                    "width" | "w" => self.phase = PPhase::VWidthStart,
                    "exit" | "x" => self.phase = PPhase::Main,
                    "" => {}
                    _ => return Err(other("Invalid option keyword.")),
                }
                Ok(Step::Continue)
            }
            PPhase::VMove | PPhase::VInsert => {
                if let Input::Point(p) = i {
                    let action = if self.phase == PPhase::VMove { "move" } else { "insert" };
                    self.apply(s, "vertex", json!({ "action": action, "index": self.vertex, "to": [p.x, p.y] }))?;
                    if action == "insert" {
                        self.vertex += 1;
                    }
                }
                self.phase = PPhase::Vertex;
                Ok(Step::Continue)
            }
            PPhase::VWidthStart => {
                if let Input::Text(t) = &i {
                    self.vwidth = number(t).filter(|w| *w >= 0.0).ok_or_else(|| other("Requires a non-negative width."))?;
                    self.phase = PPhase::VWidthEnd;
                } else {
                    self.phase = PPhase::Vertex;
                }
                Ok(Step::Continue)
            }
            PPhase::VMark => {
                let k = kw(&i).unwrap_or_else(|| if i == Input::Enter { "n".into() } else { String::new() });
                self.mark_input(s, &k)?;
                Ok(Step::Continue)
            }
            PPhase::VTangent => {
                self.tangent_input(s, &i)?;
                Ok(Step::Continue)
            }
            PPhase::VWidthEnd => {
                let ew = match &i {
                    Input::Text(t) => number(t).filter(|w| *w >= 0.0).ok_or_else(|| other("Requires a non-negative width."))?,
                    _ => self.vwidth,
                };
                self.apply(s, "vertex", json!({ "action": "width", "index": self.vertex, "startWidth": self.vwidth, "endWidth": ew }))?;
                self.phase = PPhase::Vertex;
                Ok(Step::Continue)
            }
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if !matches!(
            self.phase,
            PPhase::Vertex | PPhase::VMove | PPhase::VInsert | PPhase::VWidthStart | PPhase::VWidthEnd | PPhase::VMark | PPhase::VTangent
        ) {
            return Vec::new();
        }
        let Some(v) = self.poly(s).and_then(|p| p.vertices.get(self.vertex).copied()) else { return Vec::new() };
        let k = s.pixel_size() * 8.0;
        let mut out = vec![line(v.p - Vec2::new(k, k), v.p + Vec2::new(k, k)), line(v.p - Vec2::new(k, -k), v.p + Vec2::new(k, -k))];
        if matches!(self.phase, PPhase::VMove | PPhase::VInsert | PPhase::VTangent) {
            out.push(line(v.p, c));
        }
        if let (PPhase::VMark, Some(m)) = (self.phase, self.poly(s).and_then(|p| p.vertices.get(self.mark2).copied())) {
            out.extend([line(m.p - Vec2::new(k, k), m.p + Vec2::new(k, k)), line(m.p - Vec2::new(k, -k), m.p + Vec2::new(k, -k))]);
        }
        out
    }
}
