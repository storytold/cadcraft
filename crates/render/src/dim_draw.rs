use super::dim::ArrowGeom;
use super::display::Kind;
use super::draw::stroke_with;
use super::props::default_lineweight;
use super::text_entity::{layout_mtext_entity, place_text_entity, text_style};
use super::{Builder, Color, Ctx, DimGeometry, DimText, Drawing, Entity, EntityKind, LineRole, Lineweight, Vec2, dim, fill, ink};
use cadcraft_geom::Bounds2;

/// The font settings of a dimension style's text style.
pub fn dim_text(d: &Drawing, st: &cadcraft_doc::DimStyle) -> DimText {
    let ts = text_style(d, &st.text_style);
    let mut user_arrows: Vec<(String, ArrowGeom)> = Vec::new();
    for name in [&st.arrow_block, &st.arrow_block1, &st.arrow_block2] {
        let n = name.trim();
        if n.is_empty() || user_arrows.iter().any(|(k, _)| k.eq_ignore_ascii_case(n)) {
            continue;
        }
        if let Some(g) = user_arrow(d, n) {
            user_arrows.push((n.to_string(), g));
        }
    }
    DimText {
        font: cadcraft_fonts::TextFont::resolve_style(&ts.font, &ts.big_font),
        fixed_height: ts.height,
        width_factor: ts.width_factor,
        oblique: ts.oblique,
        user_arrows,
    }
}

/// The block a DIMBLK name refers to, if the drawing has one (names compare without case).
pub fn arrow_block<'a>(d: &'a Drawing, name: &str) -> Option<&'a cadcraft_doc::Block> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    d.blocks.get(n).or_else(|| d.blocks.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, b)| b)).map(|b| b.as_ref())
}

/// The arrowhead `name` names (a DIMBLK-style name, or a user arrowhead block of `d`) at `tip`,
/// pointing along `dir`, `size` long.
pub fn named_arrow(d: &Drawing, name: &str, tip: Vec2, dir: Vec2, size: f64) -> ArrowGeom {
    let dir = if dir == Vec2::ZERO || !dir.is_finite() { Vec2::X } else { dir };
    match user_arrow(d, name.trim()) {
        Some(g) => {
            let size = if size.is_finite() { size.abs() } else { 0.0 };
            let n = dir.perp();
            let at = |p: &Vec2| tip + dir * (p.x * size) + n * (p.y * size);
            ArrowGeom { lines: g.lines.iter().map(|l| l.iter().map(at).collect()).collect(), tris: g.tris.iter().map(at).collect() }
        }
        None => dim::arrowhead(dim::Arrowhead::parse(name), tip, dir, size),
    }
}

/// A user arrowhead: the block's lines, arcs, circles and polylines as strokes, its solids,
/// solid hatches and wide polylines as fills, relative to the block base (the arrow's tip,
/// the arrow pointing along +x at unit size). Built-in arrowhead blocks (`_Oblique`…) are
/// drawn in code instead, so they are left out.
fn user_arrow(d: &Drawing, name: &str) -> Option<ArrowGeom> {
    if name.starts_with('_') {
        return None;
    }
    // Worked out from the block each time (at most 512 of its objects): a cache keyed by the
    // block's address could hand one drawing's arrowhead to another, or keep an edited one.
    arrow_geometry(arrow_block(d, name)?)
}

