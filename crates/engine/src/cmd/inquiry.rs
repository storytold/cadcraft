//! Inquiry: DIST, ID, LIST, AREA, drawing inspection for agents.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Polyline, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::units::{format_angle, format_distance};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

mod regions;
mod shapes;

pub fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
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
        CommandSpec::new("entities", "Query Entities", run_entities)
            .params("{type?, layer?, window?: [[x,y],[x,y]], crossing?: bool (default true; false = entirely inside), limit?, offset?} → {count (all matches), returned, offset, entities}")
            .noundo(),
        CommandSpec::new("shapes", "Analyze Shapes", shapes::run).params(shapes::PARAMS).noundo(),
        CommandSpec::new("texts", "Query Texts", run_texts)
            .params("{contains?: text (any case), layer?, window?: [[x,y],[x,y]] (by insertion point), limit? (default 200), offset?} → {count (all matches), returned, offset, texts: [{handle, type, layer, text, at, height, rotation (degrees), style, tag? (attributes)}]}: the readable text of TEXT, MTEXT (formatting removed), block attributes and multileaders")
            .noundo(),
        CommandSpec::new("countlist", "Count List", count::run_countlist)
            .params("{space?: all (default)|current|model|<layout>, nested?: bool} → per-block counts, as COUNT")
            .noundo(),
        CommandSpec::new("counttable", "Count Table", count::run_counttable)
            .params("{at: [x,y], space?: current (default)|model|all|<layout>, nested?: bool} → {handle}: a table of the block counts")
            .interactive(|_| Ok(count::table_machine())),
        CommandSpec::new("count", "Count", count::run_count)
            .menu(&["Tools", "Count"])
            .params(count::PARAMS)
            .interactive(|_| Ok(count::machine()))
            .noundo(),
    ];
    v.extend(regions::specs());
    v
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

pub(super) fn area_of(pts: &[Vec2]) -> (f64, f64) {
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
        object_area(s, h)?
    };
    Ok(json!({ "area": a, "perimeter": per, "message": area_text(s, a, per) }))
}

fn area_text(s: &Session, a: f64, per: f64) -> String {
    format!("Area = {}, Perimeter = {}", fmt_d(s, a), fmt_d(s, per))
}

/// Area and perimeter of a closed object (AREA's Object option).
pub(super) fn object_area(s: &Session, h: Handle) -> Result<(f64, f64)> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| bad("area", "no such object"))?;
    Ok(match &e.kind {
        EntityKind::Circle(c) => (c.radius * c.radius * cadcraft_geom::PI, c.radius * cadcraft_geom::TAU),
        EntityKind::LwPolyline(pl) => {
            let g = Polyline { vertices: pl.vertices.clone(), closed: true };
            (g.area().abs(), g.len())
        }
        EntityKind::Ellipse(el) => {
            let arc = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            if !arc.is_full() {
                return Err(bad("area", "object has no area"));
            }
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
                let depth = probe.map_or(0, |pt| polys.iter().enumerate().filter(|(j, q)| *j != i && cadcraft_geom::point_in_polygon(q, pt)).count());
                total.0 += if depth % 2 == 0 { g.area().abs() } else { -g.area().abs() };
                total.1 += g.len();
            }
            total
        }
        EntityKind::Region(r) => (r.area(), r.perimeter()),
        _ => return Err(bad("area", "object has no area")),
    })
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
        "blockEditor": st.block_editor(),
        "space": space.tab_name(),
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
        // Records of the file the drawing came from that the reader could not place.
        "skipped": { "count": d.skipped_records.values().fold(0usize, |a, n| a.saturating_add(*n)), "sections": d.skipped_records },
    });
    if bool_or(p, "entities", true)
        && let (Some(o), Some(store)) = (out.as_object_mut(), d.space(&space))
    {
        o.insert("entities".into(), Value::Array(store.iter().take(limit).map(|e| entity_summary(e)).collect()));
    }
    Ok(out)
}

