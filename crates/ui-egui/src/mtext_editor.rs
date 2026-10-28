//! The in-place MTEXT editor: while MTEXT asks for its text, a text box opens on the canvas at
//! the text's location and width with a formatting toolbar (font, height, bold, italic,
//! underline, overline, colour, alignment, lists, tabs) and a ruler for the paragraph's indents
//! and tab stops. The text and its formats live in the engine's
//! [`cadcraft_engine::mtext_edit::Formatted`]; this file only draws and forwards edits.

use std::sync::Arc;

use cadcraft_engine::mtext_edit::{CharFmt, Flag, Formatted, ListStyle, MTextColor, ParaAlign, TabKind};
use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, TextFormat, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// Smallest and largest size of the text in the box, in points: the box stays readable at any zoom.
const MIN_PX: f32 = 13.0;
const MAX_PX: f32 = 36.0;
/// Families the font list offers besides the one set in the text.
const FAMILIES: [&str; 6] = ["Arial", "Times New Roman", "Courier New", "Verdana", "Georgia", "Calibri"];
/// The standard colours (ACI 1..=7) of the colour menu.
const SWATCHES: [u16; 7] = [1, 2, 3, 4, 5, 6, 7];
/// Ruler snap, in text heights.
const SNAP: f64 = 0.25;

