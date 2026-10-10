//! Modify menu depth (M2): PEDIT, LENGTHEN, ALIGN, BLEND, REVERSE, SPLINEDIT, NCOPY, CHSPACE,
//! FLATTEN, plus the fillet/trim/extend extensions used by modify.rs (arcs, polylines,
//! ellipses and splines).

use cadcraft_doc::{Common, Entity, EntityKind, Handle, LwPolyline, MAX_BLOCK_DEPTH, Space};
use cadcraft_geom::{
    Arc, Ellipse, Line, Mat3, PolyVertex, Polyline, Segment, Spline, TAU, Vec2, ccw_sweep, intersect_ext, line_circle, line_line_infinite, norm_angle,
};
use serde_json::{Value, json};

use super::curves::{self, Chain, MAX_GEN, TanObj};
use super::helpers::*;
use super::machines::{SelOutcome, SelectPhase, number};
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("pedit", "Polyline", run_pedit)
            .menu(&["Modify", "Object", "Polyline"])
            .alias(&["pe"])
            .params(
                "{handle | handles, option: close|open|join|width|fit|spline|decurve|reverse|ltypegen|vertex, width?, handles2? (join), fuzz?, \
                 action?: move|insert|straighten|width, index?, to?, startWidth?, endWidth?, convert?: bool}",
            )
            .interactive(|_| Ok(Box::new(PeditM::default()))),
        CommandSpec::new("splinedit", "Spline", run_splinedit)
            .menu(&["Modify", "Object", "Spline"])
            .alias(&["spe"])
            .params("{handle, option: close|open|reverse|refit|purge|polyline|move, fit?, index?, to?, precision?}")
            .interactive(|_| Ok(Box::new(SplineditM::default()))),
        CommandSpec::new("lengthen", "Lengthen", run_lengthen)
            .menu(&["Modify", "Lengthen"])
            .alias(&["len"])
            .params("{handle, pick?: [x,y] (end), delta? | deltaAngle? (deg) | percent? | total? | totalAngle? (deg) | to?: [x,y]} ({handle} alone measures)")
            .interactive(|_| Ok(Box::new(LengthenM::default()))),
        CommandSpec::new("align", "Align", run_align)
            .menu(&["Modify", "3D Operations", "Align"])
            .alias(&["al"])
            .params("{handles?, s1, d1, s2?, d2?, scale?: bool}")
            .interactive(|_| Ok(Box::new(AlignM::default()))),
        CommandSpec::new("blend", "Blend", run_blend)
            .menu(&["Modify", "Blend"])
            .alias(&["blendcurves"])
            .params("{h1, p1, h2, p2, continuity?: tangent|smooth}")
            .interactive(|_| Ok(Box::new(BlendM::default()))),
        CommandSpec::new("reverse", "Reverse", run_reverse)
            .params("{handles?} (lines, polylines, 3D polylines, splines)")
            .interactive(|_| Ok(Box::new(SelOp::new(SelKind::Reverse)))),
        CommandSpec::new("ncopy", "Copy Nested Objects", run_ncopy)
            .menu(&["Modify", "Copy Nested Objects"])
            .params("{handle (block reference), pick?: [x,y] (nearest nested object; default all), delta? | from,to}")
            .interactive(|_| Ok(Box::new(NcopyM::default()))),
        CommandSpec::new("chspace", "Change Space", run_chspace)
            .menu(&["Modify", "Change Space"])
            .params("{handles?, to?: \"model\" | layout name, viewport?: handle}")
            .interactive(|_| Ok(Box::new(SelOp::new(SelKind::Chspace)))),
        CommandSpec::new("flatten", "Flatten Objects", run_flatten)
            .menu(&["Modify", "Flatten Objects"])
            .params("{handles?} (z = 0, 3D polylines become polylines)")
            .interactive(|_| Ok(Box::new(SelOp::new(SelKind::Flatten)))),
        CommandSpec::new("arraypath", "Path Array", run_arraypath)
            .menu(&["Modify", "Array", "Path Array"])
            .params("{handles?, path (handle), count? (default 6) | spacing?, align?: bool (default true)}")
            .interactive(|_| Ok(Box::new(PathArrayM::default()))),
        CommandSpec::new("textedit", "Edit Text...", run_textedit)
            .menu(&["Modify", "Object", "Edit Text..."])
            .alias(&["ed", "ddedit"])
            .params("{handle, text} (text, mtext, attribute definitions, dimension text override)")
            .interactive(|_| Ok(Box::new(TextEditM::default()))),
    ]
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

fn set_kind(s: &mut Session, h: Handle, k: EntityKind) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
    Ok(())
}

fn push_entity(s: &mut Session, space: &Space, common: Common, kind: EntityKind) -> Result<Handle> {
    let d = s.doc_mut()?;
    d.ensure_layer(&common.layer);
    let h = d.new_handle();
    let st = d.space_mut(space).ok_or_else(|| other("no such space"))?;
    st.push(Entity { handle: h, common, kind });
    Ok(h)
}

// =====================================================================================
// PEDIT
// =====================================================================================

/// The polyline form of a line, arc or polyline.
pub(crate) fn as_lwpoly(k: &EntityKind) -> Option<LwPolyline> {
    match k {
        EntityKind::LwPolyline(p) => Some(p.clone()),
        EntityKind::Line(_) | EntityKind::Arc(_) => {
            let c = Chain::of(k)?;
            Some(LwPolyline { vertices: c.vertices(), closed: false, const_width: 0.0, elevation: 0.0, plinegen: false })
        }
        _ => None,
    }
}

/// Fit curve: a tangent-continuous pair of arcs (biarc) per segment through every vertex.
pub(crate) fn fit_vertices(vs: &[PolyVertex], closed: bool) -> Vec<PolyVertex> {
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
        "fit" | "f" => pl.vertices = fit_vertices(&pl.vertices, pl.closed),
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
                    if let Some(v) = pl.vertices.get_mut(i) {
                        v.bulge = 0.0;
                    }
                }
                "width" => {
                    let sw = f64_or(p, "startWidth", 0.0).max(0.0);
                    let ew = f64_or(p, "endWidth", sw).max(0.0);
                    if let Some(v) = pl.vertices.get_mut(i) {
                        v.start_width = sw;
                        v.end_width = ew;
                    }
                }
                _ => return Err(bad("pedit", "action must be move, insert, straighten or width")),
            }
        }
        _ => return Err(bad("pedit", "unknown option")),
    }
    set_kind(s, h, EntityKind::LwPolyline(pl))?;
    Ok(json!({ "handle": h.hex() }))
}

