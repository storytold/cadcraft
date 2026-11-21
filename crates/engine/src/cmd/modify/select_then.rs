use super::break_join::join;
use super::explode::explode;
use super::transform::{at_elevation, erase, scale_entities, transform_entities};
use super::{
    Accept, EngineError, EntityKind, Handle, Input, Interactive, Mat3, Prompt, Result, SelOutcome, SelectPhase, Session, Step, Vec2, line, number,
};

// ---------------- interactive ----------------

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Op {
    Erase,
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Explode,
    Join,
}

/// COPY's Array option: asking how many items, then where the second one goes (or, with Fit, the
/// last one).
#[derive(Clone, Copy, PartialEq)]
enum CopyArray {
    Off,
    Count,
    Items { n: u16, fit: bool },
}

/// The most items COPY's Array option makes, the original included, as in AutoCAD.
const MAX_COPY_ITEMS: u16 = 32767;

/// Select objects, then collect the points/values the operation needs.
pub(super) struct SelectThen {
    op: Op,
    sel: SelectPhase,
    objs: Vec<Handle>,
    pts: Vec<Vec2>,
    reference: bool,
    /// ROTATE Reference: the reference angle (radians), once known. Its first point, and the
    /// first of the two new-angle points, are kept in `ref_from`.
    ref_angle: Option<f64>,
    /// ROTATE Reference: the new angle is given by two points (the `Points` option).
    ref_points: bool,
    /// SCALE Reference: first point of a reference length given by two points.
    ref_from: Option<Vec2>,
    /// SCALE Reference: the reference length, once known.
    ref_len: Option<f64>,
    copy_mode: bool,
    /// COPY: one copy and done (COPYMODE 1, until the Multiple option), else copies until Exit.
    copy_single: bool,
    /// COPY: the objects each placement made, so Undo takes back the last one.
    copies: Vec<Vec<Handle>>,
    copy_array: CopyArray,
    ask_erase: bool,
    /// JOIN of a single arc or elliptical arc: the prompt asks for arcs to join to it, or cLose.
    join_source: Option<Handle>,
}

impl SelectThen {
    pub(super) fn new(op: Op) -> Self {
        SelectThen {
            op,
            sel: SelectPhase::default(),
            objs: Vec::new(),
            pts: Vec::new(),
            reference: false,
            ref_angle: None,
            ref_points: false,
            ref_from: None,
            ref_len: None,
            copy_mode: false,
            copy_single: false,
            copies: Vec::new(),
            copy_array: CopyArray::Off,
            ask_erase: false,
            join_source: None,
        }
    }
    fn apply(&mut self, s: &mut Session) -> Result<Step> {
        // Immediate operations once selection is done.
        match self.op {
            Op::Erase => {
                let n = erase(s, &self.objs)?;
                let _ = n;
                Ok(Step::Done)
            }
            Op::Explode => {
                explode(s, &self.objs)?;
                Ok(Step::Done)
            }
            Op::Join => {
                if let [h] = self.objs[..]
                    && s.doc()?.entity(h).is_some_and(|e| crate::cmd::modify2::closed_kind(&e.kind).is_some())
                {
                    self.join_source = Some(h);
                    return Ok(Step::Continue);
                }
                self.join_now(s)
            }
            _ => Ok(Step::Continue),
        }
    }

    /// COPY's prompt once the base point is known, as AutoCAD 2024 asks it.
    fn copy_prompt(&self, base: Option<Vec2>) -> Prompt {
        match self.copy_array {
            CopyArray::Count => Prompt::new("Enter number of items to array", Accept::NUMBER),
            CopyArray::Items { fit: false, .. } => Prompt::new("Specify second point", Accept::POINT).kw(&["Fit"]).base_opt(base),
            CopyArray::Items { fit: true, .. } => Prompt::new("Specify second point", Accept::POINT).kw(&["Array"]).base_opt(base),
            CopyArray::Off if self.copies.is_empty() => {
                Prompt::new("Specify second point", Accept::POINT).kw(&["Array"]).default("use first point as displacement").base_opt(base)
            }
            CopyArray::Off => Prompt::new("Specify second point", Accept::POINT).kw(&["Array", "Exit", "Undo"]).default("Exit").base_opt(base),
        }
    }

