use super::{
    Accept, Arc, EngineError, Entity, EntityKind, Handle, Input, Interactive, Line, PolyVertex, Prompt, Result, Session, Step, Value, Vec2, arc, bad,
    bool_or, ccw_sweep, f64_or, json, line, line_line_infinite, number, point_param,
};

// ---------------- fillet / chamfer ----------------

/// What CHAMFER takes (as in AutoCAD, not arcs, circles, ellipses or splines).
const CHAMFER_LINES_ONLY: &str =
    "CHAMFER works on lines (and polylines with the Polyline option); it cannot chamfer arcs, circles, ellipses or splines.";

fn as_line(e: &Entity) -> Option<Line> {
    match &e.kind {
        EntityKind::Line(l) => Some(Line::new(l.a.xy(), l.b.xy())),
        _ => None,
    }
}

/// Fillet (or chamfer) two lines. `p1`/`p2` mark the kept side of each line.
pub(crate) fn fillet_lines(
    s: &mut Session,
    h1: Handle,
    p1: Vec2,
    h2: Handle,
    p2: Vec2,
    radius: f64,
    chamfer: Option<(f64, f64)>,
) -> Result<Option<Handle>> {
    if crate::cmd::curves::is_locked(s, h1) || crate::cmd::curves::is_locked(s, h2) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let d = s.doc()?;
    let e1 = d.entity(h1).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let e2 = d.entity(h2).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let (Some(l1), Some(l2)) = (as_line(&e1), as_line(&e2)) else {
        if chamfer.is_none() {
            return crate::cmd::modify2::fillet_curves(s, h1, p1, h2, p2, radius);
        }
        return Err(EngineError::Other(CHAMFER_LINES_ONLY.into()));
    };
    let (x, _, _) = line_line_infinite(l1.a, l1.b, l2.a, l2.b).ok_or_else(|| EngineError::Other("Lines are parallel.".into()))?;
    // Keep the far end on the picked side of the corner.
    let keep = |l: &Line, p: Vec2| -> Vec2 {
        let ta = (l.a - x).dot(p - x);
        let tb = (l.b - x).dot(p - x);
        if ta >= tb { l.a } else { l.b }
    };
    let k1 = keep(&l1, p1);
    let k2 = keep(&l2, p2);
    let u1 = (k1 - x).normalized();
    let u2 = (k2 - x).normalized();
    let mut new_arc = None;
    let (t1, t2) = if let Some((d1, d2)) = chamfer {
        (x + u1 * d1, x + u2 * d2)
    } else if radius <= 1e-12 {
        (x, x)
    } else {
        let half = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if half.sin().abs() < 1e-12 {
            return Err(EngineError::Other("Lines are parallel.".into()));
        }
        let dist = radius / half.tan();
        let t1 = x + u1 * dist;
        let t2 = x + u2 * dist;
        let bis = (u1 + u2).normalized();
        let c = x + bis * (radius / half.sin());
        let a1 = c.angle_to(t1);
        let a2 = c.angle_to(t2);
        let ar = if ccw_sweep(a1, a2) <= std::f64::consts::PI { Arc::new(c, radius, a1, a2) } else { Arc::new(c, radius, a2, a1) };
        new_arc = Some(ar);
        (t1, t2)
    };
    let common = e1.common.clone();
    let doc = s.doc_mut()?;
    doc.modify_entity(h1, |e| e.kind = line(k1, t1))?;
    doc.modify_entity(h2, |e| e.kind = line(k2, t2))?;
    let space = s.space();
    let doc = s.doc_mut()?;
    let extra = match (chamfer, new_arc) {
        (Some(_), _) if !t1.near(t2, 1e-12) => Some(line(t1, t2)),
        (None, Some(a)) => Some(arc(&a)),
        _ => None,
    };
    if let Some(k) = extra {
        let nh = doc.new_handle();
        if let Some(st) = doc.space_mut(&space) {
            st.push(Entity { handle: nh, common, kind: k });
        }
        return Ok(Some(nh));
    }
    Ok(None)
}