fn run_pedit(s: &mut Session, p: &Value) -> Result<Value> {
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
enum PPhase {
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
}

#[derive(Default)]
struct PeditM {
    phase: PPhase,
    h: Option<Handle>,
    history: Vec<EntityKind>,
    original: Option<Vec<PolyVertex>>,
    join: SelectPhase,
    vertex: usize,
    vwidth: f64,
}

impl PeditM {
    fn poly(&self, s: &Session) -> Option<LwPolyline> {
        let e = s.doc().ok()?.entity(self.h?)?.clone();
        match &e.kind {
            EntityKind::LwPolyline(p) => Some(p.clone()),
            _ => None,
        }
    }
    fn snapshot(&mut self, s: &Session) {
        if let Some(h) = self.h
            && let Ok(e) = curves::entity(s, h)
        {
            self.history.push(e.kind);
        }
    }
    fn apply(&mut self, s: &mut Session, opt: &str, p: Value) -> Result<()> {
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
            if matches!(k, EntityKind::LwPolyline(_)) {
                self.h = Some(*h);
                self.original = self.poly(s).map(|p| p.vertices);
                self.phase = PPhase::Main;
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
                    "fit" | "f" => self.apply(s, "fit", json!({}))?,
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
                    "straighten" | "s" => {
                        self.apply(s, "vertex", json!({ "action": "straighten", "index": self.vertex }))?;
                    }
                    "width" | "w" => self.phase = PPhase::VWidthStart,
                    "exit" | "x" => self.phase = PPhase::Main,
                    "" => {}
                    other_k => s.echo(format!("{other_k}: not available yet")),
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
        if !matches!(self.phase, PPhase::Vertex | PPhase::VMove | PPhase::VInsert | PPhase::VWidthStart | PPhase::VWidthEnd) {
            return Vec::new();
        }
        let Some(v) = self.poly(s).and_then(|p| p.vertices.get(self.vertex).copied()) else { return Vec::new() };
        let k = s.pixel_size() * 8.0;
        let mut out = vec![line(v.p - Vec2::new(k, k), v.p + Vec2::new(k, k)), line(v.p - Vec2::new(k, -k), v.p + Vec2::new(k, -k))];
        if matches!(self.phase, PPhase::VMove | PPhase::VInsert) {
            out.push(line(v.p, c));
        }
        out
    }
}

// =====================================================================================
// SPLINEDIT
// =====================================================================================

fn spline_close(sp: &Spline) -> Spline {
    if sp.closed {
        return sp.clone();
    }
    let mut out = if sp.fit.len() >= 2 {
        let mut f = sp.fit.clone();
        f.extend(sp.fit.first().copied());
        Spline::from_fit_points(&f)
    } else {
        let mut c = sp.control.clone();
        c.extend(sp.control.first().copied());
        Spline::from_control(c, sp.degree)
    };
    out.closed = true;
    out
}

fn spline_open(sp: &Spline) -> Spline {
    if !sp.closed {
        return sp.clone();
    }
    let trim = |v: &[Vec2]| -> Vec<Vec2> {
        let mut v = v.to_vec();
        if v.len() > 2 && v.first().zip(v.last()).is_some_and(|(a, b)| a.near(*b, 1e-9)) {
            v.pop();
        }
        v
    };
    if sp.fit.len() >= 2 { Spline::from_fit_points(&trim(&sp.fit)) } else { Spline::from_control(trim(&sp.control), sp.degree) }
}

pub(crate) fn spline_to_poly(sp: &Spline, precision: usize) -> LwPolyline {
    let (lo, hi) = sp.domain();
    let n = (precision.clamp(1, 99) * sp.control.len().max(2)).min(MAX_GEN);
    let vs = (0..=n).map(|i| PolyVertex::new(sp.eval(lo + (hi - lo) * i as f64 / n as f64))).collect::<Vec<_>>();
    let mut vs = vs;
    if sp.closed && vs.len() > 2 {
        vs.pop();
    }
    LwPolyline { vertices: vs, closed: sp.closed, const_width: 0.0, elevation: 0.0, plinegen: false }
}

fn run_splinedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("splinedit", "`handle` is required"))?;
    let e = curves::entity(s, h)?;
    let EntityKind::Spline(sp) = &e.kind else { return Err(other("Object selected is not a spline.")) };
    let opt = str_param(p, "option").ok_or_else(|| bad("splinedit", "`option` is required"))?.to_ascii_lowercase();
    let k = match opt.as_str() {
        "close" => EntityKind::Spline(spline_close(sp)),
        "open" => EntityKind::Spline(spline_open(sp)),
        "reverse" => EntityKind::Spline(curves::reverse_spline(sp)),
        "purge" => EntityKind::Spline(Spline { fit: Vec::new(), ..sp.clone() }),
        "refit" => {
            let fit = points_param(p, "fit").unwrap_or_else(|| sp.fit.clone());
            if fit.len() < 2 || fit.len() > MAX_GEN {
                return Err(other("The spline has no fit data; give `fit` points."));
            }
            let mut n = Spline::from_fit_points(&fit);
            n.closed = sp.closed;
            EntityKind::Spline(n)
        }
        "polyline" => EntityKind::LwPolyline(spline_to_poly(sp, p.get("precision").and_then(Value::as_u64).unwrap_or(10) as usize)),
        "move" => {
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("splinedit", "`index` is required"))? as usize;
            let to = point_req("splinedit", p, "to")?;
            let mut n = sp.clone();
            if !n.fit.is_empty() {
                let f = n.fit.get_mut(i).ok_or_else(|| bad("splinedit", "index out of range"))?;
                *f = to;
                let closed = n.closed;
                n = Spline::from_fit_points(&n.fit);
                n.closed = closed;
            } else {
                let c = n.control.get_mut(i).ok_or_else(|| bad("splinedit", "index out of range"))?;
                *c = to;
            }
            EntityKind::Spline(n)
        }
        _ => return Err(bad("splinedit", "unknown option")),
    };
    set_kind(s, h, k)?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
struct SplineditM {
    h: Option<Handle>,
    fit_menu: bool,
    history: Vec<EntityKind>,
}

impl Interactive for SplineditM {
    fn name(&self) -> &'static str {
        "SPLINEDIT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let Some(h) = self.h else { return Prompt::new("Select spline", Accept::POINT) };
        if self.fit_menu {
            return Prompt::new("Enter a fit data option", curves::KW)
                .kw(&["Add", "Close", "Delete", "Kink", "Move", "Purge", "Tangents", "toLerance", "eXit"])
                .default("eXit");
        }
        let closed = s.doc().ok().and_then(|d| d.entity(h)).is_some_and(|e| matches!(&e.kind, EntityKind::Spline(sp) if sp.closed));
        Prompt::new("Enter an option", curves::KW)
            .kw(&[if closed { "Open" } else { "Close" }, "Join", "Fit data", "Edit vertex", "convert to Polyline", "Reverse", "Undo", "eXit"])
            .default("eXit")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(h) = self.h else {
            return match i {
                Input::Point(p) => {
                    let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                    if !matches!(curves::entity(s, h)?.kind, EntityKind::Spline(_)) {
                        return Err(other("Object selected is not a spline."));
                    }
                    self.h = Some(h);
                    s.set_selection(vec![h]);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            };
        };
        let k = match &i {
            Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
            Input::Enter => "exit".into(),
            _ => return Ok(Step::Continue),
        };
        let mut apply = |s: &mut Session, opt: &str| -> Result<()> {
            let before = curves::entity(s, h)?.kind;
            run_splinedit(s, &json!({ "handle": h.hex(), "option": opt }))?;
            self.history.push(before);
            Ok(())
        };
        if self.fit_menu {
            match k.as_str() {
                "purge" | "p" => apply(s, "purge")?,
                "close" | "c" => apply(s, "close")?,
                "exit" | "x" => {}
                _ => s.echo(format!("{k}: not available yet")),
            }
            self.fit_menu = false;
            return Ok(Step::Continue);
        }
        match k.as_str() {
            "close" | "c" => apply(s, "close")?,
            "open" | "o" => apply(s, "open")?,
            "reverse" | "r" => apply(s, "reverse")?,
            "convert to polyline" | "p" => {
                apply(s, "polyline")?;
                return Ok(Step::Done);
            }
            "fit data" | "f" => self.fit_menu = true,
            "undo" | "u" => {
                if let Some(k) = self.history.pop() {
                    set_kind(s, h, k)?;
                }
            }
            "exit" | "x" => {
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            _ => s.echo(format!("{k}: not available yet")),
        }
        Ok(Step::Continue)
    }
}

// =====================================================================================
// LENGTHEN
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LenMode {
    Delta(f64),
    DeltaAngle(f64),
    Percent(f64),
    Total(f64),
    TotalAngle(f64),
    To(Vec2),
}

/// Current length (and included angle for arcs) of a curve.
fn measure_len(k: &EntityKind) -> Option<(f64, Option<f64>)> {
    match k {
        EntityKind::Arc(a) => {
            let g = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            Some((g.len(), Some(g.sweep())))
        }
        _ => Chain::of(k).map(|c| (c.len(), None)),
    }
}

/// Lengthen a line, arc or open polyline at the end nearest `pick`.
pub(crate) fn lengthen_kind(k: &EntityKind, pick: Option<Vec2>, mode: LenMode) -> Result<EntityKind> {
    let too_short = || other("Lengthening would make the object zero length or negative.");
    match k {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let at_b = pick.is_none_or(|p| p.dist(b) <= p.dist(a));
            let (fixed, end) = if at_b { (a, b) } else { (b, a) };
            let len = fixed.dist(end);
            let dir = (end - fixed).normalized();
            if dir == Vec2::ZERO {
                return Err(other("Zero-length line."));
            }
            let nl = match mode {
                LenMode::Delta(d) => len + d,
                LenMode::Percent(p) => len * p / 100.0,
                LenMode::Total(t) => t,
                LenMode::To(q) => (q - fixed).dot(dir),
                LenMode::DeltaAngle(_) | LenMode::TotalAngle(_) => return Err(other("Angle options apply to arcs.")),
            };
            if !(nl > 1e-12 && nl.is_finite()) {
                return Err(too_short());
            }
            let ne = fixed + dir * nl;
            Ok(if at_b { line(a, ne) } else { line(ne, b) })
        }
        EntityKind::Arc(ar) => {
            let g = Arc::new(ar.center.xy(), ar.radius, ar.start, ar.end);
            let at_end = pick.is_none_or(|p| p.dist(g.end_point()) <= p.dist(g.start_point()));
            let sw = g.sweep();
            let r = g.radius.max(1e-300);
            let ns = match mode {
                LenMode::Delta(d) => sw + d / r,
                LenMode::DeltaAngle(a) => sw + a,
                LenMode::Percent(p) => sw * p / 100.0,
                LenMode::Total(t) => t / r,
                LenMode::TotalAngle(a) => a,
                LenMode::To(q) => {
                    let a = g.center.angle_to(q);
                    if at_end { ccw_sweep(g.start, a) } else { ccw_sweep(a, g.end) }
                }
            };
            if !(ns > 1e-12 && ns < TAU - 1e-9) {
                return Err(other("The new arc would be zero length or a full circle."));
            }
            Ok(if at_end { arc(&Arc::new(g.center, g.radius, g.start, g.start + ns)) } else { arc(&Arc::new(g.center, g.radius, g.end - ns, g.end)) })
        }
        EntityKind::LwPolyline(p) if !p.closed && p.vertices.len() >= 2 => {
            let total = Polyline { vertices: p.vertices.clone(), closed: false }.len();
            let mut vs = p.vertices.clone();
            let n = vs.len();
            let (first, last) = (vs.first().map(|v| v.p).unwrap_or_default(), vs.last().map(|v| v.p).unwrap_or_default());
            let at_end = pick.is_none_or(|q| q.dist(last) <= q.dist(first));
            if !at_end {
                vs = curves::reverse_vertices(&vs, false);
            }
            let seg_start = vs.get(n - 2).copied().unwrap_or_default();
            if seg_start.bulge.abs() > 1e-12 {
                return Err(other("The end segment is an arc; explode the polyline or use an open line end."));
            }
            let end = vs.get(n - 1).map(|v| v.p).unwrap_or_default();
            let seg_len = seg_start.p.dist(end);
            let dir = (end - seg_start.p).normalized();
            if dir == Vec2::ZERO {
                return Err(other("Zero-length end segment."));
            }
            let delta = match mode {
                LenMode::Delta(d) => d,
                LenMode::Percent(pc) => total * pc / 100.0 - total,
                LenMode::Total(t) => t - total,
                LenMode::To(q) => (q - seg_start.p).dot(dir) - seg_len,
                LenMode::DeltaAngle(_) | LenMode::TotalAngle(_) => return Err(other("Angle options apply to arcs.")),
            };
            let nl = seg_len + delta;
            if !(nl > 1e-12 && nl.is_finite()) {
                return Err(too_short());
            }
            if let Some(v) = vs.get_mut(n - 1) {
                v.p = seg_start.p + dir * nl;
            }
            if !at_end {
                vs = curves::reverse_vertices(&vs, false);
            }
            Ok(EntityKind::LwPolyline(LwPolyline { vertices: vs, ..p.clone() }))
        }
        _ => Err(other("Select a line, arc or open polyline.")),
    }
}