/// The geometry of a user arrowhead block (see [`user_arrow`]).
pub(crate) fn arrow_geometry(b: &cadcraft_doc::Block) -> Option<ArrowGeom> {
    use cadcraft_doc::EntityKind as K;
    const SEGS: usize = 32;
    let base = b.base.xy();
    let mut g = ArrowGeom::default();
    let arc_pts = |c: Vec2, r: f64, a0: f64, sweep: f64| -> Vec<Vec2> {
        (0..=SEGS).map(|i| c - base + Vec2::from_angle(a0 + sweep * i as f64 / SEGS as f64) * r).collect()
    };
    // A polyline's points, its arcs sampled.
    let poly_pts = |vs: &[cadcraft_geom::PolyVertex], closed: bool| -> Vec<Vec2> {
        let mut out = Vec::new();
        let n = vs.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let (Some(a), Some(c)) = (vs.get(i), vs.get((i + 1) % n.max(1))) else { break };
            out.push(a.p - base);
            if a.bulge.abs() > 1e-9 {
                let theta = 4.0 * a.bulge.atan();
                let chord = c.p - a.p;
                if chord.len() > 1e-12 {
                    // The centre sits off the chord's midpoint by (1 - b²) / 4b chords, to its left.
                    let center = a.p.mid(c.p) + chord.perp() * ((1.0 - a.bulge * a.bulge) / (4.0 * a.bulge));
                    let r = (a.p - center).len();
                    let a0 = (a.p - center).angle();
                    for k in 1..SEGS {
                        out.push(center - base + Vec2::from_angle(a0 + theta * k as f64 / SEGS as f64) * r);
                    }
                }
            }
        }
        if let Some(last) = if closed { vs.first() } else { vs.last() } {
            out.push(last.p - base);
        }
        out
    };
    // Points worked out at most (strokes and fill triangles together): past it the arrowhead
    // is not drawn from the block (the built-in one is), so a block of huge or many curves can't
    // stall drawing every dimension that uses it.
    const MAX_POINTS: usize = 200_000;
    let used = |g: &ArrowGeom| g.lines.iter().map(Vec::len).sum::<usize>().saturating_add(g.tris.len());
    for e in b.entities.iter().take(512) {
        // What this object may add before it is worked out: its vertices, every arc sampled.
        let input = match &e.kind {
            K::LwPolyline(p) => p.vertices.len().saturating_mul(SEGS),
            K::Hatch(h) => h.loops.iter().map(|l| l.vertices.len()).sum::<usize>().saturating_mul(SEGS),
            _ => SEGS,
        };
        if used(&g).saturating_add(input) > MAX_POINTS {
            return None;
        }
        match &e.kind {
            K::Line(l) => g.lines.push(vec![l.a.xy() - base, l.b.xy() - base]),
            K::Circle(c) => g.lines.push(arc_pts(c.center.xy(), c.radius, 0.0, std::f64::consts::TAU)),
            K::Arc(a) => {
                let sweep = cadcraft_geom::ccw_sweep(a.start, a.end);
                g.lines.push(arc_pts(a.center.xy(), a.radius, a.start, sweep));
            }
            K::LwPolyline(p) => {
                let w = p.const_width.max(p.vertices.iter().map(|v| v.start_width.max(v.end_width)).fold(0.0, f64::max));
                if w > 1e-12 {
                    // A wide polyline with its widths as drawn, tapers included (a tip from width
                    // 0 to 1 is a triangle, not a band).
                    let size = Bounds2::from_points(p.vertices.iter().map(|v| v.p));
                    let tol = ((size.width() + size.height()).max(w) / 2000.0).max(1e-9);
                    g.tris.extend(fill::wide_polyline(p, tol).into_iter().map(|q| q - base));
                } else {
                    g.lines.push(poly_pts(&p.vertices, p.closed));
                }
            }
            K::Solid(s) | K::Trace(s) => {
                let c: Vec<Vec2> = s.corners.iter().map(|p| p.xy() - base).collect();
                if let [a, b2, c2, d2] = c[..] {
                    g.tris.extend([a, b2, c2, b2, d2, c2]);
                }
            }
            K::Hatch(h) if h.solid => {
                // The loops its island detection fills (as the hatch itself is drawn), together
                // even-odd: an island stays a hole, a concave outline is filled as it is.
                let size = Bounds2::from_points(h.loops.iter().flat_map(|l| l.vertices.iter().map(|v| v.p)));
                let tol = ((size.width() + size.height()) / 2000.0).max(1e-9);
                let loops: Vec<Vec<Vec2>> = crate::hatch::fill_loops(h, tol).into_iter().map(|l| l.into_iter().map(|p| p - base).collect()).collect();
                g.tris.extend(fill::triangulate_evenodd(&loops));
            }
            _ => {}
        }
    }
    let ok = used(&g) <= MAX_POINTS && g.lines.iter().flatten().chain(&g.tris).all(|p| p.is_finite());
    (ok && !(g.lines.is_empty() && g.tris.is_empty())).then_some(g)
}

/// Geometry of a dimension in a drawing: its style, overrides and text font.
pub fn dimension_in(d: &Drawing, dm: &cadcraft_doc::Dimension) -> DimGeometry {
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
    let st = style.with_overrides(&dm.overrides);
    dim::dimension_geometry_with(dm, &style, d.header.f64("DIMSCALE", 1.0), &dim_text(d, &st))
}

