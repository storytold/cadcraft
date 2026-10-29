use super::display::MAX_TRANSPARENCY;
use super::{Color, Ctx, Drawing, Entity, Ink, Lineweight, ink};

/// An object's colour, lineweight (mm), linetype, linetype scale, visibility and the transparency
/// of the layer it is drawn on (for ByLayer, see [`transparency`]).
type Resolved = (Ink, f32, Option<cadcraft_doc::Linetype>, f64, bool, u8);

pub(crate) fn resolve(ctx: &Ctx, e: &Entity, plotting: bool) -> Resolved {
    resolve_common(ctx, &e.common, plotting)
}

/// [`resolve`] for an object's properties `c`.
pub(crate) fn resolve_common(ctx: &Ctx, c: &cadcraft_doc::Common, plotting: bool) -> Resolved {
    let d = ctx.d;
    // Layer "0" inside a block takes the insert's layer.
    let layer_name = if c.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { c.layer.as_str() };
    let layer = d.layer(layer_name);
    let visible =
        c.visible && layer.is_none_or(|l| l.visible() && (!plotting || l.plot)) && !ctx.frozen.iter().any(|f| f.eq_ignore_ascii_case(layer_name));
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let rgb = ink(c.color, layer_color, ctx.block_color);
    let lw = match c.lineweight {
        Lineweight::ByLayer => layer.map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        Lineweight::ByBlock => ctx.block_lw,
        x => x,
    };
    let lw_mm = match lw {
        Lineweight::Mm100(v) => f32::from(v) / 100.0,
        _ => default_lineweight(d),
    };
    let lt_name = match c.linetype.to_ascii_lowercase().as_str() {
        "bylayer" => layer.map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        "byblock" => ctx.block_ltype.clone(),
        _ => c.linetype.clone(),
    };
    let lt = d.linetype(&lt_name).filter(|l| !l.pattern.is_empty()).cloned();
    let scale = d.header.f64("LTSCALE", 1.0) * c.ltscale * ctx.lt_factor;
    (rgb, lw_mm, lt, scale, visible, layer.map_or(0, |l| l.transparency))
}

/// The transparency (percent, 0..=90) of an object whose own setting is `t`: the percentage, its
/// layer's `layer` when ByLayer (layer 0 inside a block: the block reference's layer, as
/// [`resolve_common`] finds it), the block reference's `block` when ByBlock (opaque outside
/// blocks).
pub(crate) fn transparency(t: cadcraft_doc::Transparency, layer: u8, block: u8) -> u8 {
    let t = match t {
        cadcraft_doc::Transparency::Percent(p) => p,
        cadcraft_doc::Transparency::ByBlock => block,
        cadcraft_doc::Transparency::ByLayer => layer,
    };
    t.min(MAX_TRANSPARENCY)
}

/// The width in mm that the "Default" lineweight stands for: `LWDEFAULT` (hundredths of a mm,
/// 0 to 211), 0.25 mm when it is unset or out of range.
pub(crate) fn default_lineweight(d: &Drawing) -> f32 {
    match d.header.i64("LWDEFAULT", 25) {
        v @ 0..=211 => v as f32 / 100.0,
        _ => 0.25,
    }
}
