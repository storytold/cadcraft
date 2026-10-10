//! HATCH, GRADIENT, BOUNDARY, REGION: boundaries from pick points or selected closed objects.

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, EntityKind, Gradient, Handle, Hatch, HatchLoop, Space, entity_bounds};
use cadcraft_geom::{Bounds2, PolyVertex, Polyline, Vec2, point_in_polygon};
use serde_json::{Value, json};

use super::helpers::lwpoly;
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("hatch", "Hatch...", run_hatch).menu(&["Draw", "Hatch..."]).alias(&["h", "bh", "-hatch"]).params("{points?: [[x,y]] (internal points) | handles?: [hex] (closed objects), pattern?: \"ANSI31\", scale?, angle? (degrees), associative?}").interactive(|s| Ok(Box::new(HatchM::new(s, false)))),
        CommandSpec::new("gradient", "Gradient...", run_gradient).menu(&["Draw", "Gradient..."]).alias(&["gd"]).params("{points? | handles?, color1?, color2?, angle?}").interactive(|s| Ok(Box::new(HatchM::new(s, true)))),
        CommandSpec::new("boundary", "Boundary...", run_boundary).menu(&["Draw", "Boundary..."]).alias(&["bo", "bpoly"]).params("{points: [[x,y]]}").interactive(|_| Ok(Box::new(BoundaryM { region: false }))),
        CommandSpec::new("region", "Region", run_region).menu(&["Draw", "Region"]).alias(&["reg"]).params("{handles?}"),
        CommandSpec::new("hatchedit", "Hatch Edit", run_hatchedit).menu(&["Modify", "Object", "Hatch..."]).alias(&["he"]).params("{handles?, pattern?, scale?, angle?}"),
    ]
}

/// Tessellated outlines of every visible curve in the space (hatch boundary candidates).
fn outlines(d: &Drawing, space: &Space, near: Option<Bounds2>) -> Vec<Vec<Vec2>> {
    let Some(store) = d.space(space) else { return Vec::new() };
    let ext = d.extents(space);
    let tol = (ext.width() + ext.height()).max(1e-9) / 20000.0;
    let mut out = Vec::new();
    for e in store.iter() {
        if !d.is_visible(e) || matches!(e.kind, EntityKind::Hatch(_) | EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_)) {
            continue;
        }
        if let Some(n) = near
            && !matches!(e.kind, EntityKind::XLine(_) | EntityKind::Ray(_))
            && !entity_bounds(d, e, 0).intersects(&n)
        {
            continue;
        }
        let mut polys = crate::select::hit_polylines(d, e, tol);
        if matches!(e.kind, EntityKind::XLine(_) | EntityKind::Ray(_)) {
            // Clip infinite lines to a generous box around the extents.
            let r = (ext.width() + ext.height()).max(1.0) * 4.0;
            for pl in &mut polys {
                if let [a, b] = pl.as_slice() {
                    let m = a.mid(*b);
                    let dir = (*b - *a).normalized();
                    *pl = vec![m - dir * r, m + dir * r];
                }
            }
        }
        out.extend(polys);
    }
    out
}

/// Closed loops of entities that are closed curves (for islands and "select objects").
fn closed_loop(d: &Drawing, e: &Entity) -> Option<Vec<PolyVertex>> {
    match &e.kind {
        EntityKind::Circle(c) => {
            let ctr = c.center.xy();
            Some(vec![PolyVertex::with_bulge(ctr - Vec2::X * c.radius, 1.0), PolyVertex::with_bulge(ctr + Vec2::X * c.radius, 1.0)])
        }
        EntityKind::LwPolyline(p) if p.closed || p.vertices.first().zip(p.vertices.last()).is_some_and(|(a, b)| a.p.near(b.p, 1e-9)) => {
            Some(p.vertices.clone())
        }
        EntityKind::Ellipse(el) => {
            let ge = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            if !ge.is_full() {
                return None;
            }
            let mut pts = Vec::new();
            ge.tessellate(el.major.xy().len() * 1e-3, &mut pts);
            pts.pop();
            Some(pts.into_iter().map(PolyVertex::new).collect())
        }
        EntityKind::Spline(sp) if sp.closed => Some(sp.tessellate(1e-3).into_iter().map(PolyVertex::new).collect()),
        _ => {
            let _ = d;
            None
        }
    }
}