    /// COPY once the base point is known (AutoCAD 2024, measured): a point places a copy; Enter
    /// at the first prompt copies by the base point taken as a displacement, and after a copy
    /// ends the command; Array asks for a number of items, the original included, then puts
    /// them a second point apart (or, with Fit, spread up to it, Enter keeping them at the
    /// base); Undo takes back the last placement. One placement ends a single-copy COPY.
    fn copy_input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Continue) };
        let placed = match (self.copy_array, i) {
            (CopyArray::Count, Input::Enter) => return self.copy_done(s),
            (CopyArray::Count, i) => {
                // A whole number as typed, from 2 to 32767 whatever the selection: AutoCAD 2024 takes
                // +3 and 03 here, refuses 4.0, 1e1, 8/2, 1', 3", 50002 and a point, and doesn't
                // apply MAXARRAY (with MAXARRAY 100000, four lines and 26000 items made 104000
                // lines; measured).
                let n = match i {
                    Input::Text(t) => t.trim().parse::<u16>().ok().filter(|v| (2..=MAX_COPY_ITEMS).contains(v)),
                    _ => None,
                }
                .ok_or_else(|| EngineError::Other(format!("Requires an integer between 2 and {MAX_COPY_ITEMS}.")))?;
                // Not AutoCAD's: the copies (items after the first, times the objects) stay within
                // ARRAY's own limit, whatever MAXARRAY says, so a large selection can't run out of
                // memory.
                let copies = u64::from(n - 1).saturating_mul(u64::try_from(self.objs.len()).unwrap_or(u64::MAX));
                if copies > cadcraft_doc::MAX_ARRAY_ITEMS {
                    return Err(EngineError::Other(format!(
                        "The array would create {copies} objects; the limit is {}. Use fewer items or select fewer objects.",
                        cadcraft_doc::MAX_ARRAY_ITEMS
                    )));
                }
                self.copy_array = CopyArray::Items { n, fit: false };
                return Ok(Step::Continue);
            }
            (CopyArray::Items { n, fit }, Input::Keyword(kw)) if kw == if fit { "Array" } else { "Fit" } => {
                self.copy_array = CopyArray::Items { n, fit: !fit };
                return Ok(Step::Continue);
            }
            (CopyArray::Items { n, fit }, i @ (Input::Point(_) | Input::Enter)) => {
                let p = if let Input::Point(p) = i { p } else { base };
                let step = if fit { (p - base) * (1.0 / f64::from(n - 1)) } else { p - base };
                self.copy_array = CopyArray::Off;
                let mut made = Vec::new();
                for k in 1..n {
                    match transform_entities(s, &self.objs, &Mat3::translate(step * f64::from(k)), true) {
                        Ok(hs) => made.extend(hs),
                        Err(e) => {
                            // The items made so far are a placement Undo takes back.
                            self.copies.push(made);
                            return Err(e);
                        }
                    }
                }
                made
            }
            (CopyArray::Off, Input::Keyword(kw)) if kw == "Array" => {
                self.copy_array = CopyArray::Count;
                return Ok(Step::Continue);
            }
            (CopyArray::Off, Input::Keyword(kw)) if kw == "Undo" => {
                if let Some(last) = self.copies.pop() {
                    let d = s.doc_mut()?;
                    for h in last {
                        d.remove_entity(h);
                    }
                    if self.copies.is_empty() {
                        s.echo("Command has been completely undone.");
                    }
                }
                return Ok(Step::Continue);
            }
            (CopyArray::Off, Input::Keyword(kw)) if kw == "Exit" => return self.copy_done(s),
            (CopyArray::Off, Input::Enter) if !self.copies.is_empty() => return self.copy_done(s),
            (CopyArray::Off, Input::Enter) => {
                transform_entities(s, &self.objs, &Mat3::translate(base), true)?;
                return self.copy_done(s);
            }
            (CopyArray::Off, Input::Point(p)) => transform_entities(s, &self.objs, &Mat3::translate(p - base), true)?,
            _ => return Ok(Step::Continue),
        };
        self.copies.push(placed);
        if self.copy_single {
            return self.copy_done(s);
        }
        Ok(Step::Continue)
    }

    fn copy_done(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    /// JOIN the selected objects and report what they became.
    fn join_now(&mut self, s: &mut Session) -> Result<Step> {
        match join(s, &self.objs) {
            Ok(h) => {
                // Objects outside the joined chain are kept, so count only the ones JOIN consumed
                // (and the source object an ellipse or spline join keeps).
                let d = s.doc()?;
                let n = self.objs.iter().filter(|o| d.entity(**o).is_none() || h == Some(**o)).count();
                let what = match h.and_then(|h| d.entity(h)).map(|e| &e.kind) {
                    Some(EntityKind::Spline(_)) => "spline",
                    Some(EntityKind::Ellipse(_)) => "ellipse",
                    _ => "polyline",
                };
                s.echo(format!("{n} objects joined into 1 {what}"))
            }
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }

    /// JOIN of a single arc or elliptical arc: more arcs picked join to it on Enter, cLose makes
    /// it its circle or full ellipse.
    fn join_source_input(&mut self, s: &mut Session, src: Handle, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) if k == "cLose" => {
                let ellipse = matches!(s.doc()?.entity(src).map(|e| &e.kind), Some(EntityKind::Ellipse(_)));
                crate::cmd::modify2::join_close(s, &[src])?;
                s.echo(if ellipse { "The elliptical arc is now an ellipse." } else { "The arc is now a circle." });
                Ok(Step::Done)
            }
            Input::Point(p) => {
                if let Some(h) = crate::cmd::curves::pick_at(s, p)
                    && !self.objs.contains(&h)
                {
                    self.objs.push(h);
                }
                Ok(Step::Continue)
            }
            Input::Enter => self.join_now(s),
            _ => Ok(Step::Continue),
        }
    }

    /// SCALE Reference: reference length (typed, or two points), then new length (typed, or a
    /// point measured from the base point). The scale factor is new length / reference length.
    fn scale_reference(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Done) };
        match (self.ref_len, self.ref_from, i) {
            (None, None, Input::Point(p)) => {
                self.ref_from = Some(p);
                Ok(Step::Continue)
            }
            (None, None, Input::Text(t)) => {
                self.ref_len = Some(require_length(number(&t))?);
                Ok(Step::Continue)
            }
            (None, None, Input::Enter) => {
                self.ref_len = Some(1.0);
                Ok(Step::Continue)
            }
            (None, Some(a), Input::Point(p)) => {
                self.ref_len = Some(require_length(Some(a.dist(p)))?);
                Ok(Step::Continue)
            }
            (Some(r), _, Input::Point(p)) => self.scale_by(s, base, base.dist(p) / r),
            (Some(r), _, Input::Text(t)) => {
                let n = require_length(number(&t))?;
                self.scale_by(s, base, n / r)
            }
            (_, _, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }

    /// ROTATE Reference: reference angle (typed, or two points), then the new angle (typed, a
    /// point measured from the base point, or two points after `Points`). The objects turn by the
    /// new angle minus the reference angle.
    fn rotate_reference(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Done) };
        let au = s.angle_settings();
        let angle = |t: &str| au.direction(t).filter(|a| a.is_finite()).ok_or_else(|| EngineError::Other("Requires an angle or point.".into()));
        let two_points = |a: Vec2, p: Vec2| {
            if a.near(p, 1e-12) { Err(EngineError::Other("The two points must differ.".into())) } else { Ok(a.angle_to(p)) }
        };
        let Some(r) = self.ref_angle else {
            match (self.ref_from, i) {
                (None, Input::Point(p)) => self.ref_from = Some(p),
                (None, Input::Text(t)) => self.ref_angle = Some(angle(&t)?),
                (None, Input::Enter) => self.ref_angle = Some(0.0),
                (Some(a), Input::Point(p)) => {
                    self.ref_angle = Some(two_points(a, p)?);
                    self.ref_from = None;
                }
                _ => {}
            }
            return Ok(Step::Continue);
        };
        let new = match (self.ref_points, self.ref_from, i) {
            (false, _, Input::Keyword(k)) if k == "Points" => {
                self.ref_points = true;
                return Ok(Step::Continue);
            }
            (false, _, Input::Point(p)) => base.angle_to(p),
            (false, _, Input::Text(t)) => angle(&t)?,
            (false, _, Input::Enter) => 0.0,
            (true, None, Input::Point(p)) => {
                self.ref_from = Some(p);
                return Ok(Step::Continue);
            }
            (true, Some(a), Input::Point(p)) => two_points(a, p)?,
            _ => return Ok(Step::Continue),
        };
        transform_entities(s, &self.objs, &Mat3::rotate_about(base, new - r), self.copy_mode)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    /// The prompt while ROTATE Reference collects its angles.
    fn rotate_reference_prompt(&self) -> Prompt {
        match (self.ref_angle, self.ref_points, self.ref_from) {
            (None, _, None) => Prompt::new("Specify the reference angle", Accept::POINT_OR_NUMBER).default("0"),
            (None, _, Some(p)) | (Some(_), true, Some(p)) => Prompt::new("Specify second point", Accept::POINT).base(p),
            (Some(_), false, _) => {
                Prompt::new("Specify the new angle", Accept::POINT_OR_NUMBER).kw(&["Points"]).default("0").base_opt(self.pts.first().copied())
            }
            (Some(_), true, None) => Prompt::new("Specify first point", Accept::POINT),
        }
    }

    /// Rubber band while ROTATE Reference collects its angles: the objects turned to the cursor
    /// once the reference angle is known, else the line from the first point.
    fn rotate_reference_preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(base), Ok(d)) = (self.pts.first().copied(), s.doc()) else { return Vec::new() };
        match (self.ref_angle, self.ref_points, self.ref_from) {
            (Some(r), false, _) => {
                let m = Mat3::rotate_about(base, base.angle_to(c) - r);
                let mut out: Vec<EntityKind> = self
                    .objs
                    .iter()
                    .take(500)
                    .filter_map(|h| d.entity(*h))
                    .map(|e| {
                        let mut k = e.kind.clone();
                        k.transform(&m);
                        k
                    })
                    .collect();
                out.push(line(base, c));
                out
            }
            (_, _, Some(a)) => vec![line(a, c)],
            _ => Vec::new(),
        }
    }

    fn scale_by(&mut self, s: &mut Session, base: Vec2, f: f64) -> Result<Step> {
        let f = require_length(Some(f))?;
        // The next SCALE offers this factor.
        s.last_used.scale_factor = f;
        scale_entities(s, &self.objs, at_elevation(s, base), f, self.copy_mode)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

/// Objects whose text MIRROR keeps readable instead of mirroring it with the geometry.
fn keeps_reading_when_mirrored(k: &EntityKind) -> bool {
    match k {
        EntityKind::Insert(i) => !i.attribs.is_empty(),
        k => matches!(
            k,
            EntityKind::Text(_)
                | EntityKind::AttDef(_)
                | EntityKind::MText(_)
                | EntityKind::Dimension(_)
                | EntityKind::MLeader(_)
                | EntityKind::Table(_)
                | EntityKind::Tolerance(_)
        ),
    }
}

/// Text objects transformed for each frame of a MIRROR preview (on the CPU); more are not drawn.
const MAX_MIRROR_TEXT_PREVIEW: usize = 1000;

/// A finite length greater than zero (rejects 0, negatives, NaN and infinity).
fn positive_length(v: Option<f64>) -> Option<f64> {
    v.filter(|f| f.is_finite() && *f > 1e-12)
}

/// [`positive_length`], or the error that makes the prompt ask again.
fn require_length(v: Option<f64>) -> Result<f64> {
    positive_length(v).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))
}

