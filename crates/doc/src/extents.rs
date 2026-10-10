//! Approximate entity bounds (used for extents, zoom and selection pre-filtering).

use cadcraft_geom::{Arc, Bounds2, EPS, Polyline, Vec2, bulge_to_arc};

use crate::{Drawing, Entity, EntityKind, LwPolyline, Prim};

/// Nested block references deeper than this are ignored (cyclic or hostile files).
pub const MAX_BLOCK_DEPTH: usize = 16;

/// Bounds of a single-line TEXT / ATTDEF / ATTRIB of `len` characters, following its justification
/// the same way the renderer places it (the stroke font is about 0.88 of the height per character).
fn text_box(t: &crate::Text, len: usize) -> Bounds2 {
    use crate::{HAlign, VAlign};
    let w = t.height * 0.9 * t.width_factor.abs().max(0.01) * len.max(1) as f64;
    let origin = match t.halign {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => t.insert,
        _ => t.align_pt.unwrap_or(t.insert),
    };
    let dx = match t.halign {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => 0.0,
        HAlign::Center | HAlign::Middle => -w / 2.0,
        HAlign::Right => -w,
    };
    let dy = match (t.halign, t.valign) {
        (HAlign::Middle, _) => -t.height / 2.0,
        (_, VAlign::Baseline) => 0.0,
        (_, VAlign::Bottom) => t.height / 3.0,
        (_, VAlign::Middle) => -t.height / 2.0,
        (_, VAlign::Top) => -t.height,
    };
    let local = [Vec2::new(dx, dy), Vec2::new(dx + w, dy), Vec2::new(dx + w, dy + t.height), Vec2::new(dx, dy + t.height)];
    Bounds2::from_points(local.map(|p| origin.xy() + p.rotate(t.rotation)))
}

