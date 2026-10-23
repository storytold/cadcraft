//! Inquiry: DIST, ID, LIST, AREA, drawing inspection for agents.

use cadcraft_doc::{Entity, EntityKind, Handle};
use cadcraft_geom::{Polyline, Vec2, Vec3};
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
        CommandSpec::new("xdlist", "List Xdata", run_xdlist)
            .menu(&["Tools", "Inquiry", "List Xdata"])
            .alias(&["xdl", "xdata"])
            .params("{handles?, app?}")
            .noundo()
            .interactive(|_| Ok(Box::new(XdListM::default()))),        CommandSpec::new("xdedit", "Edit Xdata", run_xdedit)
            .menu(&["Tools", "Inquiry", "Edit Xdata (XDEDIT)"])
            .alias(&["xde"])
            .params("{handle?, xdata?: [{code, value}]}")
            .interactive(|_| Ok(Box::new(XdEditM::default()))),
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
            .params("{type?, layer?, selected?: bool (default false; true = current selection in active space), window?: [[x,y],[x,y]], crossing?: bool (default true; false = entirely inside), limit?, offset?} → {count (all matches), returned, offset, entities}")
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

/// The angle of `d` up from the XY plane.
fn angle_from_xy(d: Vec3) -> f64 {
    d.z.atan2(d.xy().len())
}

fn dist_text(s: &Session, a: Vec3, b: Vec3) -> String {
    let d = b - a;
    format!(
        "Distance = {},  Angle in XY Plane = {},  Angle from XY Plane = {}\nDelta X = {},  Delta Y = {},   Delta Z = {}",
        fmt_d(s, (b - a).len()),
        fmt_a(s, d.xy().angle()),
        fmt_a(s, angle_from_xy(d)),
        fmt_d(s, d.x),
        fmt_d(s, d.y),
        fmt_d(s, d.z)
    )
}

/// A point parameter with its Z: the third number when given, else the Z of what is drawn
/// there ([`z_at`]).
fn point3_req(s: &Session, cmd: &str, p: &Value, key: &str) -> Result<Vec3> {
    let q = point_req(cmd, p, key)?;
    let z = p.get(key).and_then(|v| v.get(2)).and_then(Value::as_f64).filter(|z| z.is_finite());
    Ok(Vec3::new(q.x, q.y, z.unwrap_or_else(|| z_at(s, q))))
}

fn run_dist(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point3_req(s, "dist", p, "p1")?;
    let b = point3_req(s, "dist", p, "p2")?;
    let d = b - a;
    Ok(json!({
        "distance": d.len(),
        "angle": d.xy().angle().to_degrees(),
        "angleFromXY": angle_from_xy(d).to_degrees(),
        "dx": d.x,
        "dy": d.y,
        "dz": d.z,
        "message": dist_text(s, a, b),
    }))
}

fn id_text(s: &Session, a: Vec3) -> String {
    format!("X = {}     Y = {}     Z = {}", fmt_d(s, a.x), fmt_d(s, a.y), fmt_d(s, a.z))
}

fn run_id(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point3_req(s, "id", p, "at")?;
    s.last_point = a.xy();
    Ok(json!({ "x": a.x, "y": a.y, "z": a.z, "message": id_text(s, a) }))
}

/// The Z of what is drawn at `p`: the object a pick finds there, at that point, or the current
/// elevation (ELEVATION) when there is none. Points come from the canvas in 2D; the object
/// under a snapped point gives its Z back, as AutoCAD's ID and DIST report a snapped point's.
pub(crate) fn z_at(s: &Session, p: Vec2) -> f64 {
    let Ok(d) = s.doc() else { return 0.0 };
    let elevation = d.header.f64("ELEVATION", 0.0);
    let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
    let space = s.space();
    crate::select::pick(d, &space, p, ap)
        .and_then(|h| d.entity(h))
        .and_then(|e| entity_z(e, p))
        .filter(|z| z.is_finite())
        .unwrap_or(if elevation.is_finite() { elevation } else { 0.0 })
}