impl Interactive for SelectThen {
    fn name(&self) -> &'static str {
        match self.op {
            Op::Erase => "ERASE",
            Op::Move => "MOVE",
            Op::Copy => "COPY",
            Op::Rotate => "ROTATE",
            Op::Scale => "SCALE",
            Op::Mirror => "MIRROR",
            Op::Explode => "EXPLODE",
            Op::Join => "JOIN",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            return self.apply(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        if self.ask_erase {
            return Prompt::new("Erase source objects?", Accept::TEXT).kw(&["Yes", "No"]).default("No");
        }
        if let Some(src) = self.join_source {
            let ellipse = s.doc().is_ok_and(|d| matches!(d.entity(src).map(|e| &e.kind), Some(EntityKind::Ellipse(_))));
            let msg = if ellipse { "Select elliptical arcs to join to source" } else { "Select arcs to join to source" };
            return Prompt::new(msg, Accept::POINT).kw(&["cLose"]);
        }
        let k = self.pts.len();
        let base = self.pts.first().copied();
        if self.op == Op::Scale && self.reference {
            return match (self.ref_len, self.ref_from) {
                (Some(_), _) => Prompt::new("Specify new length", Accept::POINT_OR_NUMBER).base_opt(base),
                (None, Some(p)) => Prompt::new("Specify second point", Accept::POINT).base(p),
                (None, None) => Prompt::new("Specify reference length", Accept::POINT_OR_NUMBER).default("1"),
            };
        }
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference_prompt();
        }
        match (self.op, k) {
            (Op::Move | Op::Copy, 0) => Prompt::new("Specify base point", Accept::POINT).kw(&["Displacement"]),
            (Op::Move, _) => Prompt::new("Specify second point or <use first point as displacement>", Accept::POINT).base_opt(base),
            (Op::Copy, _) => self.copy_prompt(base),
            (Op::Rotate | Op::Scale, 0) => Prompt::new("Specify base point", Accept::POINT),
            // ROTATE and SCALE offer the angle/factor used last time.
            (Op::Rotate, _) => {
                let (au, ap) = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
                // Offered as it would be typed: clockwise is positive when ANGDIR is 1.
                let a = if s.angle_settings().clockwise { -s.last_used.rotate_angle } else { s.last_used.rotate_angle };
                Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER)
                    .kw(&["Copy", "Reference"])
                    .default(crate::units::format_angle(a, au, ap))
                    .base_opt(base)
            }
            (Op::Scale, _) => Prompt::new("Specify scale factor", Accept::POINT_OR_NUMBER)
                .kw(&["Copy", "Reference"])
                .default(format!("{:.4}", s.last_used.scale_factor))
                .base_opt(base),
            (Op::Mirror, 0) => Prompt::new("Specify first point of mirror line", Accept::POINT),
            (Op::Mirror, _) => Prompt::new("Specify second point of mirror line", Accept::POINT).base_opt(base),
            _ => Prompt::new("", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    return self.apply(s);
                }
            }
        }
        if let Some(src) = self.join_source {
            return self.join_source_input(s, src, i);
        }
        if self.ask_erase {
            let yes = matches!(&i, Input::Keyword(k) if k == "Yes") || matches!(&i, Input::Text(t) if t.trim().to_ascii_lowercase().starts_with('y'));
            let (a, b) = (self.pts[0], self.pts[1]);
            transform_entities(s, &self.objs, &Mat3::mirror(a, b), !yes)?;
            s.set_selection(Vec::new());
            return Ok(Step::Done);
        }
        let k = self.pts.len();
        if self.op == Op::Scale && self.reference {
            return self.scale_reference(s, i);
        }
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference(s, i);
        }
        match (self.op, k, i) {
            (_, _, Input::Keyword(kw)) if kw == "Copy" => {
                self.copy_mode = true;
                s.echo("Rotating/scaling a copy of the selected objects.");
                Ok(Step::Continue)
            }
            (Op::Scale | Op::Rotate, 1, Input::Keyword(kw)) if kw == "Reference" => {
                self.reference = true;
                Ok(Step::Continue)
            }
            (_, 0, Input::Point(p)) => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            (Op::Move, 1, Input::Point(p)) => {
                let d = p - self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), false)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Move, 1, Input::Enter) => {
                let d = self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), false)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Copy, _, i) => self.copy_input(s, i),
            (Op::Rotate, 1, inp @ (Input::Point(_) | Input::Text(_) | Input::Enter)) => {
                let a = match inp {
                    Input::Point(p) => self.pts[0].angle_to(p),
                    Input::Text(t) => s
                        .angle_settings()
                        .rotation(&t)
                        .filter(|a| a.is_finite())
                        .ok_or_else(|| EngineError::Other("Requires an angle or point.".into()))?,
                    _ => s.last_used.rotate_angle,
                };
                s.last_used.rotate_angle = a;
                transform_entities(s, &self.objs, &Mat3::rotate_about(self.pts[0], a), self.copy_mode)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Scale, 1, inp @ (Input::Point(_) | Input::Text(_) | Input::Enter)) => {
                let f = match inp {
                    Input::Point(p) => self.pts[0].dist(p),
                    Input::Text(t) => {
                        number(&t).filter(|f| f.is_finite() && *f > 0.0).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))?
                    }
                    _ => s.last_used.scale_factor,
                };
                if f > 1e-12 && f.is_finite() {
                    s.last_used.scale_factor = f;
                    scale_entities(s, &self.objs, at_elevation(s, self.pts[0]), f, self.copy_mode)?;
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Mirror, 1, Input::Point(p)) => {
                if p.near(self.pts[0], cadcraft_geom::EPS) {
                    return Err(EngineError::Other("Mirror line points must differ.".into()));
                }
                self.pts.push(p);
                self.ask_erase = true;
                Ok(Step::Continue)
            }
            (_, _, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference_preview(s, c);
        }
        if !self.sel.done {
            return Vec::new();
        }
        let Some(base) = self.pts.first().copied() else { return Vec::new() };
        let m = match self.op {
            Op::Move | Op::Copy => Mat3::translate(c - base),
            Op::Rotate => Mat3::rotate_about(base, base.angle_to(c)),
            Op::Scale if !self.reference => Mat3::scale_about(base, base.dist(c).max(1e-9)),
            Op::Scale => match self.ref_len.and_then(|r| positive_length(Some(base.dist(c) / r))) {
                Some(f) => Mat3::scale_about(base, f),
                None if self.ref_len.is_some() => return Vec::new(),
                None => return self.ref_from.map(|a| vec![line(a, c)]).unwrap_or_default(),
            },
            Op::Mirror if !self.ask_erase => Mat3::mirror(base, c),
            _ => return Vec::new(),
        };
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut out: Vec<EntityKind> = self
            .objs
            .iter()
            .take(500)
            .filter_map(|h| d.entity(*h))
            .map(|e| {
                let mut k = e.kind.clone();
                k.transform(&m);
                k
            })
            .collect();
        out.push(line(base, c));
        out
    }
    fn drag(&self, s: &Session, c: Vec2) -> Option<crate::prompt::Drag> {
        if (self.op == Op::Rotate && self.reference) || !self.sel.done {
            return None;
        }
        let base = self.pts.first().copied()?;
        let matrix = match self.op {
            Op::Move | Op::Copy => Mat3::translate(c - base),
            Op::Rotate => Mat3::rotate_about(base, base.angle_to(c)),
            Op::Scale if !self.reference => Mat3::scale_about(base, base.dist(c).max(1e-9)),
            Op::Scale => Mat3::scale_about(base, self.ref_len.and_then(|r| positive_length(Some(base.dist(c) / r)))?),
            Op::Mirror if !self.ask_erase => Mat3::mirror(base, c),
            _ => return None,
        };
        let mut extra = vec![line(base, c)];
        if self.op != Op::Mirror {
            return Some(crate::prompt::Drag { handles: self.objs.clone(), matrix, extra, clipboard: false });
        }
        // Mirrored text keeps reading left to right (MIRRTEXT 0), which the matrix alone can't
        // draw: those objects are transformed one by one, the rest is mirrored on the GPU.
        let d = s.doc().ok()?;
        let mut handles = Vec::with_capacity(self.objs.len());
        let mut texts = 0;
        for h in &self.objs {
            let Some(e) = d.entity(*h) else { continue };
            if !keeps_reading_when_mirrored(&e.kind) {
                handles.push(*h);
            } else if texts < MAX_MIRROR_TEXT_PREVIEW {
                texts += 1;
                let mut k = e.kind.clone();
                k.transform(&matrix);
                extra.push(k);
            }
        }
        Some(crate::prompt::Drag { handles, matrix, extra, clipboard: false })
    }
}