pub fn entity_bounds(d: &Drawing, e: &Entity, depth: usize) -> Bounds2 {
    match &e.kind {
        EntityKind::Text(t) => text_box(t, t.value.chars().count()),
        EntityKind::AttDef(a) => text_box(&a.text, a.tag.chars().count()),
        EntityKind::MText(t) => {
            let lines = t.contents.split("\\P").count().max(1);
            let longest = t.contents.split("\\P").map(|l| l.chars().count()).max().unwrap_or(1);
            let w = if t.width > 0.0 { t.width } else { t.height * 0.8 * longest as f64 };
            let h = t.height * 1.66 * lines as f64;
            let (dx, dy) = match t.attach {
                2 | 5 | 8 => (-w / 2.0, 0.0),
                3 | 6 | 9 => (-w, 0.0),
                _ => (0.0, 0.0),
            };
            let top = match t.attach {
                4..=6 => h / 2.0,
                7..=9 => h,
                _ => 0.0,
            };
            let o = t.insert.xy();
            let pts = [Vec2::new(dx, top + dy), Vec2::new(dx + w, top + dy), Vec2::new(dx + w, top - h + dy), Vec2::new(dx, top - h + dy)]
                .map(|p| o + p.rotate(t.rotation));
            Bounds2::from_points(pts)
        }
        EntityKind::Insert(ins) => {
            if depth >= MAX_BLOCK_DEPTH {
                return Bounds2::EMPTY;
            }
            let Some(blk) = d.block(&ins.block) else { return Bounds2::from_points([ins.insert.xy()]) };
            let m = ins.transform(blk.base.xy());
            let mut inner = Bounds2::EMPTY;
            for be in blk.entities.iter() {
                inner = inner.union(&entity_bounds(d, be, depth + 1));
            }
            let mut b =
                if inner.is_empty() { Bounds2::from_points([ins.insert.xy()]) } else { Bounds2::from_points(inner.corners().map(|c| m.apply(c))) };
            if ins.cols > 1 || ins.rows > 1 {
                let off = Vec2::new(ins.col_spacing * f64::from(ins.cols.saturating_sub(1)), ins.row_spacing * f64::from(ins.rows.saturating_sub(1)))
                    .rotate(ins.rotation);
                b = b.union(&Bounds2::from_points(b.corners().map(|c| c + off)));
            }
            for a in &ins.attribs {
                if !a.invisible {
                    b = b.union(&text_box(&a.text, a.text.value.chars().count()));
                }
            }
            b
        }
        EntityKind::Dimension(dm) => {
            let pts: Vec<Vec2> = match dm.kind {
                crate::DimKind::Linear { .. } | crate::DimKind::Aligned | crate::DimKind::ArcLength => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy()],
                crate::DimKind::Radius | crate::DimKind::Diameter => vec![dm.defpt.xy(), dm.p15.xy()],
                crate::DimKind::Angular3P => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy(), dm.p15.xy()],
                crate::DimKind::Angular => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy(), dm.p15.xy(), dm.p16.xy()],
                crate::DimKind::Ordinate { .. } => vec![dm.p13.xy(), dm.p14.xy()],
            };
            let mut b = Bounds2::from_points(pts);
            if dm.user_text_pos {
                b.add(dm.text_mid.xy());
            }
            // Room for text and arrows.
            // The same scale the dimension's geometry uses.
            let th = d
                .dim_style(&dm.style)
                .map(|s| {
                    let s = s.with_overrides(&dm.overrides);
                    s.text_height * s.effective_scale(d.header.f64("DIMSCALE", 1.0))
                })
                .unwrap_or(0.18 * d.header.f64("DIMSCALE", 1.0).max(1e-9));
            b = b.expand(th * 2.5);
            if let Some(blk) = dm.block.as_ref().and_then(|n| d.block(n))
                && depth < MAX_BLOCK_DEPTH
            {
                for be in blk.entities.iter() {
                    b = b.union(&entity_bounds(d, be, depth + 1));
                }
            }
            b
        }
        EntityKind::MLeader(m) => {
            let mut b = Bounds2::from_points(m.leaders.iter().flatten().map(|v| v.xy()).chain(std::iter::once(m.landing.xy())));
            if let Some(t) = &m.text {
                b.add(t.insert.xy());
            }
            b
        }
        EntityKind::Table(t) => {
            let w: f64 = t.col_widths.iter().sum();
            let h: f64 = t.row_heights.iter().sum();
            Bounds2::new(t.insert.xy(), t.insert.xy() + Vec2::new(w, -h))
        }
        EntityKind::Image(i) => {
            let o = i.insert.xy();
            let u = i.u.xy() * i.size.x;
            let v = i.v.xy() * i.size.y;
            Bounds2::from_points([o, o + u, o + v, o + u + v])
        }
        EntityKind::LwPolyline(p) => lwpolyline_bounds(p),
        kind => {
            let mut b = Bounds2::EMPTY;
            for p in kind.prims() {
                let pb = match p {
                    Prim::Seg(s) => s.bounds(),
                    Prim::Circle(c) => c.bounds(),
                    Prim::Ellipse(e) => e.bounds(),
                    Prim::Spline(s) => s.bounds(),
                    Prim::Point(p) => Bounds2::from_points([p]),
                    // Infinite lines don't contribute to extents (as in ZOOM Extents).
                    Prim::Infinite { base, ray, .. } => {
                        if ray {
                            Bounds2::from_points([base])
                        } else {
                            Bounds2::EMPTY
                        }
                    }
                    Prim::Fill(pts) => Bounds2::from_points(pts),
                };
                b = b.union(&pb);
            }
            b
        }
    }
}