/// The Z of `e` at the point of it nearest `p` (seen from above), when it has one there.
fn entity_z(e: &Entity, p: Vec2) -> Option<f64> {
    let on_segment = |a: Vec3, b: Vec3| {
        let d = b.xy() - a.xy();
        let l2 = d.dot(d);
        let t = if l2 > 0.0 { ((p - a.xy()).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        (a.z + (b.z - a.z) * t, (a.xy() + d * t).dist(p))
    };
    let along = |pts: &[Vec3], closed: bool| {
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        (0..segs)
            .filter_map(|i| Some(on_segment(*pts.get(i)?, *pts.get((i + 1) % n)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(z, _)| z)
            .or_else(|| pts.first().map(|q| q.z))
    };
    let n = e.common.extrusion;
    let plan = n.x.abs() < 1e-9 && n.y.abs() < 1e-9 && n.z != 0.0;
    match &e.kind {
        EntityKind::Point(q) => Some(q.p.z),
        EntityKind::Line(l) => Some(on_segment(l.a, l.b).0),
        EntityKind::Polyline3d(pl) => along(&pl.points, pl.closed),
        // Elevation is in the polyline's OCS: down Z for a mirrored one.
        EntityKind::LwPolyline(pl) if plan => Some(if n.z < 0.0 { -pl.elevation } else { pl.elevation }),
        // Read in world coordinates already.
        EntityKind::Circle(c) => Some(c.center.z),
        EntityKind::Arc(a) => Some(a.center.z),
        EntityKind::Text(t) | EntityKind::AttDef(cadcraft_doc::Attrib { text: t, .. }) => Some(t.insert.z),
        EntityKind::MText(t) => Some(t.insert.z),
        EntityKind::Insert(i) => Some(i.insert.z),
        EntityKind::Face3d(f) => face_z(&f.corners, p).or_else(|| along(&f.corners, true)),
        _ => None,
    }
}

/// The Z at `p` on a 3D face: on the plane of the triangle of its corners that holds `p`.
fn face_z(c: &[Vec3; 4], p: Vec2) -> Option<f64> {
    let tri = |a: Vec3, b: Vec3, d: Vec3| -> Option<f64> {
        let n = (b - a).cross(d - a);
        if n.z.abs() < 1e-12 * (1.0 + n.len()) {
            return None;
        }
        let inside = cadcraft_geom::point_in_polygon(&[a.xy(), b.xy(), d.xy()], p);
        inside.then(|| a.z - (n.x * (p.x - a.x) + n.y * (p.y - a.y)) / n.z)
    };
    tri(c[0], c[1], c[2]).or_else(|| if c[3] == c[2] { None } else { tri(c[0], c[2], c[3]) })
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
                fmt_d(s, l.a.z),
                fmt_d(s, l.b.x),
                fmt_d(s, l.b.y),
                fmt_d(s, l.b.z)
            ));
            t.push_str(&format!(
                "\n          Length = {},  Angle in XY Plane = {}",
                fmt_d(s, (l.b - l.a).len()),
                fmt_a(s, l.a.xy().angle_to(l.b.xy()))
            ));
            let d = l.b - l.a;
            t.push_str(&format!("\n          Delta X = {},  Delta Y = {},  Delta Z = {}", fmt_d(s, d.x), fmt_d(s, d.y), fmt_d(s, d.z)));
        }
        EntityKind::Circle(c) => {
            t.push_str(&format!(
                "\n            center point, X={}  Y={}  Z={}\n            radius {}\n     circumference {}\n             area {}",
                fmt_d(s, c.center.x),
                fmt_d(s, c.center.y),
                fmt_d(s, c.center.z),
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
                fmt_d(s, a.center.z),
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
            // The elevation, in world Z (down Z for a mirrored polyline).
            let z = entity_z(e, Vec2::ZERO).unwrap_or(p.elevation);
            t.push_str(&format!("\n        elevation   {}", fmt_d(s, z)));
            for v in &p.vertices {
                t.push_str(&format!("\n          at point  X={}  Y={}  Z={}", fmt_d(s, v.p.x), fmt_d(s, v.p.y), fmt_d(s, z)));
            }
        }
        EntityKind::Text(tx) => {
            t.push_str(&format!(
                "\n            Style = \"{}\"\n       start point, X={}  Y={}  Z={}\n          height {}\n           text {}\n        rotation angle {}",
                tx.style,
                fmt_d(s, tx.insert.x),
                fmt_d(s, tx.insert.y),
                fmt_d(s, tx.insert.z),
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
    if !e.common.xdata.is_empty() {
        let mut cur_app = String::new();
        for tag in &e.common.xdata {
            if tag.code == 1001 {
                cur_app = tag.value.trim().to_string();
                if !is_system_xdata_app(&cur_app) {
                    t.push_str(&format!("\n  Extended Data ({cur_app}):"));
                }
                continue;
            }
            if is_system_xdata_app(&cur_app) || tag.code == 1002 {
                continue;
            }
            t.push_str(&format!("\n    [{}] {}", tag.code, tag.value.trim()));
        }
    }
    t
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let texts: Vec<String> = hs.iter().map(|h| list_text(s, *h)).collect();
    Ok(json!({ "message": texts.join("\n\n") }))
}

fn xdlist_app_matches(app_name: &str, filter: &str) -> bool {
    let f = filter.trim();
    if f.is_empty() || f == "*" {
        return !is_system_xdata_app(app_name);
    }
    let a_low = app_name.to_ascii_lowercase();
    let f_low = f.to_ascii_lowercase();
    if let Some(prefix) = f_low.strip_suffix('*') {
        a_low.starts_with(prefix)
    } else {
        a_low == f_low || a_low.contains(&f_low)
    }
}

fn xdata_code_label(code: i32) -> &'static str {
    match code {
        1000 => "ASCII string",
        1002 => "Control string",
        1003 => "Layer name",
        1004 => "Binary data",
        1005 => "Database handle",
        1010..=1033 => "3D point",
        1040 => "Real",
        1041 => "Distance",
        1042 => "Scale factor",
        1070 => "16-bit integer",
        1071 => "32-bit signed long",
        _ => "Value",
    }
}

fn xdlist_text(s: &Session, h: Handle, filter: &str) -> String {
    let Ok(d) = s.doc() else { return String::new() };
    let Some(e) = d.entity(h) else { return String::new() };
    let mut out = format!("Object: {}   Layer: \"{}\"   Handle: {}", e.kind.dxf_name(), e.common.layer, h.hex());
    let mut cur_app;
    let mut show_cur = false;
    let mut found_any = false;
    for tag in &e.common.xdata {
        if tag.code == 1001 {
            cur_app = tag.value.trim().to_string();
            show_cur = xdlist_app_matches(&cur_app, filter);
            if show_cur {
                found_any = true;
                out.push_str(&format!("\n* Registered Application Name: {cur_app}"));
            }
            continue;
        }
        if show_cur {
            out.push_str(&format!("\n  * Code {}, {}: {}", tag.code, xdata_code_label(tag.code), tag.value.trim()));
        }
    }
    if !found_any {
        out.push_str("\nNo Xdata associated with Application Name(s).");
    }
    out
}

fn run_xdlist(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let app = p.get("app").and_then(Value::as_str).unwrap_or("*");
    let texts: Vec<String> = hs.iter().map(|h| xdlist_text(s, *h, app)).collect();
    Ok(json!({ "app": app, "message": texts.join("\n\n") }))
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
            // An open polyline is measured as if closed for the area, but its perimeter is its own length (AutoCAD's AREA help).
            let g = Polyline { vertices: pl.vertices.clone(), closed: true };
            let drawn = Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
            (g.area().abs(), drawn.len())
        }
        EntityKind::Ellipse(el) => {
            let arc = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            if !arc.is_full() {
                return Err(bad("area", "object has no area"));
            }
            let a = el.major.xy().len();
            let b = a * el.ratio;
            // The perimeter integrated around the ellipse, not a closed-form estimate (those
            // fall short as the ellipse flattens: 0.3% on a 100:1 one).
            (cadcraft_geom::PI * a * b, arc.arc_length_from(0.0, cadcraft_geom::TAU))
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
    let selected = given("selected").map_or(Ok(false), |v| v.as_bool().ok_or_else(|| bad(CMD, "`selected` must be true or false")))?;
    let limit = count("limit", 500)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let d = &st.doc;
    let Some(store) = d.space(&st.space) else { return Ok(json!({ "count": 0, "returned": 0, "entities": [] })) };
    let tol = window.as_ref().map(crate::select::window_tol).unwrap_or(0.0);
    let selection: std::collections::HashSet<Handle> = if selected { st.selection.iter().copied().collect() } else { Default::default() };
    let matches: Vec<&std::sync::Arc<cadcraft_doc::Entity>> = store
        .iter()
        .filter(|e| !selected || selection.contains(&e.handle))
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
pub(super) struct DistM {
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
                let (za, zb) = if self.first_source.is_some() || s.picked_viewport.is_some() { (0.0, 0.0) } else { (z_at(s, a), z_at(s, b)) };
                let t = dist_text(s, Vec3::new(a.x, a.y, za), Vec3::new(b.x, b.y, zb));
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
            let m = id_text(s, Vec3::new(p.x, p.y, z_at(s, p)));
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
struct XdListM {
    sel: super::machines::SelectPhase,
}

impl XdListM {
    fn report(&self, s: &mut Session, app_filter: &str) -> Step {
        let f = if app_filter.trim().is_empty() { "*" } else { app_filter.trim() };
        for h in &self.sel.picked {
            let t = xdlist_text(s, *h, f);
            for l in t.lines() {
                s.echo(l.to_string());
            }
        }
        Step::Done
    }
}

impl Interactive for XdListM {
    fn name(&self) -> &'static str {
        "XDLIST"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = super::machines::SelectPhase::begin(s);
        if !self.sel.done {
            self.sel.single = true;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            self.sel.prompt()
        } else {
            Prompt::new("Application name", Accept::TEXT).default("*")
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                super::machines::SelOutcome::More => Ok(Step::Continue),
                super::machines::SelOutcome::Empty => Ok(Step::Done),
                super::machines::SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    Ok(Step::Continue)
                }
            };
        }
        match i {
            Input::Enter => Ok(self.report(s, "*")),
            Input::Text(app) | Input::Keyword(app) => Ok(self.report(s, &app)),
            Input::Cancel => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}

/// Returns `true` if `app` is an internal AutoCAD or CADCraft system XData application
/// (`ACAD`, `CADCRAFT`, or `AcDb*`).
pub fn is_system_xdata_app(app: &str) -> bool {
    let a = app.trim();
    a.eq_ignore_ascii_case("ACAD")
        || a.eq_ignore_ascii_case("CADCRAFT")
        || a.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("AcDb"))
}

fn run_xdedit(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let Some(&h) = hs.first() else {
        return Err(bad("xdedit", "nothing selected"));
    };
    let Some(arr) = p.get("xdata").and_then(Value::as_array) else {
        return Ok(json!({ "handle": h.hex() }));
    };
    let mut new_user_tags: Vec<cadcraft_doc::RawTag> = Vec::new();
    let mut apps: Vec<String> = Vec::new();
    let mut in_app = false;
    for (i, item) in arr.iter().enumerate() {
        let code = item
            .get("code")
            .and_then(Value::as_i64)
            .and_then(|c| i32::try_from(c).ok())
            .ok_or_else(|| bad("xdedit", format!("xdata[{i}]: `code` must be an integer")))?;
        let val = match item.get("value") {
            Some(Value::String(s)) => s.clone(),
            Some(v @ Value::Number(_)) => v.to_string(),
            _ => return Err(bad("xdedit", format!("xdata[{i}]: `value` must be a string or number"))),
        };
        if code == 1001 {
            in_app = true;
            let app_name = val.trim().to_string();
            if is_system_xdata_app(&app_name) {
                return Err(bad("xdedit", format!("xdata[{i}]: system application `{app_name}` cannot be edited")));
            }
            if !app_name.is_empty() && !apps.iter().any(|a| a.eq_ignore_ascii_case(&app_name)) {
                apps.push(app_name);
            }
        } else if !in_app {
            return Err(bad("xdedit", format!("xdata[{i}]: tags must follow a 1001 application name")));
        }
        new_user_tags.push(cadcraft_doc::RawTag { code, value: val });
    }
    let d = s.doc_mut()?;
    if d.entity(h).is_none() {
        return Err(bad("xdedit", "no such object"));
    }
    for app_name in apps {
        std::sync::Arc::make_mut(&mut d.foreign)
            .records
            .entry(("APPID".to_string(), app_name.to_ascii_uppercase()))
            .or_default();
    }
    d.modify_entity(h, |e| {
        let mut kept: Vec<cadcraft_doc::RawTag> = Vec::new();
        let mut sys = false;
        for t in &e.common.xdata {
            if t.code == 1001 {
                sys = is_system_xdata_app(t.value.trim());
            }
            if sys {
                kept.push(t.clone());
            }
        }
        kept.extend(new_user_tags);
        e.common.xdata = kept;
    })?;
    Ok(json!({ "handle": h.hex(), "updated": true }))
}

#[derive(Default)]
struct XdEditM {
    sel: super::machines::SelectPhase,
}

impl Interactive for XdEditM {
    fn name(&self) -> &'static str {
        "XDEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = super::machines::SelectPhase::begin(s);
        if self.sel.done {
            return Ok(Step::Done);
        }
        self.sel.single = true;
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
                s.set_selection(hs);
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
pub(super) struct AreaM {
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

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::RawTag;

    #[test]
    fn xdedit_requires_selection_and_validates_payload() {
        let mut s = Session::default();
        assert!(s.execute("xdedit", &json!({})).is_err());

        let line = s.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).ok();
        let h_str = line
            .and_then(|v| v["handles"][0].as_str().map(str::to_string))
            .unwrap_or_default();

        // Missing or invalid code/value, system app, or orphan tag before 1001 is rejected.
        assert!(
            s.execute("xdedit", &json!({ "handles": [&h_str], "xdata": [{ "value": "bad" }] }))
                .is_err()
        );
        assert!(
            s.execute("xdedit", &json!({ "handles": [&h_str], "xdata": [{ "code": 1000, "value": true }] }))
                .is_err()
        );
        assert!(
            s.execute("xdedit", &json!({ "handles": [&h_str], "xdata": [{ "code": 1001, "value": "ACAD" }] }))
                .is_err()
        );
        assert!(
            s.execute("xdedit", &json!({ "handles": [&h_str], "xdata": [{ "code": 1000, "value": "ORPHAN=1" }] }))
                .is_err()
        );
    }

    #[test]
    fn xdedit_preserves_system_xdata_and_xdlist_filters_by_app() {
        let mut s = Session::default();
        let line = s.execute("line", &json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).ok();
        let h_str = line
            .and_then(|v| v["handles"][0].as_str().map(str::to_string))
            .unwrap_or_default();
        let h = Handle::parse_hex(&h_str).unwrap_or(Handle(1));
        if let Ok(d) = s.doc_mut() {
            let _ = d.modify_entity(h, |e| {
                e.common.xdata = vec![
                    RawTag { code: 1001, value: "ACAD".into() },
                    RawTag { code: 1000, value: "DSTYLE".into() },
                    RawTag { code: 1001, value: "AcDbBlockRepBTag".into() },
                    RawTag { code: 1070, value: "1".into() },
                    RawTag { code: 1001, value: "OLD_USER_APP".into() },
                    RawTag { code: 1000, value: "OLD=1".into() },
                ];
            });
        }

        let res = s.execute(
            "xdedit",
            &json!({
                "handles": [&h_str],
                "xdata": [
                    { "code": 1001, "value": "Lumi_KLIC" },
                    { "code": 1000, "value": "THEMA=Laagspanning" },
                    { "code": 1040, "value": 1.25 },
                    { "code": 1001, "value": "OTHER_APP" },
                    { "code": 1070, "value": 42 }
                ]
            }),
        );
        assert!(res.is_ok());

        // Wildcard XDLIST skips system apps (ACAD, AcDb*) and lists user apps.
        let all_msg = s
            .execute("xdlist", &json!({ "handles": [&h_str], "app": "*" }))
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_default();
        assert!(all_msg.contains("Lumi_KLIC"));
        assert!(all_msg.contains("OTHER_APP"));
        assert!(!all_msg.contains("OLD_USER_APP"));
        assert!(!all_msg.contains("Application Name: ACAD"));
        assert!(!all_msg.contains("AcDbBlockRepBTag"));

        // Filtered XDLIST only shows the requested application.
        let filtered_msg = s
            .execute("xdlist", &json!({ "handles": [&h_str], "app": "Lumi_KLIC" }))
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_default();
        assert!(filtered_msg.contains("Lumi_KLIC"));
        assert!(filtered_msg.contains("THEMA=Laagspanning"));
        assert!(filtered_msg.contains("1.25"));
        assert!(!filtered_msg.contains("OTHER_APP"));

        // Verify system XData is still intact on the entity.
        let xdata = s
            .doc()
            .ok()
            .and_then(|d| d.entity(h))
            .map(|e| e.common.xdata.clone())
            .unwrap_or_default();
        assert!(xdata.iter().any(|t| t.code == 1001 && t.value == "ACAD"));
        assert!(xdata.iter().any(|t| t.code == 1001 && t.value == "AcDbBlockRepBTag"));
    }
}