/// Boundary loops (outer + islands) around an internal point.
pub(crate) fn loops_at(s: &Session, p: Vec2) -> Result<Vec<HatchLoop>> {
    let d = s.doc()?;
    let space = s.space();
    let ext = d.extents(&space);
    let tol = ((ext.width() + ext.height()) * 1e-9).max(1e-9);
    let polys = outlines(d, &space, None);
    let outer = cadcraft_geom::region::enclosing_loop(&polys, p, tol).ok_or_else(|| EngineError::Other("Valid hatch boundary not found.".into()))?;
    let ob = Bounds2::from_points(outer.iter().copied());
    let mut loops = vec![HatchLoop { vertices: outer.iter().map(|q| PolyVertex::new(*q)).collect(), outer: true }];
    // Islands: closed objects entirely inside the outer loop that don't contain the pick point.
    if let Some(store) = d.space(&space) {
        for e in store.iter() {
            if !d.is_visible(e) {
                continue;
            }
            let eb = entity_bounds(d, e, 0);
            if !ob.contains_box(&eb) {
                continue;
            }
            if let Some(vs) = closed_loop(d, e) {
                let pts = Polyline { vertices: vs.clone(), closed: true }.tessellate(tol.max(eb.width() * 1e-3));
                let inside = pts.iter().all(|q| point_in_polygon(&outer, *q) || outer.iter().any(|o| o.near(*q, tol * 10.0)));
                let same_as_outer = (cadcraft_geom::shoelace(&pts).abs() - cadcraft_geom::shoelace(&outer).abs()).abs() < tol.max(1e-6) * 1000.0;
                if inside && !same_as_outer && !point_in_polygon(&pts, p) {
                    loops.push(HatchLoop { vertices: vs, outer: false });
                }
            }
        }
    }
    Ok(loops)
}

pub(crate) fn loops_from_handles(s: &Session, hs: &[Handle]) -> Result<Vec<HatchLoop>> {
    let d = s.doc()?;
    let mut loops = Vec::new();
    for h in hs {
        if let Some(vs) = d.entity(*h).and_then(|e| closed_loop(d, e)) {
            loops.push(HatchLoop { vertices: vs, outer: loops.is_empty() });
        }
    }
    if loops.is_empty() {
        return Err(EngineError::Other("No closed boundary objects selected.".into()));
    }
    Ok(loops)
}

#[derive(Clone)]
struct HatchSettings {
    pattern: String,
    scale: f64,
    angle: f64,
    gradient: Option<Gradient>,
    associative: bool,
}

impl HatchSettings {
    fn from(s: &Session, p: &Value, gradient: bool) -> Self {
        let hdr = s.doc().map(|d| (d.header.str("HPNAME", "ANSI31"), d.header.f64("HPSCALE", 1.0), d.header.f64("HPANG", 0.0))).unwrap_or((
            "ANSI31".into(),
            1.0,
            0.0,
        ));
        let pattern = str_param(p, "pattern").map(str::to_ascii_uppercase).unwrap_or(hdr.0);
        let g = gradient.then(|| Gradient {
            name: str_param(p, "name").unwrap_or("LINEAR").to_string(),
            color1: str_param(p, "color1").and_then(Color::parse).unwrap_or(Color::Index(5)),
            color2: str_param(p, "color2").and_then(Color::parse).unwrap_or(Color::Index(7)),
            angle: f64_or(p, "angle", 0.0).to_radians(),
            centered: true,
        });
        HatchSettings {
            pattern,
            scale: f64_or(p, "scale", hdr.1).max(1e-9),
            angle: p.get("angle").and_then(Value::as_f64).map(f64::to_radians).unwrap_or(hdr.2),
            gradient: g,
            associative: bool_or(p, "associative", true),
        }
    }
    fn hatch(&self, loops: Vec<HatchLoop>) -> EntityKind {
        let solid = self.gradient.is_some() || self.pattern == "SOLID";
        EntityKind::Hatch(Hatch {
            pattern: if self.gradient.is_some() { "SOLID".into() } else { self.pattern.clone() },
            solid,
            loops,
            scale: self.scale,
            angle: self.angle,
            associative: self.associative,
            style: 0,
            elevation: 0.0,
            gradient: self.gradient.clone(),
            origin: Vec2::ZERO,
            background: None,
        })
    }
}

