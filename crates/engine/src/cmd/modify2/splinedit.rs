//! SPLINEDIT: Close/Open, Join, Fit data (Add, Close/Open, Delete, Kink, Move, Purge, Tangents,
//! toLerance), Edit vertex (Add, Delete, Elevate order, Move, Weight), convert to Polyline,
//! Reverse and Undo.
//!
//! Fit-point edits rebuild the curve from the edited fit points with the spline's knot
//! parametrisation, end tangents and fit tolerance. Control-point edits change the curve
//! directly and drop the fit data, as the curve no longer follows it.

use super::super::{point_value, points_param};
use super::{
    Accept, Entity, EntityKind, Handle, Input, Interactive, LwPolyline, MAX_GEN, PolyVertex, Prompt, Result, SelOutcome, SelectPhase, Session, Space,
    Spline, Step, Value, Vec2, bad, bool_or, curves, f64_req, json, line, number, other, point_req, set_kind, str_param, targets,
};

/// The spline after a JSON SPLINEDIT option (`option` plus that option's parameters).
pub(super) fn run_splinedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("splinedit", "`handle` is required"))?;
    let e = curves::entity(s, h)?;
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let EntityKind::Spline(sp) = &e.kind else { return Err(other("Object selected is not a spline.")) };
    let opt = str_param(p, "option").ok_or_else(|| bad("splinedit", "`option` is required"))?.to_ascii_lowercase();
    if opt == "join" {
        let others = targets(s, &json!({ "handles": p.get("handles2").cloned().unwrap_or(Value::Null) }))?;
        let removed = join(s, h, sp, &others)?;
        return Ok(json!({ "handle": h.hex(), "joined": removed.len() }));
    }
    let k = edit(sp, &opt, p)?;
    set_kind(s, h, k)?;
    Ok(json!({ "handle": h.hex() }))
}

fn index(p: &Value) -> Result<usize> {
    p.get("index").and_then(Value::as_u64).map(|i| i as usize).ok_or_else(|| bad("splinedit", "`index` is required"))
}

/// An optional direction: absent or null is `None` (the system default).
fn dir_param(p: &Value, key: &str) -> Result<Option<Vec2>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let d = point_value(v).filter(|d| d.len() > 1e-12).ok_or_else(|| bad("splinedit", format!("`{key}` must be a direction [dx, dy]")))?;
            Ok(Some(d.normalized()))
        }
    }
}

fn edit(sp: &Spline, opt: &str, p: &Value) -> Result<EntityKind> {
    let spline = |s: Spline| Ok(EntityKind::Spline(s));
    match opt {
        "close" => spline(spline_close(sp)),
        "open" => spline(spline_open(sp)),
        "reverse" => spline(curves::reverse_spline(sp)),
        "purge" => spline(without_fit(sp.clone())),
        "refit" => {
            let fit = points_param(p, "fit").unwrap_or_else(|| sp.fit.clone());
            if fit.len() < 2 || fit.len() > MAX_GEN {
                return Err(other("The spline has no fit data; give `fit` points."));
            }
            spline(sp.refit(&fit, sp.closed))
        }
        "polyline" => {
            let precision = p.get("precision").and_then(Value::as_u64).unwrap_or(10).min(99) as usize;
            Ok(EntityKind::LwPolyline(spline_to_poly(sp, precision)))
        }
        "move" => {
            let (i, to) = (index(p)?, point_req("splinedit", p, "to")?);
            if sp.fit.is_empty() { spline(move_control(sp, i, to)?) } else { spline(move_fit(sp, i, to)?) }
        }
        "fitmove" => spline(move_fit(sp, index(p)?, point_req("splinedit", p, "to")?)?),
        "fitadd" => spline(add_fit(sp, index(p)?, point_req("splinedit", p, "to")?, bool_or(p, "before", false))?),
        "fitdelete" => spline(delete_fit(sp, index(p)?)?),
        "kink" => spline(kink(sp, point_req("splinedit", p, "at")?)?),
        "tangents" => spline(tangents(sp, dir_param(p, "start")?, dir_param(p, "end")?)?),
        "tolerance" => {
            let tol = f64_req("splinedit", p, "tolerance")?;
            spline(tolerance(sp, tol)?)
        }
        "vertexmove" => spline(move_control(sp, index(p)?, point_req("splinedit", p, "to")?)?),
        "vertexdelete" => spline(delete_control(sp, index(p)?)?),
        "weight" => spline(set_weight(sp, index(p)?, f64_req("splinedit", p, "weight")?)?),
        "vertexadd" => spline(add_control(sp, point_req("splinedit", p, "at")?)?),
        "elevate" => {
            let order = p.get("order").and_then(Value::as_u64).ok_or_else(|| bad("splinedit", "`order` is required"))?;
            spline(elevate(sp, order)?)
        }
        _ => Err(bad("splinedit", "unknown option")),
    }
}