/// The polyline's drawn extent: its centre line, widened by the filled band of every segment,
/// with the constant width or each segment's own start/end widths tapering along it, as drawn.
fn lwpolyline_bounds(p: &LwPolyline) -> Bounds2 {
    let mut b = Polyline { vertices: p.vertices.clone(), closed: p.closed }.bounds();
    let n = p.vertices.len();
    let count = if p.closed { n } else { n.saturating_sub(1) };
    // The constant width wins, as when drawing; non-finite or negative widths count as zero.
    let half = |w: f64| if w.is_finite() && w > 0.0 { w / 2.0 } else { 0.0 };
    for i in 0..count {
        let (Some(v), Some(w)) = (p.vertices.get(i), p.vertices.get((i + 1) % n)) else { continue };
        let (h0, h1) = if p.const_width > 0.0 { (half(p.const_width), half(p.const_width)) } else { (half(v.start_width), half(v.end_width)) };
        if h0 == 0.0 && h1 == 0.0 {
            // A thin segment: the centre line already counts.
            continue;
        }
        match bulge_to_arc(v.p, w.p, v.bulge) {
            // A straight segment's band is a trapezoid: its four corners are exact.
            None if !v.p.near(w.p, EPS) => {
                let nrm = (w.p - v.p).normalized().perp();
                for q in [v.p + nrm * h0, v.p - nrm * h0, w.p + nrm * h1, w.p - nrm * h1] {
                    b.add(q);
                }
            }
            None => {}
            // An arc segment: both band edges, sampled finely along the arc with the width
            // interpolated along it, then widened by the sampling tolerance so the curve between
            // samples is inside.
            Some((arc, ccw)) => {
                let outer = arc.radius + h0.max(h1);
                let tol = outer * 1e-4;
                let mut pts = Vec::new();
                Arc { radius: outer, ..arc }.tessellate(tol, &mut pts);
                let last = pts.len().saturating_sub(1).max(1) as f64;
                let mut edges = Bounds2::EMPTY;
                for (k, q) in pts.iter().enumerate() {
                    // The fraction along the segment, which runs against the arc when not ccw.
                    let t = k as f64 / last;
                    let t = if ccw { t } else { 1.0 - t };
                    let h = h0 + (h1 - h0) * t;
                    let dir = (*q - arc.center).normalized();
                    edges.add(arc.center + dir * (arc.radius + h));
                    edges.add(arc.center + dir * (arc.radius - h));
                }
                if !edges.is_empty() {
                    b = b.union(&edges.expand(tol));
                }
            }
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HAlign, Text, VAlign};
    use cadcraft_geom::Vec3;

    fn text(halign: HAlign, valign: VAlign) -> Text {
        Text {
            insert: Vec3::new(0.0, 0.0, 0.0),
            align_pt: Some(Vec3::new(0.0, 0.0, 0.0)),
            height: 1.0,
            value: "HELLO".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign,
            valign,
        }
    }

    fn bounds(kind: EntityKind) -> Bounds2 {
        entity_bounds(&Drawing::default(), &Entity::new(crate::Handle(1), kind), 0)
    }

    #[test]
    fn right_justified_text_is_left_of_alignment_point() {
        let b = bounds(EntityKind::Text(text(HAlign::Right, VAlign::Baseline)));
        // Rendered strokes span x -4.4..0, y 0..1.
        assert!(b.min.x <= -4.4 && b.max.x >= 0.0 && b.max.x < 1.0);
        assert!(b.min.y <= 0.0 && b.max.y >= 1.0);
    }

    #[test]
    fn centered_and_top_left_text_follow_placement() {
        let c = bounds(EntityKind::Text(text(HAlign::Center, VAlign::Baseline)));
        assert!(c.min.x <= -2.2 && c.max.x >= 2.2);
        let tl = bounds(EntityKind::Text(text(HAlign::Left, VAlign::Top)));
        assert!(tl.min.x <= 0.0 && tl.max.x >= 4.4);
        assert!(tl.min.y <= -1.0 && tl.max.y >= 0.0 && tl.max.y < 1.0);
    }

    #[test]
    fn right_justified_attdef_is_left_of_alignment_point() {
        let text = text(HAlign::Right, VAlign::Baseline);
        let a = crate::Attrib { tag: "HELLO".into(), text, invisible: false, constant: false, prompt: String::new() };
        let b = bounds(EntityKind::AttDef(a));
        assert!(b.min.x <= -4.4 && b.max.x < 1.0);
    }
}
