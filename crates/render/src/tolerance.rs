//! TOLERANCE: feature control frames. The layout lives in `cadcraft_doc::gdt`; here the text is
//! measured with the dimension style's font and everything is drawn: the boxes with DIMCLRD,
//! symbols and text with DIMCLRT (ByBlock = the entity's own colour, as for dimensions).

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, GdtLayout, GdtPrim, Tolerance};
use cadcraft_geom::{Arc as GArc, Circle as GCircle, Segment, Vec2};

use crate::{Builder, Ctx, Ink, ink};

/// The frame laid out with the font of its dimension style's text style.
pub fn tolerance_in(d: &Drawing, t: &Tolerance) -> GdtLayout {
    let st = d.dim_style(&t.style).cloned().unwrap_or_default();
    let dt = crate::dim_text(d, &st);
    t.layout(d, &|s, h| cadcraft_fonts::text_width(&dt.font, s, h, dt.width_factor))
}

/// Points of a frame primitive, tessellated at `tol` (a fill's triangle as is).
pub fn gdt_points(p: &GdtPrim, tol: f64) -> Vec<Vec2> {
    let mut pts = Vec::new();
    match p {
        GdtPrim::Line(v) => pts.clone_from(v),
        GdtPrim::Circle { center, radius } => GCircle::new(*center, *radius).tessellate(tol, &mut pts),
        GdtPrim::Arc { center, radius, start, end } => {
            Segment::Arc { arc: GArc::new(*center, *radius, *start, *end), ccw: true }.tessellate(tol, &mut pts)
        }
        GdtPrim::Fill(t) => pts.extend_from_slice(t),
    }
    pts
}

pub(crate) fn draw(b: &mut Builder, ctx: &Ctx, e: &Entity, t: &Tolerance, lw: f32, tol: f64) {
    let d = ctx.d;
    let st = d.dim_style(&t.style).cloned().unwrap_or_default();
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let frame_rgb: Ink = ink(st.dim_line_color, layer_color, own);
    let text_rgb: Ink = ink(st.text_color, layer_color, own);
    let g = tolerance_in(d, t);
    for p in &g.frame {
        b.polyline(ctx, frame_rgb, lw, &gdt_points(p, tol));
    }
    if !b.opts.text {
        return;
    }
    for p in &g.symbols {
        let pts = gdt_points(p, tol);
        if matches!(p, GdtPrim::Fill(_)) {
            b.tris(ctx, text_rgb, &pts);
        } else {
            b.polyline(ctx, text_rgb, lw, &pts);
        }
    }
    let dt = crate::dim_text(d, &st);
    for tx in &g.texts {
        let p = cadcraft_fonts::TextParams {
            rotation: tx.rotation,
            width_factor: dt.width_factor,
            oblique: dt.oblique,
            ..cadcraft_fonts::TextParams::new(tx.pos, tx.height)
        };
        let transcoded = cadcraft_fonts::vietnamese::transcode_vietnamese(&tx.value, None);
        let (sh, _) = cadcraft_fonts::place(&dt.font, &transcoded, &p);
        b.shaped(ctx, text_rgb, lw, &sh);
    }
}
