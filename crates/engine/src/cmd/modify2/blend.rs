use super::{
    Accept, Chain, EntityKind, Handle, Input, Interactive, Prompt, Result, Session, Spline, Step, Value, Vec2, bad, curves, json, other, point_req,
    str_param,
};

// =====================================================================================
// BLEND
// =====================================================================================

/// Endpoint of a curve nearest `pick` and the outward unit tangent there.
fn curve_end(k: &EntityKind, pick: Vec2) -> Option<(Vec2, Vec2)> {
    let c = Chain::of(k)?;
    if c.closed {
        return None;
    }
    let (a, b) = (c.start()?, c.end()?);
    if pick.dist(b) <= pick.dist(a) {
        let t = c.segs.last()?.tangent(1.0);
        Some((b, t))
    } else {
        let t = c.segs.first()?.tangent(0.0);
        Some((a, -t))
    }
}

pub(crate) fn blend_spline(e1: (Vec2, Vec2), e2: (Vec2, Vec2), smooth: bool) -> Option<Spline> {
    let (p1, t1) = e1;
    let (p2, t2) = e2;
    let d = p1.dist(p2);
    if d < 1e-12 {
        return None;
    }
    let ctrl = if smooth {
        let k = d / 5.0;
        vec![p1, p1 + t1 * k, p1 + t1 * (2.0 * k), p2 + t2 * (2.0 * k), p2 + t2 * k, p2]
    } else {
        let k = d / 3.0;
        vec![p1, p1 + t1 * k, p2 + t2 * k, p2]
    };
    let deg = ctrl.len() - 1;
    Some(Spline::from_control(ctrl, deg))
}

fn blend(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, smooth: bool) -> Result<Handle> {
    let k1 = curves::entity(s, h1)?.kind;
    let k2 = curves::entity(s, h2)?.kind;
    let e1 = curve_end(&k1, p1).ok_or_else(|| other("Select an open line, arc, polyline, ellipse arc or spline."))?;
    let e2 = curve_end(&k2, p2).ok_or_else(|| other("Select an open line, arc, polyline, ellipse arc or spline."))?;
    let sp = blend_spline(e1, e2, smooth).ok_or_else(|| other("The ends coincide."))?;
    s.add_entity(EntityKind::Spline(sp))
}

pub(super) fn run_blend(s: &mut Session, p: &Value) -> Result<Value> {
    let h1 = curves::handle_param(p, "h1").ok_or_else(|| bad("blend", "`h1` is required"))?;
    let h2 = curves::handle_param(p, "h2").ok_or_else(|| bad("blend", "`h2` is required"))?;
    let p1 = point_req("blend", p, "p1")?;
    let p2 = point_req("blend", p, "p2")?;
    let smooth = str_param(p, "continuity").is_some_and(|c| c.eq_ignore_ascii_case("smooth"));
    let h = blend(s, h1, p1, h2, p2, smooth)?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
pub(super) struct BlendM {
    first: Option<(Handle, Vec2)>,
    smooth: bool,
    asking: bool,
}

impl Interactive for BlendM {
    fn name(&self) -> &'static str {
        "BLEND"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        s.echo(format!("Continuity = {}", if self.smooth { "Smooth" } else { "Tangent" }));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking {
            return Prompt::new("Enter continuity", curves::KW).kw(&["Tangent", "Smooth"]).default("Tangent");
        }
        match self.first {
            None => Prompt::new("Select first object or", Accept::POINT).kw(&["CONtinuity"]),
            Some(_) => Prompt::new("Select second point", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking {
            if let Input::Keyword(k) | Input::Text(k) = &i {
                self.smooth = k.to_ascii_lowercase().starts_with('s');
            }
            self.asking = false;
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(_) => {
                self.asking = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                match self.first {
                    None => {
                        let k = curves::entity(s, h)?.kind;
                        curve_end(&k, p).ok_or_else(|| other("Select an open curve."))?;
                        self.first = Some((h, p));
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) => {
                        blend(s, h1, p1, h, p, self.smooth)?;
                        Ok(Step::Done)
                    }
                }
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}