/// `lw` / `lt` are the dimension's own lineweight and linetype (ByBlock in DIMLWD, DIMLWE,
/// DIMLTYPE, DIMLTEX1 and DIMLTEX2).
pub(crate) fn dimension(
    b: &mut Builder,
    ctx: &Ctx,
    e: &Entity,
    dm: &cadcraft_doc::Dimension,
    lw: f32,
    lt: Option<&cadcraft_doc::Linetype>,
    ltscale: f64,
) {
    let d = ctx.d;
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = dimension_in(d, dm);
    // DIMCLRD / DIMCLRE / DIMCLRT: ByBlock = the dimension's own colour.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let dim_rgb = ink(style.dim_line_color, layer_color, own);
    let ext_rgb = ink(style.ext_line_color, layer_color, own);
    let txt_rgb = ink(style.text_color, layer_color, own);
    let layer = d.layer(layer_name);
    let weight = |v: i16| -> f32 {
        if !b.opts.lineweights {
            return 0.0;
        }
        match Lineweight::from_dxf(v) {
            Lineweight::ByBlock => lw,
            Lineweight::Mm100(x) => f32::from(x) / 100.0,
            Lineweight::ByLayer => match layer.map(|l| l.lineweight) {
                Some(Lineweight::Mm100(x)) => f32::from(x) / 100.0,
                _ => default_lineweight(d),
            },
            Lineweight::Default => default_lineweight(d),
        }
    };
    let ltype = |name: &str| -> Option<cadcraft_doc::Linetype> {
        match name.trim().to_ascii_lowercase().as_str() {
            "" | "byblock" => lt.cloned(),
            "bylayer" => layer.and_then(|l| d.linetype(&l.linetype)).filter(|l| !l.pattern.is_empty()).cloned(),
            _ => d.linetype(name.trim()).filter(|l| !l.pattern.is_empty()).cloned(),
        }
    };
    let (dim_lw, ext_lw) = (weight(style.dim_lineweight), weight(style.ext_lineweight));
    let (dim_lt, ext1_lt, ext2_lt) = (ltype(&style.dim_linetype), ltype(&style.ext_linetype1), ltype(&style.ext_linetype2));
    let line_style = |i: usize| match g.line_roles.get(i).unwrap_or(&LineRole::Dim) {
        LineRole::Ext if g.ext2_lines.contains(&i) => (ext_rgb, ext_lw, ext2_lt.as_ref()),
        LineRole::Ext => (ext_rgb, ext_lw, ext1_lt.as_ref()),
        LineRole::Dim => (dim_rgb, dim_lw, dim_lt.as_ref()),
    };
    // The text's marks (arc length symbol, DIMINSPECT frame) come last, over the text background.
    for (i, l) in g.lines.iter().enumerate().filter(|(i, _)| !g.text_marks.contains(i)) {
        let (rgb, w, t) = line_style(i);
        stroke_with(b, ctx, rgb, w, t, ltscale, l);
    }
    for t in &g.fills {
        b.tris(ctx, dim_rgb, t);
    }
    dim_text_fill(b, ctx, e, &style, &g.text_fill);
    for (i, l) in g.lines.iter().enumerate().filter(|(i, _)| g.text_marks.contains(i)) {
        let (rgb, w, t) = line_style(i);
        stroke_with(b, ctx, rgb, w, t, ltscale, l);
    }
    if b.opts.text {
        let sh = cadcraft_fonts::Shaped { strokes: g.text.clone(), glyphs: g.text_glyphs.clone(), width: 0.0, text: g.text_runs.clone() };
        b.shaped(ctx, txt_rgb, lw, &sh);
    }
}

/// DIMTFILL: a dimension's text background (`corners` from [`DimGeometry::text_fill`]) hides
/// what is drawn before it, in the drawing's background color (1) or DIMTFILLCLR (2).
pub(crate) fn dim_text_fill(b: &mut Builder, ctx: &Ctx, e: &Entity, style: &cadcraft_doc::DimStyle, corners: &[Vec2]) {
    let [p0, p1, p2, p3] = corners else { return };
    if !b.opts.fill || style.text_fill == 0 {
        return;
    }
    // DIMTFILLCLR ByBlock = the dimension's own color, as DIMCLRT.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let n = b.list.prims.len();
    b.tris(ctx, ink(style.text_fill_color, layer_color, own), &[*p0, *p1, *p2, *p0, *p2, *p3]);
    if style.text_fill == 1
        && let Some(p) = b.list.prims.get_mut(n)
    {
        p.kind = Kind::Mask;
    }
}

