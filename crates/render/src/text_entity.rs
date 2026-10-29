use super::{Bounds2, Builder, Color, Ctx, Drawing, Ink, Rgb, ink};

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

pub(crate) fn mtext(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::MText, rgb: Ink, lw: f32) {
    let shown;
    let t = if cadcraft_doc::fields::has_fields(&t.contents) {
        if field_display(b, ctx) {
            // The fields' glyphs, found by colouring them with a marker colour.
            let marked =
                cadcraft_doc::fields::display_with(&t.contents, |v| format!("{{\\c{FIELD_MARK};{}}}", cadcraft_doc::fields::mtext_escape(v)));
            let probe = cadcraft_doc::MText { contents: marked.into_owned(), ..t.clone() };
            let l = layout_mtext_entity(ctx.d, &probe);
            for p in l.pieces.iter().filter(|p| p.color == Some(cadcraft_fonts::MTextColor::Rgb(FIELD_MARK))) {
                field_background(b, ctx, &p.shaped.outlines(), t.insert.xy(), t.rotation, t.height);
            }
        }
        shown = cadcraft_doc::MText { contents: cadcraft_doc::fields::display_mtext(&t.contents).into_owned(), ..t.clone() };
        &shown
    } else {
        t
    };
    let l = layout_mtext_entity(ctx.d, t);
    for p in &l.pieces {
        b.shaped(ctx, mtext_color(p.color, rgb), lw, &p.shaped);
    }
}

/// An inline colour that marks field values while their background is measured.
const FIELD_MARK: u32 = 0x0F_1E2D;
/// FIELDDISPLAY's grey behind field values.
const FIELD_GREY: Ink = Ink { rgb: Rgb(128, 128, 128), aci7: false, aci: 8 };

/// FIELDDISPLAY: field values get a grey background on screen (never on paper).
fn field_display(b: &Builder, ctx: &Ctx) -> bool {
    !b.plotting && b.opts.fill && ctx.d.header.i64("FIELDDISPLAY", 1) != 0
}

/// A grey box behind `pts` (geometry of text turned by `rotation` about `origin`), padded by a
/// sixth of the text height.
fn field_background(b: &mut Builder, ctx: &Ctx, strokes: &[Vec<Vec2>], origin: Vec2, rotation: f64, height: f64) {
    let rot = if rotation.is_finite() { rotation } else { 0.0 };
    let local = Bounds2::from_points(strokes.iter().flatten().map(|p| (*p - origin).rotate(-rot)));
    if local.is_empty() {
        return;
    }
    let pad = if height.is_finite() && height > 0.0 { height / 6.0 } else { 0.0 };
    let (x0, y0, x1, y1) = (local.min.x - pad, local.min.y - pad, local.max.x + pad, local.max.y + pad);
    let c = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)].map(|q| origin + q.rotate(rot));
    let [p0, p1, p2, p3] = c;
    b.tris(ctx, FIELD_GREY, &[p0, p1, p2, p0, p2, p3]);
}

/// FIELDDISPLAY for a TEXT or attribute with fields: a grey box behind the whole text.
pub(crate) fn field_text_background(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::Text, shown: &str) {
    if !field_display(b, ctx) || !cadcraft_doc::fields::has_fields(&t.value) {
        return;
    }
    // Shaped unturned, then turned about the justification point with the text.
    let flat = cadcraft_doc::Text { rotation: 0.0, ..t.clone() };
    let (sh, _) = place_text_entity(ctx.d, &flat, shown);
    field_background(b, ctx, &sh.outlines(), t.justify_point(), t.rotation, t.height);
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
