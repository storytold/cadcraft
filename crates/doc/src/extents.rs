//! Approximate entity bounds (used for extents, zoom and selection pre-filtering).

use std::collections::HashMap;

use cadcraft_geom::{Arc, Bounds2, EPS, Polyline, Vec2, bulge_to_arc};

use crate::{Drawing, Entity, EntityKind, LwPolyline, Prim};

/// Nested block references deeper than this are ignored (cyclic or hostile files).
pub const MAX_BLOCK_DEPTH: usize = 16;

/// Bounds of a single-line TEXT / ATTDEF / ATTRIB of `len` characters, following its justification
/// the same way the renderer places it (the stroke font is about 0.88 of the height per character).
fn text_box(t: &crate::Text, len: usize) -> Bounds2 {
    use crate::{HAlign, VAlign};
    let w = t.height * 0.9 * t.width_factor.abs().max(0.01) * len.max(1) as f64;
    // Justified text (TL, ML and BL too) hangs from its alignment point (DXF group 11).
    let origin = t.justify_point();
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
    Bounds2::from_points(local.map(|p| origin + p.rotate(t.rotation)))
}

/// Whether an entity inside a block definition shows: its own invisible flag and its layer count,
/// as when drawing. Dimensional constraints of dynamic blocks (layer `*ADSK_CONSTRAINTS`) and the
/// entities a visibility state hides are invisible this way, and they sit far from the block's
/// geometry in real drawings. Layer "0" takes the insert's layer, which the caller has checked.
/// `off_counts`: a layer turned off counts too, as in the drawing's extents ([`extent_bounds`]).
fn in_block_visible(d: &Drawing, e: &Entity, off_counts: bool) -> bool {
    // Attribute definitions are not drawn in an insert (its attribs are, see below).
    !matches!(e.kind, EntityKind::AttDef(_)) && e.common.visible && layer_counts(d, &e.common.layer, off_counts)
}

/// Whether what lies on `layer` inside an insert counts: layer "0" takes the insert's layer, which
/// the caller has checked; a frozen layer never counts, one turned off only with `off_counts`.
fn layer_counts(d: &Drawing, layer: &str, off_counts: bool) -> bool {
    layer == "0" || d.layer(layer).is_none_or(|l| !l.frozen && (l.on || off_counts))
}

/// The layer an insert's attribute draws on, as the renderer takes it: its own, else its
/// definition's (layer "0" being the insert's).
fn attrib_layer<'a>(d: &'a Drawing, ins: &crate::Insert, a: &'a crate::Attrib) -> &'a str {
    a.props.layer.as_deref().unwrap_or_else(|| {
        d.block(&ins.block)
            .and_then(|blk| blk.entities.iter().find(|be| matches!(&be.kind, EntityKind::AttDef(ad) if ad.tag.eq_ignore_ascii_case(&a.tag))))
            .map_or("0", |be| be.common.layer.as_str())
    })
}

/// A text that draws no glyph (empty, or only blanks and non-breaking spaces) takes no room.
fn blank(s: &str) -> bool {
    s.replace("\\~", " ").chars().all(char::is_whitespace)
}

