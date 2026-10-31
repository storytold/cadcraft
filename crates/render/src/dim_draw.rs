use super::display::Kind;
use super::draw::stroke_with;
use super::props::default_lineweight;
use super::text_entity::{layout_mtext_entity, place_text_entity, text_style};
use super::{Builder, Color, Ctx, DimGeometry, DimText, Drawing, Entity, EntityKind, LineRole, Lineweight, Vec2, dim, ink};

/// The font settings of a dimension style's text style.
pub fn dim_text(d: &Drawing, st: &cadcraft_doc::DimStyle) -> DimText {
    let ts = text_style(d, &st.text_style);
    DimText { font: cadcraft_fonts::TextFont::resolve(&ts.font), fixed_height: ts.height, width_factor: ts.width_factor, oblique: ts.oblique }
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