fn len_mode(p: &Value) -> Option<LenMode> {
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(v) = f("delta") {
        Some(LenMode::Delta(v))
    } else if let Some(v) = f("deltaAngle") {
        Some(LenMode::DeltaAngle(v.to_radians()))
    } else if let Some(v) = f("percent") {
        Some(LenMode::Percent(v))
    } else if let Some(v) = f("total") {
        Some(LenMode::Total(v))
    } else if let Some(v) = f("totalAngle") {
        Some(LenMode::TotalAngle(v.to_radians()))
    } else {
        point_param(p, "to").map(LenMode::To)
    }
}

fn run_lengthen(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("lengthen", "`handle` is required"))?;
    let e = curves::entity(s, h)?;
    let Some(mode) = len_mode(p) else {
        let (l, a) = measure_len(&e.kind).ok_or_else(|| other("Cannot measure that object."))?;
        return Ok(json!({ "length": l, "angle": a.map(f64::to_degrees) }));
    };
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let k = lengthen_kind(&e.kind, point_param(p, "pick"), mode)?;
    let l = measure_len(&k).map(|x| x.0).unwrap_or(0.0);
    set_kind(s, h, k)?;
    Ok(json!({ "handle": h.hex(), "length": l }))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum LPhase {
    #[default]
    Main,
    Delta,
    DeltaAngle,
    Percent,
    Total,
    TotalAngle,
    Change,
    DynEnd(Handle, Vec2),
}

#[derive(Default)]
struct LengthenM {
    phase: LPhase,
    mode: Option<LenMode>,
    dynamic: bool,
    history: Vec<(Handle, EntityKind)>,
}

