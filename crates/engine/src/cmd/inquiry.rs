//! Inquiry: DIST, ID, LIST, AREA, drawing inspection for agents.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Polyline, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::units::{format_angle, format_distance};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("dist", "Distance", run_dist)
            .menu(&["Tools", "Inquiry", "Distance"])
            .alias(&["di"])
            .params("{p1, p2}")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(DistM::default()))),
        CommandSpec::new("id", "ID Point", run_id)
            .menu(&["Tools", "Inquiry", "ID Point"])
            .params("{at}")
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(IdM))),
        CommandSpec::new("list", "List", run_list)
            .menu(&["Tools", "Inquiry", "List"])
            .alias(&["li", "ls"])
            .params("{handles?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ListM::default()))),
        CommandSpec::new("area", "Area", run_area)
            .menu(&["Tools", "Inquiry", "Area"])
            .alias(&["aa"])
            .params("{points: [[x,y],...]} | {handle}")
            .noundo()
            .interactive(|_| Ok(Box::new(AreaM::default()))),
        CommandSpec::new("time", "Time", run_time).menu(&["Tools", "Inquiry", "Time"]).noundo(),
        CommandSpec::new("status", "Status", run_status).menu(&["Tools", "Inquiry", "Status"]).noundo(),
        CommandSpec::new("drawing.inspect", "Inspect Drawing", run_inspect).params("{entities?: bool, limit?: n}").noundo(),
        CommandSpec::new("entities", "Query Entities", run_entities).params("{type?, layer?, window?: [[x,y],[x,y]], limit?, offset?}").noundo(),
        CommandSpec::new("count", "Count", run_count)
            .menu(&["Tools", "Count"])
            .params("{block?} → {blocks: {name: n}} | {block, count}, plus a `message` line")
            .noundo(),
    ]
}

fn fmt_d(s: &Session, v: f64) -> String {
    let h = s.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
    format_distance(v, h.0, h.1)
}
fn fmt_a(s: &Session, v: f64) -> String {
    let h = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
    format_angle(v, h.0, h.1)
}

fn dist_text(s: &Session, a: Vec2, b: Vec2) -> String {
    let d = b - a;
    format!(
        "Distance = {},  Angle in XY Plane = {},  Angle from XY Plane = 0\nDelta X = {},  Delta Y = {},   Delta Z = {}",
        fmt_d(s, a.dist(b)),
        fmt_a(s, d.angle()),
        fmt_d(s, d.x),
        fmt_d(s, d.y),
        fmt_d(s, 0.0)
    )
}

fn run_dist(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("dist", p, "p1")?;
    let b = point_req("dist", p, "p2")?;
    Ok(json!({ "distance": a.dist(b), "angle": (b - a).angle().to_degrees(), "dx": b.x - a.x, "dy": b.y - a.y, "message": dist_text(s, a, b) }))
}

fn run_id(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("id", p, "at")?;
    s.last_point = a;
    Ok(json!({ "x": a.x, "y": a.y, "message": format!("X = {}     Y = {}     Z = {}", fmt_d(s, a.x), fmt_d(s, a.y), fmt_d(s, 0.0)) }))
}

