//! Associative dimensions: definition points attached to objects (`Dimension::assoc`) follow
//! them. [`update`] runs after every command that changed the drawing (from
//! `Session::execute` and when an interactive command finishes).

use std::sync::Arc;

use cadcraft_doc::{AssocSnap, DimAssoc, DimKind, Dimension, Drawing, EntityKind, Handle, Space};
use cadcraft_geom::{Vec2, Vec3};

/// Definition point names in the order they are tried.
const POINTS: [&str; 5] = ["defpt", "p13", "p14", "p15", "p16"];

/// A definition point of a dimension by name.
pub fn def_point(d: &Dimension, name: &str) -> Option<Vec3> {
    Some(match name {
        "defpt" => d.defpt,
        "p13" => d.p13,
        "p14" => d.p14,
        "p15" => d.p15,
        "p16" => d.p16,
        "text" => d.text_mid,
        _ => return None,
    })
}

fn def_point_mut<'a>(d: &'a mut Dimension, name: &str) -> Option<&'a mut Vec3> {
    Some(match name {
        "defpt" => &mut d.defpt,
        "p13" => &mut d.p13,
        "p14" => &mut d.p14,
        "p15" => &mut d.p15,
        "p16" => &mut d.p16,
        _ => return None,
    })
}

fn arc_geom(a: &cadcraft_doc::Arc) -> cadcraft_geom::Arc {
    cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end)
}

/// Where a snap sits on an object now.
pub fn snap_point(d: &Drawing, a: &DimAssoc) -> Option<Vec2> {
    let e = d.entity(a.handle)?;
    match (&e.kind, &a.snap) {
        (EntityKind::Line(l), AssocSnap::Start) => Some(l.a.xy()),
        (EntityKind::Line(l), AssocSnap::End) => Some(l.b.xy()),
        (EntityKind::Line(l), AssocSnap::Mid) => Some(l.a.xy().mid(l.b.xy())),
        (EntityKind::Line(l), AssocSnap::Intersection { other }) => match &d.entity(*other)?.kind {
            EntityKind::Line(m) => cadcraft_geom::line_line_infinite(l.a.xy(), l.b.xy(), m.a.xy(), m.b.xy()).map(|x| x.0),
            _ => None,
        },
        (EntityKind::Arc(ar), AssocSnap::Start) => Some(arc_geom(ar).start_point()),
        (EntityKind::Arc(ar), AssocSnap::End) => Some(arc_geom(ar).end_point()),
        (EntityKind::Arc(ar), AssocSnap::Mid) => Some(arc_geom(ar).mid_point()),
        (EntityKind::Arc(ar), AssocSnap::Center) => Some(ar.center.xy()),
        (EntityKind::Arc(ar), AssocSnap::OnCircle { angle }) => Some(ar.center.xy() + Vec2::from_angle(*angle) * ar.radius),
        (EntityKind::Circle(c), AssocSnap::Center) => Some(c.center.xy()),
        (EntityKind::Circle(c), AssocSnap::OnCircle { angle }) => Some(c.center.xy() + Vec2::from_angle(*angle) * c.radius),
        (EntityKind::LwPolyline(p), AssocSnap::Vertex { index }) => p.vertices.get(*index).map(|v| v.p),
        (EntityKind::LwPolyline(p), AssocSnap::Start) => p.vertices.first().map(|v| v.p),
        (EntityKind::LwPolyline(p), AssocSnap::End) => p.vertices.last().map(|v| v.p),
        (EntityKind::Point(p), AssocSnap::Start | AssocSnap::Center) => Some(p.p.xy()),
        _ => None,
    }
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.dist(b) <= 1e-6 * (1.0 + a.x.abs().max(a.y.abs()))
}

/// Candidate snaps of an object (endpoints first, then centres, then midpoints).
fn candidates(kind: &EntityKind) -> Vec<(AssocSnap, Vec2)> {
    match kind {
        EntityKind::Line(l) => vec![(AssocSnap::Start, l.a.xy()), (AssocSnap::End, l.b.xy()), (AssocSnap::Mid, l.a.xy().mid(l.b.xy()))],
        EntityKind::Arc(a) => {
            let g = arc_geom(a);
            vec![
                (AssocSnap::Start, g.start_point()),
                (AssocSnap::End, g.end_point()),
                (AssocSnap::Center, a.center.xy()),
                (AssocSnap::Mid, g.mid_point()),
            ]
        }
        EntityKind::Circle(c) => vec![(AssocSnap::Center, c.center.xy())],
        EntityKind::LwPolyline(p) => p.vertices.iter().take(10_000).enumerate().map(|(i, v)| (AssocSnap::Vertex { index: i }, v.p)).collect(),
        EntityKind::Point(p) => vec![(AssocSnap::Start, p.p.xy())],
        _ => Vec::new(),
    }
}

