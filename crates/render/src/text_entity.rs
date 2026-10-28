use super::{Bounds2, Builder, Color, Ctx, Drawing, Ink, Rgb, ink};

/// The text style of an entity (Standard defaults when missing).
pub(crate) fn text_style(d: &Drawing, name: &str) -> cadcraft_doc::TextStyle {
    d.text_style(name).cloned().unwrap_or_default()
}

/// Shape a TEXT / ATTRIB / ATTDEF with its style's font and generation flags.
pub fn place_text_entity(d: &Drawing, t: &cadcraft_doc::Text, value: &str) -> (cadcraft_fonts::Shaped, Bounds2) {
    let ts = text_style(d, &t.style);
    let hint = cadcraft_fonts::legacy_encoding::font_hint(&[&ts.font, &t.style]);
    let transcoded = cadcraft_fonts::legacy_encoding::transcode(value, hint);
    let font = cadcraft_fonts::TextFont::resolve(hint.unwrap_or(&ts.font));
    let (h, v) = align(t.halign, t.valign);
    let twf = if t.width_factor.is_finite() && t.width_factor > 0.0 { t.width_factor } else { 1.0 };
    let swf = if ts.width_factor.is_finite() && ts.width_factor > 0.0 { ts.width_factor } else { 1.0 };
    let p = cadcraft_fonts::TextParams {
        insert: t.insert.xy(),
        align_pt: t.align_pt.map(|p| p.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: twf * swf,
        oblique: t.oblique + ts.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    cadcraft_fonts::place(&font, &transcoded, &p)
}

/// Lay out an MTEXT with its style's font.
pub fn layout_mtext_entity(d: &Drawing, t: &cadcraft_doc::MText) -> cadcraft_fonts::MTextLayout {
    let ts = text_style(d, &t.style);
    let hint = cadcraft_fonts::legacy_encoding::font_hint(&[&ts.font, &t.style]);
    let params = cadcraft_fonts::MTextParams {
        insert: t.insert.xy(),
        height: t.height,
        width: t.width,
        attach: t.attach,
        rotation: t.rotation,
        line_spacing: t.line_spacing,
        line_spacing_exact: t.line_spacing_exact,
        font: cadcraft_fonts::TextFont::resolve(hint.unwrap_or(&ts.font)),
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
    let transcoded = transcode_mtext(&t.contents, hint);
    cadcraft_fonts::layout_mtext_with(&transcoded, &params)
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

/// Transcode an MTEXT string preserving formatting and control codes (e.g. `\c16711680;`, `\C1;`,
/// `\H2.5;`, `\P`, `\L`, `\l`, `\S...;`). Only text runs between markup codes are transcoded.
pub fn transcode_mtext(contents: &str, hint: Option<&str>) -> String {
    if !contents.contains('\\') && !contents.contains('{') && !contents.contains('}') {
        return cadcraft_fonts::legacy_encoding::transcode(contents, hint).into_owned();
    }

    let mut out = String::with_capacity(contents.len());
    let chars: Vec<char> = contents.chars().collect();
    let mut i = 0;
    let mut text_run = String::new();
    let mut current_hint: Option<String> = hint.map(|s| s.to_string());
    let mut hint_stack: Vec<Option<String>> = Vec::new();

    let flush_text = |run: &mut String, out: &mut String, h: Option<&str>| {
        if !run.is_empty() {
            out.push_str(&cadcraft_fonts::legacy_encoding::transcode(run, h));
            run.clear();
        }
    };

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let code = chars.get(i + 1).copied().unwrap_or(' ');
                match code {
                    // 1. Argument-less 2-character control codes and toggles (no ';' needed)
                    'P' | 'N' | '~' | '\\' | '{' | '}' | 'L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'X' | 'x' => {
                        flush_text(&mut text_run, &mut out, current_hint.as_deref());
                        out.push('\\');
                        out.push(code);
                        i += 2;
                    }
                    // Stacked fraction: \S <top> <sep> <bottom> ;
                    'S' => {
                        flush_text(&mut text_run, &mut out, current_hint.as_deref());
                        out.push('\\');
                        out.push('S');
                        let mut j = i + 2;
                        let mut stack_content = String::new();
                        while let Some(&ch) = chars.get(j) {
                            j += 1;
                            if ch == ';' {
                                break;
                            }
                            stack_content.push(ch);
                        }
                        if let Some(sep_idx) = stack_content.find(['^', '/', '#']) {
                            let sep = &stack_content[sep_idx..=sep_idx];
                            let top = &stack_content[..sep_idx];
                            let bottom = &stack_content[sep_idx + 1..];
                            let t_top = cadcraft_fonts::legacy_encoding::transcode(top, current_hint.as_deref());
                            let t_bottom = cadcraft_fonts::legacy_encoding::transcode(bottom, current_hint.as_deref());
                            out.push_str(&t_top);
                            out.push_str(sep);
                            out.push_str(&t_bottom);
                        } else {
                            out.push_str(&cadcraft_fonts::legacy_encoding::transcode(&stack_content, current_hint.as_deref()));
                        }
                        out.push(';');
                        i = j;
                    }
                    // Unterminated escapes: \U+XXXX (4 hex digits) and \M+NXXXX (5 hex digits)
                    'U' | 'M' if chars.get(i + 2) == Some(&'+') => {
                        flush_text(&mut text_run, &mut out, current_hint.as_deref());
                        let digits = if code == 'U' { 4 } else { 5 };
                        let mut j = i + 3;
                        while j < i + 3 + digits && chars.get(j).is_some_and(|c| c.is_ascii_hexdigit()) {
                            j += 1;
                        }
                        for &ch in &chars[i..j] {
                            out.push(ch);
                        }
                        i = j;
                    }
                    // Codes with arguments terminated by ';' (\c, \C, \H, \W, \Q, \A, \f, \F, \T, \p, etc.)
                    _ => {
                        flush_text(&mut text_run, &mut out, current_hint.as_deref());
                        let start = i;
                        let mut j = i + 2;
                        while let Some(&ch) = chars.get(j) {
                            j += 1;
                            if ch == ';' {
                                break;
                            }
                        }
                        // Check if this is an in-line font switch \f<font_name>...;
                        if (code == 'f' || code == 'F') && j > start + 3 {
                            let font_spec: String = chars[start + 2..j.saturating_sub(1)].iter().collect();
                            let font_name = font_spec.split('|').next().unwrap_or("").trim();
                            if cadcraft_fonts::legacy_encoding::font_hint(&[font_name]).is_some() {
                                current_hint = Some(font_name.to_string());
                            } else if !font_name.is_empty() {
                                current_hint = None;
                            }
                        }
                        for k in start..j {
                            if let Some(&ch) = chars.get(k) {
                                out.push(ch);
                            }
                        }
                        i = j;
                    }
                }
            }
            '{' => {
                flush_text(&mut text_run, &mut out, current_hint.as_deref());
                if hint_stack.len() < 256 {
                    hint_stack.push(current_hint.clone());
                }
                out.push('{');
                i += 1;
            }
            '}' => {
                flush_text(&mut text_run, &mut out, current_hint.as_deref());
                if let Some(h) = hint_stack.pop() {
                    current_hint = h;
                }
                out.push('}');
                i += 1;
            }
            _ => {
                text_run.push(c);
                i += 1;
            }
        }
    }
    flush_text(&mut text_run, &mut out, current_hint.as_deref());
    out
}