// ---------------- close / open / convert ----------------

fn spline_close(sp: &Spline) -> Spline {
    if sp.closed {
        return sp.clone();
    }
    let mut out = if sp.fit.len() >= 2 {
        sp.refit(&sp.fit, true)
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
    if sp.fit.len() >= 2 { sp.refit(&trim(&sp.fit), false) } else { Spline::from_control(trim(&sp.control), sp.degree) }
}

fn spline_to_poly(sp: &Spline, precision: usize) -> LwPolyline {
    let (lo, hi) = sp.domain();
    let n = (precision.clamp(1, 99) * sp.control.len().max(2)).min(MAX_GEN);
    let mut vs = (0..=n).map(|i| PolyVertex::new(sp.eval(lo + (hi - lo) * i as f64 / n as f64))).collect::<Vec<_>>();
    if sp.closed && vs.len() > 2 {
        vs.pop();
    }
    LwPolyline { vertices: vs, closed: sp.closed, const_width: 0.0, elevation: 0.0, plinegen: false }
}

// ---------------- fit data ----------------

fn need_fit(sp: &Spline) -> Result<()> {
    if sp.fit.is_empty() { Err(other("The spline has no fit data.")) } else { Ok(()) }
}

fn without_fit(sp: Spline) -> Spline {
    Spline { fit: Vec::new(), fit_opts: Default::default(), ..sp }
}

fn move_fit(sp: &Spline, i: usize, to: Vec2) -> Result<Spline> {
    need_fit(sp)?;
    let mut fit = sp.fit.clone();
    let f = fit.get_mut(i).ok_or_else(|| bad("splinedit", "fit point index out of range"))?;
    // A kink (a repeated point) moves as one.
    let at = *f;
    for q in fit.iter_mut().skip(i).take_while(|q| q.near(at, 1e-12)) {
        *q = to;
    }
    Ok(sp.refit(&fit, sp.closed))
}

/// The index after fit point `i` and its repeats (a kink).
fn after(fit: &[Vec2], i: usize) -> usize {
    let Some(at) = fit.get(i) else { return fit.len() };
    i + 1 + fit.iter().skip(i + 1).take_while(|q| q.near(*at, 1e-12)).count()
}

/// A new fit point after (or before) fit point `i`.
fn add_fit(sp: &Spline, i: usize, to: Vec2, before: bool) -> Result<Spline> {
    need_fit(sp)?;
    if i >= sp.fit.len() {
        return Err(bad("splinedit", "fit point index out of range"));
    }
    if sp.fit.len() >= MAX_GEN {
        return Err(other("Too many fit points."));
    }
    let mut fit = sp.fit.clone();
    // After a kink means after both copies of its point.
    let at = if before { i } else { after(&fit, i) };
    fit.insert(at.min(fit.len()), to);
    Ok(sp.refit(&fit, sp.closed))
}

/// Remove fit point `i` (both copies of a kink).
fn delete_fit(sp: &Spline, i: usize) -> Result<Spline> {
    need_fit(sp)?;
    let at = *sp.fit.get(i).ok_or_else(|| bad("splinedit", "fit point index out of range"))?;
    let fit: Vec<Vec2> = sp.fit.iter().copied().filter(|q| !q.near(at, 1e-12)).collect();
    let min = if sp.closed { 3 } else { 2 };
    if fit.len() < min {
        return Err(other(format!("A spline needs at least {min} fit points.")));
    }
    Ok(sp.refit(&fit, sp.closed))
}

/// A fit point with a corner (no tangent or curvature continuity) at the point of the curve
/// nearest `at`.
fn kink(sp: &Spline, at: Vec2) -> Result<Spline> {
    need_fit(sp)?;
    if sp.fit.len() + 2 > MAX_GEN {
        return Err(other("Too many fit points."));
    }
    let t = curves::spline_param_of(sp, at);
    let q = sp.eval(t);
    let mut fit = sp.fit.clone();
    if let Some(i) = fit.iter().position(|f| f.near(q, 1e-9)) {
        let f = fit.get(i).copied().unwrap_or(q);
        if fit.get(i + 1).is_none_or(|n| !n.near(f, 1e-12)) {
            fit.insert(i, f);
        }
    } else {
        // Next to the nearest fit point, on the side the parameters put it.
        let i = nearest(&sp.fit, q).unwrap_or(0);
        let ti = sp.fit.get(i).map_or(t, |f| curves::spline_param_of(sp, *f));
        let at = if t > ti { after(&sp.fit, i) } else { i };
        let at = at.max(usize::from(!sp.closed)).min(fit.len().saturating_sub(usize::from(!sp.closed)));
        fit.splice(at..at, [q, q]);
    }
    Ok(sp.refit(&fit, sp.closed))
}

fn tangents(sp: &Spline, start: Option<Vec2>, end: Option<Vec2>) -> Result<Spline> {
    need_fit(sp)?;
    if sp.closed {
        return Err(other("A closed spline has no end tangents."));
    }
    let mut s = sp.clone();
    s.fit_opts.start_tangent = start;
    s.fit_opts.end_tangent = end;
    Ok(s.refit(&sp.fit, false))
}

fn tolerance(sp: &Spline, tol: f64) -> Result<Spline> {
    need_fit(sp)?;
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(other("Requires a non-negative fit tolerance."));
    }
    let mut s = sp.clone();
    s.fit_opts.tolerance = tol;
    Ok(s.refit(&sp.fit, sp.closed))
}

