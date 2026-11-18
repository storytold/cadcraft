use super::{Bounds2, Builder, Color, Ctx, Drawing, Ink, Rgb, ink};

/// The text style of an entity (Standard defaults when missing).
pub(crate) fn text_style(d: &Drawing, name: &str) -> cadcraft_doc::TextStyle {
    d.text_style(name).cloned().unwrap_or_default()
}

/// The font a text style draws with: its font file (TrueType, or an SHX font found by name),
/// its big font and its vertical flag.
pub fn style_font(ts: &cadcraft_doc::TextStyle) -> cadcraft_fonts::TextFont {
    cadcraft_fonts::TextFont::resolve_style(&ts.font, &ts.big_font, ts.vertical)
}

/// Shape a TEXT / ATTRIB / ATTDEF with its style's font and generation flags.
pub fn place_text_entity(d: &Drawing, t: &cadcraft_doc::Text, value: &str) -> (cadcraft_fonts::Shaped, Bounds2) {
    let ts = text_style(d, &t.style);
    let font = style_font(&ts);
    let (h, v) = align(t.halign, t.valign);
    let p = cadcraft_fonts::TextParams {
        insert: t.insert.xy(),
        align_pt: t.align_pt.map(|p| p.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: t.width_factor,
        oblique: t.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    cadcraft_fonts::place(&font, value, &p)
}

/// Lay out an MTEXT with its style's font.
pub fn layout_mtext_entity(d: &Drawing, t: &cadcraft_doc::MText) -> cadcraft_fonts::MTextLayout {
    let ts = text_style(d, &t.style);
    let params = cadcraft_fonts::MTextParams {
        insert: t.insert.xy(),
        height: t.height,
        width: t.width,
        attach: t.attach,
        rotation: t.rotation,
        line_spacing: t.line_spacing,
        line_spacing_exact: t.line_spacing_exact,
        font: style_font(&ts),
        width_factor: ts.width_factor,
        oblique: ts.oblique,
        columns: t.columns.as_ref().map(|c| cadcraft_fonts::MTextColumnSpec {
            dynamic: c.kind == cadcraft_doc::MTextColumnType::Dynamic,
            count: usize::from(c.count.max(1)),
            gutter: c.gutter,
            height: c.height,
            heights: if c.kind == cadcraft_doc::MTextColumnType::Dynamic && !c.auto_height { c.heights.clone() } else { Vec::new() },
            reversed: c.flow_reversed,
        }),
    };
    cadcraft_fonts::layout_mtext_with(&t.contents, &params)
}

fn mtext_color(c: Option<cadcraft_fonts::MTextColor>, rgb: Ink) -> Ink {
    match c {
        Some(cadcraft_fonts::MTextColor::Aci(i)) => {
            u8::try_from(i).ok().filter(|i| *i > 0).map(|i| ink(Color::Index(i), Color::Index(7), Color::Index(7))).unwrap_or(rgb)
        }
        Some(cadcraft_fonts::MTextColor::Rgb(v)) => {
            Ink { rgb: Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8), aci7: false, aci: 0 }
        }
        None => rgb,
    }
}

pub(crate) fn mtext(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::MText, rgb: Ink, lw: f32) {
    let l = layout_mtext_entity(ctx.d, t);
    if b.opts.fill
        && let Some(bg) = &t.background
        && (bg.mask & 0x03) != 0
        && let Some([p0, p1, p2, p3]) = mtext_bg_quad(ctx.d, t, &l, bg.scale)
    {
        let n = b.list.prims.len();
        let use_mask = (bg.mask & 2) != 0 || bg.color.is_none();
        let fill_ink = match bg.color {
            Some(c) if !use_mask => ink(c, Color::Index(7), Color::Index(7)),
            _ => Ink { rgb: Rgb(255, 255, 255), aci7: false, aci: 0 },
        };
        b.tris(ctx, fill_ink, &[p0, p1, p2, p0, p2, p3]);
        if use_mask && let Some(p) = b.list.prims.get_mut(n) {
            p.kind = super::Kind::Mask;
        }
    }
    for p in &l.pieces {
        b.shaped(ctx, mtext_color(p.color, rgb), lw, &p.shaped);
    }
}

fn mtext_bg_quad(d: &Drawing, t: &cadcraft_doc::MText, l: &cadcraft_fonts::MTextLayout, scale: f64) -> Option<[cadcraft_geom::Vec2; 4]> {
    let bounds = if t.rotation == 0.0 {
        l.bounds
    } else {
        layout_mtext_entity(d, &cadcraft_doc::MText { rotation: 0.0, background: None, ..t.clone() }).bounds
    };
    if !(bounds.width().is_finite() && bounds.height().is_finite()) || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return None;
    }
    let gap = (scale - 1.0).clamp(0.0, 4.0) * t.height.max(0.0);
    let (lo, hi) = (bounds.min - cadcraft_geom::Vec2::new(gap, gap), bounds.max + cadcraft_geom::Vec2::new(gap, gap));
    let at = t.insert.xy();
    let (x, y) = (cadcraft_geom::Vec2::from_angle(t.rotation), cadcraft_geom::Vec2::from_angle(t.rotation).perp());
    let turn = |p: cadcraft_geom::Vec2| at + x * (p.x - at.x) + y * (p.y - at.y);
    Some([turn(lo), turn(cadcraft_geom::Vec2::new(hi.x, lo.y)), turn(hi), turn(cadcraft_geom::Vec2::new(lo.x, hi.y))])
}

/// A TEXT's justification as the fonts' alignment (used where text is placed outside a build).
pub fn text_align(h: cadcraft_doc::HAlign, v: cadcraft_doc::VAlign) -> (cadcraft_fonts::Align, cadcraft_fonts::VAlign) {
    align(h, v)
}

fn align(h: cadcraft_doc::HAlign, v: cadcraft_doc::VAlign) -> (cadcraft_fonts::Align, cadcraft_fonts::VAlign) {
    use cadcraft_fonts::{Align as A, VAlign as V};
    let ha = match h {
        cadcraft_doc::HAlign::Left => A::Left,
        cadcraft_doc::HAlign::Center => A::Center,
        cadcraft_doc::HAlign::Right => A::Right,
        cadcraft_doc::HAlign::Aligned => A::Aligned,
        cadcraft_doc::HAlign::Middle => A::Middle,
        cadcraft_doc::HAlign::Fit => A::Fit,
    };
    let va = match v {
        cadcraft_doc::VAlign::Baseline => V::Baseline,
        cadcraft_doc::VAlign::Bottom => V::Bottom,
        cadcraft_doc::VAlign::Middle => V::Middle,
        cadcraft_doc::VAlign::Top => V::Top,
    };
    (ha, va)
}
