use super::{Bounds2, Builder, Color, Ctx, Drawing, Ink, Kind, Rgb, Vec2, ink};

/// The text style of an entity (Standard defaults when missing).
pub(crate) fn text_style(d: &Drawing, name: &str) -> cadcraft_doc::TextStyle {
    d.text_style(name).cloned().unwrap_or_default()
}

/// Shape a TEXT / ATTRIB / ATTDEF with its style's font and generation flags.
pub fn place_text_entity(d: &Drawing, t: &cadcraft_doc::Text, value: &str) -> (cadcraft_fonts::Shaped, Bounds2) {
    let ts = text_style(d, &t.style);
    let font = cadcraft_fonts::TextFont::resolve(&ts.font);
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
        backwards: t.backwards,
        upside_down: t.upside_down,
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
        font: cadcraft_fonts::TextFont::resolve(&ts.font),
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

/// The box an MTEXT's background mask fills: the text's extents in its own (rotated) frame,
/// across the defined width when it has one, out by the border offset factor (split between
/// the sides, in text heights).
pub fn mask_box(t: &cadcraft_doc::MText, l: &cadcraft_fonts::MTextLayout) -> Option<[Vec2; 4]> {
    let mask = t.background.as_ref()?;
    let (o, rot) = (t.insert.xy(), if t.rotation.is_finite() { t.rotation } else { 0.0 });
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for q in l.strokes.iter().flatten() {
        let p = (*q - o).rotate(-rot);
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return None;
    }
    if t.width > 0.0 && t.width.is_finite() && t.columns.is_none() {
        let w = t.width;
        (x0, x1) = match (t.attach.clamp(1, 9) - 1) % 3 {
            0 => (0.0, w),
            1 => (-w / 2.0, w / 2.0),
            _ => (-w, 0.0),
        };
    }
    let scale = if mask.scale.is_finite() { mask.scale.clamp(1.0, 5.0) } else { cadcraft_doc::MASK_SCALE };
    let m = (scale - 1.0) * t.height.abs() / 2.0;
    Some([(x0 - m, y0 - m), (x1 + m, y0 - m), (x1 + m, y1 + m), (x0 - m, y1 + m)].map(|(x, y)| o + Vec2::new(x, y).rotate(rot)))
}

/// `layer` is the MTEXT's (or its multileader's), for a ByLayer mask color.
pub(crate) fn mtext(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::MText, rgb: Ink, lw: f32, layer: &str) {
    let l = layout_mtext_entity(ctx.d, t);
    // The background mask hides what was drawn before it (draw order), under the text.
    if let (Some(m), Some(q), true) = (&t.background, mask_box(t, &l), b.opts.fill) {
        let layer = if layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { layer };
        let fill = if m.use_background { rgb } else { ink(m.color, ctx.layer_color(layer).unwrap_or(Color::Index(7)), ctx.block_color) };
        let n = b.list.prims.len();
        b.tris(ctx, fill, &[q[0], q[1], q[2], q[0], q[2], q[3]]);
        if m.use_background
            && let Some(p) = b.list.prims.get_mut(n)
        {
            p.kind = Kind::Mask;
        }
    }
    for p in &l.pieces {
        b.shaped(ctx, mtext_color(p.color, rgb), lw, &p.shaped);
    }
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