fn make(s: &mut Session, p: &Value, gradient: bool) -> Result<Value> {
    let st = HatchSettings::from(s, p, gradient);
    let mut loops = Vec::new();
    if let Some(pts) = points_param(p, "points") {
        for q in pts {
            loops.extend(loops_at(s, q)?);
        }
    } else {
        let hs = targets(s, p)?;
        loops = loops_from_handles(s, &hs)?;
    }
    if loops.is_empty() {
        return Err(bad("hatch", "give internal `points` or boundary `handles`"));
    }
    let h = s.add_entity(st.hatch(loops))?;
    // Hatches go under other objects (HPDRAWORDER = behind boundary).
    let space = s.space();
    if let Some(store) = s.doc_mut()?.space_mut(&space) {
        store.send_to_back(h);
    }
    Ok(json!({ "handle": h.hex() }))
}

fn run_hatch(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, false)
}
fn run_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, true)
}

fn boundary_at(s: &mut Session, q: Vec2) -> Result<Vec<Handle>> {
    let loops = loops_at(s, q)?;
    let mut out = Vec::new();
    for l in loops {
        out.push(s.add_entity(lwpoly(l.vertices, true))?);
    }
    Ok(out)
}

fn run_boundary(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("boundary", "`points` is required"))?;
    let mut out = Vec::new();
    for q in pts {
        out.extend(boundary_at(s, q)?.into_iter().map(|h| h.hex()));
    }
    Ok(json!({ "handles": out, "message": format!("BOUNDARY created {} polyline(s)", out.len()) }))
}

/// REGION: closed objects become closed polylines (our region representation for now).
fn run_region(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let mut n = 0;
    for h in hs {
        let vs = {
            let d = s.doc()?;
            d.entity(h).and_then(|e| closed_loop(d, e).map(|v| (v, e.common.clone())))
        };
        if let Some((vs, common)) = vs {
            let d = s.doc_mut()?;
            d.modify_entity(h, |e| {
                e.kind = lwpoly(vs, true);
                e.common = common;
            })?;
            n += 1;
        }
    }
    Ok(json!({ "message": format!("{n} Region(s) created.") }))
}