/// Keep the actual diameter character in editable text even without optional fonts.
/// This original circle-and-slash rasterizer is a last fallback, after installed fonts.
pub(crate) fn install_diameter_glyph(ctx: &egui::Context) {
    ctx.add_glyph_rasterizer(egui::GlyphRasterizer::new("cadcraft::diameter", |request| {
        if request.cluster != "⌀" || !request.font_size_px.is_finite() || !(1.0..=1024.0).contains(&request.font_size_px) {
            return None;
        }
        let size = request.font_size_px;
        let side = (size * 0.8).ceil() as usize + 2;
        let center = vec2(1.0 + size * 0.4, 1.0 + size * 0.4);
        let radius = size * 0.3;
        let half_stroke = (size * 0.05).max(1.0) * 0.5;
        let start = center + vec2(-radius, radius);
        let segment = vec2(2.0 * radius, -2.0 * radius);
        let mut pixels = Vec::with_capacity(side * side);
        for y in 0..side {
            for x in 0..side {
                let point = vec2(x as f32 + 0.5, y as f32 + 0.5);
                let ring_distance = ((point - center).length() - radius).abs();
                let along = ((point - start).dot(segment) / segment.length_sq()).clamp(0.0, 1.0);
                let slash_distance = (point - (start + segment * along)).length();
                let alpha = ((half_stroke + 0.5 - ring_distance.min(slash_distance)).clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels.push(Color32::from_white_alpha(alpha));
            }
        }
        Some(egui::RasterizedGlyph {
            bitmap: egui::GlyphBitmap {
                image: egui::ColorImage::new([side, side], pixels),
                offset_px: vec2(0.0, -size * 0.8 - 1.0),
                is_color: false,
            },
            advance_px: size * 0.85 + 2.0,
        })
    }));
}

#[derive(Clone)]
struct State {
    fmt: Formatted,
    /// Format of the next typed character when nothing is selected.
    typing: CharFmt,
    /// Caret the `typing` format was set for.
    typing_at: usize,
    /// Height of the toolbar and ruler above the text box (last frame).
    top: f32,
    /// Selection (characters) of the last frame, for toolbar actions of this one.
    sel: (usize, usize),
    /// A caret position to set on the text box in the next frame (after a toolbar insertion).
    caret: Option<usize>,
}

fn state_id() -> egui::Id {
    egui::Id::unique("mtext_editor_state")
}

fn edit_id() -> egui::Id {
    egui::Id::unique("mtext_editor_text")
}

/// The ACI index or true colour as a screen colour.
fn color32(c: MTextColor) -> Color32 {
    let rgb = match c {
        MTextColor::Aci(v) => cadcraft_color::aci_rgb(u8::try_from(v).unwrap_or(7)),
        MTextColor::Rgb(v) => cadcraft_color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
    };
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// Show the editor while MTEXT collects its text; clears its state when it isn't running.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let mtext = app.session.editing_mtext();
    let asking = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point);
    let (Some(mt), true) = (mtext, asking) else {
        ctx.data_mut(|d| d.remove::<State>(state_id()));
        return;
    };
    let t = Tokens::get();
    let (rect, scale) = app.canvas.xf.map_or((ctx.content_rect(), 20.0), |x| (x.rect, x.scale.max(1e-6)));
    let mut st = ctx.data_mut(|d| d.get_temp::<State>(state_id())).unwrap_or_else(|| {
        let mut fmt = Formatted::default();
        if let Some(f) = app.session.doc().ok().and_then(|d| d.text_style(&mt.style).map(|s| s.font.clone())) {
            // A family name, not a font file (`txt.shx`, `arial.ttf`).
            if !f.is_empty() && !f.contains('.') {
                fmt.base_font = f;
            }
        }
        State { fmt, typing: CharFmt::default(), typing_at: 0, top: 70.0, sel: (0, 0), caret: None }
    });
    let base_h = if mt.height > 0.0 { mt.height } else { 0.2 };
    let px = ((base_h * scale) as f32).clamp(MIN_PX, MAX_PX);
    let width = if mt.width > 0.0 { ((mt.width * scale) as f32).clamp(280.0, rect.width().max(280.0) * 0.9) } else { 380.0 };
    // The box's top-left corner on screen (rotation is ignored while editing).
    let code = usize::from(mt.attach.clamp(1, 9) - 1);
    let (col, row) = ((code % 3) as f64, (code / 3) as f64);
    let ww = f64::from(width) / scale;
    let corner = cadcraft_geom::Vec2::new(mt.insert.x - col * ww / 2.0, mt.insert.y + row * base_h * 2.0);
    let at = app.canvas.xf.map_or(rect.center(), |x| x.to_screen(corner));
    let max_x = (rect.right() - width - 8.0).max(rect.left());
    let pos = pos2(at.x.clamp(rect.left(), max_x), at.y.clamp(rect.top() + st.top, (rect.bottom() - 120.0).max(rect.top() + st.top)));

    let mut done = Done::No;
    let top_start = pos.y - st.top;
    egui::Area::new(egui::Id::unique("mtext_editor_area")).order(egui::Order::Foreground).fixed_pos(pos2(pos.x, top_start)).show(ctx, |ui| {
        egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.accent)).inner_margin(egui::Margin::same(0)).show(ui, |ui| {
            ui.set_width(width);
            let y0 = ui.cursor().top();
            toolbar(ui, &mut st, base_h);
            ruler(ui, &mut st, px, width, base_h);
            st.top = ui.cursor().top() - y0;
            text_box(ui, &mut st, base_h, px, width, &t);
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                if ui.button(crate::tl!("OK")).clicked() {
                    done = Done::Ok;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    done = Done::Cancel;
                }
                let finish = if ui.ctx().os().is_mac() { crate::tl!("⌘↩ to finish") } else { crate::tl!("Ctrl+Enter to finish") };
                ui.label(RichText::new(finish).small().color(t.text_dim));
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        done = Done::Cancel;
    } else if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
        done = Done::Ok;
    }
    match done {
        Done::No => {
            ctx.data_mut(|d| d.insert_temp(state_id(), st));
        }
        Done::Ok => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            // The editor holds the whole contents: one input, then Enter ends MTEXT's text entry.
            if !st.fmt.is_empty() {
                let _ = app.session.input(cadcraft_engine::Input::Text(st.fmt.to_contents()));
            }
            if app.session.running.as_ref().is_some_and(|r| r.id == "mtext") {
                let _ = app.session.input(cadcraft_engine::Input::Enter);
            }
        }
        Done::Cancel => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            app.session.cancel();
        }
    }
}