fn list_text(s: &Session, h: Handle) -> String {
    let Ok(d) = s.doc() else { return String::new() };
    let Some(e) = d.entity(h) else { return String::new() };
    let space = if d.model.contains(h) { "Model space" } else { "Paper space" };
    let mut t = format!(
        "                  {}     Layer: \"{}\"\n                            Space: {}\n                   Handle = {}",
        e.kind.dxf_name(),
        e.common.layer,
        space,
        h.hex()
    );
    match &e.kind {
        EntityKind::Line(l) => {
            t.push_str(&format!(
                "\n              from point, X={}  Y={}  Z={}\n                to point, X={}  Y={}  Z={}",
                fmt_d(s, l.a.x),
                fmt_d(s, l.a.y),
                fmt_d(s, 0.0),
                fmt_d(s, l.b.x),
                fmt_d(s, l.b.y),
                fmt_d(s, 0.0)
            ));
            t.push_str(&format!(
                "\n          Length = {},  Angle in XY Plane = {}",
                fmt_d(s, l.a.xy().dist(l.b.xy())),
                fmt_a(s, l.a.xy().angle_to(l.b.xy()))
            ));
        }
        EntityKind::Circle(c) => {
            t.push_str(&format!(
                "\n            center point, X={}  Y={}  Z={}\n            radius {}\n     circumference {}\n             area {}",
                fmt_d(s, c.center.x),
                fmt_d(s, c.center.y),
                fmt_d(s, 0.0),
                fmt_d(s, c.radius),
                fmt_d(s, c.radius * cadcraft_geom::TAU),
                fmt_d(s, c.radius * c.radius * cadcraft_geom::PI)
            ));
        }
        EntityKind::Arc(a) => {
            t.push_str(&format!(
                "\n            center point, X={}  Y={}  Z={}\n            radius {}\n            start angle {}\n              end angle {}",
                fmt_d(s, a.center.x),
                fmt_d(s, a.center.y),
                fmt_d(s, 0.0),
                fmt_d(s, a.radius),
                fmt_a(s, a.start),
                fmt_a(s, a.end)
            ));
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            t.push_str(&format!("\n            {}\n    Constant width    {}", if p.closed { "Closed" } else { "Open" }, fmt_d(s, p.const_width)));
            if p.closed {
                t.push_str(&format!("\n             area   {}\n        perimeter   {}", fmt_d(s, pl.area().abs()), fmt_d(s, pl.len())));
            } else {
                t.push_str(&format!("\n           length   {}", fmt_d(s, pl.len())));
            }
            for v in &p.vertices {
                t.push_str(&format!("\n          at point  X={}  Y={}  Z={}", fmt_d(s, v.p.x), fmt_d(s, v.p.y), fmt_d(s, 0.0)));
            }
        }
        EntityKind::Text(tx) => {
            t.push_str(&format!(
                "\n            Style = \"{}\"\n       start point, X={}  Y={}\n          height {}\n           text {}\n        rotation angle {}",
                tx.style,
                fmt_d(s, tx.insert.x),
                fmt_d(s, tx.insert.y),
                fmt_d(s, tx.height),
                tx.value,
                fmt_a(s, tx.rotation)
            ));
        }
        other => {
            if let Ok(js) = serde_json::to_string(other) {
                t.push_str(&format!("\n          {js}"));
            }
        }
    }
    t
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let texts: Vec<String> = hs.iter().map(|h| list_text(s, *h)).collect();
    Ok(json!({ "message": texts.join("\n\n") }))
}

fn area_of(pts: &[Vec2]) -> (f64, f64) {
    let a = cadcraft_geom::shoelace(pts).abs();
    let n = pts.len();
    let per: f64 = (0..n).filter_map(|i| Some(pts.get(i)?.dist(*pts.get((i + 1) % n)?))).sum();
    (a, per)
}

fn run_area(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, per) = if let Some(pts) = points_param(p, "points") {
        area_of(&pts)
    } else {
        let h = targets(s, p)?.first().copied().ok_or_else(|| bad("area", "`points` or `handle` is required"))?;
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad("area", "no such object"))?;
        match &e.kind {
            EntityKind::Circle(c) => (c.radius * c.radius * cadcraft_geom::PI, c.radius * cadcraft_geom::TAU),
            EntityKind::LwPolyline(pl) => {
                let g = Polyline { vertices: pl.vertices.clone(), closed: true };
                (g.area().abs(), g.len())
            }
            EntityKind::Ellipse(el) => {
                let a = el.major.xy().len();
                let b = a * el.ratio;
                (cadcraft_geom::PI * a * b, cadcraft_geom::PI * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt()))
            }
            EntityKind::Hatch(hh) => {
                // Odd parity, as drawn: a loop nested inside an odd number of other loops is an unfilled island.
                let geoms: Vec<Polyline> = hh.loops.iter().map(|l| Polyline { vertices: l.vertices.clone(), closed: true }).collect();
                let polys: Vec<Vec<Vec2>> = geoms.iter().map(|g| g.tessellate(1e-3)).collect();
                let mut total = (0.0, 0.0);
                for (i, g) in geoms.iter().enumerate() {
                    let probe = polys.get(i).and_then(|p| p.first().copied());
                    let depth = probe.map_or(0, |pt| {
                        polys.iter().enumerate().filter(|(j, q)| *j != i && cadcraft_geom::point_in_polygon(q, pt)).count()
                    });
                    total.0 += if depth % 2 == 0 { g.area().abs() } else { -g.area().abs() };
                    total.1 += g.len();
                }
                total
            }
            _ => return Err(bad("area", "object has no area")),
        }
    };
    Ok(json!({ "area": a, "perimeter": per, "message": format!("Area = {}, Perimeter = {}", fmt_d(s, a), fmt_d(s, per)) }))
}