/// A dimension block's text, or a line that is a segment of one of the text's marks in `g`
/// (as the `*D` block writer splits them): where the background goes.
pub(crate) fn starts_text_marks(be: &Entity, g: &DimGeometry) -> bool {
    match &be.kind {
        EntityKind::MText(_) | EntityKind::Text(_) => true,
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            g.text_marks
                .iter()
                .filter_map(|i| g.lines.get(*i))
                .any(|m| m.windows(2).any(|w| matches!(w, [p, q] if p.dist(a) < 1e-6 && q.dist(b) < 1e-6)))
        }
        _ => false,
    }
}

/// The background of a dimension block's MTEXT or TEXT: its box widened by `gap` on every side,
/// turned with the text (corners in order); `None` for other entities.
pub(crate) fn block_text_box(d: &Drawing, be: &Entity, gap: f64) -> Option<Vec<Vec2>> {
    let t = match &be.kind {
        EntityKind::MText(t) => t,
        EntityKind::Text(t) => return text_box(d, t, gap),
        _ => return None,
    };
    let (bounds, at, rot) = (layout_mtext_entity(d, &cadcraft_doc::MText { rotation: 0.0, ..t.clone() }).bounds, t.insert.xy(), t.rotation);
    if !(bounds.width().is_finite() && bounds.height().is_finite()) || bounds.width() <= 0.0 {
        return None;
    }
    let (lo, hi) = (bounds.min - Vec2::new(gap, gap), bounds.max + Vec2::new(gap, gap));
    let (x, y) = (Vec2::from_angle(rot), Vec2::from_angle(rot).perp());
    let turn = |p: Vec2| at + x * (p.x - at.x) + y * (p.y - at.y);
    Some([lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)].into_iter().map(turn).collect())
}

/// The background of a TEXT as placed: the box of its strokes and glyph outlines along the text's
/// direction (the baseline from the insertion to the alignment point for Aligned and Fit),
/// widened by `gap` on every side.
fn text_box(d: &Drawing, t: &cadcraft_doc::Text, gap: f64) -> Option<Vec<Vec2>> {
    let (sh, _) = place_text_entity(d, t, &t.value);
    let rot = match (t.halign, t.align_pt) {
        (cadcraft_doc::HAlign::Aligned | cadcraft_doc::HAlign::Fit, Some(p)) if p.xy().dist(t.insert.xy()) > 1e-12 => t.insert.xy().angle_to(p.xy()),
        _ => t.rotation,
    };
    let (x, y) = (Vec2::from_angle(rot), Vec2::from_angle(rot).perp());
    let pts: Vec<Vec2> = sh.strokes.iter().flatten().chain(sh.glyphs.iter().flatten().flatten()).copied().collect();
    let first = pts.first()?;
    let start = (first.dot(x), first.dot(y), first.dot(x), first.dot(y));
    let (s0, u0, s1, u1) = pts.iter().fold(start, |(s0, u0, s1, u1), p| (s0.min(p.dot(x)), u0.min(p.dot(y)), s1.max(p.dot(x)), u1.max(p.dot(y))));
    if !(s1 - s0 > 0.0 && (s0 + s1 + u0 + u1).is_finite()) {
        return None;
    }
    let at = |s: f64, u: f64| x * s + y * u;
    Some(vec![at(s0 - gap, u0 - gap), at(s1 + gap, u0 - gap), at(s1 + gap, u1 + gap), at(s0 - gap, u1 + gap)])
}

/// Most texts of one dimension block that get a DIMTFILL background, and most of its SOLIDs
/// checked for covering one.
pub(crate) const MAX_FILL_TEXTS: usize = 64;

pub(crate) const MAX_FILL_SOLIDS: usize = 256;

/// Whether `be` is a SOLID that covers `p` (a text background the block holds already, as the
/// `*D` block of DIMTFILL 2 does).
pub(crate) fn solid_covers(be: &Entity, p: Vec2) -> bool {
    let EntityKind::Solid(s) = &be.kind else { return false };
    // SOLID corners go 1, 2, 4, 3 around the outline.
    let [c1, c2, c3, c4] = s.corners.map(|c| c.xy());
    let ring = [c1, c2, c4, c3];
    let next = [c2, c4, c3, c1];
    let sides: Vec<f64> = ring.iter().zip(next.iter()).map(|(a, b)| (*b - *a).cross(p - *a)).filter(|c| c.abs() > 1e-12).collect();
    !sides.is_empty() && (sides.iter().all(|c| *c > 0.0) || sides.iter().all(|c| *c < 0.0))
}