/// Approximate box of an MTEXT, from its attachment point, width and line count.
fn mtext_box(t: &crate::MText) -> Bounds2 {
    let lines = t.contents.split("\\P").count().max(1);
    let longest = t.contents.split("\\P").map(|l| l.chars().count()).max().unwrap_or(1);
    let estimate = t.height * 0.8 * longest as f64;
    // The wrap width only narrows the box: a garbage width must not stretch the drawing extents (#2465).
    let mut w = if t.width > 0.0 { t.width.min(estimate) } else { estimate };
    let mut h = t.height * 1.66 * lines as f64;
    if let Some(c) = t.columns.as_ref().filter(|_| t.width > 0.0) {
        // Columns side by side, the text flowing down each in turn.
        w = c.total_width(t.width);
        h = if c.max_height() > 0.0 { c.max_height() } else { h };
    }
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

/// The box of what `e` draws (block contents on layers turned off left out).
pub fn entity_bounds(d: &Drawing, e: &Entity, depth: usize) -> Bounds2 {
    bounds_of(d, e, depth, false)
}

/// The box `e` takes in the drawing's extents: as [`entity_bounds`], but block contents on a layer
/// turned off count, as AutoCAD 2024's ZOOM Extents counts them (a frozen layer's do not).
pub fn extent_bounds(d: &Drawing, e: &Entity) -> Bounds2 {
    bounds_of(d, e, 0, true)
}

fn bounds_of(d: &Drawing, e: &Entity, depth: usize, off_counts: bool) -> Bounds2 {
    // A top-level annotative object measures as it draws at the current annotation scale.
    if depth == 0 {
        match d.anno_draw(e) {
            crate::AnnoDraw::Plain => {}
            crate::AnnoDraw::Hidden => return Bounds2::EMPTY,
            crate::AnnoDraw::Scaled(f) => return bounds_in(d, &crate::anno_scaled(e, f), 0, &mut HashMap::new(), off_counts),
        }
    }
    bounds_in(d, e, depth, &mut HashMap::new(), off_counts)
}

/// [`entity_bounds`] with the bounds of block contents (in block coordinates) already computed,
/// by block name and depth: every further reference to a block at that depth reuses them instead of
/// expanding the block again, so nested or self-referencing blocks cost at most one pass over each
/// definition per depth instead of one per reference (which grows exponentially with the nesting
/// depth). The depth is part of the key because [`MAX_BLOCK_DEPTH`] cuts a deep visit short: its
/// bounds must not stand in for a shallower reference to the same block (#706).
fn bounds_in<'a>(d: &'a Drawing, e: &Entity, depth: usize, blocks: &mut HashMap<(&'a str, usize), Bounds2>, off_counts: bool) -> Bounds2 {
    match &e.kind {
        EntityKind::Table(t) if t.is_broken() => t.bounds(),
        EntityKind::Text(t) if blank(&t.value) => Bounds2::EMPTY,
        EntityKind::MText(t) if blank(&t.contents) => Bounds2::EMPTY,
        // Text that matches the layout's orientation is drawn upright (#2498).
        EntityKind::Text(t) if e.common.matches_layout_orientation() => {
            text_box(&crate::Text { rotation: 0.0, ..t.clone() }, t.value.chars().count())
        }
        EntityKind::MText(t) if e.common.matches_layout_orientation() => mtext_box(&crate::MText { rotation: 0.0, ..t.clone() }),
        EntityKind::Text(t) => text_box(t, t.value.chars().count()),
        EntityKind::AttDef(a) => text_box(&a.text, a.tag.chars().count()),
        EntityKind::MText(t) => mtext_box(t),
        EntityKind::Insert(ins) => {
            if depth >= MAX_BLOCK_DEPTH {
                return Bounds2::EMPTY;
            }
            // A reference to a missing block (an unloaded xref, a purged definition) or to an empty
            // one draws nothing, so it contributes nothing: ZOOM Extents must not fly off to its
            // insertion point. Attributes below still count.
            let mut b = Bounds2::EMPTY;
            if let Some(blk) = d.block(&ins.block) {
                let m = ins.transform(blk.base.xy());
                let inner = match blocks.get(&(blk.name.as_str(), depth)) {
                    Some(b) => *b,
                    None => {
                        let mut inner = Bounds2::EMPTY;
                        for be in blk.entities.iter().filter(|be| in_block_visible(d, be, off_counts)) {
                            inner = inner.union(&bounds_in(d, be, depth + 1, blocks, off_counts));
                        }
                        blocks.insert((blk.name.as_str(), depth), inner);
                        inner
                    }
                };
                b = if inner.is_empty() { Bounds2::EMPTY } else { Bounds2::from_points(inner.corners().map(|c| m.apply(c))) };
                if ins.cols > 1 || ins.rows > 1 {
                    let off =
                        Vec2::new(ins.col_spacing * f64::from(ins.cols.saturating_sub(1)), ins.row_spacing * f64::from(ins.rows.saturating_sub(1)))
                            .rotate(ins.rotation);
                    b = b.union(&Bounds2::from_points(b.corners().map(|c| c + off)));
                }
            }
            for a in &ins.attribs {
                if !a.invisible && layer_counts(d, attrib_layer(d, ins, a), off_counts) {
                    b = b.union(&match a.multiline_text() {
                        Some(m) => mtext_box(&m),
                        None => text_box(&a.text, a.text.value.chars().count()),
                    });
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
                // Reuse the shared block-bounds cache (as the Insert branch does) so a dimension
                // block with branching or self-referencing children costs one pass per definition
                // per depth, not one per reference (which grows exponentially) (CWE-400).
                let inner = match blocks.get(&(blk.name.as_str(), depth)) {
                    Some(cached) => *cached,
                    None => {
                        let mut inner = Bounds2::EMPTY;
                        for be in blk.entities.iter().filter(|be| in_block_visible(d, be, off_counts)) {
                            inner = inner.union(&bounds_in(d, be, depth + 1, blocks, off_counts));
                        }
                        blocks.insert((blk.name.as_str(), depth), inner);
                        inner
                    }
                };
                b = b.union(&inner);
            }
            b
        }
        EntityKind::MLeader(m) => {
            let mut b = Bounds2::from_points(m.leaders.iter().flatten().map(|v| v.xy()).chain(std::iter::once(m.landing.xy())));
            if let Some(t) = &m.text {
                b.add(t.insert.xy());
                if !blank(&t.contents) {
                    b = b.union(&mtext_box(t));
                }
            }
            if let Some(ins) = &m.block {
                let be = Entity { handle: e.handle, common: e.common.clone(), kind: EntityKind::Insert(ins.clone()) };
                b = b.union(&bounds_in(d, &be, depth, blocks, off_counts));
            }
            b
        }
        EntityKind::Table(t) => {
            let w: f64 = t.col_widths.iter().sum();
            let h: f64 = t.row_heights.iter().sum();
            Bounds2::new(t.insert.xy(), t.insert.xy() + Vec2::new(w, -h))
        }
        // Text widths estimated like `text_box`; the renderer measures the font.
        EntityKind::Tolerance(t) => t.layout(d, &|s, h| h * 0.9 * s.chars().count() as f64).bounds,
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
                    // Infinite lines don't contribute to extents, rays included (as in ZOOM Extents;
                    // real drawings carry stray rays millions of units away).
                    Prim::Infinite { .. } => Bounds2::EMPTY,
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

    fn mtext(contents: &str, width: f64) -> crate::MText {
        crate::MText {
            insert: Vec3::new(10.0, 10.0, 0.0),
            height: 2.5,
            width,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: contents.into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }
    }

    /// A garbage wrap width only narrows an MTEXT's box, never widens it: a huge group 41 must not
    /// stretch the drawing extents (#2465).
    #[test]
    fn a_huge_mtext_wrap_width_does_not_inflate_the_extents() {
        let b = bounds(EntityKind::MText(mtext("AB", 300_000_009_519_104.0)));
        // "AB" at height 2.5 is 2 chars * 0.8 * 2.5 = 4 wide.
        assert!(b.max.x <= 10.0 + 4.0 + 1e-9, "box ends at x = {}", b.max.x);
        assert!(b.min.x >= 10.0 - 1e-9);
    }

    /// A wrap width narrower than the text still bounds the box to that width.
    #[test]
    fn a_narrow_mtext_wrap_width_still_sets_the_box_width() {
        let b = bounds(EntityKind::MText(mtext("A LONG LINE OF WRAPPING TEXT", 5.0)));
        assert!((b.max.x - b.min.x - 5.0).abs() < 1e-9, "width {}", b.max.x - b.min.x);
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
        let a = crate::Attrib {
            tag: "HELLO".into(),
            text,
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        };
        let b = bounds(EntityKind::AttDef(a));
        assert!(b.min.x <= -4.4 && b.max.x < 1.0);
    }

    #[test]
    fn multileader_text_is_inside_its_bounds() {
        let mtext = crate::MText {
            insert: Vec3::new(25.45, 0.0, 0.0),
            height: 5.0,
            width: 0.0,
            attach: 4,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "N".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        let ml = crate::MLeader {
            leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
            landing: Vec3::new(25.0, 0.0, 0.0),
            dogleg: 0.45,
            text: Some(mtext.clone()),
            style: "Standard".into(),
            arrow_size: 1.0,
            spline: false,
            block: None,
        };
        let b = bounds(EntityKind::MLeader(ml.clone()));
        // The glyph spans x 25.45..28.78, y -2.5..2.5.
        assert!(b.min.x <= 0.0 && b.max.x >= 28.79 && b.min.y <= -2.5 && b.max.y >= 2.5);
        let mut empty = ml;
        empty.text = Some(crate::MText { contents: " ".into(), ..mtext });
        assert!(bounds(EntityKind::MLeader(empty)).max.x < 26.0);
    }
}
