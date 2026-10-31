use super::{
    Accept, EntityKind, Handle, Input, Interactive, Prompt, Result, Session, Step, Value, Vec2, added, bad, curves, json, line, other, targets,
};

// =====================================================================================
// CENTERMARK / CENTERLINE
// =====================================================================================

/// Add a line on the CENTER linetype (loading it from the standard library when missing).
fn add_center_line(s: &mut Session, a: Vec2, b: Vec2) -> Result<Handle> {
    let d = s.doc_mut()?;
    if d.linetype("CENTER").is_none()
        && let Some(lt) = cadcraft_doc::library::standard_linetypes_for(d).into_iter().find(|l| l.name == "CENTER")
    {
        d.linetypes.push(lt);
    }
    let h = s.add_entity(line(a, b))?;
    s.doc_mut()?.modify_entity(h, |e| e.common.linetype = "CENTER".into())?;
    Ok(h)
}

fn center_ext(s: &Session) -> f64 {
    let d = s.doc().ok();
    let metric = d.is_some_and(|d| d.header.i64("MEASUREMENT", 0) == 1);
    d.map(|d| d.header.f64("CENTEREXE", if metric { 3.5 } else { 0.12 })).unwrap_or(0.12)
}

fn center_mark(s: &mut Session, h: Handle) -> Result<Vec<String>> {
    let e = curves::entity(s, h)?;
    let (c, r) = match &e.kind {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        EntityKind::Arc(a) => (a.center.xy(), a.radius),
        _ => return Err(other("Select a circle or arc.")),
    };
    let ext = r + center_ext(s);
    let h1 = add_center_line(s, c - Vec2::X * ext, c + Vec2::X * ext)?;
    let h2 = add_center_line(s, c - Vec2::Y * ext, c + Vec2::Y * ext)?;
    Ok(vec![h1.hex(), h2.hex()])
}

fn center_line(s: &mut Session, h1: Handle, h2: Handle) -> Result<Handle> {
    let l = |h: Handle| -> Result<(Vec2, Vec2)> {
        match curves::entity(s, h)?.kind {
            EntityKind::Line(l) => Ok((l.a.xy(), l.b.xy())),
            _ => Err(other("Select a line.")),
        }
    };
    let (a1, b1) = l(h1)?;
    let (mut a2, mut b2) = l(h2)?;
    if (b1 - a1).dot(b2 - a2) < 0.0 {
        std::mem::swap(&mut a2, &mut b2);
    }
    let (st, en) = (a1.mid(a2), b1.mid(b2));
    let dir = (en - st).normalized();
    if dir == Vec2::ZERO {
        return Err(other("The lines do not define a center line."));
    }
    let ext = center_ext(s);
    add_center_line(s, st - dir * ext, en + dir * ext)
}

pub(super) fn run_centermark(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("centermark", "`handle` or `handles` is required"));
    }
    let mut out = Vec::new();
    for h in hs {
        out.extend(center_mark(s, h)?);
    }
    Ok(json!({ "handles": out }))
}

pub(super) fn run_centerline(s: &mut Session, p: &Value) -> Result<Value> {
    let h1 = curves::handle_param(p, "h1").ok_or_else(|| bad("centerline", "`h1` is required"))?;
    let h2 = curves::handle_param(p, "h2").ok_or_else(|| bad("centerline", "`h2` is required"))?;
    added(center_line(s, h1, h2)?)
}

pub(super) struct CenterM {
    pub(super) first: Option<Handle>,
    pub(super) mark: bool,
}

impl Interactive for CenterM {
    fn name(&self) -> &'static str {
        if self.mark { "CENTERMARK" } else { "CENTERLINE" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.mark, self.first) {
            (true, _) => Prompt::new("Select circle or arc to add center mark", Accept::POINT),
            (false, None) => Prompt::new("Select first line", Accept::POINT),
            (false, Some(_)) => Prompt::new("Select second line", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                if self.mark {
                    center_mark(s, h)?;
                    return Ok(Step::Continue);
                }
                match self.first {
                    None => {
                        if !matches!(curves::entity(s, h)?.kind, EntityKind::Line(_)) {
                            return Err(other("Select a line."));
                        }
                        self.first = Some(h);
                        Ok(Step::Continue)
                    }
                    Some(h1) => {
                        center_line(s, h1, h)?;
                        Ok(Step::Done)
                    }
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}
