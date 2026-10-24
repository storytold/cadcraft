//! The in-place text editor: while MTEXT asks for its text, a text box opens on the canvas at
//! the text's location and width with a formatting toolbar (font, height, bold, italic,
//! underline, overline, colour, alignment, lists, tabs) and a ruler for the paragraph's indents
//! and tab stops. The text and its formats live in the engine's
//! [`cadcraft_engine::mtext_edit::Formatted`]; this file only draws and forwards edits.
//!
//! The same editor edits existing text when a command describes it
//! ([`cadcraft_engine::Session::text_editor`]): double-click multiline text or a multileader
//! (toolbar and ruler), one line of text (a plain line, Enter finishes), a dimension (`<>` is
//! the measured value) or a table cell (Tab and Shift+Tab move to the next and previous cell,
//! Alt+↑ and Alt+↓ to the cell above and below).

use cadcraft_engine::EditorKind;
use cadcraft_engine::mtext_edit::{CharFmt, Flag, Formatted, ListStyle, MTextColor, ParaAlign, TabKind};
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// Smallest and largest size of the text in the box, in points: the box stays readable at any zoom.
const MIN_PX: f32 = 18.0;
const MAX_PX: f32 = 36.0;
/// Families the font list offers besides the one set in the text.
const FAMILIES: [&str; 6] = ["Arial", "Times New Roman", "Courier New", "Verdana", "Georgia", "Calibri"];
/// The standard colours (ACI 1..=7) of the colour menu.
const SWATCHES: [u16; 7] = [1, 2, 3, 4, 5, 6, 7];
/// Ruler snap, in text heights.
const SNAP: f64 = 0.25;

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
    /// The object being edited ([`cadcraft_engine::TextEditor::key`]): another one reloads.
    key: String,
    /// Give the text box the keyboard in the next frame.
    focus: bool,
    /// The text style's font: the text box shows the text in it.
    font: crate::editor_glyphs::EditorFont,
    /// The text box's caret and selection.
    edit: crate::bidi_edit::EditState,
    /// Formula completion: the highlighted function, and the text it was closed for (Escape).
    fx_index: usize,
    fx_closed: Option<String>,
    /// Where the completion list goes (the caret's bottom, last frame).
    fx_at: egui::Pos2,
    /// Undo and redo steps of the whole editor (text and formatting), with the selection then.
    undo: Vec<(Formatted, usize, usize)>,
    redo: Vec<(Formatted, usize, usize)>,
    /// When the last undo step was recorded (typing within a second makes one step).
    undo_at: f64,
}

fn state_id() -> egui::Id {
    egui::Id::new("mtext_editor_state")
}

