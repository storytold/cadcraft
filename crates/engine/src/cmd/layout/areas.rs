//! The part of the model each layout shows: `layout.areas` lists the rectangle every layout
//! viewport covers in model space, `layout.setarea` moves or resizes one of them.
//!
//! A viewport keeps its shape, so an area keeps its aspect ratio: it is a centre and a height,
//! and its width follows from the viewport's.

use cadcraft_doc::{EntityKind, Handle, Space, Viewport};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::super::{bad, point_param};
use crate::{Result, Session};

/// The largest model extent an area may have.
const MAX_EXTENT: f64 = 1e12;

/// The corners `[min, max]` of the model area `v` shows, or `None` for a viewport that shows nothing.
fn window(v: &Viewport) -> Option<[Vec2; 2]> {
    if v.id == 1 || !(v.height > 1e-12 && v.view_height > 1e-12) {
        return None;
    }
    let half = Vec2::new(v.view_height * v.width / v.height, v.view_height) * 0.5;
    Some([v.view_center - half, v.view_center + half])
}

/// `layout.areas`: every layout's viewports, in tab order, with the model area each shows.
pub(super) fn run_areas(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut layouts: Vec<_> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    let mut areas = Vec::new();
    for l in layouts {
        for e in l.entities.iter() {
            let EntityKind::Viewport(v) = &e.kind else { continue };
            let Some([a, b]) = window(v) else { continue };
            areas.push(json!({
                "layout": l.name,
                "tabOrder": l.tab_order,
                "handle": e.handle.hex(),
                "window": [[a.x, a.y], [b.x, b.y]],
                "on": v.on,
                "locked": v.locked,
            }));
        }
    }
    Ok(Value::Array(areas))
}

/// `layout.setarea {handle, center?: [x, y], height?}`: show another part of the model through a
/// viewport. `center` moves the area and `height` resizes it (its width follows); the one left out
/// stays. A locked viewport is refused, as for a zoom inside it.
pub(super) fn run_setarea(s: &mut Session, p: &Value) -> Result<Value> {
    let h = p
        .get("handle")
        .and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
        .ok_or_else(|| bad("layout.setarea", "`handle` is required"))?;
    let space = s.doc()?.space_of(h);
    let Some(Space::Paper(layout)) = space else {
        return Err(bad("layout.setarea", "`handle` is not a layout viewport"));
    };
    let current = match s.doc()?.space(&Space::Paper(layout.clone())).and_then(|st| st.get(h)).map(|e| e.kind.clone()) {
        Some(EntityKind::Viewport(v)) if v.id != 1 => v,
        _ => return Err(bad("layout.setarea", "`handle` is not a layout viewport")),
    };
    if current.locked {
        return Err(bad("layout.setarea", "the viewport is locked"));
    }
    let center = match p.get("center").filter(|c| !c.is_null()) {
        Some(_) => point_param(p, "center").map(|c| Vec2::new(c.x, c.y)).ok_or_else(|| bad("layout.setarea", "`center` is [x, y]"))?,
        None => current.view_center,
    };
    let height = match p.get("height").filter(|v| !v.is_null()) {
        Some(v) => v.as_f64().ok_or_else(|| bad("layout.setarea", "`height` is a number"))?,
        None => current.view_height,
    };
    let width = height * current.width / current.height.max(1e-12);
    let finite = center.is_finite() && height.is_finite();
    if !finite
        || !(height > 1e-9 && height < MAX_EXTENT && width > 1e-9 && width < MAX_EXTENT)
        || center.x.abs() > MAX_EXTENT
        || center.y.abs() > MAX_EXTENT
    {
        return Err(bad("layout.setarea", "the area needs a finite centre and a height above zero"));
    }
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Viewport(v) = &mut e.kind {
            v.view_center = center;
            v.view_height = height;
        }
    })?;
    Ok(json!({ "layout": layout, "handle": h.hex(), "center": [center.x, center.y], "height": height }))
}