#[derive(PartialEq)]
enum Done {
    No,
    Ok,
    Cancel,
}

/// Change the selected characters' format, or the next typed ones' when nothing is selected.
fn change(st: &mut State, f: impl Fn(&mut CharFmt)) {
    let (a, b) = st.sel;
    if a == b {
        f(&mut st.typing);
    } else {
        st.fmt.edit(a.min(b)..a.max(b), f);
    }
}

fn current(st: &State) -> CharFmt {
    let (a, b) = st.sel;
    if a == b { st.typing.clone() } else { st.fmt.fmt_at(a.min(b)) }
}

fn toggle(st: &mut State, flag: Flag) {
    let (a, b) = st.sel;
    if a == b {
        let on = !flag_of(&st.typing, flag);
        change(st, |c| set_flag(c, flag, on));
    } else {
        st.fmt.toggle(a.min(b)..a.max(b), flag);
    }
}

fn flag_of(c: &CharFmt, f: Flag) -> bool {
    match f {
        Flag::Bold => c.bold,
        Flag::Italic => c.italic,
        Flag::Underline => c.underline,
        Flag::Overline => c.overline,
    }
}

fn set_flag(c: &mut CharFmt, f: Flag, on: bool) {
    match f {
        Flag::Bold => c.bold = on,
        Flag::Italic => c.italic = on,
        Flag::Underline => c.underline = on,
        Flag::Overline => c.overline = on,
    }
}

/// MTEXT formatting toolbar for the current selection: font, height, bold/italic/underline/overline, colour, alignment, lists, tab, degree/plus-minus and Diameter. Font, bold/italic/underline/overline, ByLayer/palette colour, alignment, list, tab and special-character changes refocus the editor; height and custom RGB colour changes do not.
fn toolbar(ui: &mut egui::Ui, st: &mut State, base_h: f64) {
    let cur = current(st);
    let range = st.sel.0.min(st.sel.1)..st.sel.0.max(st.sel.1);
    let mut refocus = false;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(4.0);
        // Font
        let shown = cur.font.clone().unwrap_or_else(|| st.fmt.base_font.clone());
        egui::ComboBox::from_id_salt("mtext_font")
            .width(120.0)
            .selected_text(shown.clone())
            .show_ui(ui, |ui| {
                for fam in FAMILIES.iter().map(ToString::to_string).chain(cur.font.clone().filter(|f| !FAMILIES.contains(&f.as_str()))) {
                    if ui.selectable_label(shown == fam, &fam).clicked() {
                        change(st, |c| c.font = Some(fam.clone()));
                        refocus = true;
                    }
                }
            })
            .response
            .accessible_name(crate::tl!("Font"));
        // Height
        let mut h = cur.height.unwrap_or(base_h);
        if height_field(ui, &mut h, base_h) {
            let none = (h - base_h).abs() < 1e-9;
            change(st, |c| c.height = (!none).then_some(h));
        }
        ui.separator();
        for (flag, label, tip) in [
            (Flag::Bold, RichText::new("B").strong(), crate::tl!("Bold")),
            (Flag::Italic, RichText::new("I").italics(), crate::tl!("Italic")),
            (Flag::Underline, RichText::new("U").underline(), crate::tl!("Underline")),
            (Flag::Overline, RichText::new("O").strikethrough(), crate::tl!("Overline")),
        ] {
            if ui.selectable_label(flag_of(&cur, flag), label).on_hover_text(tip).clicked() {
                toggle(st, flag);
                refocus = true;
            }
        }
        // Colour
        let swatch = cur.color.map_or(ui.visuals().text_color(), color32);
        ui.menu_button(RichText::new("A").color(swatch).strong(), |ui| {
            if ui.button(crate::tl!("ByLayer")).clicked() {
                change(st, |c| c.color = None);
                refocus = true;
                ui.close();
            }
            ui.horizontal(|ui| {
                for v in SWATCHES {
                    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    ui.painter().rect_filled(r, 2.0, color32(MTextColor::Aci(v)));
                    if resp.clicked() {
                        change(st, |c| c.color = Some(MTextColor::Aci(v)));
                        refocus = true;
                        ui.close();
                    }
                }
            });
            let mut rgb = cur.color.map_or([255, 255, 255], |c| {
                let k = color32(c);
                [k.r(), k.g(), k.b()]
            });
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                let v = (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]);
                change(st, |c| c.color = Some(MTextColor::Rgb(v)));
            }
        })
        .response
        .accessible_name(crate::tl!("Text color"))
        .on_hover_text(crate::tl!("Text color"));
        ui.separator();
        // Paragraph alignment
        let para = st.fmt.para(st.fmt.paragraph_of(range.start));
        for (al, tip) in [
            (ParaAlign::Left, crate::tl!("Left")),
            (ParaAlign::Center, crate::tl!("Center")),
            (ParaAlign::Right, crate::tl!("Right")),
            (ParaAlign::Justify, crate::tl!("Justify")),
        ] {
            let on = para.align == al;
            if formatting_button(ui, FormattingIcon::Align(al), tip, on) {
                st.fmt.edit_paras(range.clone(), |p| p.align = if on { ParaAlign::Default } else { al });
                refocus = true;
            }
        }
        ui.separator();
        // Lists and special characters
        if formatting_button(ui, FormattingIcon::Bullets, crate::tl!("Bulleted list"), false) {
            st.fmt.make_list(range.clone(), ListStyle::Bullet, 3.0 * base_h);
            refocus = true;
        }
        if formatting_button(ui, FormattingIcon::Numbers, crate::tl!("Numbered list"), false) {
            st.fmt.make_list(range.clone(), ListStyle::Number, 3.0 * base_h);
            refocus = true;
        }
        if formatting_button(ui, FormattingIcon::Tab, crate::tl!("Insert tab"), false) {
            insert(st, "\t");
            refocus = true;
        }
        for (glyph, tip, code) in [("°", crate::tl!("Degree"), "°"), ("±", crate::tl!("Plus/minus"), "±")] {
            if ui.button(glyph).on_hover_text(tip).clicked() {
                insert(st, code);
                refocus = true;
            }
        }
        if diameter_button(ui) {
            insert(st, "⌀");
            refocus = true;
        }
    });
    if refocus {
        ui.ctx().memory_mut(|m| m.request_focus(edit_id()));
    }
}

