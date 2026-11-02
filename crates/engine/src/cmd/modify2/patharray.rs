use super::{
    Accept, Chain, Handle, Input, Interactive, Mat3, Prompt, Result, SelOutcome, SelectPhase, Session, Step, Value, bad, bool_or, curves, json,
    number, other, targets,
};

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
        (_, Some(sp)) if sp.is_finite() && sp > 0.0 => (((l / sp).floor() as usize).saturating_add(1), sp),
        (Some(n), _) => {
            let n = n.max(1);
            (n, if c.closed { l / n as f64 } else { l / (n.max(2) - 1) as f64 })
        }
        _ => (6, if c.closed { l / 6.0 } else { l / 5.0 }),
    };
    crate::cmd::array::check_size(s, n as u64, objs.len())?;
    let Some((p0, t0)) = c.at_length(0.0) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for i in 1..n {
        let Some((p, t)) = c.at_length(step * i as f64) else { continue };
        let mut m = Mat3::translate(p - p0);
        if align {
            m = Mat3::rotate_about(p, t.angle() - t0.angle()).then_before(m);
        }
        out.extend(crate::cmd::modify::transform_entities(s, &objs, &m, true)?);
    }
    Ok(out)
}

pub(super) fn run_arraypath(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let path = curves::handle_param(p, "path").ok_or_else(|| bad("arraypath", "`path` (handle) is required"))?;
    let count = p.get("count").and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    let spacing = p.get("spacing").and_then(Value::as_f64);
    let r = arraypath(s, &hs, path, count, spacing, bool_or(p, "align", true))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
pub(in crate::cmd) struct PathArrayM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    path: Option<Handle>,
}

impl PathArrayM {
    /// Start at the path prompt with the objects already selected (ARRAY > PAth).
    pub(in crate::cmd) fn with_objects(objs: Vec<Handle>) -> Self {
        PathArrayM { sel: SelectPhase { picked: objs.clone(), done: true, ..SelectPhase::default() }, objs, path: None }
    }
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
                    Input::Text(t) => number(t).filter(|n| *n >= 1.0).ok_or_else(|| other("Requires a count of 1 or more."))? as usize,
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