// ---------------- control points ----------------

/// The number of distinct control points: a periodic spline repeats its first `degree` at the
/// end, and a clamped closed one repeats its first at the end.
fn distinct(sp: &Spline) -> usize {
    let n = sp.control.len();
    if sp.is_periodic() {
        n.saturating_sub(sp.degree)
    } else if sp.closed && n > 2 && sp.control.first().zip(sp.control.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
        n - 1
    } else {
        n
    }
}

/// Indices holding distinct control point `j` (with its repeats).
fn copies(sp: &Spline, j: usize) -> Vec<usize> {
    let (n, u) = (sp.control.len(), distinct(sp));
    if j >= u {
        return Vec::new();
    }
    let mut out = vec![j];
    if u < n {
        out.extend((j + u..n).step_by(u.max(1)));
    }
    out
}

fn move_control(sp: &Spline, j: usize, to: Vec2) -> Result<Spline> {
    let ix = copies(sp, j);
    if ix.is_empty() {
        return Err(bad("splinedit", "control point index out of range"));
    }
    let mut s = without_fit(sp.clone());
    for i in ix {
        if let Some(c) = s.control.get_mut(i) {
            *c = to;
        }
    }
    Ok(s)
}

fn set_weight(sp: &Spline, j: usize, w: f64) -> Result<Spline> {
    if !(w.is_finite() && w > 0.0) {
        return Err(other("Requires a positive weight."));
    }
    let ix = copies(sp, j);
    if ix.is_empty() {
        return Err(bad("splinedit", "control point index out of range"));
    }
    let mut s = without_fit(sp.clone());
    if s.weights.len() != s.control.len() {
        s.weights = vec![1.0; s.control.len()];
    }
    for i in ix {
        if let Some(x) = s.weights.get_mut(i) {
            *x = w;
        }
    }
    if s.weights.iter().all(|x| *x == 1.0) {
        s.weights.clear();
    }
    Ok(s)
}