fn run_entities(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "entities";
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let text = |k: &str| -> Result<Option<String>> {
        given(k).map(|v| v.as_str().map(str::to_ascii_lowercase).ok_or_else(|| bad(CMD, format!("`{k}` must be a string")))).transpose()
    };
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let ty = text("type")?;
    let layer = text("layer")?;
    let window = given("window")
        .map(|w| {
            let corners: Option<Vec<Vec2>> = w.as_array().filter(|a| a.len() == 2).and_then(|a| a.iter().map(point_value).collect());
            match corners.as_deref() {
                Some([a, b]) => Ok(cadcraft_geom::Bounds2::new(*a, *b)),
                _ => Err(bad(CMD, "`window` must be two corner points [[x0, y0], [x1, y1]]")),
            }
        })
        .transpose()?;
    let crossing = given("crossing").map_or(Ok(true), |v| v.as_bool().ok_or_else(|| bad(CMD, "`crossing` must be true or false")))?;
    let limit = count("limit", 500)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let d = &st.doc;
    let Some(store) = d.space(&st.space) else { return Ok(json!({ "count": 0, "returned": 0, "entities": [] })) };
    let tol = window.as_ref().map(crate::select::window_tol).unwrap_or(0.0);
    let matches: Vec<&std::sync::Arc<cadcraft_doc::Entity>> = store
        .iter()
        .filter(|e| ty.as_ref().is_none_or(|t| e.kind.type_name().to_ascii_lowercase() == *t || e.kind.dxf_name().to_ascii_lowercase() == *t))
        .filter(|e| layer.as_ref().is_none_or(|l| e.common.layer.to_ascii_lowercase() == *l))
        .filter(|e| window.as_ref().is_none_or(|w| crate::select::in_window(d, e, None, w, crossing, tol)))
        .collect();
    let page: Vec<Value> = matches.iter().skip(offset).take(limit).map(|e| entity_summary(e)).collect();
    // `count` is every match; `returned` is this page of them (`offset`, `limit`).
    Ok(json!({ "count": matches.len(), "returned": page.len(), "offset": offset, "entities": page }))
}

/// TEXTS: the readable text of the space's texts, multiline texts, block attributes and
/// multileaders, with where they are, filtered by content, layer and a window around their
/// insertion points. Read-only (FIND selects and can replace; this only reports).
fn run_texts(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "texts";
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let text = |k: &str| -> Result<Option<String>> {
        given(k).map(|v| v.as_str().map(str::to_lowercase).ok_or_else(|| bad(CMD, format!("`{k}` must be a string")))).transpose()
    };
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let contains = text("contains")?;
    let layer = text("layer")?;
    let window = given("window")
        .map(|w| {
            let corners: Option<Vec<Vec2>> = w.as_array().filter(|a| a.len() == 2).and_then(|a| a.iter().map(point_value).collect());
            match corners.as_deref() {
                Some([a, b]) => Ok(cadcraft_geom::Bounds2::new(*a, *b)),
                _ => Err(bad(CMD, "`window` must be two corner points [[x0, y0], [x1, y1]]")),
            }
        })
        .transpose()?;
    let limit = count("limit", 200)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let Some(store) = st.doc.space(&st.space) else { return Ok(json!({ "count": 0, "returned": 0, "offset": offset, "texts": [] })) };
    let mut found: Vec<Value> = Vec::new();
    for e in store.iter() {
        if layer.as_ref().is_some_and(|l| e.common.layer.to_lowercase() != *l) {
            continue;
        }
        let mut push = |ty: &str, value: String, at: cadcraft_geom::Vec3, height: f64, rotation: f64, style: &str, tag: Option<&str>| {
            let value = value.trim().to_string();
            if value.is_empty()
                || contains.as_ref().is_some_and(|c| !value.to_lowercase().contains(c.as_str()))
                || window.as_ref().is_some_and(|w| !w.contains(Vec2::new(at.x, at.y)))
            {
                return;
            }
            let mut v = json!({
                "handle": e.handle.hex(), "type": ty, "layer": e.common.layer, "text": value,
                "at": [at.x, at.y], "height": height, "rotation": rotation.to_degrees(), "style": style,
            });
            if let (Some(t), Some(o)) = (tag, v.as_object_mut()) {
                o.insert("tag".into(), json!(t));
            }
            found.push(v);
        };
        match &e.kind {
            EntityKind::Text(t) => push("Text", t.value.clone(), t.insert, t.height, t.rotation, &t.style, None),
            EntityKind::MText(t) => push("MText", cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style, None),
            EntityKind::Insert(i) => {
                for a in i.attribs.iter().filter(|a| !a.invisible) {
                    push("Attribute", a.text.value.clone(), a.text.insert, a.text.height, a.text.rotation, &a.text.style, Some(&a.tag));
                }
            }
            EntityKind::MLeader(m) => {
                if let Some(t) = &m.text {
                    push("MLeader", cadcraft_fonts::plain_mtext(&t.contents), t.insert, t.height, t.rotation, &t.style, None);
                }
            }
            _ => {}
        }
    }
    let n = found.len();
    let page: Vec<Value> = found.into_iter().skip(offset).take(limit).collect();
    Ok(json!({ "count": n, "returned": page.len(), "offset": offset, "texts": page }))
}

mod count;

#[derive(Default)]
struct DistM {
    first: Option<Vec2>,
    first_source: Option<crate::snap::ViewportPoint>,
    /// Multiple points: the points so far, the first included.
    multi: Option<Vec<Vec2>>,
    sources: Vec<Option<crate::snap::ViewportPoint>>,
    /// Asking for the length of a segment (the Length option).
    length: bool,
}