fn run_time(s: &mut Session, _p: &Value) -> Result<Value> {
    let rev = s.state()?.revision;
    Ok(json!({ "message": format!("Drawing revision: {rev}\nEdits recorded: {}", s.state()?.undo.len()) }))
}

fn run_status(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let ext = d.extents(&cadcraft_doc::Space::Model);
    let msg = format!(
        "{} objects in {}\nModel space limits are X: {}  Y: {}  (Off)\nModel space uses X: {}  Y: {}\n                 X: {}  Y: {}\nCurrent layer: {}\nCurrent color: {}\nCurrent linetype: {}",
        d.entity_count(),
        s.state()?.title,
        fmt_d(s, d.header.point("LIMMIN").map(|p| p.x).unwrap_or(0.0)),
        fmt_d(s, d.header.point("LIMMIN").map(|p| p.y).unwrap_or(0.0)),
        fmt_d(s, ext.min.x),
        fmt_d(s, ext.min.y),
        fmt_d(s, ext.max.x),
        fmt_d(s, ext.max.y),
        d.header.str("CLAYER", "0"),
        cadcraft_color::Color::from_aci(d.header.i64("CECOLOR", 256) as i16).name(),
        d.header.str("CELTYPE", "ByLayer"),
    );
    Ok(json!({ "message": msg }))
}

fn entity_summary(e: &cadcraft_doc::Entity) -> Value {
    json!({ "handle": e.handle.hex(), "type": e.kind.type_name(), "layer": e.common.layer, "color": e.common.color.name(), "transparency": super::props::transparency_value(e.common.transparency), "geometry": e.kind })
}

fn run_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state()?;
    let d = &st.doc;
    let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
    let space = st.space.clone();
    let ext = d.extents(&space);
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    if let Some(store) = d.space(&space) {
        for e in store.iter() {
            *counts.entry(e.kind.type_name()).or_default() += 1;
        }
    }
    let mut out = json!({
        "title": st.title,
        "path": st.path,
        "dirty": st.is_dirty(),
        "space": match &space { cadcraft_doc::Space::Model => "Model".to_string(), cadcraft_doc::Space::Paper(n) => n.clone() },
        "entityCount": d.entity_count(),
        "counts": counts,
        "extents": if ext.is_empty() { Value::Null } else { json!([[ext.min.x, ext.min.y], [ext.max.x, ext.max.y]]) },
        "layers": d.layers.iter().map(|l| json!({"name": l.name, "on": l.on, "frozen": l.frozen, "locked": l.locked, "color": l.color.name(), "linetype": l.linetype})).collect::<Vec<_>>(),
        "currentLayer": d.header.str("CLAYER", "0"),
        "linetypes": d.linetypes.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "textStyles": d.text_styles.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "dimStyles": d.dim_styles.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "blocks": d.blocks.keys().filter(|k| !k.starts_with('*')).cloned().collect::<Vec<_>>(),
        "layouts": d.layouts.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "selection": st.selection.iter().map(|h| h.hex()).collect::<Vec<_>>(),
        "undo": st.undo.iter().rev().take(20).map(|u| u.label.clone()).collect::<Vec<_>>(),
        "view": { "center": [st.view().center.x, st.view().center.y], "height": st.view().height },
    });
    if bool_or(p, "entities", true)
        && let (Some(o), Some(store)) = (out.as_object_mut(), d.space(&space))
    {
        o.insert("entities".into(), Value::Array(store.iter().take(limit).map(|e| entity_summary(e)).collect()));
    }
    Ok(out)
}

