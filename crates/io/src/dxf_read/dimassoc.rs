use super::common::T;
use super::{Arc, AssocSnap, Circle, DimAssoc, DimKind, Dimension, Drawing, EntityKind, Handle, Tag, Vec2, Vec3};

/// Associativity from a DIMASSOC object (files from other writers). The dimension kind decides
/// which definition points the references name, in the order AutoCAD writes them: `p13`/`p14`
/// for linear, aligned and arc-length dimensions, both ends of both lines (`p13`..`p16`) for
/// angular dimensions, the three points of an angular-3-point dimension, `p13` for ordinate
/// dimensions, and for radial or diametric dimensions the single circle reference
/// ([`radial_links`]). CADCraft's own xdata links, when present, win.
pub(super) fn dimassoc(d: &mut Drawing, tags: &[Tag]) {
    let start = tags.iter().position(|x| x.code == 100 && x.str() == "AcDbDimAssoc").unwrap_or(0);
    let tail = tags.get(start..).unwrap_or(&[]);
    let Some(dim_h) = tail.iter().find(|x| x.code == 330).and_then(|x| Handle::parse_hex(&x.str())) else { return };
    let Some(EntityKind::Dimension(dm)) = d.entity(dim_h).map(|e| &e.kind) else { return };
    if !dm.assoc.is_empty() {
        return;
    }
    // A radial or diametric dimension names its circle once, by its centre.
    if matches!(dm.kind, DimKind::Radius | DimKind::Diameter) {
        let links = radial_links(d, dm, tail);
        if !links.is_empty() {
            let _ = d.modify_entity(dim_h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.assoc = links;
                }
            });
        }
        return;
    }
    let slots: &[&str] = match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned => &["p13", "p14"],
        DimKind::Angular => &["p13", "p14", "p15", "p16"],
        DimKind::Angular3P => &["p13", "p14", "p15"],
        DimKind::ArcLength => &["p13", "p14"],
        DimKind::Ordinate { .. } => &["p13"],
        DimKind::Radius | DimKind::Diameter => return,
    };
    let flags = T(tail).i(90).unwrap_or(0);
    let used: Vec<&str> = slots.iter().enumerate().filter(|(i, _)| flags & (1 << i) != 0).map(|(_, s)| *s).collect();
    // Point references start at each `1 AcDbOsnapPointRef`.
    let starts: Vec<usize> = tail.iter().enumerate().filter(|(_, x)| x.code == 1).map(|(i, _)| i).take(8).collect();
    let mut links = Vec::new();
    for (k, s) in starts.iter().enumerate() {
        let Some(point) = used.get(k) else { break };
        let end = starts.get(k + 1).copied().unwrap_or(tail.len());
        let r = T(tail.get(s + 1..end).unwrap_or(&[]));
        let Some(main) = r.s(331).and_then(|h| Handle::parse_hex(&h)) else { continue };
        let other = r.s(332).and_then(|h| Handle::parse_hex(&h));
        let at = r.p(10).xy();
        // A polyline's segment (subentity type 73 = 2, edge; 91 its 1-based index).
        let segment = r.i(91).filter(|k| *k >= 1 && r.i(73) == Some(2));
        if let Some(snap) = infer_snap(d, r.i(72).unwrap_or(0), main, other, at, segment) {
            links.push(DimAssoc { point: (*point).to_string(), handle: main, snap });
        }
    }
    if !links.is_empty() {
        let _ = d.modify_entity(dim_h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.assoc = links;
            }
        });
    }
}