/// Remove distinct control point `j` and one knot near it, so the rest of the curve keeps its
/// shape as far as it can.
fn delete_control(sp: &Spline, j: usize) -> Result<Spline> {
    let p = sp.degree;
    let (n, u) = (sp.control.len(), distinct(sp));
    if j >= u {
        return Err(bad("splinedit", "control point index out of range"));
    }
    if !sp.is_valid() {
        return Err(other("The spline is invalid."));
    }
    let mut s = without_fit(sp.clone());
    let w = |s: &Spline, i: usize| s.weights.get(i).copied().unwrap_or(1.0);
    if sp.is_periodic() {
        if u <= p + 1 {
            return Err(other(format!("A closed spline of degree {p} needs more than {} control points.", p + 1)));
        }
        // Breakpoints of one period; drop one near the point and extend periodically again.
        let mut b: Vec<f64> = sp.knots.get(p..=p + u).ok_or_else(|| other("The spline is invalid."))?.to_vec();
        let period = b.last().copied().unwrap_or(1.0) - b.first().copied().unwrap_or(0.0);
        let r = ((j + p.div_ceil(2)) % u).clamp(1, u - 1);
        b.remove(r);
        let m = u - 1;
        let knot = |i: isize| {
            let (q, r) = (i.div_euclid(m as isize), i.rem_euclid(m as isize) as usize);
            b.get(r).copied().unwrap_or(0.0) + q as f64 * period
        };
        s.knots = (0..m + 2 * p + 1).map(|i| knot(i as isize - p as isize)).collect();
        let mut ctrl: Vec<Vec2> = sp.control.iter().take(u).copied().collect();
        ctrl.remove(j);
        let rational = !sp.weights.is_empty();
        let mut ws: Vec<f64> = (0..u).map(|i| w(sp, i)).collect();
        ws.remove(j);
        for i in 0..p {
            let (c, x) = (ctrl.get(i).copied().unwrap_or_default(), ws.get(i).copied().unwrap_or(1.0));
            ctrl.push(c);
            ws.push(x);
        }
        s.control = ctrl;
        s.weights = if rational { ws } else { Vec::new() };
    } else {
        if n <= p + 1 {
            return Err(other(format!("A spline of degree {p} needs at least {} control points.", p + 1)));
        }
        if u < n && (j == 0 || j + 1 >= n) {
            return Err(other("The start point of a closed spline can't be deleted."));
        }
        s.control.remove(j);
        if !s.weights.is_empty() {
            s.weights.remove(j.min(s.weights.len() - 1));
        }
        s.knots.remove((j + p.div_ceil(2)).clamp(p + 1, n - 1));
    }
    if !s.is_valid() {
        return Err(other("Cannot delete that control point."));
    }
    Ok(s)
}

// ---------------- join / add / elevate ----------------

/// Join open curves that connect end to end with the spline into it (JOIN's spline rules: the
/// source keeps its handle, the others are erased). Returns the erased objects with their
/// spaces, so an interactive Undo can bring them back.
fn join(s: &mut Session, h: Handle, sp: &Spline, others: &[Handle]) -> Result<Vec<(Space, Entity)>> {
    if sp.closed {
        return Err(other("Cannot join to a closed spline."));
    }
    let mut hs = vec![h];
    hs.extend(others.iter().copied().filter(|o| *o != h));
    if hs.len() < 2 {
        return Err(other("Select objects to join to the spline."));
    }
    let d = s.doc()?;
    let before: Vec<(Space, Entity)> = hs.iter().skip(1).filter_map(|o| Some((d.space_of(*o)?, (**d.entity(*o)?).clone()))).collect();
    super::join_curves(s, &hs)?;
    let d = s.doc()?;
    Ok(before.into_iter().filter(|(_, e)| d.entity(e.handle).is_none()).collect())
}

/// A control point added by inserting a knot where the curve passes nearest `at`; the curve
/// keeps its shape. A periodic spline is clamped at its seam first.
fn add_control(sp: &Spline, at: Vec2) -> Result<Spline> {
    let base = if sp.is_periodic() { sp.clamped().ok_or_else(|| other("Cannot add a control point to this spline."))? } else { sp.clone() };
    let t = curves::spline_param_of(&base, at);
    let mut out = base.insert_knot(t).ok_or_else(|| other("Cannot add a control point to this spline."))?;
    out.closed = sp.closed;
    Ok(out)
}

/// The same curve at a higher order (degree + 1), with more control points.
fn elevate(sp: &Spline, order: u64) -> Result<Spline> {
    // Degree 25 is the most `Spline::elevated` builds.
    let max = 26;
    let current = sp.degree as u64 + 1;
    if order < current || order > max {
        return Err(other(format!("Requires an order between {current} and {max}.")));
    }
    if order == current {
        return Ok(sp.clone());
    }
    let mut out = sp.elevated(order as usize - 1).ok_or_else(|| other("Cannot elevate the order of this spline."))?;
    out.closed = sp.closed;
    Ok(out)
}