/// MOVE and COPY: [`SelectThen`], plus the Displacement option at the base point prompt
/// (the keyword, or Enter): the typed point is the displacement itself.
pub(super) struct DisplaceM {
    inner: SelectThen,
    displacement: bool,
    /// COPY's mOde option: asking Single or Multiple.
    ask_mode: bool,
}

impl DisplaceM {
    pub(super) fn new(op: Op) -> Self {
        DisplaceM { inner: SelectThen::new(op), displacement: false, ask_mode: false }
    }
    /// At "Specify base point": objects selected, no point yet.
    fn at_base(&self) -> bool {
        self.inner.sel.done && self.inner.pts.is_empty() && !self.displacement
    }
}

impl Interactive for DisplaceM {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.inner.copy_single = self.inner.op == Op::Copy && s.settings.copymode;
        self.inner.begin(s)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.displacement {
            return Prompt::new("Specify displacement", Accept::POINT).default("0,0");
        }
        if self.ask_mode {
            let current = if s.settings.copymode { "Single" } else { "Multiple" };
            return Prompt::new("Enter a copy mode option", crate::cmd::curves::KW).kw(&["Single", "Multiple"]).default(current);
        }
        if self.at_base() {
            // COPY: mOde sets COPYMODE; Multiple, offered while it is single, copies until Exit
            // this once.
            let kw: &[&str] = match self.inner.op {
                Op::Copy if self.inner.copy_single => &["Displacement", "mOde", "Multiple"],
                Op::Copy => &["Displacement", "mOde"],
                _ => &["Displacement"],
            };
            return Prompt::new("Specify base point", Accept::POINT).kw(kw).default("Displacement");
        }
        self.inner.prompt(s)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.displacement {
            let d = match i {
                Input::Point(p) => p,
                Input::Enter => Vec2::ZERO,
                _ => return Ok(Step::Continue),
            };
            if d != Vec2::ZERO {
                transform_entities(s, &self.inner.objs, &Mat3::translate(d), self.inner.op == Op::Copy)?;
            }
            s.set_selection(Vec::new());
            return Ok(Step::Done);
        }
        if self.ask_mode {
            match &i {
                Input::Keyword(k) if k == "Single" || k == "Multiple" => s.settings.copymode = k == "Single",
                Input::Enter => {}
                _ => return Ok(Step::Continue),
            }
            // The mode shown is the mode this COPY runs in, Enter keeping it.
            self.inner.copy_single = s.settings.copymode;
            self.ask_mode = false;
            return Ok(Step::Continue);
        }
        if self.at_base() && (matches!(&i, Input::Enter) || matches!(&i, Input::Keyword(k) if k == "Displacement")) {
            self.displacement = true;
            return Ok(Step::Continue);
        }
        if self.at_base() && self.inner.op == Op::Copy {
            match &i {
                Input::Keyword(k) if k == "mOde" => {
                    self.ask_mode = true;
                    return Ok(Step::Continue);
                }
                Input::Keyword(k) if k == "Multiple" => {
                    self.inner.copy_single = false;
                    return Ok(Step::Continue);
                }
                _ => {}
            }
        }
        self.inner.input(s, i)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.displacement {
            return Vec::new();
        }
        self.inner.preview(s, c)
    }
    fn drag(&self, s: &Session, c: Vec2) -> Option<crate::prompt::Drag> {
        if self.displacement {
            return None;
        }
        self.inner.drag(s, c)
    }
}