fn edit_id() -> egui::Id {
    egui::Id::new("mtext_editor_text")
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
    let editor = app.session.text_editor();
    let asking = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point);
    let (Some(editor), true) = (editor, asking) else {
        ctx.data_mut(|d| d.remove::<State>(state_id()));
        return;
    };
    let mt = &editor.mtext;
    let kind = editor.kind.clone();
    let t = Tokens::get();
    let (rect, scale) = app.canvas.xf.map_or((ctx.content_rect(), 20.0), |x| (x.rect, x.scale.max(1e-6)));
    let mut st = ctx.data_mut(|d| d.get_temp::<State>(state_id())).filter(|st| st.key == editor.key).unwrap_or_else(|| {
        let mut fmt = match &kind {
            EditorKind::NewMText => Formatted::default(),
            // One line of plain text: backslashes and braces are text, not codes.
            EditorKind::Line => {
                let mut f = Formatted::default();
                f.set_text(&mt.contents, &CharFmt::default());
                f
            }
            _ => Formatted::from_contents(&mt.contents),
        };
        let style = app.session.doc().ok().and_then(|d| d.text_style(&mt.style).cloned());
        if let Some(f) = style.as_ref().map(|s| s.font.clone()) {
            // A family name, not a font file (`txt.shx`, `arial.ttf`).
            if !f.is_empty() && !f.contains('.') {
                fmt.base_font = f;
            }
        }
        let font = style
            .map(|s| crate::editor_glyphs::EditorFont { font: s.font, big_font: s.big_font, width_factor: s.width_factor, oblique: s.oblique })
            .unwrap_or_else(|| crate::editor_glyphs::EditorFont { width_factor: 1.0, ..Default::default() });
        let end = fmt.len();
        // Existing text opens selected as a whole: typing replaces it, an arrow key keeps it.
        let existing = !matches!(kind, EditorKind::NewMText) && end > 0;
        State {
            fmt,
            typing: CharFmt::default(),
            typing_at: end,
            top: 70.0,
            sel: if existing { (end, 0) } else { (end, end) },
            caret: (!existing).then_some(end),
            key: editor.key.clone(),
            focus: true,
            font,
            edit: if existing { crate::bidi_edit::EditState::all(end) } else { crate::bidi_edit::EditState::at(end) },
            fx_index: 0,
            fx_closed: None,
            fx_at: pos2(0.0, 0.0),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_at: 0.0,
        }
    });
    let line = kind == EditorKind::Line;
    let cell = matches!(kind, EditorKind::TableCell { .. });
    let formatting = !line;
    let ruler_shown = matches!(kind, EditorKind::NewMText | EditorKind::MText);
    // Keys the editor takes before the text box: Tab between table cells, Enter ends one line.
    let mut done = Done::No;
    if cell {
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                done = Done::Move("Previous");
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                done = Done::Move("Next");
            } else if i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowUp) {
                done = Done::Move("Up");
            } else if i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown) {
                done = Done::Move("Down");
            }
        });
    }
    // Undo and redo (Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z, ⌘ on a Mac), taken before the text box.
    let before = (st.fmt.clone(), st.edit.cursor, st.edit.anchor);
    let mut undo_redo: Option<bool> = None;
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) {
            undo_redo = Some(false);
        } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
            undo_redo = Some(true);
        }
    });
    // Formula completion: the arrows choose a function, Tab or Enter puts it in, Escape closes
    // the list (taken before the text box and before Enter finishes one line of text).
    let fx = completion(&st);
    if let Some((_, list)) = &fx {
        let n = list.len().max(1);
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                st.fx_index = (st.fx_index + 1) % n;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                st.fx_index = (st.fx_index + n - 1) % n;
            }
        });
        let pick = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if pick {
            let k = st.fx_index;
            accept_completion(&mut st, k);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            st.fx_closed = Some(st.fmt.text().to_string());
        }
    }
    if line && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        done = Done::Ok;
    }
    // Ctrl+Enter (⌘↩) finishes, taken before the text box can make it a new line.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        done = Done::Ok;
    }
    let base_h = if mt.height > 0.0 { mt.height } else { 0.2 };
    let px = ((base_h * scale) as f32).clamp(MIN_PX, MAX_PX);
    let width = if mt.width > 0.0 { ((mt.width * scale) as f32).clamp(380.0, rect.width().max(380.0) * 0.9) } else { 380.0 };
    // The box's top-left corner on screen (rotation is ignored while editing).
    let code = usize::from(mt.attach.clamp(1, 9) - 1);
    let (col, row) = ((code % 3) as f64, (code / 3) as f64);
    let ww = f64::from(width) / scale;
    // One line of text: the editor's line sits on the text's own line (its top a little above the
    // cap height), so what is typed shows where the text goes.
    let rise = if line { 0.75 } else { 2.0 };
    let corner = cadcraft_geom::Vec2::new(mt.insert.x - col * ww / 2.0, mt.insert.y + row * base_h * rise);
    let at = app.canvas.xf.map_or(rect.center(), |x| x.to_screen(corner));
    let max_x = (rect.right() - width - 8.0).max(rect.left());
    let pos = pos2(at.x.clamp(rect.left(), max_x), at.y.clamp(rect.top() + st.top, (rect.bottom() - 120.0).max(rect.top() + st.top)));

    let top_start = pos.y - st.top;
    egui::Area::new(egui::Id::new("mtext_editor_area")).order(egui::Order::Foreground).fixed_pos(pos2(pos.x, top_start)).show(ctx, |ui| {
        egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.accent)).inner_margin(egui::Margin::same(0)).show(ui, |ui| {
            ui.set_width(width);
            let y0 = ui.cursor().top();
            let title = match &kind {
                EditorKind::NewMText | EditorKind::MText => crate::tl!("Multiline Text").to_string(),
                EditorKind::Line => crate::tl!("Text").to_string(),
                EditorKind::Dimension { .. } => crate::tl!("Dimension Text").to_string(),
                EditorKind::TableCell { name } => format!("{} {name}", crate::tl!("Cell")),
            };
            if header(ui, &title, cell) {
                done = Done::Cancel;
            }
            if formatting {
                toolbar(ui, &mut st, base_h);
            }
            if ruler_shown {
                ruler(ui, &mut st, px, width, base_h);
            }
            if let EditorKind::Dimension { measured } = &kind {
                dimension_row(ui, &mut st, measured);
            }
            st.top = ui.cursor().top() - y0;
            text_box(ui, &mut st, base_h, px, width, &t, line);
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                if ui.button(crate::tl!("OK")).clicked() {
                    done = Done::Ok;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    done = Done::Cancel;
                }
                ui.add_space(6.0);
                let mac = ui.ctx().os().is_mac();
                let tip = |undo: bool| match (undo, mac) {
                    (true, true) => crate::tl!("Undo (⌘Z)"),
                    (true, false) => crate::tl!("Undo (Ctrl+Z)"),
                    (false, true) => crate::tl!("Redo (⇧⌘Z)"),
                    (false, false) => crate::tl!("Redo (Ctrl+Y)"),
                };
                for (undo, icon) in [(true, crate::icons::Icon::Undo), (false, crate::icons::Icon::Redo)] {
                    let any = if undo { !st.undo.is_empty() } else { !st.redo.is_empty() };
                    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
                    if resp.hovered() && any {
                        ui.painter().rect_filled(r, 3.0, t.control_hover);
                    }
                    crate::icons::paint(ui.painter(), r.shrink(3.0), icon, !any);
                    if resp.on_hover_text(tip(undo)).clicked() && any {
                        undo_redo = Some(undo);
                    }
                }
                if cell {
                    if icon_button(ui, false, crate::tl!("Previous cell (Shift+Tab)"), |p, r, c| paint_triangle(p, r, c, -1.0)).clicked() {
                        done = Done::Move("Previous");
                    }
                    if icon_button(ui, false, crate::tl!("Next cell (Tab)"), |p, r, c| paint_triangle(p, r, c, 1.0)).clicked() {
                        done = Done::Move("Next");
                    }
                }
                let finish = match (line, ui.ctx().os().is_mac()) {
                    (true, _) => crate::tl!("Enter to finish"),
                    (false, true) => crate::tl!("⌘↩ to finish"),
                    (false, false) => crate::tl!("Ctrl+Enter to finish"),
                };
                ui.label(RichText::new(finish).small().color(t.text_dim));
            });
        });
    });
    let now = ctx.input(|i| i.time);
    match undo_redo {
        Some(undo) => {
            let (from, to) = if undo { (&mut st.undo, &mut st.redo) } else { (&mut st.redo, &mut st.undo) };
            if let Some((f, c, a)) = from.pop() {
                to.push((st.fmt.clone(), st.edit.cursor, st.edit.anchor));
                st.fmt = f;
                st.edit.cursor = c.min(st.fmt.len());
                st.edit.anchor = a.min(st.fmt.len());
                st.sel = (st.edit.cursor, st.edit.anchor);
                st.typing_at = st.edit.cursor;
                st.undo_at = 0.0;
            }
        }
        None if st.fmt != before.0 => {
            // Typing within a second of the last change is one step.
            if now - st.undo_at > 1.0 || st.undo.is_empty() {
                st.undo.push(before);
                if st.undo.len() > 200 {
                    st.undo.remove(0);
                }
            }
            st.undo_at = now;
            st.redo.clear();
        }
        None => {}
    }
    if let Some((_, list)) = completion(&st) {
        let mut clicked = None;
        egui::Area::new(egui::Id::new("mtext_formula_list")).order(egui::Order::Tooltip).fixed_pos(st.fx_at + vec2(0.0, 4.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(260.0);
                for (k, (name, args, _)) in list.iter().enumerate().take(10) {
                    let r = ui.selectable_label(k == st.fx_index, RichText::new(format!("{name}({args})")).monospace());
                    if r.clicked() {
                        clicked = Some(k);
                    }
                }
                if let Some((_, _, what)) = list.get(st.fx_index) {
                    ui.separator();
                    ui.label(RichText::new(crate::i18n::t(what)).small().color(t.text_dim));
                }
                ui.label(RichText::new(crate::tl!("Tab or Enter: insert · Esc: close")).small().color(t.text_faint));
            });
        });
        if let Some(k) = clicked {
            accept_completion(&mut st, k);
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        done = Done::Cancel;
    }
    let contents = if line { st.fmt.text().to_string() } else { st.fmt.to_contents() };
    match done {
        Done::No => {
            ctx.data_mut(|d| d.insert_temp(state_id(), st));
        }
        Done::Ok if kind == EditorKind::NewMText => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            // The editor holds the whole contents: one input, then Enter ends MTEXT's text entry.
            if !st.fmt.is_empty() {
                let r = app.session.input(cadcraft_engine::Input::Text(contents));
                report(app, r);
            }
            // MTEXT ends its text with Enter; MLEADER takes Enter for "no text" when nothing was typed.
            let still_asking = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point);
            if app.session.running.as_ref().is_some_and(|r| r.id == "mtext") || (st.fmt.is_empty() && still_asking) {
                let _ = app.session.input(cadcraft_engine::Input::Enter);
            }
        }
        Done::Ok => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            // Existing text: the new contents (even empty), then a table cell ends its editing.
            let r = app.session.input(cadcraft_engine::Input::Text(contents));
            report(app, r);
            if cell {
                let _ = app.session.input(cadcraft_engine::Input::Enter);
            }
        }
        Done::Move(dir) => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            let r = app.session.input(cadcraft_engine::Input::Text(contents));
            report(app, r);
            let r = app.session.input(cadcraft_engine::Input::Keyword(dir.into()));
            report(app, r);
        }
        Done::Cancel => {
            ctx.data_mut(|d| d.remove::<State>(state_id()));
            app.session.cancel();
        }
    }
}

