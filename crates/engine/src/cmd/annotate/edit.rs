use super::common::{hex_param, style};
use super::{
    Accept, DimKind, EntityKind, Handle, Input, Interactive, Prompt, Result, SelOutcome, SelectPhase, Session, Step, Value, Vec2, bad, json,
    point_param, str_param, targets, v3,
};

pub(super) fn run_update(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let st = style(s);
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let _ = d.modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.style = st.clone();
                dm.block = None;
                n += 1;
            }
        });
    }
    Ok(json!({ "updated": n }))
}

/// DIMOVERRIDE: per-dimension style overrides (`clear` removes them); `text` sets the text.
pub(super) fn run_override(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("text").is_none() {
        return crate::cmd::props::dim_override(s, p);
    }
    let hs = targets(s, p)?;
    let t = str_param(p, "text").unwrap_or("").to_string();
    // Dimension variables given next to `text` are applied first, so an unknown name still errors before anything changes.
    let vars: serde_json::Map<String, Value> = p
        .as_object()
        .map(|o| o.iter().filter(|(k, _)| !matches!(k.as_str(), "text" | "handles" | "handle")).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let mut ignored = Value::Null;
    if !vars.is_empty() {
        let mut rest = vars;
        rest.insert("handles".into(), json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        ignored = crate::cmd::props::dim_override(s, &Value::Object(rest))?.get("ignored").cloned().unwrap_or(Value::Null);
    }
    let d = s.doc_mut()?;
    for h in &hs {
        let _ = d.modify_entity(*h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.text = t.clone();
                dm.block = None;
            }
        });
    }
    match ignored {
        Value::Null => Ok(json!({ "changed": hs.len() })),
        ig => Ok(json!({ "changed": hs.len(), "ignored": ig })),
    }
}

pub(super) fn dims_of(s: &Session, hs: &[Handle]) -> Vec<Handle> {
    let Ok(d) = s.doc() else { return Vec::new() };
    hs.iter().copied().filter(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Dimension(_)))).collect()
}

pub(super) fn run_reassociate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    let space = s.space();
    let mut plan = Vec::new();
    {
        let d = s.doc()?;
        for h in &hs {
            if let Some(EntityKind::Dimension(dm)) = d.entity(*h).map(|e| &e.kind) {
                plan.push((*h, crate::assoc::reassociate(d, &space, dm)));
            }
        }
    }
    let mut points = 0;
    let mut n = 0;
    let d = s.doc_mut()?;
    for (h, a) in plan {
        points += a.len();
        n += usize::from(!a.is_empty());
        let _ = d.modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.assoc = a;
            }
        });
    }
    Ok(json!({ "dimensions": hs.len(), "associated": n, "points": points, "message": format!("{n} of {} dimension(s) associated.", hs.len()) }))
}

pub(super) fn run_disassociate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in &hs {
        let _ = d.modify_entity(*h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind
                && !dm.assoc.is_empty()
            {
                dm.assoc.clear();
                n += 1;
            }
        });
    }
    Ok(json!({ "disassociated": n, "message": format!("{n} disassociated.") }))
}

/// DIMTEDIT: move/rotate/justify dimension text.
pub(super) fn run_tedit(s: &mut Session, p: &Value, mode: &str) -> Result<Value> {
    let mut q = p.as_object().cloned().unwrap_or_default();
    q.insert("mode".into(), json!(mode));
    run_dimtedit(s, &Value::Object(q))
}

pub(super) fn run_dimtedit(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("dimtedit", "select dimensions"));
    }
    let mode = str_param(p, "mode").unwrap_or("").to_ascii_lowercase();
    let at = point_param(p, "at");
    let angle = p.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).map(f64::to_radians);
    if mode == "angle" && angle.is_none() {
        return Err(bad("dimtedit", "`angle` (degrees) is required"));
    }
    let d = s.doc_mut()?;
    let dimscale = d.header.f64("DIMSCALE", 1.0);
    for h in &hs {
        let st = match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::Dimension(dm)) => d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides),
            _ => continue,
        };
        d.modify_entity(*h, |e| {
            let EntityKind::Dimension(dm) = &mut e.kind else { return };
            dm.block = None;
            if let Some(q) = at {
                dm.text_mid = v3(q);
                dm.user_text_pos = true;
                // DIMTMOVE 0: the dimension line moves with the text.
                st.follow_text(dm, dimscale);
            }
            match mode.as_str() {
                "home" => {
                    dm.user_text_pos = false;
                    dm.text_rotation = 0.0;
                    dm.overrides.remove("textJust");
                }
                "angle" => dm.text_rotation = angle.unwrap_or(0.0),
                "left" | "center" | "right" => {
                    dm.user_text_pos = false;
                    let j = match mode.as_str() {
                        "left" => 1,
                        "right" => 2,
                        _ => 0,
                    };
                    dm.overrides.insert("textJust".into(), json!(j));
                }
                _ => {}
            }
        })?;
    }
    Ok(json!({ "changed": hs.len() }))
}

