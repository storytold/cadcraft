//! Associative hatches: a hatch keeps the handles of the objects each loop was made from
//! (`HatchLoop::sources`) and is filled again when a command changes them. [`update`] runs after
//! every command that changed the drawing, next to the associative dimension pass.
//!
//! - A changed boundary object: the loop is traced again from its objects.
//! - An erased island: the loop goes, the hatch fills the area.
//! - An erased outer boundary, or one that no longer closes: the hatch keeps its shape and
//!   stops being associative (with a message).
//! - The hatch moved or reshaped without its boundary: it stops being associative.
//! - A new hatch (COPY, paste, EXPLODE) whose loops aren't where its objects are: it stops being
//!   associative.
//!
//! Only hatches whose own objects changed are traced again, so a command costs a scan of the
//! hatches plus the work on the ones it touched.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, Handle, Hatch, HatchLoop, entity_bounds};
use cadcraft_geom::{Bounds2, PolyVertex, Polyline, Vec2, region, shoelace};

use super::{closed_loop, gap_tolerance, object_polys};

/// Upper bound on the source objects of one loop that are traced (hostile files).
const MAX_SOURCES: usize = 4096;
/// Upper bound on new associative hatches checked against their objects in one command (bulk
/// paste); the ones beyond it stop being associative.
const MAX_NEW_CHECKED: usize = 1000;

/// How a loop came out of its objects.
enum Refill {
    Loop(Vec<PolyVertex>),
    /// Every object of the loop was erased.
    Erased,
    /// The objects no longer enclose an area.
    Open,
}

fn changed(before: &Drawing, after: &Drawing, h: Handle) -> bool {
    match (before.entity(h), after.entity(h)) {
        (Some(x), Some(y)) => !Arc::ptr_eq(x, y) && x.kind != y.kind,
        (Some(_), None) => true,
        _ => false,
    }
}

/// A boundary object that is still there (hatches never are boundaries).
fn exists(d: &Drawing, h: Handle) -> bool {
    d.entity(h).is_some_and(|e| !matches!(e.kind, EntityKind::Hatch(_)))
}

fn bounds_of(d: &Drawing, hs: &[Handle]) -> Bounds2 {
    let mut b = Bounds2::EMPTY;
    for e in hs.iter().filter_map(|h| d.entity(*h)) {
        b = b.union(&entity_bounds(d, e, 0));
    }
    b
}

/// `p` moved from where it sat in `from` to the same place in `to` (per axis, so a move, a
/// scale or a stretch of the objects carries a point inside the loop along).
fn map_point(p: Vec2, from: &Bounds2, to: &Bounds2) -> Vec2 {
    let axis = |v: f64, a0: f64, a1: f64, b0: f64, b1: f64| {
        let w = a1 - a0;
        if w.abs() > 1e-12 && (b1 - b0).is_finite() { b0 + (v - a0) / w * (b1 - b0) } else { v + (b0 - a0) }
    };
    Vec2::new(axis(p.x, from.min.x, from.max.x, to.min.x, to.max.x), axis(p.y, from.min.y, from.max.y, to.min.y, to.max.y))
}

fn outline_of(vs: &[PolyVertex], tol: f64) -> Vec<Vec2> {
    Polyline { vertices: vs.to_vec(), closed: true }.tessellate(tol)
}

/// The loop traced again from its objects in `after`; `before` places the point the loop is
/// found around (the drawing the loop's vertices belong to).
fn refill(before: &Drawing, after: &Drawing, l: &HatchLoop) -> Refill {
    let live: Vec<Handle> = l.sources.iter().copied().filter(|h| exists(after, *h)).take(MAX_SOURCES).collect();
    if live.is_empty() {
        return Refill::Erased;
    }
    if let [h] = live.as_slice()
        && let Some(vs) = after.entity(*h).and_then(|e| closed_loop(after, e))
    {
        return Refill::Loop(vs);
    }
    let nb = bounds_of(after, &live);
    if nb.is_empty() {
        return Refill::Open;
    }
    let tol = (nb.width() + nb.height()).max(1e-9) / 20000.0;
    let polys: Vec<Vec<Vec2>> = live.iter().filter_map(|h| after.entity(*h)).flat_map(|e| object_polys(after, e, tol, nb)).collect();
    let snap = ((nb.width() + nb.height()) * 1e-9).max(1e-9);
    let gap = gap_tolerance(after);
    let found = if l.outer {
        // The face around a point inside the old loop, carried along with the objects.
        let ob = bounds_of(before, &live);
        let ob = if ob.is_empty() { nb } else { ob };
        let old = outline_of(&l.vertices, (ob.width() + ob.height()).max(1e-9) / 20000.0);
        region::interior_point(&old)
            .and_then(|q| region::enclosing_loop_gap(&polys, map_point(q, &ob, &nb), snap, gap))
            .or_else(|| region::outline(&polys, snap, gap))
    } else {
        region::outline(&polys, snap, gap)
    };
    match found {
        Some(pts) => Refill::Loop(pts.into_iter().map(PolyVertex::new).collect()),
        None => Refill::Open,
    }
}

/// Two loops cover the same area (same extents and area, within a small tolerance).
pub(super) fn same_shape(a: &[PolyVertex], b: &[PolyVertex]) -> bool {
    let pa = outline_of(a, 1e-3);
    let pb = outline_of(b, 1e-3);
    let (ba, bb) = (Bounds2::from_points(pa.iter().copied()), Bounds2::from_points(pb.iter().copied()));
    if ba.is_empty() || bb.is_empty() {
        return false;
    }
    let size = ba.width() + ba.height();
    let eps = 1e-4 * (1.0 + size);
    ba.min.dist(bb.min) <= eps && ba.max.dist(bb.max) <= eps && (shoelace(&pa).abs() - shoelace(&pb).abs()).abs() <= eps * (1.0 + size)
}