fn run_hatchedit(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let mut q = p.clone();
    if let Some(o) = q.as_object_mut() {
        o.insert("handles".into(), json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
    }
    s.execute("properties.set", &q)
}

struct HatchM {
    gradient: bool,
    settings: HatchSettings,
    loops: Vec<HatchLoop>,
    asking: Option<&'static str>,
    selecting: bool,
}

impl HatchM {
    fn new(s: &Session, gradient: bool) -> Self {
        HatchM { gradient, settings: HatchSettings::from(s, &Value::Null, gradient), loops: Vec::new(), asking: None, selecting: false }
    }
}

impl Interactive for HatchM {
    fn name(&self) -> &'static str {
        if self.gradient { "GRADIENT" } else { "HATCH" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!(
            "Current pattern: {}  Scale: {:.4}  Angle: {:.0}",
            self.settings.pattern,
            self.settings.scale,
            self.settings.angle.to_degrees()
        ));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(a) = self.asking {
            return Prompt::new(a, Accept::TEXT);
        }
        if self.selecting {
            return Prompt::new("Select objects", Accept::SELECT);
        }
        Prompt::new("Pick internal point", Accept::POINT).kw(&["Select objects", "Undo", "seTtings", "Pattern", "sCale", "Angle"])
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(a) = self.asking.take() {
            if let Input::Text(t) | Input::Keyword(t) = &i {
                match a {
                    "Enter a pattern name" => {
                        let up = t.trim().to_ascii_uppercase();
                        if cadcraft_doc::library::pattern(&up).is_some() {
                            self.settings.pattern = up.clone();
                            s.doc_mut()?.header.set_str("HPNAME", &up);
                        } else {
                            s.echo(format!(
                                "Unknown pattern {up}. Available: {}",
                                cadcraft_doc::library::standard_patterns().iter().map(|p| p.name).collect::<Vec<_>>().join(", ")
                            ));
                        }
                    }
                    "Specify a scale for the hatch pattern" => {
                        if let Some(v) = super::machines::number(t).filter(|v| *v > 0.0) {
                            self.settings.scale = v;
                            s.doc_mut()?.header.set_f64("HPSCALE", v);
                        }
                    }
                    _ => {
                        if let Some(a) = crate::units::parse_angle(t) {
                            self.settings.angle = a;
                            s.doc_mut()?.header.set_f64("HPANG", a);
                        }
                    }
                }
            }
            return Ok(Step::Continue);
        }
        if self.selecting {
            match i {
                Input::Pick(hs) => match loops_from_handles(s, &hs) {
                    Ok(l) => self.loops.extend(l),
                    Err(e) => s.echo(e.to_string()),
                },
                Input::Enter => self.selecting = false,
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => {
                match loops_at(s, p) {
                    Ok(l) => {
                        s.echo(format!("Analyzing the selected data... {} loop(s)", l.len()));
                        self.loops.extend(l);
                    }
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) => {
                match k.as_str() {
                    "Select objects" => self.selecting = true,
                    "Undo" => {
                        self.loops.clear();
                    }
                    "Pattern" | "seTtings" => self.asking = Some("Enter a pattern name"),
                    "sCale" => self.asking = Some("Specify a scale for the hatch pattern"),
                    _ => self.asking = Some("Specify an angle for the hatch pattern"),
                }
                Ok(Step::Continue)
            }
            Input::Enter => {
                if !self.loops.is_empty() {
                    let k = self.settings.hatch(std::mem::take(&mut self.loops));
                    let h = s.add_entity(k)?;
                    let space = s.space();
                    if let Some(store) = s.doc_mut()?.space_mut(&space) {
                        store.send_to_back(h);
                    }
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        if self.loops.is_empty() {
            return Vec::new();
        }
        vec![self.settings.hatch(self.loops.clone())]
    }
}

struct BoundaryM {
    region: bool,
}

impl Interactive for BoundaryM {
    fn name(&self) -> &'static str {
        if self.region { "REGION" } else { "BOUNDARY" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Pick internal point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                match boundary_at(s, p) {
                    Ok(hs) => s.echo(format!("BOUNDARY created {} polyline(s)", hs.len())),
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The drawing's only hatch and its handle (hatches go to the back of the draw order, so not the last entity).
    fn only_hatch(s: &Session) -> (String, Hatch) {
        let d = s.doc().unwrap();
        d.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((e.handle.hex(), h.clone())) } else { None }).unwrap()
    }

    #[test]
    fn hatchedit_pattern_replaces_gradient() {
        let mut s = Session::new();
        s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        s.execute("gradient", &json!({ "points": [[5, 5]], "color1": "1", "color2": "3" })).unwrap();
        let (h, hatch) = only_hatch(&s);
        assert!(hatch.gradient.is_some());
        // Scale-only and angle-only edits keep the gradient.
        s.execute("hatchedit", &json!({ "handles": [h.clone()], "scale": 2, "angle": 30 })).unwrap();
        assert!(only_hatch(&s).1.gradient.is_some());
        s.execute("hatchedit", &json!({ "handles": [h], "pattern": "ANSI31" })).unwrap();
        let hatch = only_hatch(&s).1;
        assert_eq!(hatch.pattern, "ANSI31");
        assert!(!hatch.solid);
        assert!(hatch.gradient.is_none());
    }
}