/// Show a refused edit on the command line.
fn report<T, E: std::fmt::Display>(app: &mut CadApp, r: Result<T, E>) {
    if let Err(e) = r {
        app.session.echo(e.to_string());
    }
}

#[derive(PartialEq)]
enum Done {
    No,
    Ok,
    Cancel,
    /// Keep this cell's text and move to another (a TABLEDIT move keyword).
    Move(&'static str),
}

/// A dimension's text: what `<>` stands for, and buttons to insert it or go back to it alone.
fn dimension_row(ui: &mut egui::Ui, st: &mut State, measured: &str) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("<> = {measured}")).monospace().color(t.text));
        ui.label(RichText::new(crate::tl!("the measured value")).small().color(t.text_dim));
        if ui.small_button(crate::tl!("Insert <>")).on_hover_text(crate::tl!("Insert the measured value at the cursor")).clicked() {
            insert(st, "<>");
        }
        if ui.small_button(crate::tl!("Measured Value Only")).on_hover_text(crate::tl!("Replace the text with the measured value")).clicked() {
            st.fmt = Formatted::from_contents("<>");
            st.sel = (2, 2);
            st.caret = Some(2);
            st.typing_at = 2;
        }
    });
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

fn toolbar(ui: &mut egui::Ui, st: &mut State, base_h: f64) {
    let cur = current(st);
    let range = st.sel.0.min(st.sel.1)..st.sel.0.max(st.sel.1);
    let mut refocus = false;
    // Row 1: character format.
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        // Font
        let shown = cur.font.clone().unwrap_or_else(|| st.fmt.base_font.clone());
        egui::ComboBox::from_id_salt("mtext_font").width(120.0).selected_text(shown.clone()).show_ui(ui, |ui| {
            // The fonts in the user's font folders, then the usual ones, then the text's own.
            let mut families = crate::options::font_choices().1;
            for f in FAMILIES.iter().map(|s| s.to_string()).chain(cur.font.clone()) {
                if !families.iter().any(|x| x.eq_ignore_ascii_case(&f)) {
                    families.push(f);
                }
            }
            for fam in families {
                if ui.selectable_label(shown == fam, &fam).clicked() {
                    change(st, |c| c.font = Some(fam.clone()));
                    refocus = true;
                }
            }
        });
        // Height
        let mut h = cur.height.unwrap_or(base_h);
        if ui
            .add(egui::DragValue::new(&mut h).speed(base_h * 0.02).range(base_h * 0.05..=base_h * 50.0).max_decimals(4))
            .on_hover_text(crate::tl!("Text height"))
            .changed()
        {
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
        ui.separator();
        // Colour: an A over a bar in the colour, as word processors show it.
        let swatch = cur.color.map_or(ui.visuals().text_color(), color32);
        let resp = icon_button_wide(ui, false, crate::tl!("Text color"), |p, r, c| {
            let a = Rect::from_min_size(r.min, vec2(r.height(), r.height()));
            p.text(pos2(a.center().x, a.top() + a.height() * 0.42), Align2::CENTER_CENTER, "A", FontId::proportional(a.height() * 0.8), c);
            p.rect_filled(Rect::from_min_max(pos2(a.left() + 1.0, a.bottom() - 3.0), pos2(a.right() - 1.0, a.bottom())), 0.0, swatch);
            paint_chevron(p, Rect::from_min_max(pos2(a.right() + 1.0, r.top()), r.max), c);
        });
        egui::Popup::menu(&resp).show(|ui| {
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
        });
    });
    // Row 2: paragraph, lists, tab and symbols.
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let para = st.fmt.para(st.fmt.paragraph_of(range.start));
        for (al, tip) in [
            (ParaAlign::Left, crate::tl!("Align left")),
            (ParaAlign::Center, crate::tl!("Center")),
            (ParaAlign::Right, crate::tl!("Align right")),
            (ParaAlign::Justify, crate::tl!("Justify")),
        ] {
            let on = para.align == al;
            if icon_button(ui, on, tip, |p, r, c| paint_align(p, r, c, al)).clicked() {
                st.fmt.edit_paras(range.clone(), |p| p.align = if on { ParaAlign::Default } else { al });
                refocus = true;
            }
        }
        ui.separator();
        if ui.button("•≡").on_hover_text(crate::tl!("Bulleted list")).clicked() {
            st.fmt.make_list(range.clone(), ListStyle::Bullet, 3.0 * base_h);
            refocus = true;
        }
        if ui.button("1≡").on_hover_text(crate::tl!("Numbered list")).clicked() {
            st.fmt.make_list(range.clone(), ListStyle::Number, 3.0 * base_h);
            refocus = true;
        }
        ui.separator();
        if icon_button(ui, false, crate::tl!("Insert tab"), paint_tab).clicked() {
            insert(st, "\t");
            refocus = true;
        }
        let resp = icon_button_wide(ui, false, crate::tl!("Additional symbols"), |p, r, c| {
            let a = Rect::from_min_size(r.min, vec2(r.height(), r.height()));
            p.text(a.center(), Align2::CENTER_CENTER, "Ω", FontId::proportional(a.height() * 0.85), c);
            paint_chevron(p, Rect::from_min_max(pos2(a.right() + 1.0, r.top()), r.max), c);
        });
        egui::Popup::menu(&resp).show(|ui| {
            ui.label(RichText::new(crate::tl!("Additional symbols")).strong());
            egui::Grid::new("mtext_symbols").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                for (i, (sym, name)) in SYMBOLS.iter().enumerate() {
                    let b = egui::Button::new(RichText::new(*sym).size(17.0)).min_size(vec2(30.0, 28.0));
                    if ui
                        .add(b)
                        .on_hover_text(format!("{}  ({sym}, U+{:04X})", crate::i18n::t(name), sym.chars().next().map_or(0, u32::from)))
                        .clicked()
                    {
                        insert(st, sym);
                        refocus = true;
                        ui.close();
                    }
                    if i % 8 == 7 {
                        ui.end_row();
                    }
                }
            });
        });
    });
    if refocus {
        ui.ctx().memory_mut(|m| m.request_focus(edit_id()));
    }
}