/// Formatting marks use geometry so the toolbar works with the bundled font stack on web too.
enum FormattingIcon {
    Align(ParaAlign),
    Bullets,
    Numbers,
    Tab,
}

/// Glyph-only MTEXT toolbar button: paints `icon`, exposes `label` as its accessible name and tooltip, and returns whether it was clicked.
fn formatting_button(ui: &mut egui::Ui, icon: FormattingIcon, label: &str, selected: bool) -> bool {
    let response = ui
        .add(egui::Button::new(" ").selected(selected).min_size(vec2(22.0, ui.spacing().interact_size.y)))
        .accessible_name(label)
        .on_hover_text(label);
    let painter = ui.painter();
    let c = response.rect.center();
    let color = if response.enabled() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let stroke = Stroke::new(1.2, color);
    match icon {
        FormattingIcon::Align(align) => {
            for row in 0..3 {
                let width = if row == 1 && align != ParaAlign::Justify { 8.0 } else { 14.0 };
                let x = match align {
                    ParaAlign::Center => -width / 2.0,
                    ParaAlign::Right => 7.0 - width,
                    _ => -7.0,
                };
                let y = (row as f32 - 1.0) * 4.0;
                painter.line_segment([c + vec2(x, y), c + vec2(x + width, y)], stroke);
            }
        }
        FormattingIcon::Bullets | FormattingIcon::Numbers => {
            for row in 0..3 {
                let y = (row as f32 - 1.0) * 5.0;
                if matches!(icon, FormattingIcon::Bullets) {
                    painter.circle_filled(c + vec2(-6.0, y), 1.2, color);
                } else {
                    painter.text(c + vec2(-6.0, y), Align2::CENTER_CENTER, (row + 1).to_string(), FontId::proportional(7.0), color);
                }
                painter.line_segment([c + vec2(-2.0, y), c + vec2(7.0, y)], stroke);
            }
        }
        FormattingIcon::Tab => {
            for [a, b] in [
                [vec2(-7.0, 0.0), vec2(4.0, 0.0)],
                [vec2(0.0, -4.0), vec2(4.0, 0.0)],
                [vec2(0.0, 4.0), vec2(4.0, 0.0)],
                [vec2(7.0, -5.0), vec2(7.0, 5.0)],
            ] {
                painter.line_segment([c + a, c + b], stroke);
            }
        }
    }
    response.clicked()
}