fn run_entities(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.state()?;
    let d = &st.doc;
    let ty = str_param(p, "type").map(str::to_ascii_lowercase);
    let layer = str_param(p, "layer").map(str::to_ascii_lowercase);
    let window = points_param(p, "window").and_then(|w| Some(cadcraft_geom::Bounds2::new(*w.first()?, *w.get(1)?)));
    let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(500) as usize;
    let offset = p.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let Some(store) = d.space(&st.space) else { return Ok(json!({ "entities": [] })) };
    let matches: Vec<Value> = store
        .iter()
        .filter(|e| ty.as_ref().is_none_or(|t| e.kind.type_name().to_ascii_lowercase() == *t || e.kind.dxf_name().to_ascii_lowercase() == *t))
        .filter(|e| layer.as_ref().is_none_or(|l| e.common.layer.to_ascii_lowercase() == *l))
        .filter(|e| window.is_none_or(|w| w.intersects(&cadcraft_doc::entity_bounds(d, e, 0))))
        .skip(offset)
        .take(limit)
        .map(|e| entity_summary(e))
        .collect();
    Ok(json!({ "count": matches.len(), "entities": matches }))
}

fn run_count(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for e in d.model.iter() {
        if let EntityKind::Insert(i) = &e.kind {
            *counts.entry(i.block.clone()).or_default() += 1;
        }
    }
    // `message` is what the command line shows when COUNT is typed or chosen from the menu.
    if let Some(b) = str_param(p, "block") {
        let n = counts.get(b).copied().unwrap_or(0);
        return Ok(json!({ "block": b, "count": n, "message": format!("Block {b}: {n} in model space.") }));
    }
    let message = if counts.is_empty() {
        "No block references in model space.".to_string()
    } else {
        let mut m = String::from("Block references in model space:");
        for (name, n) in &counts {
            m.push_str(&format!("\n  {name}: {n}"));
        }
        m.push_str(&format!("\n  Total: {}", counts.values().sum::<usize>()));
        m
    };
    Ok(json!({ "blocks": counts, "message": message }))
}

#[derive(Default)]
struct DistM {
    first: Option<Vec2>,
}

impl Interactive for DistM {
    fn name(&self) -> &'static str {
        "DIST"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.first {
            None => Prompt::new("Specify first point", Accept::POINT),
            Some(a) => Prompt::new("Specify second point", Accept::POINT).kw(&["Multiple points"]).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, i) {
            (None, Input::Point(p)) => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(b)) => {
                let t = dist_text(s, a, b);
                for l in t.lines() {
                    s.echo(l.to_string());
                }
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.first.map(|a| vec![super::helpers::line(a, c)]).unwrap_or_default()
    }
}

struct IdM;

impl Interactive for IdM {
    fn name(&self) -> &'static str {
        "ID"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Input::Point(p) = i {
            let m = format!("X = {}     Y = {}     Z = {}", fmt_d(s, p.x), fmt_d(s, p.y), fmt_d(s, 0.0));
            s.echo(m);
        }
        Ok(Step::Done)
    }
}

#[derive(Default)]
struct ListM {
    sel: super::machines::SelectPhase,
}

impl Interactive for ListM {
    fn name(&self) -> &'static str {
        "LIST"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = super::machines::SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            for h in hs {
                let t = list_text(s, h);
                for l in t.lines() {
                    s.echo(l.to_string());
                }
            }
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            super::machines::SelOutcome::More => Ok(Step::Continue),
            super::machines::SelOutcome::Empty => Ok(Step::Done),
            super::machines::SelOutcome::Done(hs) => {
                for h in hs {
                    let t = list_text(s, h);
                    for l in t.lines() {
                        s.echo(l.to_string());
                    }
                }
                Ok(Step::Done)
            }
        }
    }
}

#[derive(Default)]
struct AreaM {
    pts: Vec<Vec2>,
}

impl Interactive for AreaM {
    fn name(&self) -> &'static str {
        "AREA"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.pts.len() {
            0 => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", "Add area", "Subtract area"]).default("Object"),
            1 => Prompt::new("Specify next point", Accept::POINT).kw(&["Arc", "Length", "Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify next point", Accept::POINT)
                .kw(&["Arc", "Length", "Undo", "Total"])
                .default("Total")
                .base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Enter | Input::Keyword(_) if self.pts.len() >= 3 => {
                let (a, per) = area_of(&self.pts);
                let m = format!("Area = {}, Perimeter = {}", fmt_d(s, a), fmt_d(s, per));
                s.echo(m);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut v: Vec<cadcraft_geom::PolyVertex> = self.pts.iter().map(|p| cadcraft_geom::PolyVertex::new(*p)).collect();
        v.push(cadcraft_geom::PolyVertex::new(c));
        vec![super::helpers::lwpoly(v, true)]
    }
}