/// The symbols of the Additional symbols menu: drafting marks, comparisons, Greek letters,
/// powers and fractions.
const SYMBOLS: [(&str, &str); 32] = [
    ("°", "Degree"),
    ("±", "Plus/minus"),
    ("⌀", "Diameter"),
    ("∠", "Angle"),
    ("⊥", "Perpendicular"),
    ("∥", "Parallel"),
    ("℄", "Center line"),
    ("⌒", "Arc"),
    ("≈", "Almost equal"),
    ("≠", "Not equal"),
    ("≤", "Less than or equal"),
    ("≥", "Greater than or equal"),
    ("×", "Multiplication"),
    ("÷", "Division"),
    ("√", "Square root"),
    ("∞", "Infinity"),
    ("Δ", "Delta"),
    ("Ω", "Ohm"),
    ("µ", "Micro"),
    ("π", "Pi"),
    ("α", "Alpha"),
    ("β", "Beta"),
    ("γ", "Gamma"),
    ("φ", "Phi"),
    ("²", "Squared"),
    ("³", "Cubed"),
    ("½", "One half"),
    ("¼", "One quarter"),
    ("¾", "Three quarters"),
    ("‰", "Per mille"),
    ("•", "Bullet"),
    ("№", "Number sign"),
];

/// A square toolbar button drawn by `paint` (icon rectangle, colour): lit when `on`.
fn icon_button(ui: &mut egui::Ui, on: bool, tip: &str, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    icon_button_sized(ui, on, tip, vec2(24.0, 22.0), vec2(16.0, 16.0), paint)
}