/// An object point exactly at `p` (an endpoint, vertex, centre or midpoint) to attach to.
pub fn find_snap(d: &Drawing, space: &Space, p: Vec2) -> Option<(Handle, AssocSnap)> {
    if !p.is_finite() {
        return None;
    }
    let store = d.space(space)?;
    let mut best: Option<(usize, Handle, AssocSnap)> = None;
    for e in store.iter() {
        for (rank, (snap, q)) in candidates(&e.kind).into_iter().enumerate() {
            let rank = match snap {
                AssocSnap::Mid => 100 + rank,
                AssocSnap::Center => 50 + rank,
                _ => rank.min(1),
            };
            if close(p, q) && best.as_ref().is_none_or(|b| rank < b.0) {
                best = Some((rank, e.handle, snap));
            }
        }
    }
    best.map(|(_, h, s)| (h, s))
}

/// Associations for named definition points found at their positions.
pub fn auto_assoc(d: &Drawing, space: &Space, pts: &[(&str, Vec2)]) -> Vec<DimAssoc> {
    pts.iter().filter_map(|(name, p)| find_snap(d, space, *p).map(|(handle, snap)| DimAssoc { point: (*name).into(), handle, snap })).collect()
}

/// A circle or arc (centre, radius) by handle.
fn circle_of(d: &Drawing, h: Handle) -> Option<(Vec2, f64)> {
    match &d.entity(h)?.kind {
        EntityKind::Circle(c) => Some((c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some((a.center.xy(), a.radius)),
        _ => None,
    }
}

/// A circle or arc whose centre is at `c` (and radius `r`, when given).
fn circle_at(d: &Drawing, space: &Space, c: Vec2, r: Option<f64>) -> Option<Handle> {
    d.space(space)?.iter().find_map(|e| {
        let (cc, rr) = match &e.kind {
            EntityKind::Circle(x) => (x.center.xy(), x.radius),
            EntityKind::Arc(x) => (x.center.xy(), x.radius),
            _ => return None,
        };
        (close(cc, c) && r.is_none_or(|r| (r - rr).abs() <= 1e-6 * (1.0 + r.abs()))).then_some(e.handle)
    })
}

/// Work out the associations of a dimension from the geometry under its definition points
/// (DIMREASSOCIATE).
pub fn reassociate(d: &Drawing, space: &Space, dm: &Dimension) -> Vec<DimAssoc> {
    match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned => auto_assoc(d, space, &[("p13", dm.p13.xy()), ("p14", dm.p14.xy())]),
        DimKind::Radius => {
            let (c, p) = (dm.defpt.xy(), dm.p15.xy());
            match circle_at(d, space, c, Some(c.dist(p))) {
                Some(h) => vec![
                    DimAssoc { point: "defpt".into(), handle: h, snap: AssocSnap::Center },
                    DimAssoc { point: "p15".into(), handle: h, snap: AssocSnap::OnCircle { angle: c.angle_to(p) } },
                ],
                None => Vec::new(),
            }
        }
        DimKind::Diameter => {
            let (a, b) = (dm.defpt.xy(), dm.p15.xy());
            let c = a.mid(b);
            match circle_at(d, space, c, Some(a.dist(b) / 2.0)) {
                Some(h) => vec![
                    DimAssoc { point: "defpt".into(), handle: h, snap: AssocSnap::OnCircle { angle: c.angle_to(a) } },
                    DimAssoc { point: "p15".into(), handle: h, snap: AssocSnap::OnCircle { angle: c.angle_to(b) } },
                ],
                None => Vec::new(),
            }
        }
        DimKind::Angular3P | DimKind::ArcLength => auto_assoc(d, space, &[("p13", dm.p13.xy()), ("p14", dm.p14.xy()), ("p15", dm.p15.xy())]),
        DimKind::Angular => auto_assoc(d, space, &[("p13", dm.p13.xy()), ("p14", dm.p14.xy()), ("defpt", dm.defpt.xy()), ("p15", dm.p15.xy())]),
        DimKind::Ordinate { .. } => auto_assoc(d, space, &[("p13", dm.p13.xy())]),
    }
}

/// Associations for a radius/diameter dimension made by picking a circle or arc.
pub fn radial_assoc(d: &Drawing, h: Handle, diameter: bool, defpt: Vec2, p15: Vec2) -> Vec<DimAssoc> {
    let Some((c, _)) = circle_of(d, h) else { return Vec::new() };
    let on = |name: &str, p: Vec2| DimAssoc { point: name.into(), handle: h, snap: AssocSnap::OnCircle { angle: c.angle_to(p) } };
    if diameter {
        vec![on("defpt", defpt), on("p15", p15)]
    } else {
        vec![DimAssoc { point: "defpt".into(), handle: h, snap: AssocSnap::Center }, on("p15", p15)]
    }
}

fn src_changed(before: &Drawing, after: &Drawing, h: Handle) -> bool {
    match (before.entity(h), after.entity(h)) {
        (Some(x), Some(y)) => !Arc::ptr_eq(x, y) && x.kind != y.kind,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Recompute an associative dimension's definition points from its objects. Attachments that
/// no longer hold (the object was erased, or the point had been moved off the object before
/// this change) are dropped.
/// `prev` is the dimension as it was before the change (when it existed).
pub fn recompute(dm: &Dimension, prev: Option<&Dimension>, before: &Drawing, after: &Drawing) -> Dimension {
    let mut nd = dm.clone();
    let mut deltas: Vec<Vec2> = Vec::new();
    let mut moved: Vec<String> = Vec::new();
    let was = prev.unwrap_or(dm);
    nd.assoc.retain(|a| {
        let Some(old) = def_point(was, &a.point) else { return false };
        // Only follow if the point sat on the object before the change.
        if let Some(prev) = snap_point(before, a)
            && !close(prev, old.xy())
        {
            return false;
        }
        snap_point(after, a).is_some()
    });
    for a in &nd.assoc {
        if let (Some(p), Some(old)) = (snap_point(after, a), def_point(dm, &a.point)) {
            deltas.push(p - old.xy());
            moved.push(a.point.clone());
        }
    }
    for a in nd.assoc.clone() {
        if let (Some(p), Some(slot)) = (snap_point(after, &a), def_point_mut(&mut nd, &a.point)) {
            slot.x = p.x;
            slot.y = p.y;
        }
    }
    // A rigid move of every attached point carries the rest of the dimension along.
    if let Some(first) = deltas.first().copied()
        && deltas.iter().all(|d| d.dist(first) <= 1e-9 * (1.0 + first.len()))
        && first.len() > 0.0
    {
        // An ordinate's `defpt` is its fixed datum (the drawing origin); it must not follow the feature.
        let ordinate = matches!(nd.kind, DimKind::Ordinate { .. });
        for name in POINTS {
            if !moved.iter().any(|m| m == name)
                && !(ordinate && name == "defpt")
                && let Some(slot) = def_point_mut(&mut nd, name)
            {
                slot.x += first.x;
                slot.y += first.y;
            }
        }
        nd.text_mid.x += first.x;
        nd.text_mid.y += first.y;
    }
    if nd != *dm {
        nd.block = None;
    }
    nd
}

/// After a change: update every associative dimension whose objects changed.
/// Returns the number of dimensions updated.
pub fn update(after: &mut Arc<Drawing>, before: &Drawing) -> usize {
    let mut changes: Vec<(Handle, Dimension)> = Vec::new();
    {
        let d: &Drawing = after;
        let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
        for store in stores {
            for e in store.iter() {
                let EntityKind::Dimension(dm) = &e.kind else { continue };
                if dm.assoc.is_empty() {
                    continue;
                }
                let touched = dm.assoc.iter().any(|a| {
                    src_changed(before, d, a.handle) || matches!(a.snap, AssocSnap::Intersection { other } if src_changed(before, d, other))
                });
                if !touched {
                    continue;
                }
                let prev = match before.entity(e.handle).map(|x| &x.kind) {
                    Some(EntityKind::Dimension(p)) => Some(p),
                    _ => None,
                };
                let nd = recompute(dm, prev, before, d);
                if nd != *dm {
                    changes.push((e.handle, nd));
                }
            }
        }
    }
    if changes.is_empty() {
        return 0;
    }
    let d = Arc::make_mut(after);
    let n = changes.len();
    for (h, nd) in changes {
        let _ = d.modify_entity(h, |e| e.kind = EntityKind::Dimension(nd));
    }
    n
}

/// The post-command pass: called by `Session::execute` and when an interactive command
/// finishes. `uid` (when known) is the document the command started in.
pub fn after_command(s: &mut crate::Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) {
    let Some(before) = before else { return };
    if let Ok(st) = s.state_mut()
        && uid.is_none_or(|u| u == st.uid)
        && !Arc::ptr_eq(before, &st.doc)
    {
        update(&mut st.doc, before);
    }
}