// ---------------- interactive ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sub {
    #[default]
    Main,
    Precision,
    Fit,
    FitAddPick,
    FitAddNew,
    FitDelete,
    FitKink,
    FitMove,
    FitStartTangent,
    FitEndTangent,
    FitTolerance,
    Vertex,
    VertexDelete,
    VertexMove,
    VertexWeight,
    VertexAdd,
    VertexOrder,
    Join,
    /// "Select point" from FitMove / VertexMove / VertexWeight (in `back`).
    SelectPoint,
}

#[derive(Default)]
pub(super) struct SplineditM {
    h: Option<Handle>,
    sub: Sub,
    back: Sub,
    history: Vec<EntityKind>,
    /// Marked fit point (Fit data) or distinct control point (Edit vertex).
    mark: usize,
    before: bool,
    start_tangent: Option<Vec2>,
    join: SelectPhase,
    /// Objects erased by Join, with the history length after it (restored by Undo).
    joined: Vec<(usize, Vec<(Space, Entity)>)>,
}

fn nearest(pts: &[Vec2], p: Vec2) -> Option<usize> {
    pts.iter().enumerate().min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p))).map(|(i, _)| i)
}

impl SplineditM {
    fn spline(&self, s: &Session) -> Option<Spline> {
        match &s.doc().ok()?.entity(self.h?)?.kind {
            EntityKind::Spline(sp) => Some(sp.clone()),
            _ => None,
        }
    }

    /// Points the marker walks: fit points in the fit data menus, else distinct control points.
    fn marks(&self, s: &Session) -> Vec<Vec2> {
        let Some(sp) = self.spline(s) else { return Vec::new() };
        let fit = matches!(self.sub, Sub::FitMove | Sub::FitAddPick | Sub::FitDelete) || (self.sub == Sub::SelectPoint && self.back == Sub::FitMove);
        if fit {
            sp.fit.clone()
        } else {
            let u = distinct(&sp);
            sp.control.into_iter().take(u).collect()
        }
    }

    fn apply(&mut self, s: &mut Session, p: Value) -> Result<()> {
        let h = self.h.ok_or_else(|| other("No spline."))?;
        let before = curves::entity(s, h)?.kind;
        let mut p = p;
        if let Some(o) = p.as_object_mut() {
            o.insert("handle".into(), json!(h.hex()));
        }
        run_splinedit(s, &p)?;
        self.history.push(before);
        Ok(())
    }

    /// Next / Previous / Select point / eXit at the marker prompts; `true` when handled.
    fn step_mark(&mut self, s: &Session, k: &str) -> bool {
        let n = self.marks(s).len().max(1);
        match k {
            "next" | "n" => self.mark = (self.mark + 1) % n,
            "previous" | "p" => self.mark = (self.mark + n - 1) % n,
            "select point" | "s" => {
                self.back = self.sub;
                self.sub = Sub::SelectPoint;
            }
            "exit" | "x" => self.sub = if self.sub == Sub::FitMove { Sub::Fit } else { Sub::Vertex },
            _ => return false,
        }
        true
    }
}

impl SplineditM {
    fn input_main(&mut self, s: &mut Session, h: Handle, sp: &Spline, enter: bool, kw: &str) -> Result<Option<Step>> {
        match (enter, kw) {
            (true, _) | (_, "exit" | "x") => {
                s.set_selection(Vec::new());
                return Ok(Some(Step::Done));
            }
            (_, "close" | "c") => self.apply(s, json!({ "option": "close" }))?,
            (_, "open" | "o") => self.apply(s, json!({ "option": "open" }))?,
            (_, "reverse" | "r") => self.apply(s, json!({ "option": "reverse" }))?,
            (_, "convert to polyline" | "p") => self.sub = Sub::Precision,
            (_, "fit data" | "f") if sp.fit.is_empty() => s.echo("The spline has no fit data."),
            (_, "fit data" | "f") => self.sub = Sub::Fit,
            (_, "edit vertex" | "e") => {
                self.mark = 0;
                self.sub = Sub::Vertex;
            }
            (_, "join" | "j") if sp.closed => s.echo("Cannot join to a closed spline."),
            (_, "join" | "j") => {
                self.join = SelectPhase::default();
                self.sub = Sub::Join;
            }
            (_, "undo" | "u") => {
                if let Some((at, ents)) = self.joined.last()
                    && *at == self.history.len()
                {
                    let d = s.doc_mut()?;
                    for (space, e) in ents {
                        if let Some(st) = d.space_mut(space) {
                            st.push(e.clone());
                        }
                    }
                    self.joined.pop();
                }
                match self.history.pop() {
                    Some(k) => set_kind(s, h, k)?,
                    None => s.echo("All edits have been undone."),
                }
            }
            (_, "") => {}
            _ => return Err(other("Invalid option keyword.")),
        }
        Ok(None)
    }