/// A toolbar button with room for a drop-down arrow after its icon.
fn icon_button_wide(ui: &mut egui::Ui, on: bool, tip: &str, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    icon_button_sized(ui, on, tip, vec2(32.0, 22.0), vec2(24.0, 16.0), paint)
}

fn icon_button_sized(
    ui: &mut egui::Ui,
    on: bool,
    tip: &str,
    size: egui::Vec2,
    icon: egui::Vec2,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let t = Tokens::get();
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        Some(t.accent.gamma_multiply(0.45))
    } else if resp.hovered() {
        Some(t.control_hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 3.0, fill);
    }
    let color = if resp.hovered() || on { t.text } else { ui.visuals().text_color() };
    paint(ui.painter(), Rect::from_center_size(rect.center(), icon), color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    resp.on_hover_text(crate::i18n::t(tip))
}

/// Paragraph alignment as word processors draw it: five lines of text, long and short, lined up
/// on the left, the centre or the right, or all full width.
fn paint_align(p: &egui::Painter, r: Rect, c: Color32, al: ParaAlign) {
    let stroke = Stroke::new(1.6, c);
    for i in 0..5 {
        let y = r.top() + 1.5 + i as f32 * 3.25;
        let w = match al {
            ParaAlign::Justify => r.width(),
            _ if i % 2 == 0 => r.width(),
            ParaAlign::Center => r.width() * 0.6,
            _ => r.width() * 0.62,
        };
        let x0 = match al {
            ParaAlign::Center => r.center().x - w / 2.0,
            ParaAlign::Right => r.right() - w,
            _ => r.left(),
        };
        p.line_segment([pos2(x0, y), pos2(x0 + w, y)], stroke);
    }
}