/// Total length of the segments through `pts`.
fn path_len(pts: &[Vec2]) -> f64 {
    pts.windows(2)
        .map(|w| match w {
            [a, b] => a.dist(*b),
            _ => 0.0,
        })
        .sum()
}

/// The point `len` beyond the last of `pts`, continuing the last segment (along X after a single
/// point): the Length option of AREA and DIST's point sequences.
fn extend_last(pts: &[Vec2], len: f64) -> Option<Vec2> {
    let last = *pts.last()?;
    let dir = match pts.len().checked_sub(2).and_then(|i| pts.get(i)) {
        Some(prev) => (last - *prev).normalized(),
        None => Vec2::X,
    };
    let dir = if dir == Vec2::ZERO { Vec2::X } else { dir };
    let p = last + dir * len;
    p.is_finite().then_some(p)
}

fn length_prompt() -> Prompt {
    Prompt::new("Specify length of line", Accept::NUMBER)
}

/// Feed the Length option's answer: the next point, or `None` when the option was dismissed (Enter).
fn length_input(pts: &[Vec2], i: Input) -> Result<Option<Vec2>> {
    let t = match i {
        Input::Text(t) => t,
        _ => return Ok(None),
    };
    let len = crate::units::parse_distance(&t).filter(|v| v.is_finite()).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
    Ok(extend_last(pts, len))
}

/// Model units apply only when every point was snapped through one and the same viewport.
fn model_points(sources: &[Option<crate::snap::ViewportPoint>]) -> Option<Vec<Vec2>> {
    let viewport = sources.first().copied().flatten()?.viewport;
    sources.iter().map(|s| s.filter(|s| s.viewport == viewport).map(|s| s.point)).collect()
}