impl Interactive for LengthenM {
    fn name(&self) -> &'static str {
        "LENGTHEN"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.phase {
            LPhase::Main => {
                Prompt::new("Select an object to measure or", Accept::POINT).kw(&["DElta", "Percent", "Total", "DYnamic"]).default("DYnamic")
            }
            LPhase::Delta => Prompt::new("Enter delta length or", Accept::NUMBER).kw(&["Angle"]).default("0.0000"),
            LPhase::DeltaAngle => Prompt::new("Enter delta angle", Accept::NUMBER).default("0"),
            LPhase::Percent => Prompt::new("Enter percentage length", Accept::NUMBER).default("100.0000"),
            LPhase::Total => Prompt::new("Specify total length or", Accept::NUMBER).kw(&["Angle"]).default("1.0000"),
            LPhase::TotalAngle => Prompt::new("Specify total angle", Accept::NUMBER).default("57"),
            LPhase::Change => Prompt::new("Select an object to change or", Accept::POINT).kw(&["Undo"]),
            LPhase::DynEnd(..) => Prompt::new("Specify new end point", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let num = |i: &Input| -> Option<f64> { if let Input::Text(t) = i { number(t) } else { None } };
        let ang = |i: &Input| -> Option<f64> { if let Input::Text(t) = i { crate::units::parse_angle(t) } else { None } };
        match (self.phase, &i) {
            (LPhase::Main, Input::Point(p)) => {
                let h = curves::pick_at(s, *p).ok_or_else(|| other("*Invalid selection*"))?;
                let k = curves::entity(s, h)?.kind;
                match measure_len(&k) {
                    Some((l, Some(a))) => s.echo(format!("Current length: {l:.4}, included angle: {:.0}", a.to_degrees())),
                    Some((l, None)) => s.echo(format!("Current length: {l:.4}")),
                    None => s.echo("Cannot measure that object."),
                }
                Ok(Step::Continue)
            }
            (LPhase::Main, Input::Keyword(k)) => {
                self.phase = match k.as_str() {
                    "DElta" => LPhase::Delta,
                    "Percent" => LPhase::Percent,
                    "Total" => LPhase::Total,
                    _ => {
                        self.dynamic = true;
                        LPhase::Change
                    }
                };
                Ok(Step::Continue)
            }
            (LPhase::Main, Input::Enter) => {
                self.dynamic = true;
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::Delta | LPhase::Total, Input::Keyword(_)) => {
                self.phase = if self.phase == LPhase::Delta { LPhase::DeltaAngle } else { LPhase::TotalAngle };
                Ok(Step::Continue)
            }
            (LPhase::Delta, _) => {
                self.mode = Some(LenMode::Delta(num(&i).ok_or_else(|| other("Requires a distance."))?));
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::DeltaAngle, _) => {
                self.mode = Some(LenMode::DeltaAngle(ang(&i).ok_or_else(|| other("Requires an angle."))?));
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::Percent, _) => {
                let v = num(&i).filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive percentage."))?;
                self.mode = Some(LenMode::Percent(v));
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::Total, _) => {
                let v = num(&i).filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive length."))?;
                self.mode = Some(LenMode::Total(v));
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::TotalAngle, _) => {
                self.mode = Some(LenMode::TotalAngle(ang(&i).ok_or_else(|| other("Requires an angle."))?));
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::Change, Input::Point(p)) => {
                let h = curves::pick_at(s, *p).ok_or_else(|| other("*Invalid selection*"))?;
                if self.dynamic {
                    self.phase = LPhase::DynEnd(h, *p);
                    return Ok(Step::Continue);
                }
                let mode = self.mode.ok_or_else(|| other("No lengthen mode."))?;
                let k = curves::entity(s, h)?.kind;
                let nk = lengthen_kind(&k, Some(*p), mode)?;
                set_kind(s, h, nk)?;
                self.history.push((h, k));
                Ok(Step::Continue)
            }
            (LPhase::DynEnd(h, pick), Input::Point(p)) => {
                let k = curves::entity(s, h)?.kind;
                let nk = lengthen_kind(&k, Some(pick), LenMode::To(*p))?;
                set_kind(s, h, nk)?;
                self.history.push((h, k));
                self.phase = LPhase::Change;
                Ok(Step::Continue)
            }
            (LPhase::Change, Input::Keyword(_)) => {
                if let Some((h, k)) = self.history.pop() {
                    set_kind(s, h, k)?;
                }
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err(other("Invalid input.")),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let LPhase::DynEnd(h, pick) = self.phase
            && let Ok(e) = curves::entity(s, h)
        {
            return lengthen_kind(&e.kind, Some(pick), LenMode::To(c)).map(|k| vec![k]).unwrap_or_default();
        }
        Vec::new()
    }
}

// =====================================================================================
// ALIGN
// =====================================================================================

pub(crate) fn align_matrix(s1: Vec2, d1: Vec2, pair2: Option<(Vec2, Vec2)>, scale: bool) -> Mat3 {
    let mut m = Mat3::translate(d1 - s1);
    if let Some((s2, d2)) = pair2 {
        let vs = s2 - s1;
        let vd = d2 - d1;
        if vs.len() > 1e-12 && vd.len() > 1e-12 {
            m = Mat3::rotate_about(d1, vd.angle() - vs.angle()).then_before(m);
            if scale {
                m = Mat3::scale_about(d1, vd.len() / vs.len()).then_before(m);
            }
        }
    }
    m
}

fn run_align(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let s1 = point_req("align", p, "s1")?;
    let d1 = point_req("align", p, "d1")?;
    let pair2 = point_param(p, "s2").zip(point_param(p, "d2"));
    let m = align_matrix(s1, d1, pair2, bool_or(p, "scale", false));
    let r = super::modify::transform_entities(s, &hs, &m, false)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct AlignM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    pts: Vec<Vec2>,
    ask_scale: bool,
}

impl AlignM {
    fn finish(&self, s: &mut Session, scale: bool) -> Result<Step> {
        let (Some(s1), Some(d1)) = (self.pts.first().copied(), self.pts.get(1).copied()) else { return Ok(Step::Cancel) };
        let pair2 = self.pts.get(2).copied().zip(self.pts.get(3).copied());
        let m = align_matrix(s1, d1, pair2, scale);
        super::modify::transform_entities(s, &self.objs, &m, false)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for AlignM {
    fn name(&self) -> &'static str {
        "ALIGN"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        if self.ask_scale {
            return Prompt::new("Scale objects based on alignment points?", curves::KW).kw(&["Yes", "No"]).default("N");
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first source point", Accept::POINT),
            1 => Prompt::new("Specify first destination point", Accept::POINT).base_opt(self.pts.first().copied()),
            2 => Prompt::new("Specify second source point", Accept::POINT),
            3 => Prompt::new("Specify second destination point", Accept::POINT).base_opt(self.pts.get(2).copied()),
            _ => Prompt::new("Specify third source point or <continue>", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Cancel),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    Ok(Step::Continue)
                }
            };
        }
        if self.ask_scale {
            let yes = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.to_ascii_lowercase().starts_with('y'));
            return self.finish(s, yes);
        }
        match i {
            Input::Point(p) => {
                if self.pts.len() < 4 {
                    self.pts.push(p);
                } else {
                    s.echo("Third pair ignored (2D align).");
                    self.ask_scale = true;
                }
                Ok(Step::Continue)
            }
            Input::Enter => match self.pts.len() {
                n if n >= 4 => {
                    self.ask_scale = true;
                    Ok(Step::Continue)
                }
                2 | 3 => {
                    self.pts.truncate(2);
                    self.finish(s, false)
                }
                _ => Ok(Step::Cancel),
            },
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match self.pts.as_slice() {
            [a] => vec![line(*a, c)],
            [_, _, a] => vec![line(*a, c)],
            _ => Vec::new(),
        }
    }
}

// =====================================================================================
// BLEND
// =====================================================================================

/// Endpoint of a curve nearest `pick` and the outward unit tangent there.
fn curve_end(k: &EntityKind, pick: Vec2) -> Option<(Vec2, Vec2)> {
    let c = Chain::of(k)?;
    if c.closed {
        return None;
    }
    let (a, b) = (c.start()?, c.end()?);
    if pick.dist(b) <= pick.dist(a) {
        let t = c.segs.last()?.tangent(1.0);
        Some((b, t))
    } else {
        let t = c.segs.first()?.tangent(0.0);
        Some((a, -t))
    }
}

pub(crate) fn blend_spline(e1: (Vec2, Vec2), e2: (Vec2, Vec2), smooth: bool) -> Option<Spline> {
    let (p1, t1) = e1;
    let (p2, t2) = e2;
    let d = p1.dist(p2);
    if d < 1e-12 {
        return None;
    }
    let ctrl = if smooth {
        let k = d / 5.0;
        vec![p1, p1 + t1 * k, p1 + t1 * (2.0 * k), p2 + t2 * (2.0 * k), p2 + t2 * k, p2]
    } else {
        let k = d / 3.0;
        vec![p1, p1 + t1 * k, p2 + t2 * k, p2]
    };
    let deg = ctrl.len() - 1;
    Some(Spline::from_control(ctrl, deg))
}

fn blend(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, smooth: bool) -> Result<Handle> {
    let k1 = curves::entity(s, h1)?.kind;
    let k2 = curves::entity(s, h2)?.kind;
    let e1 = curve_end(&k1, p1).ok_or_else(|| other("Select an open line, arc, polyline, ellipse arc or spline."))?;
    let e2 = curve_end(&k2, p2).ok_or_else(|| other("Select an open line, arc, polyline, ellipse arc or spline."))?;
    let sp = blend_spline(e1, e2, smooth).ok_or_else(|| other("The ends coincide."))?;
    s.add_entity(EntityKind::Spline(sp))
}

fn run_blend(s: &mut Session, p: &Value) -> Result<Value> {
    let h1 = curves::handle_param(p, "h1").ok_or_else(|| bad("blend", "`h1` is required"))?;
    let h2 = curves::handle_param(p, "h2").ok_or_else(|| bad("blend", "`h2` is required"))?;
    let p1 = point_req("blend", p, "p1")?;
    let p2 = point_req("blend", p, "p2")?;
    let smooth = str_param(p, "continuity").is_some_and(|c| c.eq_ignore_ascii_case("smooth"));
    let h = blend(s, h1, p1, h2, p2, smooth)?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
struct BlendM {
    first: Option<(Handle, Vec2)>,
    smooth: bool,
    asking: bool,
}

impl Interactive for BlendM {
    fn name(&self) -> &'static str {
        "BLEND"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        s.echo(format!("Continuity = {}", if self.smooth { "Smooth" } else { "Tangent" }));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking {
            return Prompt::new("Enter continuity", curves::KW).kw(&["Tangent", "Smooth"]).default("Tangent");
        }
        match self.first {
            None => Prompt::new("Select first object or", Accept::POINT).kw(&["CONtinuity"]),
            Some(_) => Prompt::new("Select second point", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking {
            if let Input::Keyword(k) | Input::Text(k) = &i {
                self.smooth = k.to_ascii_lowercase().starts_with('s');
            }
            self.asking = false;
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(_) => {
                self.asking = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                match self.first {
                    None => {
                        let k = curves::entity(s, h)?.kind;
                        curve_end(&k, p).ok_or_else(|| other("Select an open curve."))?;
                        self.first = Some((h, p));
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) => {
                        blend(s, h1, p1, h, p, self.smooth)?;
                        Ok(Step::Done)
                    }
                }
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}

// =====================================================================================
// REVERSE / CHSPACE / FLATTEN (select, then act)
// =====================================================================================

pub(crate) fn reverse_kind(k: &EntityKind) -> Option<EntityKind> {
    Some(match k {
        EntityKind::Line(l) => EntityKind::Line(cadcraft_doc::Line { a: l.b, b: l.a }),
        EntityKind::LwPolyline(p) => EntityKind::LwPolyline(LwPolyline { vertices: curves::reverse_vertices(&p.vertices, p.closed), ..p.clone() }),
        EntityKind::Polyline3d(p) => {
            EntityKind::Polyline3d(cadcraft_doc::Polyline3d { points: p.points.iter().rev().copied().collect(), closed: p.closed })
        }
        EntityKind::Spline(sp) => EntityKind::Spline(curves::reverse_spline(sp)),
        _ => return None,
    })
}

fn reverse(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let mut n = 0;
    for h in hs {
        if curves::is_locked(s, *h) {
            continue;
        }
        let Ok(e) = curves::entity(s, *h) else { continue };
        if let Some(k) = reverse_kind(&e.kind) {
            set_kind(s, *h, k)?;
            n += 1;
        }
    }
    Ok(n)
}

fn run_reverse(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = reverse(s, &hs)?;
    Ok(json!({ "reversed": n }))
}

/// Zero every z coordinate; 3D polylines become 2D polylines.
pub(crate) fn flatten_kind(k: &mut EntityKind) {
    let z = |p: &mut cadcraft_geom::Vec3| p.z = 0.0;
    match k {
        EntityKind::Line(l) => {
            z(&mut l.a);
            z(&mut l.b);
        }
        EntityKind::Point(p) => z(&mut p.p),
        EntityKind::Circle(c) => z(&mut c.center),
        EntityKind::Arc(a) => z(&mut a.center),
        EntityKind::Ellipse(e) => {
            z(&mut e.center);
            z(&mut e.major);
        }
        EntityKind::LwPolyline(p) => p.elevation = 0.0,
        EntityKind::Polyline3d(p) => {
            *k = lwpoly(p.points.iter().map(|q| PolyVertex::new(q.xy())).collect(), p.closed);
        }
        EntityKind::Ray(r) | EntityKind::XLine(r) => {
            z(&mut r.base);
            r.dir.z = 0.0;
            r.dir = r.dir.normalized();
        }
        EntityKind::Text(t) => {
            z(&mut t.insert);
            t.align_pt.iter_mut().for_each(z);
        }
        EntityKind::AttDef(a) => {
            z(&mut a.text.insert);
            a.text.align_pt.iter_mut().for_each(z);
        }
        EntityKind::MText(t) => z(&mut t.insert),
        EntityKind::Insert(i) => {
            z(&mut i.insert);
            for a in &mut i.attribs {
                z(&mut a.text.insert);
                a.text.align_pt.iter_mut().for_each(z);
            }
        }
        EntityKind::Dimension(d) => {
            for p in [&mut d.defpt, &mut d.text_mid, &mut d.p13, &mut d.p14, &mut d.p15, &mut d.p16] {
                z(p);
            }
            d.block = None;
        }
        EntityKind::Leader(l) => l.vertices.iter_mut().for_each(z),
        EntityKind::MLeader(m) => {
            m.leaders.iter_mut().for_each(|l| l.iter_mut().for_each(z));
            z(&mut m.landing);
        }
        EntityKind::Hatch(h) => h.elevation = 0.0,
        EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(z),
        EntityKind::Face3d(f) => f.corners.iter_mut().for_each(z),
        EntityKind::Image(i) => {
            z(&mut i.insert);
            i.u.z = 0.0;
            i.v.z = 0.0;
        }
        EntityKind::Table(t) => z(&mut t.insert),
        EntityKind::Spline(_) | EntityKind::Viewport(_) | EntityKind::Wipeout(_) | EntityKind::Unknown(_) => {}
    }
}

fn flatten(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let mut n = 0;
    for h in hs {
        if curves::is_locked(s, *h) {
            continue;
        }
        if s.doc()?.entity(*h).is_none() {
            continue;
        }
        s.doc_mut()?.modify_entity(*h, |e| {
            flatten_kind(&mut e.kind);
            e.common.thickness = 0.0;
        })?;
        n += 1;
    }
    Ok(n)
}

fn run_flatten(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = flatten(s, &hs)?;
    Ok(json!({ "flattened": n }))
}

/// Model↔paper transform through a viewport: paper = center + (model − viewCenter) / scale.
fn viewport_of(s: &Session, layout: &str, vp: Option<Handle>) -> Option<cadcraft_doc::Viewport> {
    let d = s.doc().ok()?;
    if let Some(h) = vp
        && let Some(e) = d.entity(h)
        && let EntityKind::Viewport(v) = &e.kind
    {
        return Some(v.clone());
    }
    let st = d.space(&Space::Paper(layout.to_string()))?;
    st.iter().find_map(|e| match &e.kind {
        EntityKind::Viewport(v) if v.height > 1e-12 && v.view_height > 1e-12 && v.id != 1 => Some(v.clone()),
        _ => None,
    })
}

fn chspace(s: &mut Session, hs: &[Handle], to: Option<&str>, vp: Option<Handle>) -> Result<(usize, Space, Space)> {
    let from = s.space();
    let (target, layout) = match (&from, to) {
        (Space::Model, t) => {
            let name = match t {
                Some(n) if !n.eq_ignore_ascii_case("model") => n.to_string(),
                _ => {
                    let d = s.doc()?;
                    let mut ls: Vec<_> = d.layouts.iter().collect();
                    ls.sort_by_key(|l| l.tab_order);
                    ls.first().map(|l| l.name.clone()).ok_or_else(|| other("No layout to move objects to."))?
                }
            };
            if s.doc()?.layout(&name).is_none() {
                return Err(other(format!("Layout \"{name}\" not found.")));
            }
            let real = s.doc()?.layout(&name).map(|l| l.name.clone()).unwrap_or(name);
            (Space::Paper(real.clone()), real)
        }
        (Space::Paper(n), _) => (Space::Model, n.clone()),
    };
    let m = match viewport_of(s, &layout, vp) {
        Some(v) => {
            let k = v.view_height / v.height;
            let to_paper = Mat3::translate(v.center.xy()).then_before(Mat3::scale(1.0 / k, 1.0 / k)).then_before(Mat3::translate(-v.view_center));
            if target == Space::Model { to_paper.inverse().unwrap_or(Mat3::IDENTITY) } else { to_paper }
        }
        None => Mat3::IDENTITY,
    };
    let mut n = 0;
    for h in hs {
        if curves::is_locked(s, *h) {
            continue;
        }
        let d = s.doc_mut()?;
        if d.space_of(*h).as_ref() != Some(&from) {
            continue;
        }
        let Some(e) = d.entity(*h).map(|e| (**e).clone()) else { continue };
        if matches!(e.kind, EntityKind::Viewport(_)) {
            continue;
        }
        let mut e = e;
        e.kind.transform(&m);
        d.remove_entity(*h);
        if let Some(st) = d.space_mut(&target) {
            st.push(e);
            n += 1;
        }
    }
    s.set_selection(Vec::new());
    Ok((n, from, target))
}

fn run_chspace(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let (n, _, target) = chspace(s, &hs, str_param(p, "to"), curves::handle_param(p, "viewport"))?;
    Ok(json!({ "moved": n, "to": match target { Space::Model => "model".to_string(), Space::Paper(n) => n } }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelKind {
    Reverse,
    Chspace,
    Flatten,
}

struct SelOp {
    kind: SelKind,
    sel: SelectPhase,
}

impl SelOp {
    fn new(kind: SelKind) -> Self {
        SelOp { kind, sel: SelectPhase::default() }
    }
    fn act(&self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        match self.kind {
            SelKind::Reverse => {
                let n = reverse(s, hs)?;
                s.echo(format!("{n} object(s) direction has been reversed."));
            }
            SelKind::Flatten => {
                let n = flatten(s, hs)?;
                s.echo(format!("{n} object(s) flattened."));
            }
            SelKind::Chspace => {
                let (n, from, to) = chspace(s, hs, None, None)?;
                let nm = |sp: &Space| if *sp == Space::Model { "MODEL" } else { "PAPER" };
                s.echo(format!("{n} object(s) changed from {} space to {} space.", nm(&from), nm(&to)));
            }
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for SelOp {
    fn name(&self) -> &'static str {
        match self.kind {
            SelKind::Reverse => "REVERSE",
            SelKind::Chspace => "CHSPACE",
            SelKind::Flatten => "FLATTEN",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.act(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Cancel),
            SelOutcome::Done(hs) => self.act(s, &hs),
        }
    }
}

// =====================================================================================
// NCOPY
// =====================================================================================

/// The leaf objects of a block reference in world coordinates (nested references expanded).
fn nested_leaves(d: &cadcraft_doc::Drawing, ins: &cadcraft_doc::Insert, m: &Mat3, depth: usize, out: &mut Vec<(Common, EntityKind)>) {
    if depth >= MAX_BLOCK_DEPTH || out.len() >= MAX_GEN {
        return;
    }
    let Some(b) = d.block(&ins.block) else { return };
    let mm = m.then_before(ins.transform(b.base.xy()));
    for e in b.entities.iter() {
        match &e.kind {
            EntityKind::Insert(inner) => nested_leaves(d, inner, &mm, depth + 1, out),
            EntityKind::AttDef(_) => {}
            k => {
                let mut k = k.clone();
                k.transform(&mm);
                out.push((e.common.clone(), k));
            }
        }
    }
}

fn leaves_of(s: &Session, h: Handle) -> Result<Vec<(Common, EntityKind)>> {
    let e = curves::entity(s, h)?;
    let EntityKind::Insert(ins) = &e.kind else { return Err(other("Select a block reference.")) };
    let mut out = Vec::new();
    nested_leaves(s.doc()?, ins, &Mat3::IDENTITY, 0, &mut out);
    Ok(out)
}

fn nearest_leaf(leaves: Vec<(Common, EntityKind)>, p: Vec2) -> Option<(Common, EntityKind)> {
    leaves.into_iter().min_by(|a, b| curves::kind_distance(&a.1, p).total_cmp(&curves::kind_distance(&b.1, p)))
}

fn place_copies(s: &mut Session, items: &[(Common, EntityKind)], delta: Vec2) -> Result<Vec<Handle>> {
    let space = s.space();
    let m = Mat3::translate(delta);
    let mut out = Vec::new();
    for (c, k) in items {
        let mut k = k.clone();
        k.transform(&m);
        out.push(push_entity(s, &space, c.clone(), k)?);
    }
    Ok(out)
}

fn run_ncopy(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("ncopy", "`handle` (block reference) is required"))?;
    let leaves = leaves_of(s, h)?;
    let items: Vec<(Common, EntityKind)> = match point_param(p, "pick") {
        Some(q) => nearest_leaf(leaves, q).into_iter().collect(),
        None => leaves,
    };
    if items.is_empty() {
        return Err(other("The block reference has no nested objects."));
    }
    let delta = match (point_param(p, "delta"), point_param(p, "from"), point_param(p, "to")) {
        (Some(d), _, _) => d,
        (None, Some(a), Some(b)) => b - a,
        _ => Vec2::ZERO,
    };
    let hs = place_copies(s, &items, delta)?;
    Ok(json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct NcopyM {
    items: Vec<(Common, EntityKind)>,
    selecting_done: bool,
    base: Option<Vec2>,
}

impl Interactive for NcopyM {
    fn name(&self) -> &'static str {
        "NCOPY"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.selecting_done, self.base) {
            (false, _) => Prompt::new("Select nested objects to copy or", Accept::POINT).kw(&["Settings"]),
            (true, None) => Prompt::new("Specify base point or", Accept::POINT).kw(&["Displacement", "Multiple"]).default("Displacement"),
            (true, Some(b)) => Prompt::new("Specify second point or <use first point as displacement>", Accept::POINT).base(b),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.selecting_done {
            return match i {
                Input::Point(p) => {
                    let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                    let leaf = nearest_leaf(leaves_of(s, h)?, p).ok_or_else(|| other("Nothing nested there."))?;
                    s.echo(format!("Nested object: {}", leaf.1.type_name()));
                    if self.items.len() < MAX_GEN {
                        self.items.push(leaf);
                    }
                    Ok(Step::Continue)
                }
                Input::Keyword(_) => {
                    s.echo("Insert mode: Copy (nested objects are copied as independent objects).");
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    if self.items.is_empty() {
                        return Ok(Step::Cancel);
                    }
                    self.selecting_done = true;
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            };
        }
        match (self.base, i) {
            (None, Input::Point(p)) => {
                self.base = Some(p);
                Ok(Step::Continue)
            }
            (None, Input::Enter | Input::Keyword(_)) => {
                place_copies(s, &self.items, Vec2::ZERO)?;
                Ok(Step::Done)
            }
            (Some(b), Input::Point(p)) => {
                place_copies(s, &self.items, p - b)?;
                Ok(Step::Done)
            }
            (Some(b), Input::Enter) => {
                place_copies(s, &self.items, b)?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let Some(b) = self.base else { return Vec::new() };
        let m = Mat3::translate(c - b);
        self.items
            .iter()
            .take(500)
            .map(|(_, k)| {
                let mut k = k.clone();
                k.transform(&m);
                k
            })
            .collect()
    }
}

// =====================================================================================
// FILLET extensions (arcs, circles, polylines)
// =====================================================================================

/// How a fillet operand is trimmed: the new kind and the arrival direction at the tangent point.
fn trim_for_fillet(k: &EntityKind, pick: Vec2, t: Vec2) -> Option<(Option<EntityKind>, Option<Vec2>)> {
    match k {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let keep = if (a - t).dot(pick - t) >= (b - t).dot(pick - t) { a } else { b };
            let dir = (t - keep).normalized();
            if dir == Vec2::ZERO {
                return Some((None, None));
            }
            Some((Some(if keep == a { line(a, t) } else { line(t, b) }), Some(dir)))
        }
        EntityKind::Arc(ar) => {
            let g = Arc::new(ar.center.xy(), ar.radius, ar.start, ar.end);
            let ta = g.center.angle_to(t);
            let ccw_tan = (t - g.center).perp().normalized();
            let ap = g.center.angle_to(pick);
            let keep_start_side = if g.contains_angle(ta) {
                // Keep the side of the tangent point that holds the pick.
                cadcraft_geom::angle_in_sweep(ap, g.start, ta)
            } else {
                // Extend the nearer end.
                ccw_sweep(g.end, ta) <= ccw_sweep(ta, g.start)
            };
            if keep_start_side {
                let na = Arc::new(g.center, g.radius, g.start, ta);
                Some((Some(arc(&na)), Some(ccw_tan)))
            } else {
                let na = Arc::new(g.center, g.radius, ta, g.end);
                Some((Some(arc(&na)), Some(-ccw_tan)))
            }
        }
        EntityKind::Circle(_) => Some((None, None)),
        _ => None,
    }
}

/// Fillet two curves (lines, arcs, circles) with radius `r`. Returns the new arc, if any.
pub(crate) fn fillet_curves(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, r: f64) -> Result<Option<Handle>> {
    let e1 = curves::entity(s, h1)?;
    let e2 = curves::entity(s, h2)?;
    let unsupported = || other("Fillet works on lines, arcs and circles (and polylines with the Polyline option).");
    let o1 = TanObj::from_kind(&e1.kind, p1).filter(|_| matches!(e1.kind, EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::Circle(_)));
    let o2 = TanObj::from_kind(&e2.kind, p2).filter(|_| matches!(e2.kind, EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::Circle(_)));
    let (Some(o1), Some(o2)) = (o1, o2) else { return Err(unsupported()) };
    if h1 == h2 {
        return Err(other("Cannot fillet an object to itself."));
    }
    let (t1, t2, fc) = if r <= 1e-12 {
        // Sharp corner: the intersection nearest the picks.
        let hits: Vec<Vec2> = match (o1, o2) {
            (TanObj::Line(a), TanObj::Line(b)) => line_line_infinite(a.a, a.b, b.a, b.b).map(|x| vec![x.0]).unwrap_or_default(),
            (TanObj::Line(l), TanObj::Circle(c)) | (TanObj::Circle(c), TanObj::Line(l)) => line_circle(&l, &c).into_iter().map(|x| x.0).collect(),
            (TanObj::Circle(a), TanObj::Circle(b)) => cadcraft_geom::circle_circle(&a, &b),
        };
        let x = hits
            .into_iter()
            .min_by(|a, b| (a.dist(p1) + a.dist(p2)).total_cmp(&(b.dist(p1) + b.dist(p2))))
            .ok_or_else(|| other("The objects do not intersect."))?;
        (x, x, None)
    } else {
        let c = curves::ttr(o1, p1, o2, p2, r).ok_or_else(|| other("Radius is too large."))?;
        (o1.touch(&c), o2.touch(&c), Some(c))
    };
    let (n1, d1) = trim_for_fillet(&e1.kind, p1, t1).ok_or_else(unsupported)?;
    let (n2, d2) = trim_for_fillet(&e2.kind, p2, t2).ok_or_else(unsupported)?;
    let new_arc = fc.map(|c| {
        let a1 = c.center.angle_to(t1);
        let a2 = c.center.angle_to(t2);
        let ccw1 = (t1 - c.center).perp().normalized();
        let ccw2 = (t2 - c.center).perp().normalized();
        let ccw = match (d1, d2) {
            (Some(d), _) => ccw1.dot(d) > 0.0,
            (None, Some(d)) => ccw2.dot(-d) > 0.0,
            _ => ccw_sweep(a1, a2) <= std::f64::consts::PI,
        };
        if ccw { Arc::new(c.center, c.radius, a1, a2) } else { Arc::new(c.center, c.radius, a2, a1) }
    });
    if let Some(k) = n1 {
        set_kind(s, h1, k)?;
    }
    if let Some(k) = n2 {
        set_kind(s, h2, k)?;
    }
    match new_arc {
        Some(a) => {
            let space = s.doc()?.space_of(h1).unwrap_or_else(|| s.space());
            Ok(Some(push_entity(s, &space, e1.common.clone(), arc(&a))?))
        }
        None => Ok(None),
    }
}

/// Fillet every corner between two straight segments of a polyline. Returns the count.
pub(crate) fn fillet_polyline(s: &mut Session, h: Handle, r: f64) -> Result<usize> {
    let e = curves::entity(s, h)?;
    let EntityKind::LwPolyline(pl) = &e.kind else { return Err(other("Select a 2D polyline.")) };
    let vs = &pl.vertices;
    let n = vs.len();
    if n < 3 {
        return Ok(0);
    }
    let at = |i: usize| vs.get(i % n).copied().unwrap_or_default();
    // Distance cut back from each corner (0 = not filleted).
    let mut cut = vec![0.0f64; n];
    let mut turn = vec![0.0f64; n];
    for (i, c) in cut.iter_mut().enumerate() {
        if !pl.closed && (i == 0 || i + 1 == n) {
            continue;
        }
        let prev = at(i + n - 1);
        let cur = at(i);
        let next = at(i + 1);
        if prev.bulge.abs() > 1e-12 || cur.bulge.abs() > 1e-12 {
            continue;
        }
        let u1 = (prev.p - cur.p).normalized();
        let u2 = (next.p - cur.p).normalized();
        if u1 == Vec2::ZERO || u2 == Vec2::ZERO {
            continue;
        }
        let half = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if half < 1e-9 || (std::f64::consts::FRAC_PI_2 - half).abs() < 1e-9 {
            continue;
        }
        *c = if r <= 1e-12 { 0.0 } else { r / half.tan() };
        if let Some(t) = turn.get_mut(i) {
            *t = (-u1).cross(u2).atan2((-u1).dot(u2));
        }
    }
    // Drop fillets that do not fit their segments.
    for i in 0..n {
        let j = (i + 1) % n;
        let seg = at(i).p.dist(at(j).p);
        let (ci, cj) = (cut.get(i).copied().unwrap_or(0.0), cut.get(j).copied().unwrap_or(0.0));
        if ci + cj > seg + 1e-9 {
            if let Some(x) = cut.get_mut(i) {
                *x = 0.0;
            }
            if let Some(x) = cut.get_mut(j) {
                *x = 0.0;
            }
        }
    }
    let mut out: Vec<PolyVertex> = Vec::with_capacity(n * 2);
    let mut count = 0;
    for i in 0..n {
        let v = at(i);
        let c = cut.get(i).copied().unwrap_or(0.0);
        if c <= 1e-12 {
            out.push(v);
            continue;
        }
        let prev = at(i + n - 1).p;
        let next = at(i + 1).p;
        let t1 = v.p + (prev - v.p).normalized() * c;
        let t2 = v.p + (next - v.p).normalized() * c;
        let b = cadcraft_geom::arc_to_bulge(turn.get(i).copied().unwrap_or(0.0));
        out.push(PolyVertex { p: t1, bulge: b, ..v });
        out.push(PolyVertex { p: t2, bulge: 0.0, ..v });
        count += 1;
    }
    set_kind(s, h, EntityKind::LwPolyline(LwPolyline { vertices: out, ..pl.clone() }))?;
    Ok(count)
}

// =====================================================================================
// TRIM / EXTEND for ellipses and splines
// =====================================================================================

fn ellipse_of(e: &cadcraft_doc::Ellipse) -> Ellipse {
    Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end }
}

fn ellipse_kind(e: &Ellipse) -> EntityKind {
    EntityKind::Ellipse(cadcraft_doc::Ellipse { center: v3(e.center), major: v3(e.major), ratio: e.ratio, start: e.start, end: e.end })
}

/// Intersections of a sampled curve `eval(t)` (parameters `ts`) with the cutting segments,
/// as curve parameters refined by bisection on the exact curve.
fn hits_on(eval: &dyn Fn(f64) -> Vec2, ts: &[f64], cut: &[Segment]) -> Vec<f64> {
    let mut out = Vec::new();
    for w in ts.windows(2) {
        let (Some(t0), Some(t1)) = (w.first().copied(), w.get(1).copied()) else { continue };
        let (a, b) = (eval(t0), eval(t1));
        let sg = Segment::Line(Line::new(a, b));
        for c in cut {
            for x in intersect_ext(&sg, c, false) {
                let f = Line::new(a, b).param_of(x).clamp(0.0, 1.0);
                out.push(refine_hit(eval, c, t0, t1, t0 + (t1 - t0) * f));
            }
        }
    }
    out
}

/// Signed side of `p` relative to a cutting segment (line side, or inside/outside an arc's circle).
fn side_of(c: &Segment, p: Vec2) -> f64 {
    match c {
        Segment::Line(l) => l.side(p) / l.len().max(1e-300),
        Segment::Arc { arc, .. } => p.dist(arc.center) - arc.radius,
    }
}

fn refine_hit(eval: &dyn Fn(f64) -> Vec2, c: &Segment, t0: f64, t1: f64, guess: f64) -> f64 {
    let (mut a, mut b) = (t0, t1);
    let (mut fa, fb) = (side_of(c, eval(a)), side_of(c, eval(b)));
    if !(fa.is_finite() && fb.is_finite()) || fa * fb > 0.0 {
        return guess;
    }
    for _ in 0..60 {
        let m = (a + b) / 2.0;
        let fm = side_of(c, eval(m));
        if fa * fm <= 0.0 {
            b = m;
        } else {
            a = m;
            fa = fm;
        }
    }
    (a + b) / 2.0
}

fn ellipse_samples(ge: &Ellipse) -> Vec<f64> {
    let n = 720usize;
    let sw = ge.sweep();
    (0..=n).map(|i| ge.start + sw * i as f64 / n as f64).collect()
}

/// Trim objects that modify.rs does not handle itself (ellipses and splines).
pub(crate) fn trim_other(k: &EntityKind, cut: &[Segment], pick: Vec2) -> Result<Vec<EntityKind>> {
    let none = || other("Object does not intersect a cutting edge.");
    match k {
        EntityKind::Ellipse(e) => {
            let ge = ellipse_of(e);
            let full = Ellipse { start: 0.0, end: TAU, ..ge };
            let eval = |t: f64| full.at_param(t);
            let params: Vec<f64> = hits_on(&eval, &ellipse_samples(&ge), cut).into_iter().map(norm_angle).collect();
            if ge.is_full() {
                let mut ps = params;
                ps.sort_by(f64::total_cmp);
                ps.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                if ps.len() < 2 {
                    return Err(other("Ellipse must intersect cutting edges at two or more points."));
                }
                let tp = full.param_of(pick);
                let n = ps.len();
                for i in 0..n {
                    let (Some(a0), Some(a1)) = (ps.get(i).copied(), ps.get((i + 1) % n).copied()) else { continue };
                    if cadcraft_geom::angle_in_sweep(tp, a0, a1) {
                        return Ok(vec![ellipse_kind(&Ellipse { start: a1, end: a0, ..ge })]);
                    }
                }
                return Err(none());
            }
            let sweep = ge.sweep();
            let rel = |t: f64| norm_angle(t - ge.start);
            let mut ts: Vec<f64> = params.into_iter().map(rel).filter(|t| *t > 1e-9 && *t < sweep - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            let tp = rel(full.param_of(pick)).min(sweep);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(0.0, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(sweep, f64::min);
            if lo == 0.0 && hi == sweep {
                return Err(none());
            }
            let mut v = Vec::new();
            if lo > 0.0 {
                v.push(ellipse_kind(&Ellipse { end: norm_angle(ge.start + lo), ..ge }));
            }
            if hi < sweep {
                v.push(ellipse_kind(&Ellipse { start: norm_angle(ge.start + hi), ..ge }));
            }
            Ok(v)
        }
        EntityKind::Spline(sp) => {
            if !sp.is_valid() {
                return Err(other("Invalid spline."));
            }
            let (lo_d, hi_d) = sp.domain();
            let n = 512;
            let params: Vec<f64> = (0..=n).map(|i| lo_d + (hi_d - lo_d) * i as f64 / n as f64).collect();
            let eval = |t: f64| sp.eval(t);
            let mut ts: Vec<f64> =
                hits_on(&eval, &params, cut).into_iter().filter(|t| *t > lo_d + 1e-9 * (hi_d - lo_d) && *t < hi_d - 1e-9 * (hi_d - lo_d)).collect();
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            let tp = curves::spline_param_of(sp, pick);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(lo_d, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(hi_d, f64::min);
            if lo == lo_d && hi == hi_d {
                return Err(none());
            }
            let mut v = Vec::new();
            if lo > lo_d
                && let Some(a) = curves::sub_spline(sp, lo_d, lo)
            {
                v.push(EntityKind::Spline(a));
            }
            if hi < hi_d
                && let Some(b) = curves::sub_spline(sp, hi, hi_d)
            {
                v.push(EntityKind::Spline(b));
            }
            Ok(v)
        }
        _ => Err(other("Cannot trim that object.")),
    }
}

/// Extend objects that modify.rs does not handle itself (elliptical arcs).
pub(crate) fn extend_other(k: &EntityKind, cut: &[Segment], pick: Vec2) -> Result<EntityKind> {
    match k {
        EntityKind::Ellipse(e) => {
            let ge = ellipse_of(e);
            if ge.is_full() {
                return Err(other("Cannot extend a closed ellipse."));
            }
            let full = Ellipse { start: 0.0, end: TAU, ..ge };
            let from_end = pick.dist(ge.at_param(ge.end)) < pick.dist(ge.at_param(ge.start));
            let eval = |t: f64| full.at_param(t);
            let hits: Vec<f64> = hits_on(&eval, &ellipse_samples(&full), cut)
                .into_iter()
                .map(norm_angle)
                .filter(|t| !cadcraft_geom::angle_in_sweep(*t, ge.start, ge.end))
                .collect();
            let t = if from_end {
                hits.iter().copied().min_by(|x, y| ccw_sweep(ge.end, *x).total_cmp(&ccw_sweep(ge.end, *y)))
            } else {
                hits.iter().copied().min_by(|x, y| ccw_sweep(*x, ge.start).total_cmp(&ccw_sweep(*y, ge.start)))
            };
            let t = t.ok_or_else(|| other("Object does not intersect an edge."))?;
            Ok(ellipse_kind(&if from_end { Ellipse { end: t, ..ge } } else { Ellipse { start: t, ..ge } }))
        }
        _ => Err(other("Cannot extend that object.")),
    }
}

// =====================================================================================
// ARRAYPATH (non-associative copies along a curve)
// =====================================================================================

pub(crate) fn arraypath(
    s: &mut Session,
    hs: &[Handle],
    path: Handle,
    count: Option<usize>,
    spacing: Option<f64>,
    align: bool,
) -> Result<Vec<Handle>> {
    let pk = curves::entity(s, path)?.kind;
    let c = Chain::of(&pk).ok_or_else(|| other("Select a line, arc, circle, polyline, ellipse or spline as the path."))?;
    let l = c.len();
    if l <= 1e-12 {
        return Err(other("The path has no length."));
    }
    let objs: Vec<Handle> = hs.iter().copied().filter(|h| *h != path).collect();
    if objs.is_empty() {
        return Err(other("Select objects to array."));
    }
    let (n, step) = match (count, spacing) {
        (_, Some(sp)) if sp.is_finite() && sp > 0.0 => (((l / sp).floor() as usize + 1).clamp(1, MAX_GEN), sp),
        (Some(n), _) => {
            let n = n.clamp(1, MAX_GEN);
            (n, if c.closed { l / n as f64 } else { l / (n.max(2) - 1) as f64 })
        }
        _ => (6, if c.closed { l / 6.0 } else { l / 5.0 }),
    };
    let Some((p0, t0)) = c.at_length(0.0) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for i in 1..n {
        let Some((p, t)) = c.at_length(step * i as f64) else { continue };
        let mut m = Mat3::translate(p - p0);
        if align {
            m = Mat3::rotate_about(p, t.angle() - t0.angle()).then_before(m);
        }
        out.extend(super::modify::transform_entities(s, &objs, &m, true)?);
    }
    Ok(out)
}

fn run_arraypath(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let path = curves::handle_param(p, "path").ok_or_else(|| bad("arraypath", "`path` (handle) is required"))?;
    let count = p.get("count").and_then(Value::as_u64).map(|n| n.min(MAX_GEN as u64) as usize);
    let spacing = p.get("spacing").and_then(Value::as_f64);
    let r = arraypath(s, &hs, path, count, spacing, bool_or(p, "align", true))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct PathArrayM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    path: Option<Handle>,
}

impl Interactive for PathArrayM {
    fn name(&self) -> &'static str {
        "ARRAYPATH"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        match self.path {
            None => Prompt::new("Select path curve", Accept::POINT),
            Some(_) => Prompt::new("Enter number of items along path", Accept::NUMBER).default("6"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Cancel),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    Ok(Step::Continue)
                }
            };
        }
        match (self.path, i) {
            (None, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                Chain::of(&curves::entity(s, h)?.kind).ok_or_else(|| other("Select a curve as the path."))?;
                self.path = Some(h);
                Ok(Step::Continue)
            }
            (Some(path), inp @ (Input::Text(_) | Input::Enter)) => {
                let n = match &inp {
                    Input::Text(t) => {
                        number(t).filter(|n| *n >= 1.0 && *n <= MAX_GEN as f64).ok_or_else(|| other("Requires a count of 1 or more."))? as usize
                    }
                    _ => 6,
                };
                arraypath(s, &self.objs, path, Some(n), None, true)?;
                s.set_selection(Vec::new());
                s.echo(format!("Type = Path  Associative = No ({n} items)"));
                Ok(Step::Done)
            }
            (None, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}

// =====================================================================================
// TEXTEDIT
// =====================================================================================

fn text_of(k: &EntityKind) -> Option<String> {
    match k {
        EntityKind::Text(t) => Some(t.value.clone()),
        EntityKind::AttDef(a) => Some(a.text.value.clone()),
        EntityKind::MText(t) => Some(t.contents.replace("\\P", "\n")),
        EntityKind::Dimension(d) => Some(if d.text.is_empty() { "<>".into() } else { d.text.clone() }),
        _ => None,
    }
}

pub(crate) fn set_text(s: &mut Session, h: Handle, text: &str) -> Result<()> {
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let mut k = curves::entity(s, h)?.kind;
    match &mut k {
        EntityKind::Text(t) => t.value = text.to_string(),
        EntityKind::AttDef(a) => a.text.value = text.to_string(),
        EntityKind::MText(t) => t.contents = text.replace('\n', "\\P"),
        EntityKind::Dimension(d) => {
            d.text = if text == "<>" { String::new() } else { text.to_string() };
            d.block = None;
        }
        _ => return Err(other("Select a text, mtext, attribute definition or dimension.")),
    }
    set_kind(s, h, k)
}

fn run_textedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("textedit", "`handle` is required"))?;
    let text = str_param(p, "text").ok_or_else(|| bad("textedit", "`text` is required"))?;
    if text.len() > 1_000_000 {
        return Err(bad("textedit", "text is too long"));
    }
    set_text(s, h, text)?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
struct TextEditM {
    h: Option<(Handle, String)>,
    /// Edits made in this run, newest last: each object as it was before (the Undo option).
    edits: Vec<(Handle, EntityKind)>,
    /// Single mode: end after one edit (the Mode option; Multiple by default).
    single: bool,
    /// Asking for the edit mode.
    mode: bool,
}

impl Interactive for TextEditM {
    fn name(&self) -> &'static str {
        "TEXTEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.mode {
            let current = if self.single { "Single" } else { "Multiple" };
            return Prompt::new("Enter a text edit mode option", curves::KW).kw(&["Single", "Multiple"]).default(current);
        }
        match &self.h {
            None => Prompt::new("Select an annotation object", Accept::POINT).kw(&["Undo", "Mode"]),
            Some((_, old)) => Prompt::new("Enter new text", Accept::TEXT).default(old.chars().take(40).collect::<String>()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.mode {
            match i {
                Input::Keyword(k) if k == "Single" => self.single = true,
                Input::Keyword(k) if k == "Multiple" => self.single = false,
                Input::Enter => {}
                _ => return Err(other("Enter Single or Multiple.")),
            }
            self.mode = false;
            return Ok(Step::Continue);
        }
        match (&self.h, i) {
            (None, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                let old = text_of(&curves::entity(s, h)?.kind).ok_or_else(|| other("Select a text, mtext, attribute definition or dimension."))?;
                self.h = Some((h, old));
                Ok(Step::Continue)
            }
            (Some((h, _)), Input::Text(t) | Input::Keyword(t)) => {
                let h = *h;
                let before = curves::entity(s, h)?.kind;
                set_text(s, h, &t)?;
                self.edits.push((h, before));
                self.h = None;
                Ok(if self.single { Step::Done } else { Step::Continue })
            }
            (Some(_), Input::Enter) => {
                self.h = None;
                Ok(if self.single { Step::Done } else { Step::Continue })
            }
            (None, Input::Enter) => Ok(Step::Done),
            (None, Input::Keyword(k)) if k == "Undo" => {
                match self.edits.pop() {
                    Some((h, before)) => set_kind(s, h, before)?,
                    None => s.echo("Everything has been undone"),
                }
                Ok(Step::Continue)
            }
            (None, Input::Keyword(k)) if k == "Mode" => {
                self.mode = true;
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
#[path = "modify2_tests.rs"]
mod tests;
