//! SHAPES: geometry facts for every closed shape in the current space — perimeter, area, centroid,
//! winding, the first self-intersection and coincident vertices — for agents that read a drawing.
//! Read-only.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::{Entity, EntityKind};
use cadcraft_geom::{Bounds2, Polyline, Vec2, intersect_segments};
use serde_json::{Value, json};

use super::super::*;
use super::{area_of, object_area};
use crate::{Result, Session};

const CMD: &str = "shapes";

pub const PARAMS: &str = "{type?, layer?, window?: [[x,y],[x,y]], crossing?: bool (default true; false = entirely inside), limit? (default 200), offset?, tolerance?: positive number (default 1e-9 × the extents diagonal; near-duplicate vertices and a first vertex equal to the last)} → {schema: 1, count (all closed shapes), returned, offset, truncated, shapes: [{handle, type, layer, perimeter, area (≥ 0), winding: ccw|cw, centroid: [x,y] | null (zero area), selfIntersection: [x,y] | null (the first crossing of two non-adjacent edges), duplicateVertices: [{index, distance} (vertices index and index+1, wrapping)]}], skipped: {<type>: {count, reason}} (the other matches)}: closed polylines, circles, full ellipses and closed splines, arcs and bulges exact";

/// The facts SHAPES reports for one closed shape, or why an entity is not one.
enum Facts {
    Shape { perimeter: f64, area: f64, signed: f64, centroid: Option<Vec2>, cross: Option<Vec2>, dups: Vec<(usize, f64)> },
    Skip(&'static str),
}

pub fn run(s: &mut Session, p: &Value) -> Result<Value> {
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
                Some([a, b]) => Ok(Bounds2::new(*a, *b)),
                _ => Err(bad(CMD, "`window` must be two corner points [[x0, y0], [x1, y1]]")),
            }
        })
        .transpose()?;
    let crossing = given("crossing").map_or(Ok(true), |v| v.as_bool().ok_or_else(|| bad(CMD, "`crossing` must be true or false")))?;
    let limit = count("limit", 200)?;
    let offset = count("offset", 0)?;
    let tolerance = given("tolerance")
        .map(|v| v.as_f64().filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| bad(CMD, "`tolerance` must be a positive number")))
        .transpose()?;
    let s: &Session = s;
    let st = s.state()?;
    let d = &st.doc;
    let empty = || json!({ "schema": 1, "count": 0, "returned": 0, "offset": offset, "truncated": false, "shapes": [], "skipped": {} });
    let Some(store) = d.space(&st.space) else { return Ok(empty()) };
    let tol = tolerance.unwrap_or_else(|| {
        let ext = d.extents(&st.space);
        let diag = if ext.is_empty() { 0.0 } else { ext.min.dist(ext.max) };
        (diag * 1e-9).max(1e-12)
    });
    let wtol = window.as_ref().map(crate::select::window_tol).unwrap_or(0.0);
    let mut skipped: BTreeMap<&str, (usize, BTreeSet<&str>)> = BTreeMap::new();
    let mut shapes: Vec<(&Entity, Facts)> = Vec::new();
    for e in store
        .iter()
        .filter(|e| ty.as_ref().is_none_or(|t| e.kind.type_name().to_ascii_lowercase() == *t || e.kind.dxf_name().to_ascii_lowercase() == *t))
        .filter(|e| layer.as_ref().is_none_or(|l| e.common.layer.to_ascii_lowercase() == *l))
        .filter(|e| window.as_ref().is_none_or(|w| crate::select::in_window(d, e, None, w, crossing, wtol)))
    {
        match facts(s, e, tol)? {
            Facts::Skip(why) => {
                let entry = skipped.entry(e.kind.type_name()).or_default();
                entry.0 += 1;
                entry.1.insert(why);
            }
            f => shapes.push((e, f)),
        }
    }
    let page: Vec<Value> = shapes.iter().skip(offset).take(limit).map(|(e, f)| shape_json(e, f)).collect();
    let skipped: BTreeMap<&str, Value> =
        skipped.into_iter().map(|(k, (n, why))| (k, json!({ "count": n, "reason": why.into_iter().collect::<Vec<_>>().join("; ") }))).collect();
    Ok(json!({
        "schema": 1,
        "count": shapes.len(),
        "returned": page.len(),
        "offset": offset,
        "truncated": page.len() < shapes.len().saturating_sub(offset),
        "shapes": page,
        "skipped": skipped,
    }))
}