impl Interactive for DistM {
    fn name(&self) -> &'static str {
        "DIST"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.length {
            return length_prompt();
        }
        match (&self.multi, self.first) {
            (Some(pts), _) if pts.len() < 2 => {
                Prompt::new("Specify second point", Accept::POINT).kw(&["Arc", "Length", "Undo"]).base_opt(pts.last().copied())
            }
            (Some(pts), _) => Prompt::new("Specify next point", Accept::POINT)
                .kw(&["Arc", "Length", "Undo", "Total"])
                .default("Total")
                .base_opt(pts.last().copied()),
            (None, None) => Prompt::new("Specify first point", Accept::POINT),
            (None, Some(a)) => Prompt::new("Specify second point", Accept::POINT).kw(&["Multiple points"]).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(pts) = &mut self.multi {
            if self.length {
                let model = model_points(&self.sources);
                if let Some(p) = length_input(model.as_deref().unwrap_or(pts), i)? {
                    let source = model.as_ref().and_then(|_| self.sources.last().copied().flatten());
                    let paper = match (source, pts.last()) {
                        (Some(source), Some(last)) => *last + (p - source.point) * source.scale,
                        _ => p,
                    };
                    if !paper.is_finite() {
                        return Err(bad("dist", "the next point is too large"));
                    }
                    pts.push(paper);
                    self.sources.push(source.map(|source| crate::snap::ViewportPoint { point: p, ..source }));
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts)))));
                }
                self.length = false;
                return Ok(Step::Continue);
            }
            match i {
                Input::Point(p) => {
                    pts.push(p);
                    self.sources.push(s.picked_viewport);
                    let model = model_points(&self.sources);
                    s.echo(format!("Distance = {}", fmt_d(s, path_len(model.as_deref().unwrap_or(pts)))));
                }
                Input::Keyword(k) if k == "Undo" => {
                    if pts.len() > 1 {
                        pts.pop();
                        self.sources.pop();
                    }
                }
                Input::Keyword(k) if k == "Length" => self.length = true,
                Input::Keyword(k) if k == "Total" => return self.total(s),
                Input::Keyword(k) => s.echo(format!("{k}: not available yet")),
                Input::Enter => return self.total(s),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match (self.first, i) {
            (None, Input::Point(p)) => {
                self.first = Some(p);
                self.first_source = s.picked_viewport;
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(b)) => {
                let (a, b) = match (self.first_source, s.picked_viewport) {
                    (Some(first), Some(second)) if first.viewport == second.viewport => (first.point, second.point),
                    _ => (a, b),
                };
                let t = dist_text(s, a, b);
                for l in t.lines() {
                    s.echo(l.to_string());
                }
                Ok(Step::Done)
            }
            (Some(a), Input::Keyword(k)) if k == "Multiple points" => {
                self.multi = Some(vec![a]);
                self.sources = vec![self.first_source];
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match &self.multi {
            Some(pts) => {
                let mut v: Vec<cadcraft_geom::PolyVertex> = pts.iter().map(|p| cadcraft_geom::PolyVertex::new(*p)).collect();
                v.push(cadcraft_geom::PolyVertex::new(c));
                vec![super::helpers::lwpoly(v, false)]
            }
            None => self.first.map(|a| vec![super::helpers::line(a, c)]).unwrap_or_default(),
        }
    }
}

impl DistM {
    fn total(&self, s: &mut Session) -> Result<Step> {
        let model = model_points(&self.sources);
        let d = model.as_deref().or(self.multi.as_deref()).map_or(0.0, path_len);
        s.echo(format!("Distance = {}", fmt_d(s, d)));
        Ok(Step::Done)
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AreaMode {
    /// Measure one area and end.
    #[default]
    Single,
    /// Add each measured area to the running total.
    Add,
    /// Subtract each measured area from the running total.
    Subtract,
}

#[derive(Default)]
struct AreaM {
    pts: Vec<Vec2>,
    mode: AreaMode,
    total: f64,
    /// Selecting objects (the Object option).
    object: bool,
    /// Asking for the length of a segment (the Length option).
    length: bool,
}

impl AreaM {
    fn mode_prefix(&self) -> &'static str {
        match self.mode {
            AreaMode::Single => "",
            AreaMode::Add => "(ADD mode) ",
            AreaMode::Subtract => "(SUBTRACT mode) ",
        }
    }
    /// Report an area (and the running total in add/subtract mode); ends the command in single mode.
    fn measured(&mut self, s: &mut Session, a: f64, per: f64) -> Step {
        s.echo(area_text(s, a, per));
        match self.mode {
            AreaMode::Single => return Step::Done,
            AreaMode::Add => self.total += a,
            AreaMode::Subtract => self.total -= a,
        }
        s.echo(format!("Total area = {}", fmt_d(s, self.total)));
        Step::Continue
    }
    /// Close the boundary picked so far (Enter or Total at the next-point prompt).
    fn close(&mut self, s: &mut Session) -> Step {
        let pts = std::mem::take(&mut self.pts);
        if pts.len() < 3 {
            return if self.mode == AreaMode::Single { Step::Done } else { Step::Continue };
        }
        let (a, per) = area_of(&pts);
        self.measured(s, a, per)
    }
}

impl Interactive for AreaM {
    fn name(&self) -> &'static str {
        "AREA"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.object {
            return Prompt::new(format!("{}Select objects", self.mode_prefix()), Accept::SELECT);
        }
        if self.length {
            return length_prompt();
        }
        let next = format!("{}Specify next point", self.mode_prefix());
        match (self.pts.len(), self.mode) {
            (0, AreaMode::Single) => {
                Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", "Add area", "Subtract area"]).default("Object")
            }
            (0, AreaMode::Add) => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", "Subtract area"]),
            (0, AreaMode::Subtract) => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Object", "Add area"]),
            (1, _) => Prompt::new(next, Accept::POINT).kw(&["Arc", "Length", "Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new(next, Accept::POINT).kw(&["Arc", "Length", "Undo", "Total"]).default("Total").base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.object {
            match i {
                Input::Pick(hs) => {
                    for h in hs {
                        match object_area(s, h) {
                            Ok((a, per)) => {
                                if self.measured(s, a, per) == Step::Done {
                                    return Ok(Step::Done);
                                }
                            }
                            Err(_) => s.echo("The selected object has no area."),
                        }
                    }
                }
                Input::Enter => {
                    self.object = false;
                    if self.mode == AreaMode::Single {
                        return Ok(Step::Done);
                    }
                }
                Input::Text(_) | Input::Keyword(_) => s.echo("*Invalid selection*"),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        if self.length {
            if let Some(p) = length_input(&self.pts, i)? {
                self.pts.push(p);
            }
            self.length = false;
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => self.pts.push(p),
            Input::Keyword(k) => match k.as_str() {
                "Object" => self.object = true,
                "Add area" => self.mode = AreaMode::Add,
                "Subtract area" => self.mode = AreaMode::Subtract,
                "Undo" => {
                    self.pts.pop();
                }
                "Length" => self.length = true,
                "Total" => return Ok(self.close(s)),
                _ => s.echo(format!("{k}: not available yet")),
            },
            Input::Enter if self.pts.is_empty() => match self.mode {
                // Enter takes the default, Object; in add/subtract mode it ends the command.
                AreaMode::Single => self.object = true,
                AreaMode::Add | AreaMode::Subtract => return Ok(Step::Done),
            },
            Input::Enter => return Ok(self.close(s)),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() || self.object || self.length {
            return Vec::new();
        }
        let mut v: Vec<cadcraft_geom::PolyVertex> = self.pts.iter().map(|p| cadcraft_geom::PolyVertex::new(*p)).collect();
        v.push(cadcraft_geom::PolyVertex::new(c));
        vec![super::helpers::lwpoly(v, true)]
    }
}
