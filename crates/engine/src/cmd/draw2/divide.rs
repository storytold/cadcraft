use super::{
    Accept, Chain, EntityKind, Handle, Input, Interactive, MAX_GEN, Point, Prompt, Result, Session, Step, Value, Vec2, bad, bool_or, curves, f64_req,
    json, number, other, point_param, str_param, targets, v3,
};

// =====================================================================================
// DIVIDE / MEASURE
// =====================================================================================

fn chain_of(s: &Session, h: Handle) -> Result<Chain> {
    let e = curves::entity(s, h)?;
    Chain::of(&e.kind).ok_or_else(|| other("Cannot divide/measure that object."))
}

fn place_markers(s: &mut Session, at: &[(Vec2, Vec2)], block: Option<&str>, align: bool) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for (p, t) in at {
        let h = match block {
            Some(b) => crate::cmd::blocks::insert(s, b, *p, 1.0, if align { t.angle() } else { 0.0 }, &serde_json::Map::new())?,
            None => s.add_entity(EntityKind::Point(Point { p: v3(*p), angle: 0.0 }))?,
        };
        out.push(h.hex());
    }
    Ok(out)
}

fn check_block(s: &Session, b: Option<&str>) -> Result<()> {
    if let Some(b) = b
        && s.doc()?.block(b).is_none()
    {
        return Err(other(format!("Block \"{b}\" not found.")));
    }
    Ok(())
}

pub(crate) fn divide_points(c: &Chain, n: usize) -> Vec<(Vec2, Vec2)> {
    let l = c.len();
    let range = if c.closed { 0..n } else { 1..n };
    range.filter_map(|i| c.at_length(l * i as f64 / n as f64)).collect()
}

pub(crate) fn measure_points(c: &Chain, step: f64) -> Vec<(Vec2, Vec2)> {
    let l = c.len();
    let mut out = Vec::new();
    let mut k = 1usize;
    while (k as f64) * step < l - 1e-9 && out.len() < MAX_GEN {
        let d = k as f64 * step;
        out.extend(c.at_length(d));
        k += 1;
    }
    out
}

pub(super) fn run_divide(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("divide", "`handle` is required"))?;
    let n = p.get("segments").and_then(Value::as_u64).ok_or_else(|| bad("divide", "`segments` (2..32767) is required"))?;
    if !(2..=32767).contains(&n) {
        return Err(bad("divide", "segments must be 2..32767"));
    }
    let block = str_param(p, "block").map(str::to_string);
    check_block(s, block.as_deref())?;
    let c = chain_of(s, h)?;
    let pts = divide_points(&c, n as usize);
    let hs = place_markers(s, &pts, block.as_deref(), bool_or(p, "align", true))?;
    Ok(json!({ "handles": hs }))
}

pub(super) fn run_measure(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("measure", "`handle` is required"))?;
    let step = f64_req("measure", p, "length")?;
    let mut c = chain_of(s, h)?;
    if step.is_nan() || step <= 0.0 || c.len() / step > MAX_GEN as f64 {
        return Err(bad("measure", "length must be positive (and not tiny)"));
    }
    let block = str_param(p, "block").map(str::to_string);
    check_block(s, block.as_deref())?;
    if let Some(from) = point_param(p, "from")
        && !c.closed
        && let (Some(a), Some(b)) = (c.start(), c.end())
        && from.dist(b) < from.dist(a)
    {
        c = c.reversed();
    }
    let pts = measure_points(&c, step);
    let hs = place_markers(s, &pts, block.as_deref(), bool_or(p, "align", true))?;
    Ok(json!({ "handles": hs }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DivPhase {
    Object,
    Value,
    BlockName,
    Align,
}

pub(super) struct DivideM {
    measure: bool,
    obj: Option<(Handle, Vec2)>,
    phase: DivPhase,
    block: Option<String>,
    align: bool,
}

impl DivideM {
    pub(super) fn new(measure: bool) -> Self {
        DivideM { measure, obj: None, phase: DivPhase::Object, block: None, align: true }
    }
}

impl Interactive for DivideM {
    fn name(&self) -> &'static str {
        if self.measure { "MEASURE" } else { "DIVIDE" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.phase {
            DivPhase::Object => Prompt::new(if self.measure { "Select object to measure" } else { "Select object to divide" }, Accept::POINT),
            DivPhase::Value => {
                let m = if self.measure { "Specify length of segment" } else { "Enter the number of segments" };
                let p = Prompt::new(m, if self.measure { Accept::POINT_OR_NUMBER } else { Accept::NUMBER });
                let p = if self.measure { p.base_opt(self.obj.map(|o| o.1)) } else { p };
                if self.block.is_some() { p } else { p.kw(&["Block"]) }
            }
            DivPhase::BlockName => Prompt::new("Enter name of block to insert", Accept::TEXT),
            DivPhase::Align => Prompt::new("Align block with object?", curves::KW).kw(&["Yes", "No"]).default("Y"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.phase, i) {
            (DivPhase::Object, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                chain_of(s, h)?;
                self.obj = Some((h, p));
                self.phase = DivPhase::Value;
                Ok(Step::Continue)
            }
            (DivPhase::Value, Input::Keyword(k)) if k == "Block" => {
                self.phase = DivPhase::BlockName;
                Ok(Step::Continue)
            }
            (DivPhase::BlockName, Input::Text(t) | Input::Keyword(t)) => {
                let name = t.trim().to_string();
                check_block(s, Some(&name))?;
                self.block = Some(name);
                self.phase = DivPhase::Align;
                Ok(Step::Continue)
            }
            (DivPhase::Align, Input::Keyword(k) | Input::Text(k)) => {
                self.align = !k.trim().to_ascii_lowercase().starts_with('n');
                self.phase = DivPhase::Value;
                Ok(Step::Continue)
            }
            (DivPhase::Align, Input::Enter) => {
                self.phase = DivPhase::Value;
                Ok(Step::Continue)
            }
            (DivPhase::Value, inp @ (Input::Text(_) | Input::Point(_))) => {
                let Some((h, pick)) = self.obj else { return Ok(Step::Cancel) };
                let v = match inp {
                    Input::Text(t) => number(&t).ok_or_else(|| other("Requires a number."))?,
                    Input::Point(q) => pick.dist(q),
                    _ => return Ok(Step::Continue),
                };
                let mut params = json!({ "handle": h.hex(), "align": self.align, "from": [pick.x, pick.y] });
                if let Some(b) = &self.block {
                    params["block"] = json!(b);
                }
                if self.measure {
                    params["length"] = json!(v);
                    run_measure(s, &params)?;
                } else {
                    if !(2.0..=32767.0).contains(&v) || v.fract() != 0.0 {
                        return Err(other("Requires an integer between 2 and 32767."));
                    }
                    params["segments"] = json!(v as u64);
                    run_divide(s, &params)?;
                }
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Err(other("Invalid input.")),
        }
    }
}