fn h_param(p: &Value, k: &str) -> Option<Handle> {
    p.get(k).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

pub(super) fn run_fillet(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "polyline", false) {
        let h = h_param(p, "handle").or_else(|| h_param(p, "h1")).ok_or_else(|| bad("fillet", "`handle` (polyline) is required"))?;
        let r = f64_or(p, "radius", s.doc()?.header.f64("FILLETRAD", 0.0));
        let n = crate::cmd::modify2::fillet_polyline(s, h, r)?;
        return Ok(json!({ "filleted": n }));
    }
    let h1 = h_param(p, "h1").ok_or_else(|| bad("fillet", "`h1` is required"))?;
    let h2 = h_param(p, "h2").ok_or_else(|| bad("fillet", "`h2` is required"))?;
    let r = f64_or(p, "radius", s.doc()?.header.f64("FILLETRAD", 0.0));
    let p1 = point_param(p, "p1").unwrap_or_default();
    let p2 = point_param(p, "p2").unwrap_or_default();
    let a = fillet_corner(s, h1, p1, h2, p2, r, None, bool_or(p, "trim", true))?;
    Ok(json!({ "arc": a.map(|h| h.hex()) }))
}

pub(super) fn run_chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let cut = match (p.get("length").and_then(Value::as_f64), p.get("angle").and_then(Value::as_f64)) {
        (Some(len), Some(deg)) if len.is_finite() && len >= 0.0 && deg.is_finite() && (0.0..180.0).contains(&deg) => {
            ChamferCut::Angle(len, deg.to_radians())
        }
        (None, None) => {
            let d1 = f64_or(p, "d1", s.doc()?.header.f64("CHAMFERA", 0.0));
            let d2 = f64_or(p, "d2", d1);
            if !(d1.is_finite() && d1 >= 0.0 && d2.is_finite() && d2 >= 0.0) {
                return Err(bad("chamfer", "`d1` and `d2` must be >= 0"));
            }
            ChamferCut::Dist(d1, d2)
        }
        _ => return Err(bad("chamfer", "`length` (>= 0) and `angle` (degrees, 0 to 180) go together")),
    };
    if bool_or(p, "polyline", false) {
        let h = h_param(p, "handle").or_else(|| h_param(p, "h1")).ok_or_else(|| bad("chamfer", "`handle` (polyline) is required"))?;
        return Ok(json!({ "chamfered": chamfer_polyline(s, h, cut)? }));
    }
    let h1 = h_param(p, "h1").ok_or_else(|| bad("chamfer", "`h1` is required"))?;
    let h2 = h_param(p, "h2").ok_or_else(|| bad("chamfer", "`h2` is required"))?;
    let p1 = point_param(p, "p1").unwrap_or_default();
    let p2 = point_param(p, "p2").unwrap_or_default();
    let a = fillet_corner(s, h1, p1, h2, p2, 0.0, Some(cut), bool_or(p, "trim", true))?;
    Ok(json!({ "line": a.map(|h| h.hex()) }))
}

/// A FILLET/CHAMFER option prompt opened from the select prompts.
#[derive(Clone, Copy, PartialEq)]
enum FilletAsk {
    /// "Enter Trim mode option [Trim/No trim]" (TRIMMODE).
    Trim,
    /// CHAMFER "Enter trim method [Distance/Angle]" (CHAMMODE).
    Method,
    /// CHAMFER Angle: the length on the first line (CHAMFERC), then the angle (CHAMFERD).
    Length,
    Angle,
}

pub(super) struct FilletM {
    chamfer: bool,
    first: Option<(Handle, Vec2)>,
    /// Asking for the radius (FILLET) or a chamfer distance: 1 = first, 2 = second.
    asking: u8,
    polyline: bool,
    ask: Option<FilletAsk>,
    /// Multiple: keep asking for pairs until Enter.
    multiple: bool,
    /// Undo: the drawing before each fillet/chamfer made in this command.
    undo: Vec<std::sync::Arc<cadcraft_doc::Drawing>>,
}

impl FilletM {
    pub(super) fn new(_s: &Session, chamfer: bool) -> Self {
        FilletM { chamfer, first: None, asking: 0, polyline: false, ask: None, multiple: false, undo: Vec::new() }
    }