/// The MTEXT toolbar's height field. It has no visible caption, so the AccessKit label carries
/// the meaning of its hover tooltip; a tooltip alone leaves an unnamed spin button.
fn height_field(ui: &mut egui::Ui, height: &mut f64, base_h: f64) -> bool {
    ui.add(egui::DragValue::new(height).speed(base_h * 0.02).range(base_h * 0.05..=base_h * 50.0).max_decimals(4))
        .on_hover_text(crate::tl!("Text height"))
        .accessible_name(crate::tl!("Text height"))
        .changed()
}

/// The MTEXT Diameter button. U+2300 is absent from every bundled face (including the
/// monochrome and emoji fallbacks) and the web build has no system fonts, so the sign is painted
/// rather than shaped from text. The semantic button name keeps the glyph-only control
/// discoverable to accessibility tools.
fn diameter_button(ui: &mut egui::Ui) -> bool {
    let min_size = vec2(22.0, ui.spacing().interact_size.y);
    let resp = ui.add(egui::Button::new(" ").min_size(min_size)).on_hover_text(crate::tl!("Diameter"));
    let color = if resp.enabled() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let stroke = Stroke::new(1.4, color);
    let center = resp.rect.center();
    let radius = resp.rect.height() * 0.3;
    let diagonal = radius * std::f32::consts::FRAC_1_SQRT_2;
    let painter = ui.painter();
    painter.circle_stroke(center, radius, stroke);
    painter.line_segment([center + vec2(-diagonal, diagonal), center + vec2(diagonal, -diagonal)], stroke);
    // Name the node directly as well as through `WidgetInfo`: on a clicked frame `widget_info`
    // emits an output event and does not update the AccessKit node, which would leave the
    // glyph-only button unnamed for that frame.
    let resp = resp.accessible_name(crate::tl!("Diameter"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, resp.enabled(), crate::tl!("Diameter")));
    resp.clicked()
}

/// Insert `s` at the caret (over the selection).
fn insert(st: &mut State, s: &str) {
    let (a, b) = (st.sel.0.min(st.sel.1), st.sel.0.max(st.sel.1));
    let chars: Vec<char> = st.fmt.text().chars().collect();
    let new: String = chars.iter().take(a).chain(s.chars().collect::<Vec<_>>().iter()).chain(chars.iter().skip(b)).collect();
    if st.fmt.set_text(&new, &st.typing.clone()) {
        let at = a + s.chars().count();
        st.sel = (at, at);
        st.typing_at = at;
        st.caret = Some(at);
    }
}