fn shape_json(e: &Entity, f: &Facts) -> Value {
    let Facts::Shape { perimeter, area, signed, centroid, cross, dups } = f else { return Value::Null };
    json!({
        "handle": e.handle.hex(),
        "type": e.kind.type_name(),
        "layer": e.common.layer,
        "perimeter": perimeter,
        "area": area,
        "winding": if *signed < 0.0 { "cw" } else { "ccw" },
        "centroid": centroid.map(|c| [c.x, c.y]),
        "selfIntersection": cross.map(|c| [c.x, c.y]),
        "duplicateVertices": dups.iter().map(|(i, dist)| json!({ "index": i, "distance": dist })).collect::<Vec<_>>(),
    })
}

fn facts(s: &Session, e: &Entity, tol: f64) -> Result<Facts> {
    Ok(match &e.kind {
        EntityKind::Circle(c) => {
            let (area, perimeter) = object_area(s, e.handle)?;
            Facts::Shape { perimeter, area, signed: area, centroid: Some(c.center.xy()), cross: None, dups: Vec::new() }
        }
        EntityKind::Ellipse(el) => {
            let geom = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            if !geom.is_full() {
                return Ok(Facts::Skip("open"));
            }
            let (area, perimeter) = object_area(s, e.handle)?;
            Facts::Shape { perimeter, area, signed: area, centroid: Some(geom.center), cross: None, dups: Vec::new() }
        }
        EntityKind::LwPolyline(pl) => {
            let mut vertices = pl.vertices.clone();
            let ends_meet = match (vertices.first(), vertices.last()) {
                (Some(a), Some(b)) => vertices.len() > 1 && a.p.dist(b.p) <= tol,
                _ => false,
            };
            if !pl.closed {
                if !ends_meet {
                    return Ok(Facts::Skip("open"));
                }
                // The last vertex repeats the first: the shape closes there.
                vertices.pop();
            }
            if vertices.len() < 2 {
                return Ok(Facts::Skip("fewer than two vertices"));
            }
            let g = Polyline { vertices, closed: true };
            let signed = g.area();
            let n = g.vertices.len();
            let dups = (0..n)
                .filter_map(|i| {
                    let dist = g.vertices.get(i)?.p.dist(g.vertices.get((i + 1) % n)?.p);
                    (dist <= tol).then_some((i, dist))
                })
                .collect();
            Facts::Shape { perimeter: g.len(), area: signed.abs(), signed, centroid: g.centroid(), cross: first_crossing(&g), dups }
        }
        EntityKind::Spline(sp) => {
            let mut pts = sp.tessellate(1e-3);
            let ends_meet = match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => pts.len() > 2 && a.dist(*b) <= tol.max(1e-9),
                _ => false,
            };
            if !sp.closed && !ends_meet {
                return Ok(Facts::Skip("open"));
            }
            if ends_meet {
                pts.pop();
            }
            if pts.len() < 3 {
                return Ok(Facts::Skip("degenerate"));
            }
            let (area, perimeter) = area_of(&pts);
            let g = Polyline::from_points(&pts, true);
            Facts::Shape { perimeter, area, signed: g.area(), centroid: g.centroid(), cross: first_crossing(&g), dups: Vec::new() }
        }
        EntityKind::Arc(_) => Facts::Skip("open"),
        _ => Facts::Skip("not a closed shape"),
    })
}

/// The first crossing between two non-adjacent edges, in edge order.
fn first_crossing(g: &Polyline) -> Option<Vec2> {
    let segs = g.segments();
    let n = segs.len();
    for (i, a) in segs.iter().enumerate() {
        for (j, b) in segs.iter().enumerate().skip(i + 2) {
            if i == 0 && j + 1 == n {
                continue;
            }
            if let Some(x) = intersect_segments(a, b).first() {
                return Some(*x);
            }
        }
    }
    None
}
