use super::trimext::ellipse_of;
use super::{
    Accept, Arc, Chain, EntityKind, Handle, Input, Interactive, LwPolyline, Polyline, Prompt, Result, Session, Step, TAU, Value, Vec2, bad,
    ccw_sweep, curve_edits, curves, json, number, other, point_param, set_kind, targets,
};

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
        EntityKind::Ellipse(e) => Some((ellipse_of(e).length(), None)),
        EntityKind::Spline(sp) if sp.is_valid() => Some((sp.length(), None)),
        _ => Chain::of(k).map(|c| (c.len(), None)),
    }
}

/// Lengthen a line, arc, open polyline, elliptical arc or open spline at the end nearest `pick`.
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
            let mut out = l.clone();
            if at_b {
                out.b = ne.to3(l.b.z);
            } else {
                out.a = ne.to3(l.a.z);
            }
            Ok(EntityKind::Line(out))
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
            let na = if at_end { Arc::new(g.center, g.radius, g.start, g.start + ns) } else { Arc::new(g.center, g.radius, g.end - ns, g.end) };
            Ok(EntityKind::Arc(cadcraft_doc::Arc { radius: na.radius, start: na.start, end: na.end, ..ar.clone() }))
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
        EntityKind::Ellipse(e) => curve_edits::lengthen_ellipse(e, pick, mode),
        EntityKind::Spline(sp) => curve_edits::lengthen_spline(sp, pick, mode),
        _ => Err(other("Select a line, arc, open polyline, elliptical arc or open spline.")),
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

pub(super) fn run_lengthen(s: &mut Session, p: &Value) -> Result<Value> {
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
pub(super) struct LengthenM {
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
        let au = s.angle_settings();
        let ang = |i: &Input| -> Option<f64> { if let Input::Text(t) = i { au.amount(t) } else { None } };
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
