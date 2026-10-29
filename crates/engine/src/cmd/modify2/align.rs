use super::{
    Accept, EntityKind, Handle, Input, Interactive, Mat3, Prompt, Result, SelOutcome, SelectPhase, Session, Step, Value, Vec2, bool_or, curves, json,
    line, point_param, point_req, targets,
};

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

pub(super) fn run_align(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let s1 = point_req("align", p, "s1")?;
    let d1 = point_req("align", p, "d1")?;
    let pair2 = point_param(p, "s2").zip(point_param(p, "d2"));
    let m = align_matrix(s1, d1, pair2, bool_or(p, "scale", false));
    let r = crate::cmd::modify::transform_entities(s, &hs, &m, false)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
pub(super) struct AlignM {
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
        crate::cmd::modify::transform_entities(s, &self.objs, &m, false)?;
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