    fn input_fit(&mut self, s: &mut Session, sp: &Spline, enter: bool, kw: &str) -> Result<()> {
        match (enter, kw) {
            (true, _) | (_, "exit" | "x") => self.sub = Sub::Main,
            (_, "add" | "a") => self.sub = Sub::FitAddPick,
            (_, "close" | "c") => self.apply(s, json!({ "option": "close" }))?,
            (_, "open" | "o") => self.apply(s, json!({ "option": "open" }))?,
            (_, "delete" | "d") => self.sub = Sub::FitDelete,
            (_, "kink" | "k") => self.sub = Sub::FitKink,
            (_, "move" | "m") => {
                self.mark = 0;
                self.sub = Sub::FitMove;
            }
            (_, "purge" | "p") => {
                self.apply(s, json!({ "option": "purge" }))?;
                self.sub = Sub::Main;
            }
            (_, "tangents" | "t") if sp.closed => s.echo("A closed spline has no end tangents."),
            (_, "tangents" | "t") => self.sub = Sub::FitStartTangent,
            (_, "tolerance" | "l") => self.sub = Sub::FitTolerance,
            (_, "") => {}
            _ => return Err(other("Invalid option keyword.")),
        }
        Ok(())
    }

    fn input_join(&mut self, s: &mut Session, h: Handle, sp: &Spline, input: &Input) -> Result<()> {
        match self.join.feed(s, input)? {
            SelOutcome::More => {}
            SelOutcome::Empty => self.sub = Sub::Main,
            SelOutcome::Done(handles) => {
                self.sub = Sub::Main;
                let before = curves::entity(s, h)?.kind;
                let removed = join(s, h, sp, &handles)?;
                self.history.push(before);
                s.echo(format!("{} object(s) joined into the spline", removed.len()));
                self.joined.push((self.history.len(), removed));
                s.set_selection(vec![h]);
            }
        }
        Ok(())
    }
}