/// The ruler: `px` points per text height (`base_h` drawing units); paragraph indents and tab
/// stops are in drawing units, as `\p` codes are.
fn ruler(ui: &mut egui::Ui, st: &mut State, px: f32, width: f32, base_h: f64) {
    let t = Tokens::get();
    let range = st.sel.0.min(st.sel.1)..st.sel.0.max(st.sel.1);
    let p = st.fmt.para(st.fmt.paragraph_of(range.start));
    let (rect, bg) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.control);
    let tick = |u: f64| rect.left() + (u as f32) * px;
    let x_of = |v: f64| tick(v / base_h);
    let units = (f64::from(width) / f64::from(px)).floor() as i32;
    for u in 0..=units.clamp(0, 400) {
        let x = tick(f64::from(u));
        let major = u % 5 == 0;
        painter.line_segment([pos2(x, rect.bottom() - if major { 7.0 } else { 3.0 }), pos2(x, rect.bottom())], Stroke::new(1.0, t.text_faint));
        if major && u > 0 {
            painter.text(pos2(x + 2.0, rect.top() + 1.0), Align2::LEFT_TOP, u.to_string(), FontId::proportional(8.0), t.text_faint);
        }
    }
    let to_units = |x: f32| ((f64::from(x - rect.left()) / f64::from(px) / SNAP).round() * SNAP).clamp(0.0, 400.0) * base_h;
    // Tab stops: an L at the stop; a right-click removes one.
    for (pos, _kind) in &p.tabs {
        let x = x_of(*pos);
        painter.add(Shape::line(
            vec![pos2(x, rect.bottom() - 9.0), pos2(x, rect.bottom() - 2.0), pos2(x + 4.0, rect.bottom() - 2.0)],
            Stroke::new(1.5, t.accent),
        ));
    }
    let hover = bg.hover_pos();
    // Indent markers: the first line's above, the left indent's below.
    let first_abs = p.left + p.first;
    let mut drag = None;
    for (k, (u, up)) in [(first_abs, false), (p.left, true)].into_iter().enumerate() {
        let x = x_of(u);
        let y = if up { rect.bottom() - 1.0 } else { rect.top() + 1.0 };
        let dir = if up { -1.0 } else { 1.0 };
        let hit = Rect::from_center_size(pos2(x, y + dir * 4.0), vec2(10.0, 10.0));
        let r = ui.interact(hit, ui.scope_id().with(("mtext_indent", k)), Sense::drag());
        let col = if r.hovered() || r.dragged() { t.accent } else { t.icon };
        painter.add(Shape::convex_polygon(vec![pos2(x - 4.0, y), pos2(x + 4.0, y), pos2(x, y + dir * 8.0)], col, Stroke::NONE));
        if r.dragged()
            && let Some(m) = r.interact_pointer_pos()
        {
            drag = Some((k, to_units(m.x)));
        }
    }
    if let Some((k, u)) = drag {
        if k == 0 {
            st.fmt.set_indents(range.clone(), u - p.left, p.left);
        } else {
            st.fmt.set_indents(range.clone(), p.first, u);
        }
    } else if bg.clicked()
        && let Some(m) = hover
    {
        st.fmt.add_tab(range.clone(), to_units(m.x), TabKind::Left);
    } else if bg.secondary_clicked()
        && let Some(m) = hover
    {
        st.fmt.remove_tab(range, to_units(m.x), 0.6 * base_h);
    }
    bg.on_hover_text(crate::tl!("Click the ruler to add a tab stop, right-click to remove one"));
}

