//! Multilines: each element in its style colour and linetype (ByLayer and ByBlock elements
//! follow the multiline's own), caps and joints in the multiline's properties, and the fill.

use super::{Builder, Ctx, Ink, fill, ink, linetype, resolve_common};
use cadcraft_color::Color;
use cadcraft_doc::{Entity, MLine};
use cadcraft_geom::Vec2;

pub(super) fn draw(b: &mut Builder, ctx: &Ctx, e: &Entity, m: &MLine, rgb: Ink, lw: f32, tol: f64) {
    let g = m.geometry();
    if let Some(fc) = m.def.fill
        && b.opts.fill
        && !g.fill.is_empty()
    {
        let c = match fc {
            Color::ByLayer | Color::ByBlock => rgb,
            c => ink(c, Color::Index(7), ctx.block_color),
        };
        b.tris(ctx, c, &fill::triangulate_evenodd(&g.fill));
    }
    let min = b.opts.min_dash / ctx.xf.scale_factor().max(1e-12);
    // One look per element: colour, lineweight, linetype and its scale.
    let looks: Vec<_> = m
        .def
        .elements
        .iter()
        .map(|el| {
            let mut c = e.common.clone();
            if !matches!(el.color, Color::ByLayer | Color::ByBlock) {
                c.color = el.color;
            }
            let lt = el.linetype.trim();
            if !lt.is_empty() && !lt.eq_ignore_ascii_case("bylayer") && !lt.eq_ignore_ascii_case("byblock") {
                c.linetype = lt.to_string();
            }
            let (rgb, w, lt, scale, _, _) = resolve_common(ctx, &c, b.plotting);
            let lt = match b.styles.and_then(|s| s.linetype(rgb, b.named)) {
                Some(over) => over,
                None => lt,
            };
            (rgb, if b.opts.lineweights { w } else { 0.0 }, lt, scale)
        })
        .collect();
    for (i, a, z) in &g.lines {
        let Some((rgb, w, lt, scale)) = looks.get(*i) else { continue };
        stroke(b, ctx, *rgb, *w, lt.as_ref(), *scale, min, &[*a, *z]);
    }
    let own = resolve_common(ctx, &e.common, b.plotting);
    let lt = match b.styles.and_then(|s| s.linetype(rgb, b.named)) {
        Some(over) => over,
        None => own.2,
    };
    for s in &g.caps {
        let mut pts = Vec::new();
        s.tessellate(tol, &mut pts);
        stroke(b, ctx, rgb, lw, lt.as_ref(), own.3, min, &pts);
    }
}

#[allow(clippy::too_many_arguments)]
fn stroke(b: &mut Builder, ctx: &Ctx, rgb: Ink, lw: f32, lt: Option<&cadcraft_doc::Linetype>, scale: f64, min: f64, pts: &[Vec2]) {
    match lt.filter(|_| b.opts.linetypes) {
        Some(lt) => {
            for dash in linetype::apply(pts, lt, scale, min) {
                match dash.as_slice() {
                    [p] => b.point(ctx, rgb, *p),
                    _ => b.polyline(ctx, rgb, lw, &dash),
                }
            }
        }
        None => b.polyline(ctx, rgb, lw, pts),
    }
}