/// Insert tab: an arrow running into a stop.
fn paint_tab(p: &egui::Painter, r: Rect, c: Color32) {
    let stroke = Stroke::new(1.6, c);
    let y = r.center().y;
    let stop = r.right() - 1.5;
    p.line_segment([pos2(r.left() + 1.0, y), pos2(stop - 2.0, y)], stroke);
    p.line_segment([pos2(stop - 6.5, y - 4.5), pos2(stop - 2.0, y)], stroke);
    p.line_segment([pos2(stop - 6.5, y + 4.5), pos2(stop - 2.0, y)], stroke);
    p.line_segment([pos2(stop, r.top() + 2.0), pos2(stop, r.bottom() - 2.0)], stroke);
}

/// A small drop-down arrow in `r`.
fn paint_chevron(p: &egui::Painter, r: Rect, c: Color32) {
    let m = r.center();
    p.add(Shape::convex_polygon(vec![pos2(m.x - 3.0, m.y - 1.5), pos2(m.x + 3.0, m.y - 1.5), pos2(m.x, m.y + 2.0)], c, Stroke::NONE));
}

/// A triangle pointing left (`dir` -1) or right (1).
fn paint_triangle(p: &egui::Painter, r: Rect, c: Color32, dir: f32) {
    let m = r.center();
    let pts = vec![pos2(m.x + 4.0 * dir, m.y), pos2(m.x - 3.0 * dir, m.y - 5.0), pos2(m.x - 3.0 * dir, m.y + 5.0)];
    p.add(Shape::convex_polygon(pts, c, Stroke::NONE));
}