/// DIMSPACE: equal spacing of parallel linear dimensions from a base dimension.
pub(super) fn run_dimspace(s: &mut Session, p: &Value) -> Result<Value> {
    let base = hex_param(p, "base").ok_or_else(|| bad("dimspace", "`base` dimension is required"))?;
    let hs: Vec<Handle> = dims_of(s, &targets(s, p)?).into_iter().filter(|h| *h != base).collect();
    let d = s.doc()?;
    let Some(EntityKind::Dimension(b)) = d.entity(base).map(|e| e.kind.clone()) else { return Err(bad("dimspace", "`base` is not a dimension")) };
    let st = d.dim_style(&b.style).cloned().unwrap_or_default().with_overrides(&b.overrides);
    let spacing = p
        .get("spacing")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(st.baseline_spacing * st.effective_scale(d.header.f64("DIMSCALE", 1.0)) * 2.0);
    let dir = match b.kind {
        DimKind::Linear { rotation } => Vec2::from_angle(rotation),
        DimKind::Aligned => (b.p14.xy() - b.p13.xy()).normalized(),
        _ => return Err(bad("dimspace", "base must be a linear or aligned dimension")),
    };
    let n = dir.perp();
    let base_off = (b.defpt.xy() - b.p13.xy()).dot(n);
    let side = if base_off >= 0.0 { 1.0 } else { -1.0 };
    // Order the others by their current distance from the base line.
    let mut others: Vec<(Handle, f64)> = hs
        .iter()
        .filter_map(|h| match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::Dimension(o)) if matches!(o.kind, DimKind::Linear { .. } | DimKind::Aligned) => {
                Some((*h, (o.defpt.xy() - b.defpt.xy()).dot(n) * side))
            }
            _ => None,
        })
        .collect();
    others.sort_by(|a, c| a.1.total_cmp(&c.1));
    let line_pt = b.defpt.xy();
    let dm = s.doc_mut()?;
    for (i, (h, _)) in others.iter().enumerate() {
        let target = line_pt + n * (side * spacing * (i as f64 + 1.0));
        dm.modify_entity(*h, |e| {
            if let EntityKind::Dimension(o) = &mut e.kind {
                let cur = o.defpt.xy();
                let moved = cur + n * (target - cur).dot(n);
                o.defpt = v3(moved);
                o.user_text_pos = false;
                o.block = None;
            }
        })?;
    }
    Ok(json!({ "spaced": others.len() }))
}

/// DIMSPACE at the command line: select the base dimension, select the dimensions to space, then
/// the spacing (Enter or Auto takes the DIMDLI-based default).
#[derive(Default)]
pub(super) struct DimSpaceM {
    base: Option<Handle>,
    sel: SelectPhase,
    spacing_phase: bool,
}