/// Associations of a radial or diametric dimension from its one standard reference: the circle
/// or arc it measures, by its centre (osnap 3). The definition points are then on that circle,
/// so the dimension follows its radius or diameter.
fn radial_links(d: &Drawing, dm: &Dimension, tail: &[Tag]) -> Vec<DimAssoc> {
    let starts: Vec<usize> = tail.iter().enumerate().filter(|(_, x)| x.code == 1 && x.str() == "AcDbOsnapPointRef").map(|(i, _)| i).take(8).collect();
    for (k, s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(tail.len());
        let r = T(tail.get(s + 1..end).unwrap_or(&[]));
        if !matches!(r.i(72), Some(3 | 4)) {
            continue;
        }
        let Some(handle) = r.s(331).and_then(|h| Handle::parse_hex(&h)) else { continue };
        let Some(centre) = circle_centre(d, handle) else { continue };
        let on = |point: &str, p: Vec3| {
            let angle = centre.angle_to(p.xy());
            angle.is_finite().then(|| DimAssoc { point: point.into(), handle, snap: AssocSnap::OnCircle { angle } })
        };
        let links = match dm.kind {
            DimKind::Radius => on("p15", dm.p15).map(|p15| vec![DimAssoc { point: "defpt".into(), handle, snap: AssocSnap::Center }, p15]),
            DimKind::Diameter => [on("defpt", dm.defpt), on("p15", dm.p15)].into_iter().collect::<Option<Vec<_>>>(),
            _ => None,
        };
        if let Some(links) = links {
            return links;
        }
    }
    Vec::new()
}

/// The planar centre of a circle or arc, or `None` for any other entity: the point a radial or
/// diametric dimension's single DIMASSOC reference names.
fn circle_centre(d: &Drawing, h: Handle) -> Option<Vec2> {
    match &d.entity(h)?.kind {
        EntityKind::Circle(c) => Some(c.center.xy()),
        EntityKind::Arc(a) => Some(a.center.xy()),
        _ => None,
    }
}

/// A CADCraft snap from a DIMASSOC object snap type and point, and for a polyline the segment
/// the point is an end of: AutoCAD writes no usable point there (10/20 are 0, #1049), the segment
/// and the osnap (1 its end, 13 its start) name the vertex. For a line or arc the osnap alone
/// says which end: 13 is the start and 1 the end, whatever the stored point (the point is
/// 0,0,0 for curves, and choosing by distance gave the wrong end for arcs). A point entity has
/// only one position, so both codes mean it.
fn infer_snap(d: &Drawing, osnap: i64, main: Handle, other: Option<Handle>, at: Vec2, segment: Option<i64>) -> Option<AssocSnap> {
    let e = d.entity(main)?;
    if let (EntityKind::LwPolyline(p), Some(k), 1 | 13) = (&e.kind, segment, osnap) {
        let n = p.vertices.len();
        let segments = if p.closed { n } else { n.saturating_sub(1) };
        let k = usize::try_from(k).ok().filter(|k| *k <= segments)?;
        let index = if osnap == 13 { k - 1 } else { k % n };
        return Some(AssocSnap::Vertex { index });
    }
    Some(match osnap {
        1 | 13 => match &e.kind {
            // AutoCAD names a curve's start with 13 and its end with 1 (the stored point is 0,0,0).
            // Other writers use 1 for either end of a line with the point stored: take the nearer.
            EntityKind::Line(l) => {
                if osnap == 13 || (at != Vec2::ZERO && l.a.xy().dist(at) <= l.b.xy().dist(at)) {
                    AssocSnap::Start
                } else {
                    AssocSnap::End
                }
            }
            EntityKind::Arc(_) => {
                if osnap == 13 {
                    AssocSnap::Start
                } else {
                    AssocSnap::End
                }
            }
            EntityKind::LwPolyline(p) => {
                let index = p.vertices.iter().enumerate().min_by(|x, y| x.1.p.dist(at).total_cmp(&y.1.p.dist(at))).map(|(i, _)| i)?;
                AssocSnap::Vertex { index }
            }
            EntityKind::Point(_) => AssocSnap::Start,
            _ => return None,
        },
        2 => AssocSnap::Mid,
        3 | 4 => AssocSnap::Center,
        5 | 10 => match &e.kind {
            EntityKind::Circle(Circle { center, .. }) | EntityKind::Arc(Arc { center, .. }) => {
                let v = at - center.xy();
                AssocSnap::OnCircle { angle: v.y.atan2(v.x) }
            }
            _ => return None,
        },
        6 => AssocSnap::Intersection { other: other.filter(|o| d.entity(*o).is_some())? },
        _ => return None,
    })
}