impl Interactive for SplineditM {
    fn name(&self) -> &'static str {
        "SPLINEDIT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let Some(sp) = self.spline(s) else { return Prompt::new("Select spline", Accept::POINT) };
        let close = if sp.closed { "Open" } else { "Close" };
        let marker = ["Next", "Previous", "Select point", "eXit"];
        match self.sub {
            Sub::Main => {
                let mut kw = vec![close, "Join"];
                if !sp.fit.is_empty() {
                    kw.push("Fit data");
                }
                kw.extend(["Edit vertex", "convert to Polyline", "Reverse", "Undo", "eXit"]);
                Prompt::new("Enter an option", curves::KW).kw(&kw).default("eXit")
            }
            Sub::Precision => Prompt::new("Specify a precision", Accept::NUMBER).default("10"),
            Sub::Fit => Prompt::new("Enter a fit data option", curves::KW)
                .kw(&["Add", close, "Delete", "Kink", "Move", "Purge", "Tangents", "toLerance", "eXit"])
                .default("eXit"),
            Sub::FitAddPick | Sub::FitDelete => Prompt::new("Specify existing fit point <exit>", Accept::POINT),
            Sub::FitAddNew => {
                Prompt::new("Specify new fit point <exit>", Accept::POINT).kw(&["After", "Before"]).base_opt(sp.fit.get(self.mark).copied())
            }
            Sub::FitKink => Prompt::new("Specify new point on spline <exit>", Accept::POINT),
            Sub::FitMove => Prompt::new("Specify new location", Accept::POINT).kw(&marker).default("N").base_opt(sp.fit.get(self.mark).copied()),
            Sub::FitStartTangent => Prompt::new("Specify start tangent", Accept::POINT).kw(&["System default"]).base_opt(sp.fit.first().copied()),
            Sub::FitEndTangent => Prompt::new("Specify end tangent", Accept::POINT).kw(&["System default"]).base_opt(sp.fit.last().copied()),
            Sub::FitTolerance => Prompt::new("Enter fit tolerance", Accept::NUMBER).default(format!("{:.4}", sp.fit_opts.tolerance)),
            Sub::Vertex => Prompt::new("Enter a vertex editing option", curves::KW)
                .kw(&["Add", "Delete", "Elevate order", "Move", "Weight", "eXit"])
                .default("eXit"),
            Sub::VertexDelete => Prompt::new("Specify control point <exit>", Accept::POINT),
            Sub::VertexAdd => Prompt::new("Specify a point on the spline <exit>", Accept::POINT),
            Sub::VertexOrder => Prompt::new("Enter new order", Accept::NUMBER).default((sp.degree + 1).to_string()),
            Sub::Join => {
                let mut p = self.join.prompt();
                p.message = "Select objects to join to the spline".into();
                p
            }
            Sub::VertexMove => {
                Prompt::new("Specify new location", Accept::POINT).kw(&marker).default("N").base_opt(sp.control.get(self.mark).copied())
            }
            Sub::VertexWeight => {
                let w = sp.weights.get(self.mark).copied().unwrap_or(1.0);
                Prompt::new(format!("Enter new weight (current = {w:.4})"), Accept::NUMBER).kw(&marker).default("N")
            }
            Sub::SelectPoint => Prompt::new(if self.back == Sub::FitMove { "Specify fit point" } else { "Specify control point" }, Accept::POINT),
        }
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
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
            _ => None,
        };
        let kw = k.as_deref().unwrap_or("");
        let point = match &i {
            Input::Point(p) => Some(*p),
            _ => None,
        };
        let enter = i == Input::Enter;
        let sp = self.spline(s).ok_or_else(|| other("The spline was erased."))?;
        match self.sub {
            Sub::Main => {
                if let Some(step) = self.input_main(s, h, &sp, enter, kw)? {
                    return Ok(step);
                }
            }
            Sub::Join => self.input_join(s, h, &sp, &i)?,
            Sub::Precision => {
                let precision = match &i {
                    Input::Text(t) => number(t).filter(|n| (0.0..=99.0).contains(n)).ok_or_else(|| other("Requires an integer between 0 and 99."))?,
                    _ => 10.0,
                };
                self.apply(s, json!({ "option": "polyline", "precision": precision.round() as u64 }))?;
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            Sub::Fit => self.input_fit(s, &sp, enter, kw)?,
            Sub::FitAddPick => match point {
                Some(p) => {
                    self.mark = nearest(&sp.fit, p).ok_or_else(|| other("The spline has no fit data."))?;
                    self.before = false;
                    self.sub = Sub::FitAddNew;
                }
                None if enter => self.sub = Sub::Fit,
                None => {}
            },
            Sub::FitAddNew => match (point, kw) {
                (Some(p), _) => {
                    self.apply(s, json!({ "option": "fitadd", "index": self.mark, "to": [p.x, p.y], "before": self.before }))?;
                    // Keep adding after the new point.
                    if let Some(sp) = self.spline(s) {
                        self.mark = nearest(&sp.fit, p).unwrap_or(self.mark);
                    }
                    self.before = false;
                }
                (None, "after" | "a") => self.before = false,
                (None, "before" | "b") => self.before = true,
                _ if enter => self.sub = Sub::FitAddPick,
                _ => {}
            },
            Sub::FitDelete => match point {
                Some(p) => {
                    let i = nearest(&sp.fit, p).ok_or_else(|| other("The spline has no fit data."))?;
                    self.apply(s, json!({ "option": "fitdelete", "index": i }))?;
                }
                None if enter => self.sub = Sub::Fit,
                None => {}
            },
            Sub::FitKink => match point {
                Some(p) => self.apply(s, json!({ "option": "kink", "at": [p.x, p.y] }))?,
                None if enter => self.sub = Sub::Fit,
                None => {}
            },
            Sub::FitMove | Sub::VertexMove => match point {
                Some(p) => {
                    let opt = if self.sub == Sub::FitMove { "fitmove" } else { "vertexmove" };
                    self.apply(s, json!({ "option": opt, "index": self.mark, "to": [p.x, p.y] }))?;
                }
                None if enter => {
                    self.step_mark(s, "next");
                }
                None => {
                    if !self.step_mark(s, kw) && !kw.is_empty() {
                        return Err(other("Invalid option keyword."));
                    }
                }
            },
            Sub::FitStartTangent | Sub::FitEndTangent => {
                let end = self.sub == Sub::FitEndTangent;
                let from = if end { sp.fit.last() } else { sp.fit.first() }.copied().unwrap_or_default();
                let dir = match (point, kw) {
                    (Some(p), _) => {
                        let d = p - from;
                        if d.len() <= 1e-12 {
                            return Err(other("The tangent needs a point away from the end point."));
                        }
                        Some(d)
                    }
                    (None, "system default" | "s") => None,
                    _ if enter => None,
                    _ => return Ok(Step::Continue),
                };
                if end {
                    let start = self.start_tangent.map(|d| [d.x, d.y]);
                    self.apply(s, json!({ "option": "tangents", "start": start, "end": dir.map(|d| [d.x, d.y]) }))?;
                    self.sub = Sub::Fit;
                } else {
                    self.start_tangent = dir;
                    self.sub = Sub::FitEndTangent;
                }
            }
            Sub::FitTolerance => {
                if let Input::Text(t) = &i {
                    let tol = number(t).filter(|v| *v >= 0.0).ok_or_else(|| other("Requires a non-negative fit tolerance."))?;
                    self.apply(s, json!({ "option": "tolerance", "tolerance": tol }))?;
                }
                self.sub = Sub::Fit;
            }
            Sub::Vertex => match (enter, kw) {
                (true, _) | (_, "exit" | "x") => self.sub = Sub::Main,
                (_, "delete" | "d") => self.sub = Sub::VertexDelete,
                (_, "move" | "m") => {
                    self.mark = 0;
                    self.sub = Sub::VertexMove;
                }
                (_, "weight" | "w") => {
                    self.mark = 0;
                    self.sub = Sub::VertexWeight;
                }
                (_, "add" | "a") => self.sub = Sub::VertexAdd,
                (_, "elevate order" | "e") => self.sub = Sub::VertexOrder,
                (_, "") => {}
                _ => return Err(other("Invalid option keyword.")),
            },
            Sub::VertexAdd => match point {
                Some(p) => self.apply(s, json!({ "option": "vertexadd", "at": [p.x, p.y] }))?,
                None if enter => self.sub = Sub::Vertex,
                None => {}
            },
            Sub::VertexOrder => {
                if let Input::Text(t) = &i {
                    let order =
                        number(t).filter(|o| o.fract() == 0.0 && *o >= 1.0 && *o <= 1000.0).ok_or_else(|| other("Requires a whole number."))?;
                    self.apply(s, json!({ "option": "elevate", "order": order as u64 }))?;
                }
                self.sub = Sub::Vertex;
            }
            Sub::VertexDelete => match point {
                Some(p) => {
                    let u = distinct(&sp);
                    let j = nearest(sp.control.get(..u).unwrap_or_default(), p).ok_or_else(|| other("The spline has no control points."))?;
                    self.apply(s, json!({ "option": "vertexdelete", "index": j }))?;
                }
                None if enter => self.sub = Sub::Vertex,
                None => {}
            },
            Sub::VertexWeight => match &i {
                Input::Text(t) if number(t).is_some() => {
                    let w = number(t).filter(|w| *w > 0.0).ok_or_else(|| other("Requires a positive weight."))?;
                    self.apply(s, json!({ "option": "weight", "index": self.mark, "weight": w }))?;
                }
                Input::Enter => {
                    self.step_mark(s, "next");
                }
                _ => {
                    if !self.step_mark(s, kw) && !kw.is_empty() {
                        return Err(other("Invalid option keyword."));
                    }
                }
            },
            Sub::SelectPoint => {
                if let Some(p) = point {
                    self.sub = self.back;
                    self.mark = nearest(&self.marks(s), p).unwrap_or(0);
                } else if enter {
                    self.sub = self.back;
                }
            }
        }
        Ok(Step::Continue)
    }

    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let marked = matches!(self.sub, Sub::FitMove | Sub::VertexMove | Sub::VertexWeight | Sub::FitAddNew);
        let Some(v) = marked.then(|| self.marks(s).get(self.mark).copied()).flatten() else { return Vec::new() };
        let k = s.pixel_size() * 8.0;
        let mut out = vec![line(v - Vec2::new(k, k), v + Vec2::new(k, k)), line(v - Vec2::new(k, -k), v + Vec2::new(k, -k))];
        if matches!(self.sub, Sub::FitMove | Sub::VertexMove | Sub::FitAddNew) {
            out.push(line(v, c));
        }
        out
    }
}