    /// One fillet/chamfer done: carry on in Multiple mode, else finish.
    fn next(&mut self, s: &mut Session, before: std::sync::Arc<cadcraft_doc::Drawing>) -> Step {
        self.undo.push(before);
        self.first = None;
        self.polyline = false;
        s.set_selection(Vec::new());
        if self.multiple { Step::Continue } else { Step::Done }
    }
}

/// TRIMMODE: FILLET and CHAMFER trim or extend the selected objects to the new corner (default).
fn trim_mode(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("TRIMMODE", 1) != 0).unwrap_or(true)
}

/// The chamfer CHAMFER uses now: two distances, or length and angle when CHAMMODE = 1.
fn chamfer_cut(s: &Session) -> Result<ChamferCut> {
    let h = &s.doc()?.header;
    Ok(if h.i64("CHAMMODE", 0) == 1 {
        ChamferCut::Angle(h.f64("CHAMFERC", 0.0), h.f64("CHAMFERD", 0.0))
    } else {
        ChamferCut::Dist(h.f64("CHAMFERA", 0.0), h.f64("CHAMFERB", 0.0))
    })
}

impl Interactive for FilletM {
    fn name(&self) -> &'static str {
        if self.chamfer { "CHAMFER" } else { "FILLET" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        let trim = trim_mode(s);
        let d = s.doc()?;
        let msg = if self.chamfer {
            let mode = if trim { "TRIM" } else { "NOTRIM" };
            if d.header.i64("CHAMMODE", 0) == 1 {
                let (au, ap) = (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0));
                let a = crate::units::format_angle(d.header.f64("CHAMFERD", 0.0), au, ap);
                format!("({mode} mode) Current chamfer Length = {:.4}, Angle = {a}", d.header.f64("CHAMFERC", 0.0))
            } else {
                format!("({mode} mode) Current chamfer Dist1 = {:.4}, Dist2 = {:.4}", d.header.f64("CHAMFERA", 0.0), d.header.f64("CHAMFERB", 0.0))
            }
        } else {
            format!("Current settings: Mode = {}, Radius = {:.4}", if trim { "TRIM" } else { "NOTRIM" }, d.header.f64("FILLETRAD", 0.0))
        };
        s.echo(msg);
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        // The radius and distances are remembered in the drawing (FILLETRAD, CHAMFERA/B) and
        // offered as defaults; the second chamfer distance defaults to the first.
        if self.asking > 0 {
            let hdr = |k: &str| s.doc().map(|d| d.header.f64(k, 0.0)).unwrap_or(0.0);
            let (msg, v) = match (self.chamfer, self.asking) {
                (false, _) => ("Specify fillet radius", hdr("FILLETRAD")),
                (true, 1) => ("Specify first chamfer distance", hdr("CHAMFERA")),
                (true, _) => ("Specify second chamfer distance", hdr("CHAMFERA")),
            };
            return Prompt::new(msg, Accept::NUMBER).default(format!("{v:.4}"));
        }
        let hdr_i = |k: &str, dflt: i64| s.doc().map(|d| d.header.i64(k, dflt)).unwrap_or(dflt);
        match self.ask {
            Some(FilletAsk::Trim) => {
                let cur = if trim_mode(s) { "Trim" } else { "No trim" };
                return Prompt::new("Enter Trim mode option", Accept::TEXT).kw(&["Trim", "No trim"]).default(cur);
            }
            Some(FilletAsk::Method) => {
                let cur = if hdr_i("CHAMMODE", 0) == 1 { "Angle" } else { "Distance" };
                return Prompt::new("Enter trim method", Accept::TEXT).kw(&["Distance", "Angle"]).default(cur);
            }
            Some(FilletAsk::Length) => {
                let v = s.doc().map(|d| d.header.f64("CHAMFERC", 0.0)).unwrap_or(0.0);
                return Prompt::new("Specify chamfer length on the first line", Accept::NUMBER).default(format!("{v:.4}"));
            }
            Some(FilletAsk::Angle) => {
                let (a, au, ap) =
                    s.doc().map(|d| (d.header.f64("CHAMFERD", 0.0), d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0.0, 0, 0));
                return Prompt::new("Specify chamfer angle from the first line", Accept::NUMBER).default(crate::units::format_angle(a, au, ap));
            }
            None => {}
        }
        if self.polyline {
            return Prompt::new("Select 2D polyline", Accept::POINT);
        }
        match (self.first, self.chamfer) {
            (None, false) => Prompt::new("Select first object", Accept::POINT).kw(&["Undo", "Polyline", "Radius", "Trim", "Multiple"]),
            (None, true) => {
                Prompt::new("Select first line", Accept::POINT).kw(&["Undo", "Polyline", "Distance", "Angle", "Trim", "mEthod", "Multiple"])
            }
            (Some(_), false) => Prompt::new("Select second object or shift-select to apply corner", Accept::POINT).kw(&["Radius"]),
            (Some(_), true) => Prompt::new("Select second line or shift-select to apply corner", Accept::POINT).kw(&["Distance", "Angle", "Method"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking > 0 {
            // Enter keeps the default.
            let v = match &i {
                Input::Text(t) => Some(
                    number(t).filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| EngineError::Other("Requires a non-negative distance.".into()))?,
                ),
                Input::Enter => None,
                _ => return Ok(Step::Continue),
            };
            let d = s.doc_mut()?;
            match (self.chamfer, self.asking) {
                (false, _) => {
                    if let Some(v) = v {
                        d.header.set_f64("FILLETRAD", v);
                    }
                    self.asking = 0;
                }
                (true, 1) => {
                    if let Some(v) = v {
                        d.header.set_f64("CHAMFERA", v);
                    }
                    self.asking = 2;
                }
                (true, _) => {
                    let v = v.unwrap_or_else(|| d.header.f64("CHAMFERA", 0.0));
                    d.header.set_f64("CHAMFERB", v);
                    self.asking = 0;
                }
            }
            return Ok(Step::Continue);
        }
        if let Some(ask) = self.ask {
            return self.option(s, ask, i);
        }
        match i {
            Input::Keyword(k) if k == "Radius" || k == "Distance" => {
                if self.chamfer {
                    s.doc_mut()?.header.set_i64("CHAMMODE", 0);
                }
                self.asking = 1;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Angle" && self.chamfer => {
                s.doc_mut()?.header.set_i64("CHAMMODE", 1);
                self.ask = Some(FilletAsk::Length);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "mEthod" || k == "Method" => {
                self.ask = Some(FilletAsk::Method);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Trim" => {
                self.ask = Some(FilletAsk::Trim);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Multiple" => {
                self.multiple = true;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Polyline" => {
                self.polyline = true;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                match self.undo.pop() {
                    Some(doc) => {
                        // Settings changed meanwhile stay.
                        let header = s.doc()?.header.clone();
                        s.state_mut()?.doc = doc;
                        s.doc_mut()?.header = header;
                    }
                    None => s.echo("Nothing to undo."),
                }
                self.first = None;
                s.set_selection(Vec::new());
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                let Some(h) = s.pick_object(&space, p, ap)? else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                if self.polyline {
                    let before = s.state()?.doc.clone();
                    if self.chamfer {
                        let n = chamfer_polyline(s, h, chamfer_cut(s)?)?;
                        s.echo(format!("{n} lines were chamfered"));
                    } else {
                        let r = s.doc()?.header.f64("FILLETRAD", 0.0);
                        let n = crate::cmd::modify2::fillet_polyline(s, h, r)?;
                        s.echo(format!("{n} lines were filleted"));
                    }
                    return Ok(self.next(s, before));
                }
                match self.first {
                    None => {
                        self.first = Some((h, p));
                        s.set_selection(vec![h]);
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) => {
                        let r = s.doc()?.header.f64("FILLETRAD", 0.0);
                        let cut = if self.chamfer { Some(chamfer_cut(s)?) } else { None };
                        let before = s.state()?.doc.clone();
                        let trim = trim_mode(s);
                        match fillet_corner(s, h1, p1, h, p, r, cut, trim) {
                            Ok(_) => Ok(self.next(s, before)),
                            Err(e) => {
                                s.echo(e.to_string());
                                self.first = None;
                                s.set_selection(Vec::new());
                                Ok(Step::Continue)
                            }
                        }
                    }
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

impl FilletM {
    /// Input at a Trim / Method / Length / Angle option prompt; Enter keeps the current value.
    fn option(&mut self, s: &mut Session, ask: FilletAsk, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
            Input::Enter => String::new(),
            _ => return Ok(Step::Continue),
        };
        let invalid = || EngineError::Other("Invalid option keyword.".into());
        let flag = |yes: &str, no: &str| -> Result<Option<i64>> {
            if text.is_empty() {
                Ok(None)
            } else if text.starts_with(yes) {
                Ok(Some(1))
            } else if text.starts_with(no) {
                Ok(Some(0))
            } else {
                Err(invalid())
            }
        };
        let au = s.angle_settings();
        let d = s.doc_mut()?;
        self.ask = None;
        match ask {
            FilletAsk::Trim => {
                if let Some(v) = flag("t", "n")? {
                    d.header.set_i64("TRIMMODE", v);
                }
            }
            FilletAsk::Method => {
                if let Some(v) = flag("a", "d")? {
                    d.header.set_i64("CHAMMODE", v);
                }
            }
            FilletAsk::Length => {
                if !text.is_empty() {
                    let v = number(&text)
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or_else(|| EngineError::Other("Requires a non-negative distance.".into()))?;
                    d.header.set_f64("CHAMFERC", v);
                }
                self.ask = Some(FilletAsk::Angle);
            }
            FilletAsk::Angle => {
                if !text.is_empty() {
                    let a = au
                        .amount(&text)
                        .filter(|a| a.is_finite() && *a >= 0.0 && *a < std::f64::consts::PI)
                        .ok_or_else(|| EngineError::Other("Requires an angle from 0 to 180 degrees.".into()))?;
                    d.header.set_f64("CHAMFERD", a);
                }
            }
        }
        Ok(Step::Continue)
    }
}

/// How CHAMFER cuts a corner: two distances, or a length on the first line and an angle
/// (radians) from it.
#[derive(Clone, Copy, Debug)]
enum ChamferCut {
    Dist(f64, f64),
    Angle(f64, f64),
}

impl ChamferCut {
    /// The distances back from the corner along the first and second lines, which leave it
    /// along the unit directions `u1` and `u2`.
    fn distances(self, u1: Vec2, u2: Vec2) -> Result<(f64, f64)> {
        match self {
            ChamferCut::Dist(a, b) => Ok((a, b)),
            ChamferCut::Angle(len, ang) => {
                // Triangle corner–t1–t2: the corner angle and the chamfer angle at t1 give t2 by
                // the law of sines.
                let corner = u1.dot(u2).clamp(-1.0, 1.0).acos();
                let s = (corner + ang).sin();
                let d2 = len * ang.sin() / s;
                if corner + ang >= std::f64::consts::PI - 1e-9 || s.abs() < 1e-12 || !d2.is_finite() || d2 < 0.0 {
                    return Err(EngineError::Other("The chamfer angle does not fit this corner.".into()));
                }
                Ok((len, d2))
            }
        }
    }
}

/// FILLET (`cut` None) or CHAMFER two objects, honouring TRIMMODE: with `trim` false the new
/// arc or line is added and both objects are left as they were.
#[allow(clippy::too_many_arguments)]
fn fillet_corner(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, r: f64, cut: Option<ChamferCut>, trim: bool) -> Result<Option<Handle>> {
    let dist = cut.map(|c| move |u1: Vec2, u2: Vec2| c.distances(u1, u2));
    if let Some(made) =
        crate::cmd::modify2::fillet_pline(s, h1, p1, h2, p2, r, dist.as_ref().map(|f| f as crate::cmd::modify2::ChamferDistances), trim)?
    {
        return Ok(made);
    }
    let d = s.doc()?;
    let k1 = d.entity(h1).map(|e| e.kind.clone());
    let k2 = d.entity(h2).map(|e| e.kind.clone());
    let chamfer = match cut {
        None => None,
        Some(c) => {
            let l = |h| d.entity(h).and_then(|e| as_line(e)).ok_or_else(|| EngineError::Other(CHAMFER_LINES_ONLY.into()));
            let (l1, l2) = (l(h1)?, l(h2)?);
            let (x, _, _) = line_line_infinite(l1.a, l1.b, l2.a, l2.b).ok_or_else(|| EngineError::Other("Lines are parallel.".into()))?;
            // The direction along each line towards its picked side.
            let dir = |l: &Line, p: Vec2| if (l.a - x).dot(p - x) >= (l.b - x).dot(p - x) { (l.a - x).normalized() } else { (l.b - x).normalized() };
            Some(c.distances(dir(&l1, p1), dir(&l2, p2))?)
        }
    };
    let made = fillet_lines(s, h1, p1, h2, p2, r, chamfer)?;
    if !trim {
        let doc = s.doc_mut()?;
        for (h, k) in [(h1, k1), (h2, k2)] {
            if let Some(k) = k {
                doc.modify_entity(h, |e| e.kind = k)?;
            }
        }
    }
    Ok(made)
}

/// CHAMFER Polyline: cut every corner between two straight segments of a 2D polyline. Returns
/// how many corners were chamfered.
fn chamfer_polyline(s: &mut Session, h: Handle, cut: ChamferCut) -> Result<usize> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| EngineError::Other("no such object".into()))?;
    if d.layer(&e.common.layer).is_some_and(|l| l.locked) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let EntityKind::LwPolyline(pl) = &e.kind else { return Err(EngineError::Other("Select a 2D polyline.".into())) };
    let vs = &pl.vertices;
    let n = vs.len();
    if n < 3 {
        return Ok(0);
    }
    let at = |i: usize| vs.get(i % n).copied().unwrap_or_default();
    // Distances cut back from each corner along the incoming and outgoing segments.
    let mut cuts = vec![(0.0f64, 0.0f64); n];
    for (i, c) in cuts.iter_mut().enumerate() {
        if !pl.closed && (i == 0 || i + 1 == n) {
            continue;
        }
        let (prev, cur, next) = (at(i + n - 1), at(i), at(i + 1));
        if prev.bulge.abs() > 1e-12 || cur.bulge.abs() > 1e-12 {
            continue;
        }
        let u1 = (prev.p - cur.p).normalized();
        let u2 = (next.p - cur.p).normalized();
        if u1 == Vec2::ZERO || u2 == Vec2::ZERO || u1.dot(u2) < -1.0 + 1e-9 {
            continue;
        }
        if let Ok(ab) = cut.distances(u1, u2) {
            *c = ab;
        }
    }
    // Drop chamfers that do not fit their segments.
    for i in 0..n {
        let j = (i + 1) % n;
        let seg = at(i).p.dist(at(j).p);
        let (out_i, in_j) = (cuts.get(i).map_or(0.0, |c| c.1), cuts.get(j).map_or(0.0, |c| c.0));
        if out_i + in_j > seg + 1e-9 {
            for k in [i, j] {
                if let Some(c) = cuts.get_mut(k) {
                    *c = (0.0, 0.0);
                }
            }
        }
    }
    let mut out: Vec<PolyVertex> = Vec::with_capacity(n * 2);
    let mut count = 0;
    for i in 0..n {
        let v = at(i);
        let (a, b) = cuts.get(i).copied().unwrap_or_default();
        if a <= 1e-12 && b <= 1e-12 {
            out.push(v);
            continue;
        }
        let t1 = v.p + (at(i + n - 1).p - v.p).normalized() * a;
        let t2 = v.p + (at(i + 1).p - v.p).normalized() * b;
        out.push(PolyVertex { p: t1, bulge: 0.0, ..v });
        out.push(PolyVertex { p: t2, bulge: 0.0, ..v });
        count += 1;
    }
    let mut npl = pl.clone();
    npl.vertices = out;
    s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::LwPolyline(npl))?;
    Ok(count)
}