/// MTEXT editor area with the formatted preview; restores a pending caret position before laying text out with `base_h`, `px`, `width` and the theme tokens.
fn text_box(ui: &mut egui::Ui, st: &mut State, base_h: f64, px: f32, width: f32, t: &Tokens) {
    let id = edit_id();
    if let Some(at) = st.caret.take()
        && let Some(mut s) = egui::TextEdit::load_state(ui.ctx(), id)
    {
        s.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        s.store(ui.ctx(), id);
    }
    let fmt = st.fmt.clone();
    let default_color = t.text;
    let mut layouter = move |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap;
        let s = text.as_str();
        let mut run = String::new();
        let mut cur: Option<CharFmt> = None;
        let flush = |job: &mut LayoutJob, run: &mut String, f: &CharFmt| {
            if run.is_empty() {
                return;
            }
            let size = f.height.map_or(px, |h| (px * (h / base_h) as f32).clamp(6.0, 120.0));
            let color = f.color.map_or(default_color, color32);
            job.append(
                run,
                0.0,
                TextFormat {
                    font_id: FontId::proportional(size),
                    color,
                    italics: f.italic,
                    underline: if f.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
                    // The editor has one weight of each font: bold is shown wider.
                    extra_letter_spacing: if f.bold { size * 0.06 } else { 0.0 },
                    ..TextFormat::default()
                },
            );
            run.clear();
        };
        for (i, c) in s.chars().enumerate() {
            let f = fmt.fmt_at(i);
            if cur.as_ref() != Some(&f) {
                if let Some(p) = &cur {
                    flush(&mut job, &mut run, p);
                }
                cur = Some(f);
            }
            run.push(c);
        }
        if let Some(p) = &cur {
            flush(&mut job, &mut run, p);
        }
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let mut buf = st.fmt.text().to_string();
    let out = egui::TextEdit::multiline(&mut buf)
        .id(id)
        .desired_width(width)
        .desired_rows(5)
        .margin(egui::Margin::symmetric(4, 4))
        .lock_focus(true)
        .char_limit(cadcraft_engine::mtext_edit::MAX_CHARS)
        .layouter(&mut layouter)
        .show(ui);
    out.response.response.clone().accessible_name(crate::tl!("Text"));
    if !out.response.has_focus() && buf.is_empty() && st.fmt.is_empty() {
        out.response.request_focus();
    }
    if out.response.changed() {
        let typing = st.typing.clone();
        st.fmt.set_text(&buf, &typing);
    }
    if let Some(r) = out.cursor_range {
        let (a, b) = (r.primary.index.0, r.secondary.index.0);
        st.sel = (a, b);
        if a == b {
            if out.response.changed() {
                st.typing_at = a;
            } else if a != st.typing_at {
                // The caret moved: typing continues the formatting of the character before it.
                st.typing = st.fmt.fmt_at(a.saturating_sub(1));
                st.typing_at = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use crate::Services;

    /// Clicking Diameter inserts U+2300 into both the formatted text and `to_contents()`, selects it, and leaves no unnamed widget.
    #[test]
    fn diameter_insertion_keeps_real_text_and_renders_without_optional_fonts() {
        let state = State { fmt: Formatted::default(), typing: CharFmt::default(), typing_at: 0, top: 70.0, sel: (0, 0), caret: None };
        let mut harness = Harness::builder().with_size(vec2(1100.0, 300.0)).with_context_setup(install_diameter_glyph).build_ui_state(
            |ui, state| {
                toolbar(ui, state, 2.5);
                text_box(ui, state, 2.5, 20.0, 1000.0, &Tokens::get());
            },
            state,
        );
        harness.run();
        harness.get_by_role_and_label(Role::Button, "Diameter").click();
        harness.run();
        assert_eq!(harness.state().fmt.text(), "⌀");
        assert_eq!(harness.state().fmt.to_contents(), "⌀");
        assert_eq!(harness.state().sel, (1, 1));
        assert!(harness.unnamed_widgets().is_empty());
    }

    /// The ten toolbar controls listed in the test are found by semantic name under the harness's default bundled fonts, and no toolbar widget is unnamed.
    #[test]
    fn the_whole_toolbar_uses_bundled_glyphs_and_names_its_controls() {
        let mut state = State { fmt: Formatted::default(), typing: CharFmt::default(), typing_at: 0, top: 70.0, sel: (0, 0), caret: None };
        let mut harness = Harness::builder().with_size(vec2(1100.0, 120.0)).build_ui(move |ui| toolbar(ui, &mut state, 2.5));
        harness.run();
        for name in ["Left", "Center", "Right", "Justify", "Bulleted list", "Numbered list", "Insert tab", "Diameter", "Text color", "Font"] {
            assert!(harness.query_by_label(name).is_some(), "missing semantic control {name}");
        }
        assert!(harness.unnamed_widgets().is_empty(), "unnamed controls: {:?}", harness.unnamed_widgets());
    }

    /// Strict bundled-font and accessibility regression for the glyph-only Diameter button: the
    /// test context uses egui's bundled faces only (no `theme::install_fonts` system prepend),
    /// and the harness panics on a missing glyph by default. The sign must be painted, not
    /// shaped from U+2300.
    #[test]
    fn diameter_button_is_painted_and_named_under_bundled_fonts() {
        let ctx = egui::Context::default();
        ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 60.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            diameter_button(ui);
        });
        out.textures_delta.clear();
        let mut text = String::new();
        let (mut circle, mut slash) = (false, false);
        for shape in &out.shapes {
            match &shape.shape {
                Shape::Circle(_) => circle = true,
                Shape::LineSegment { .. } => slash = true,
                Shape::Text(t) => text.push_str(&t.galley.job.text),
                _ => {}
            }
        }
        assert!(circle, "the diameter sign's circle is painted");
        assert!(slash, "the diameter sign's slash is painted");
        assert!(!text.contains('\u{2300}'), "U+2300 must not be shaped from a font: {text:?}");

        let mut harness = Harness::builder().with_size(vec2(240.0, 80.0)).build_ui(|ui| {
            diameter_button(ui);
        });
        harness.run();
        let button = harness.get_by_label("Diameter");
        assert_eq!(button.accesskit_node().role(), Role::Button);
        assert!(harness.unnamed_widgets().is_empty(), "unnamed controls: {:?}", harness.unnamed_widgets());
    }

    /// The height DragValue has no visible caption, so its AccessKit label must be set explicitly.
    /// Bundled fonts only: no system face can mask a missing glyph in the harness.
    #[test]
    fn height_field_is_named_for_accessibility_under_bundled_fonts() {
        let mut harness = Harness::builder().with_size(vec2(360.0, 80.0)).build_ui(|ui| {
            let mut height = 2.5;
            height_field(ui, &mut height, 2.5);
        });
        harness.run();
        let field = harness.get_by_role_and_label(Role::SpinButton, "Text height");
        assert_eq!(field.accesskit_node().label().as_deref(), Some("Text height"));
        assert!(harness.unnamed_widgets().is_empty(), "unnamed controls: {:?}", harness.unnamed_widgets());
    }

    #[test]
    fn editor_opens_only_for_mtext_text_entry_and_finishes_with_formatted_contents() {
        let ctx = egui::Context::default();
        let mut app = CadApp::new(Session::new(), Services::default());
        let frame = |app: &mut CadApp| {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), ..Default::default() };
            for _ in 0..2 {
                let mut out = ctx.run_ui(input.clone(), |ui| show(app, ui.ctx()));
                out.textures_delta.clear();
            }
        };
        frame(&mut app);
        assert!(ctx.data_mut(|d| d.get_temp::<State>(state_id())).is_none());
        app.start("mtext");
        assert!(app.session.editing_mtext().is_none(), "no box yet");
        app.session.input(cadcraft_engine::Input::Point(cadcraft_geom::Vec2::new(1.0, 1.0))).unwrap();
        app.session.input(cadcraft_engine::Input::Point(cadcraft_geom::Vec2::new(6.0, 0.0))).unwrap();
        let m = app.session.editing_mtext().expect("box known");
        assert!((m.width - 5.0).abs() < 1e-9, "{}", m.width);
        frame(&mut app);
        let mut st = ctx.data_mut(|d| d.get_temp::<State>(state_id())).expect("editor open");
        st.fmt.set_text("hello", &CharFmt::default());
        st.fmt.toggle(0..5, Flag::Bold);
        let contents = st.fmt.to_contents();
        app.session.input(cadcraft_engine::Input::Text(contents.clone())).unwrap();
        app.session.input(cadcraft_engine::Input::Enter).unwrap();
        let d = app.session.doc().unwrap();
        let found = d.model.iter().any(|e| matches!(&e.kind, cadcraft_engine::doc::EntityKind::MText(m) if m.contents == contents));
        assert!(found, "{contents}");
    }
}