/// The editor's title and its close button (the same as Cancel); a table cell's hint. True when
/// the close button was clicked.
fn header(ui: &mut egui::Ui, title: &str, cell: bool) -> bool {
    let t = Tokens::get();
    let mut close = false;
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let label = ui.label(RichText::new(title).strong());
        if cell {
            label.on_hover_text(crate::tl!("Tab: next cell · Shift+Tab: previous · Alt+↑↓: cell above, below"));
            ui.label(RichText::new(crate::tl!("Tab: next cell")).small().color(t.text_dim));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(196, 43, 28));
            }
            let c = if resp.hovered() { Color32::WHITE } else { ui.visuals().text_color() };
            let k = Rect::from_center_size(rect.center(), vec2(9.0, 9.0));
            ui.painter().line_segment([k.left_top(), k.right_bottom()], Stroke::new(1.5, c));
            ui.painter().line_segment([k.right_top(), k.left_bottom()], Stroke::new(1.5, c));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::tl!("Close")));
            close = resp.on_hover_text(crate::tl!("Close (same as Cancel)")).clicked();
        });
    });
    close
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
        let r = ui.interact(hit, ui.id().with(("mtext_indent", k)), Sense::drag());
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

fn text_box(ui: &mut egui::Ui, st: &mut State, base_h: f64, px: f32, width: f32, t: &Tokens, line: bool) {
    let id = edit_id();
    if let Some(at) = st.caret.take() {
        st.edit.cursor = at;
        st.edit.anchor = at;
    }
    let fmt = st.fmt.clone();
    let default_color = t.text;
    let base = st.font.clone();
    // The font each character is drawn in: an inline family (`\f`) or the style's.
    let font_of = |f: &CharFmt| match &f.font {
        Some(fam) if !fam.trim().is_empty() && !fam.eq_ignore_ascii_case(&fmt_base(&base)) => {
            crate::editor_glyphs::EditorFont { font: fam.clone(), big_font: String::new(), ..base.clone() }
        }
        _ => base.clone(),
    };
    let size_of = |f: &CharFmt| f.height.map_or(px, |h| (px * (h / base_h) as f32).clamp(6.0, 120.0));
    let look = |i: usize| {
        let f = fmt.fmt_at(i);
        crate::bidi_edit::Look {
            font: font_of(&f),
            italic: f.italic,
            bold: f.bold,
            underline: f.underline,
            size: size_of(&f),
            color: f.color.map_or(default_color, color32),
        }
    };
    let mut buf = st.fmt.text().to_string();
    let out = crate::bidi_edit::show(
        ui,
        id,
        &mut buf,
        &mut st.edit,
        line,
        width,
        if line { 1 } else { 5 },
        &look,
        px,
        cadcraft_engine::mtext_edit::MAX_CHARS,
    );
    st.fx_at = out.caret;
    let focused = ui.memory(|m| m.has_focus(id));
    if !focused && (st.focus || (buf.is_empty() && st.fmt.is_empty())) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    st.focus = false;
    if out.changed {
        let typing = st.typing.clone();
        st.fmt.set_text(&buf, &typing);
    }
    let (a, b) = (st.edit.cursor, st.edit.anchor);
    st.sel = (a, b);
    if a == b {
        if out.changed {
            st.typing_at = a;
        } else if a != st.typing_at {
            // The caret moved: typing continues the formatting of the character before it.
            st.typing = st.fmt.fmt_at(a.saturating_sub(1));
            st.typing_at = a;
        }
    }
}

