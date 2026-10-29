use super::break_join::join;
use super::explode::explode;
use super::transform::{erase, transform_entities};
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
        transform_entities(s, &self.objs, &Mat3::scale_about(base, f), self.copy_mode)?;
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
            (Op::Copy, _) => Prompt::new("Specify second point", Accept::POINT).kw(&["Array", "Exit", "Undo"]).base_opt(base),
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
            (Op::Copy, _, Input::Keyword(kw)) if kw == "Exit" => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
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
            (Op::Copy, _, Input::Point(p)) => {
                let d = p - self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), true)?;
                Ok(Step::Continue)
            }
            (Op::Copy, _, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
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
                    transform_entities(s, &self.objs, &Mat3::scale_about(self.pts[0], f), self.copy_mode)?;
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Mirror, 1, Input::Point(p)) => {
                if p.near(self.pts[0], 1e-12) {
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
}

impl DisplaceM {
    pub(super) fn new(op: Op) -> Self {
        DisplaceM { inner: SelectThen::new(op), displacement: false }
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
        self.inner.begin(s)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.displacement {
            return Prompt::new("Specify displacement", Accept::POINT).default("0,0");
        }
        if self.at_base() {
            return Prompt::new("Specify base point", Accept::POINT).kw(&["Displacement"]).default("Displacement");
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
        if self.at_base() && (matches!(&i, Input::Enter) || matches!(&i, Input::Keyword(k) if k == "Displacement")) {
            self.displacement = true;
            return Ok(Step::Continue);
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