pub(super) fn detach(h: &mut Hatch) {
    h.associative = false;
    h.loops.iter_mut().for_each(|l| l.sources.clear());
}

/// Loops of `h` that aren't loops of `prev` (a hatch split by HATCHEDIT, or one that lost an
/// island, keeps the rest unchanged).
fn reshaped(prev: &Hatch, h: &Hatch) -> bool {
    if prev.loops.len() == h.loops.len() {
        return prev.loops.iter().zip(&h.loops).any(|(a, b)| a.vertices != b.vertices);
    }
    if prev.loops.len().saturating_mul(h.loops.len()) > 4_000_000 {
        return true;
    }
    h.loops.iter().any(|l| !prev.loops.iter().any(|q| q.vertices == l.vertices))
}

/// HATCHEDIT may add or remove whole loops without moving the ones it kept. New associative
/// loops must still match their source objects; a hatch moved on its own must not keep its link.
fn valid_loop_edit(prev: &Hatch, h: &Hatch, after: &Drawing) -> bool {
    if prev.loops.len().saturating_mul(h.loops.len()) > 4_000_000 {
        return false;
    }
    h.loops.iter().all(|l| {
        prev.loops.iter().any(|old| old.vertices == l.vertices)
            || (!l.sources.is_empty() && matches!(refill(after, after, l), Refill::Loop(vs) if same_shape(&vs, &l.vertices)))
    })
}

/// What happens to one associative hatch after a command; `None` when nothing changes.
fn follow(before: &Drawing, after: &Drawing, handle: Handle, h: &Hatch, checked: &mut usize) -> Option<(Hatch, Option<String>)> {
    let prev = match before.entity(handle).map(|e| &e.kind) {
        Some(EntityKind::Hatch(p)) => Some(p),
        _ => None,
    };
    let touched = h.loops.iter().flat_map(|l| &l.sources).any(|s| changed(before, after, *s));
    let mut nh = h.clone();
    if !touched {
        if let (Some(x), Some(y)) = (before.entity(handle), after.entity(handle))
            && Arc::ptr_eq(x, y)
        {
            return None;
        }
        match prev {
            // Moved or reshaped on its own: no longer tied to the boundary.
            Some(p) if reshaped(p, h) && !valid_loop_edit(p, h, after) => {
                detach(&mut nh);
                return Some((nh, None));
            }
            Some(_) => return None,
            None => {
                // A new hatch (a copy, a paste, an exploded block) keeps its link only when its
                // loops are where its objects are.
                *checked += 1;
                let fits = *checked <= MAX_NEW_CHECKED
                    && h.loops
                        .iter()
                        .all(|l| l.sources.is_empty() || matches!(refill(after, after, l), Refill::Loop(vs) if same_shape(&vs, &l.vertices)));
                if fits {
                    return None;
                }
                detach(&mut nh);
                return Some((nh, None));
            }
        }
    }
    let mut loops = Vec::with_capacity(h.loops.len());
    for l in &h.loops {
        if l.sources.is_empty() {
            loops.push(l.clone());
            continue;
        }
        let from = if prev.is_some() { before } else { after };
        match refill(from, after, l) {
            Refill::Loop(vertices) => {
                let sources = l.sources.iter().copied().filter(|s| exists(after, *s)).take(MAX_SOURCES).collect();
                loops.push(HatchLoop { vertices, outer: l.outer, sources });
            }
            // An erased island: the hatch fills its area.
            Refill::Erased if !l.outer => {}
            r @ (Refill::Erased | Refill::Open) => {
                detach(&mut nh);
                let why = if matches!(r, Refill::Erased) { "was erased" } else { "no longer encloses an area" };
                return Some((nh, Some(format!("Hatch {}: its boundary {why}; the hatch is no longer associative.", handle.hex()))));
            }
        }
    }
    if !loops.iter().any(|l| l.outer) && h.loops.iter().any(|l| l.outer) {
        detach(&mut nh);
        return Some((nh, None));
    }
    nh.loops = loops;
    (nh != *h).then_some((nh, None))
}

/// After a change: fill every associative hatch whose boundary objects changed again, and
/// unlink hatches that no longer follow their boundary. Returns the messages to show.
pub fn update(after: &mut Arc<Drawing>, before: &Drawing) -> Vec<String> {
    let mut changes: Vec<(Handle, Hatch)> = Vec::new();
    let mut notes = Vec::new();
    {
        let d: &Drawing = after;
        let mut checked = 0;
        for store in std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)) {
            for e in store.iter() {
                let EntityKind::Hatch(h) = &e.kind else { continue };
                if !h.associative || h.loops.iter().all(|l| l.sources.is_empty()) {
                    continue;
                }
                if let Some((nh, note)) = follow(before, d, e.handle, h, &mut checked) {
                    changes.push((e.handle, nh));
                    notes.extend(note);
                }
            }
        }
    }
    if changes.is_empty() {
        return notes;
    }
    let d = Arc::make_mut(after);
    for (h, nh) in changes {
        let _ = d.modify_entity(h, |e| e.kind = EntityKind::Hatch(nh));
    }
    notes
}
