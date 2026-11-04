use super::{
    Accept, EntityKind, Handle, Input, Interactive, MAX_GEN, PolyVertex, Prompt, Result, Session, Step, Value, Vec2, Wipeout, added, bad, bool_or,
    curves, json, lwpoly, other, points_param, shoelace, str_param,
};

// =====================================================================================
// WIPEOUT
// =====================================================================================

fn set_frames(s: &mut Session, mode: &str) -> Result<i64> {
    let v = match mode.trim().to_ascii_lowercase().as_str() {
        "on" | "1" => 1,
        "off" | "0" => 0,
        "display" | "d" | "2" | "display but not plot" => 2,
        _ => return Err(other("Frames mode must be ON, OFF or Display.")),
    };
    s.doc_mut()?.header.set_i64("WIPEOUTFRAME", v);
    Ok(v)
}

fn wipeout_from_polyline(s: &mut Session, h: Handle, erase: bool) -> Result<Handle> {
    let e = curves::entity(s, h)?;
    let EntityKind::LwPolyline(p) = &e.kind else { return Err(other("Select a closed polyline.")) };
    if !p.closed || p.vertices.len() < 3 {
        return Err(other("The polyline must be closed with at least three vertices."));
    }
    if p.vertices.iter().any(|v| v.bulge.abs() > 1e-12) {
        return Err(other("Polylines with arc segments cannot be used for wipeouts."));
    }
    let nh = s.add_entity(EntityKind::Wipeout(Wipeout { boundary: p.vertices.iter().map(|v| v.p).collect() }))?;
    if erase {
        s.doc_mut()?.remove_entity(h);
    }
    Ok(nh)
}

pub(super) fn run_wipeout(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(f) = str_param(p, "frames") {
        let v = set_frames(s, f)?;
        return Ok(json!({ "WIPEOUTFRAME": v }));
    }
    if let Some(h) = curves::handle_param(p, "handle") {
        return added(wipeout_from_polyline(s, h, bool_or(p, "erase", false))?);
    }
    let pts = points_param(p, "points").ok_or_else(|| bad("wipeout", "`points` (3 or more) or `handle` is required"))?;
    if pts.len() < 3 || pts.len() > MAX_GEN || shoelace(&pts).abs() < 1e-12 {
        return Err(bad("wipeout", "need 3+ points enclosing an area"));
    }
    added(s.add_entity(EntityKind::Wipeout(Wipeout { boundary: pts }))?)
}

#[derive(Default)]
pub(super) struct WipeoutM {
    pts: Vec<Vec2>,
    phase: u8, // 0 points, 1 select polyline, 2 erase?, 3 frames
    poly: Option<Handle>,
}

impl Interactive for WipeoutM {
    fn name(&self) -> &'static str {
        "WIPEOUT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.phase, self.pts.len()) {
            (1, _) => Prompt::new("Select a closed polyline", Accept::POINT),
            (2, _) => Prompt::new("Erase polyline?", curves::KW).kw(&["Yes", "No"]).default("No"),
            (3, _) => Prompt::new("Enter mode", curves::KW).kw(&["ON", "OFF", "Display but not plot"]).default("ON"),
            (_, 0) => Prompt::new("Specify first point", Accept::POINT).kw(&["Frames", "Polyline"]).default("Polyline"),
            (_, 1) => Prompt::new("Specify next point", Accept::POINT).base_opt(self.pts.last().copied()),
            (_, _) => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.phase, i) {
            (1, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                let e = curves::entity(s, h)?;
                if !matches!(&e.kind, EntityKind::LwPolyline(pl) if pl.closed) {
                    return Err(other("Select a closed polyline."));
                }
                self.poly = Some(h);
                self.phase = 2;
                Ok(Step::Continue)
            }
            (2, inp @ (Input::Keyword(_) | Input::Text(_) | Input::Enter)) => {
                let erase = matches!(&inp, Input::Keyword(k) | Input::Text(k) if k.to_ascii_lowercase().starts_with('y'));
                let h = self.poly.ok_or_else(|| other("No polyline."))?;
                wipeout_from_polyline(s, h, erase)?;
                Ok(Step::Done)
            }
            (3, inp @ (Input::Keyword(_) | Input::Text(_) | Input::Enter)) => {
                let m = match &inp {
                    Input::Keyword(k) | Input::Text(k) => k.clone(),
                    _ => "on".into(),
                };
                let m = if m.to_ascii_lowercase().starts_with('d') { "display".to_string() } else { m };
                set_frames(s, &m)?;
                Ok(Step::Done)
            }
            (0, Input::Keyword(k)) => {
                match k.as_str() {
                    "Frames" => self.phase = 3,
                    "Polyline" => self.phase = 1,
                    "Undo" => {
                        self.pts.pop();
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            (0, Input::Point(p)) => {
                if self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                }
                Ok(Step::Continue)
            }
            (0, Input::Enter) if self.pts.is_empty() => {
                self.phase = 1;
                Ok(Step::Continue)
            }
            (0, Input::Enter) => {
                if self.pts.len() < 3 || shoelace(&self.pts).abs() < 1e-12 {
                    return Err(other("A wipeout needs three or more points enclosing an area."));
                }
                s.add_entity(EntityKind::Wipeout(Wipeout { boundary: self.pts.clone() }))?;
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.phase != 0 || self.pts.is_empty() {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        pts.push(c);
        vec![lwpoly(pts.into_iter().map(PolyVertex::new).collect(), true)]
    }
}