/// The formula functions whose names start with the word before the caret, when the text is
/// a formula (`=...`) and the caret is not inside a quoted string: (the word's start, the
/// functions).
fn completion(st: &State) -> Option<(usize, Vec<&'static (&'static str, &'static str, &'static str)>)> {
    let text = st.fmt.text();
    if !text.trim_start().starts_with('=') || st.edit.cursor != st.edit.anchor || st.fx_closed.as_deref() == Some(text) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let at = st.edit.cursor.min(chars.len());
    if chars.get(..at)?.iter().filter(|c| **c == '"').count() % 2 == 1 {
        return None;
    }
    let mut s = at;
    while s > 0 && chars.get(s - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
        s -= 1;
    }
    let word: String = chars.get(s..at)?.iter().collect::<String>().to_ascii_uppercase();
    if word.is_empty() || !word.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let list: Vec<_> = cadcraft_engine::textformula::FUNCTIONS.iter().filter(|(n, _, _)| n.starts_with(&word) && *n != word).collect();
    (!list.is_empty()).then_some((s, list))
}

/// Put function `k` of the completion list in place of the word before the caret, with its
/// opening parenthesis (and the closing one when it takes no arguments).
fn accept_completion(st: &mut State, k: usize) {
    let Some((start, list)) = completion(st) else { return };
    let Some((name, args, _)) = list.get(k).map(|f| **f) else { return };
    let mut chars: Vec<char> = st.fmt.text().chars().collect();
    let at = st.edit.cursor.min(chars.len());
    let ins: Vec<char> = format!("{name}({}", if args.is_empty() { ")" } else { "" }).chars().collect();
    let n = ins.len();
    chars.splice(start..at, ins);
    let text: String = chars.iter().collect();
    let typing = st.typing.clone();
    st.fmt.set_text(&text, &typing);
    st.edit.cursor = start + n;
    st.edit.anchor = st.edit.cursor;
    st.typing_at = st.edit.cursor;
    st.fx_index = 0;
}

/// The style font's family as an inline font switch would name it (`txt.shx` → `txt`).
fn fmt_base(f: &crate::editor_glyphs::EditorFont) -> String {
    let n = f.font.trim();
    n.rsplit(['\\', '/']).next().unwrap_or(n).split('.').next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    use crate::Services;

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