impl DimSpaceM {
    fn apply(&self, s: &mut Session, base: Handle, spacing: Option<f64>) -> Result<Step> {
        let mut p = serde_json::Map::new();
        p.insert("base".into(), json!(base.hex()));
        p.insert("handles".into(), json!(self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        if let Some(v) = spacing {
            p.insert("spacing".into(), json!(v));
        }
        let r = run_dimspace(s, &Value::Object(p))?;
        s.set_selection(Vec::new());
        s.echo(format!("{} dimension(s) spaced.", r.get("spaced").and_then(Value::as_u64).unwrap_or(0)));
        Ok(Step::Done)
    }
}

impl Interactive for DimSpaceM {
    fn name(&self) -> &'static str {
        "DIMSPACE"
    }
    fn begin(&mut self, _s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::single();
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.spacing_phase {
            return Prompt::new("Enter value", Accept::NUMBER).kw(&["Auto"]).default("Auto");
        }
        if self.base.is_none() {
            return Prompt::new("Select base dimension", Accept::SELECT);
        }
        Prompt::new("Select dimensions to space", Accept::SELECT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.spacing_phase {
            let Some(base) = self.base else { return Ok(Step::Cancel) };
            return match i {
                Input::Enter => self.apply(s, base, None),
                Input::Keyword(k) | Input::Text(k) if k.trim().eq_ignore_ascii_case("a") || k.trim().eq_ignore_ascii_case("auto") => {
                    self.apply(s, base, None)
                }
                Input::Text(t) => match crate::cmd::machines::number(&t).filter(|v| v.is_finite() && *v >= 0.0) {
                    Some(v) => self.apply(s, base, Some(v)),
                    None => {
                        s.echo("Requires a non-negative number or Auto.");
                        Ok(Step::Continue)
                    }
                },
                _ => Ok(Step::Continue),
            };
        }
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => {
                if self.base.is_none() {
                    match hs.first().copied().filter(|h| !dims_of(s, &[*h]).is_empty()) {
                        Some(b) => {
                            self.base = Some(b);
                            self.sel = SelectPhase::default();
                        }
                        None => {
                            s.echo("Select a dimension.");
                            self.sel = SelectPhase::single();
                        }
                    }
                } else {
                    self.sel.picked = hs;
                    self.spacing_phase = true;
                }
                Ok(Step::Continue)
            }
        }
    }
}

/// DIMOVERRIDE at the command line: variable, value (repeat), Enter, then select dimensions.
pub(super) struct OverrideM {
    vars: serde_json::Map<String, Value>,
    var: Option<String>,
    clear: bool,
    selecting: Option<crate::cmd::machines::SelectPhase>,
}

impl OverrideM {
    pub(super) fn new(_s: &Session) -> Self {
        OverrideM { vars: serde_json::Map::new(), var: None, clear: false, selecting: None }
    }
    fn apply(&self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        let mut p = self.vars.clone();
        p.insert("handles".into(), json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        if self.clear {
            p.insert("clear".into(), json!(true));
        }
        let r = crate::cmd::props::dim_override(s, &Value::Object(p))?;
        s.echo(format!("{} dimension(s) changed.", r.get("changed").and_then(Value::as_u64).unwrap_or(0)));
        Ok(Step::Done)
    }
}

impl Interactive for OverrideM {
    fn name(&self) -> &'static str {
        "DIMOVERRIDE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.selecting {
            return sel.prompt();
        }
        match &self.var {
            Some(v) => Prompt::new(format!("Enter new value for dimension variable {}", v.to_ascii_uppercase()), Accept::TEXT),
            None => Prompt::new("Enter dimension variable name to override", Accept::TEXT).kw(&["Clear overrides"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.selecting {
            return match sel.feed(s, &i)? {
                crate::cmd::machines::SelOutcome::More => Ok(Step::Continue),
                crate::cmd::machines::SelOutcome::Empty => Ok(Step::Done),
                crate::cmd::machines::SelOutcome::Done(hs) => self.apply(s, hs),
            };
        }
        match (self.var.take(), i) {
            (None, Input::Keyword(k)) if k.starts_with("Clear") => {
                self.clear = true;
                self.selecting = Some(crate::cmd::machines::SelectPhase::default());
            }
            (None, Input::Text(t)) => {
                let t = t.trim().to_string();
                if t.eq_ignore_ascii_case("c") {
                    self.clear = true;
                    self.selecting = Some(crate::cmd::machines::SelectPhase::default());
                } else if cadcraft_doc::DimStyle::field_name(&t).is_some() {
                    self.var = Some(t);
                } else {
                    s.echo(format!("Unknown dimension variable \"{t}\"."));
                }
            }
            (None, Input::Enter) => {
                if self.vars.is_empty() {
                    return Ok(Step::Cancel);
                }
                self.selecting = Some(crate::cmd::machines::SelectPhase::default());
            }
            (Some(v), Input::Text(t)) => {
                let val = t.trim().parse::<f64>().map(|x| json!(x)).unwrap_or_else(|_| json!(t.trim()));
                self.vars.insert(v, val);
            }
            (Some(v), _) => self.var = Some(v),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}
