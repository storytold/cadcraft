//! The Text Style dialog (STYLE): the drawing's text styles (all, or those in use) with a
//! preview, Set Current, New, Rename and Delete; the selected style's font by family and style
//! from the installed fonts, its height (the paper height of an annotative style) and its
//! effects. Edits wait for Apply, and leaving a style with edits not applied asks first, as
//! AutoCAD does.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cadcraft_engine::doc::{Common, Drawing, EntityKind, Space, Text, TextStyle};
use cadcraft_fonts::BUILTIN_FONT;
use cadcraft_fonts::ttf::FontEntry;
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::parametric::expr_field;
use crate::theme::Tokens;

const STATE: &str = "textstyle_dialog";

/// What waits for an answer about edits not applied yet.
#[derive(Clone, PartialEq)]
enum After {
    Select(String),
    SetCurrent,
    New,
    Close,
}

#[derive(Clone, Default)]
struct State {
    /// The style chosen in the list.
    selected: String,
    in_use_only: bool,
    /// The chosen style as edited, until Apply (`None`: no edits).
    edit: Option<TextStyle>,
    /// Asking whether to apply the edits before doing this.
    confirm: Option<After>,
    /// The name being typed for New.
    new_name: Option<String>,
    /// (style, new name) while renaming.
    renaming: Option<(String, String)>,
    font_filter: String,
    /// Use Big Font ticked while no big font is named yet.
    big_font_on: bool,
}

/// The style command's parameters for `st` as `name`, not made current.
fn apply_params(name: &str, st: &TextStyle) -> Value {
    json!({
        "name": name,
        "font": st.font,
        "bigFont": st.big_font,
        "height": st.height,
        "widthFactor": st.width_factor,
        "oblique": st.oblique.to_degrees(),
        "backwards": st.backwards,
        "upsideDown": st.upside_down,
        "vertical": st.vertical,
        "annotative": st.annotative,
        "current": false,
    })
}

/// "style1", "style2", … : the first name not taken, as AutoCAD offers.
fn new_name(d: &Drawing) -> String {
    (1..10_000).map(|i| format!("style{i}")).find(|n| d.text_style(n).is_none()).unwrap_or_else(|| "style".into())
}

/// A name no text style can have (as -STYLE refuses it), trimmed.
fn bad_name(name: &str) -> bool {
    name.is_empty() || name.len() > 255 || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'])
}

/// Whether style `name` can be deleted: not Standard, the current style or a style in use.
fn deletable(name: &str, current: &str, used: &[String]) -> bool {
    !name.eq_ignore_ascii_case("Standard") && !name.eq_ignore_ascii_case(current) && !used.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// The text styles in use, worked out again only when the drawing changes.
fn in_use(ctx: &egui::Context, app: &CadApp) -> Arc<Vec<String>> {
    let Ok(st) = app.session.state() else { return Arc::default() };
    let key = (st.uid, st.revision);
    let id = egui::Id::new("textstyle_in_use");
    if let Some((k, v)) = ctx.data(|m| m.get_temp::<((u64, u64), Arc<Vec<String>>)>(id))
        && k == key
    {
        return v;
    }
    let v = Arc::new(cadcraft_engine::cmd::text_styles_in_use(&st.doc));
    ctx.data_mut(|m| m.insert_temp(id, (key, v.clone())));
    v
}

/// The installed fonts, listed once.
fn fonts(ctx: &egui::Context) -> Arc<Vec<FontEntry>> {
    let id = egui::Id::new("textstyle_fonts");
    if let Some(v) = ctx.data(|m| m.get_temp::<Arc<Vec<FontEntry>>>(id)) {
        return v;
    }
    let v = Arc::new(cadcraft_fonts::ttf::installed_fonts());
    ctx.data_mut(|m| m.insert_temp(id, v.clone()));
    v
}

fn is_builtin(font: &str) -> bool {
    font.trim().is_empty() || font.trim().eq_ignore_ascii_case(BUILTIN_FONT)
}

fn is_shx(font: &str) -> bool {
    font.trim().to_ascii_lowercase().ends_with(".shx")
}

/// The installed face a style's font names: by file name ("arialbd.ttf", "arialbd"), else by
/// family (its Regular face, or its first).
fn face_of<'a>(font: &str, fonts: &'a [FontEntry]) -> Option<&'a FontEntry> {
    let f = font.trim().to_ascii_lowercase();
    if f.is_empty() || is_shx(&f) {
        return None;
    }
    let stem = |s: &str| s.rsplit_once('.').map_or(s.to_string(), |(a, _)| a.to_string());
    fonts
        .iter()
        .find(|e| e.file.eq_ignore_ascii_case(&f) || stem(&e.file) == stem(&f))
        .or_else(|| fonts.iter().filter(|e| e.family.eq_ignore_ascii_case(&f)).min_by_key(|e| !e.style.eq_ignore_ascii_case("Regular")))
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> super::Action {
    let used = in_use(ctx, app);
    let fonts = fonts(ctx);
    let Ok(d) = app.session.doc() else {
        super::no_drawing(ctx, crate::tl!("Text Style"), open);
        return None;
    };
    let mut st: State = super::temp(ctx, STATE).unwrap_or_default();
    let current = d.header.str("TEXTSTYLE", "Standard");
    if d.text_style(&st.selected).is_none() {
        st.selected = current.clone();
        st.edit = None;
    }
    let saved = d.text_style(&st.selected).cloned().unwrap_or_default();
    let mut actions: Vec<(&'static str, Value)> = Vec::new();
    let mut keep = true;
    egui::Window::new(crate::tl!("Text Style"))
        .id(egui::Id::new(STATE))
        .open(&mut keep)
        .default_size(vec2(720.0, 400.0))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            if let Some(after) = st.confirm.clone() {
                confirm_page(ui, d, &mut st, &saved, after, &mut actions, open);
            } else if st.new_name.is_some() {
                new_page(ui, d, &mut st, &saved, &mut actions);
            } else {
                main_page(ui, d, &mut st, &saved, &current, &used, &fonts, &mut actions, open);
            }
        });
    if !keep {
        request(d, &mut st, After::Close, &mut actions, open);
    }
    super::set_temp(ctx, STATE, st);
    // Errors are echoed on the command line by `run`; a failed step stops the rest.
    for (cmd, p) in actions {
        if app.run(cmd, p).is_err() {
            break;
        }
    }
    None
}

/// Do `after` now, or ask first when the chosen style has edits not applied.
fn request(d: &Drawing, st: &mut State, after: After, actions: &mut Vec<(&'static str, Value)>, open: &mut bool) {
    if st.edit.is_some() {
        st.confirm = Some(after);
    } else {
        perform(d, st, after, actions, open);
    }
}

fn perform(d: &Drawing, st: &mut State, after: After, actions: &mut Vec<(&'static str, Value)>, open: &mut bool) {
    match after {
        After::Select(name) => {
            st.selected = name;
            st.edit = None;
            st.big_font_on = false;
        }
        After::SetCurrent => actions.push(("style.current", json!({ "name": st.selected }))),
        After::New => st.new_name = Some(new_name(d)),
        After::Close => *open = false,
    }
}

fn confirm_page(
    ui: &mut egui::Ui,
    d: &Drawing,
    st: &mut State,
    saved: &TextStyle,
    after: After,
    actions: &mut Vec<(&'static str, Value)>,
    open: &mut bool,
) {
    ui.label(crate::tf!("The style {style} has been modified. Do you want to save your changes?", style = saved.name));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(crate::tl!("Yes")).clicked() {
            if let Some(e) = st.edit.take() {
                actions.push(("style", apply_params(&saved.name, &e)));
            }
            st.confirm = None;
            perform(d, st, after.clone(), actions, open);
        }
        if ui.button(crate::tl!("No")).clicked() {
            st.edit = None;
            st.confirm = None;
            perform(d, st, after.clone(), actions, open);
        }
        if ui.button(crate::tl!("Cancel")).clicked() {
            st.confirm = None;
        }
    });
}

fn new_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State, saved: &TextStyle, actions: &mut Vec<(&'static str, Value)>) {
    let mut name = st.new_name.clone().unwrap_or_default();
    ui.label(RichText::new(crate::tl!("New Text Style")).strong());
    let mut enter = false;
    ui.horizontal(|ui| {
        ui.label(crate::tl!("Style Name"));
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(220.0));
        enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    });
    let trimmed = name.trim().to_string();
    let taken = d.text_style(&trimmed).is_some();
    let bad = bad_name(&trimmed);
    if taken {
        ui.label(RichText::new(crate::tl!("A text style with this name already exists.")).color(Tokens::get().text_dim));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if (ui.add_enabled(!taken && !bad, egui::Button::new(crate::tl!("OK"))).clicked() || enter && !taken && !bad) && st.new_name.is_some() {
            // The new style starts with the chosen style's settings.
            actions.push(("style", apply_params(&trimmed, saved)));
            st.selected = trimmed.clone();
            st.edit = None;
            st.new_name = None;
        }
        if ui.button(crate::tl!("Cancel")).clicked() {
            st.new_name = None;
        }
    });
    if st.new_name.is_some() {
        st.new_name = Some(name);
    }
}

#[allow(clippy::too_many_arguments)]
fn main_page(
    ui: &mut egui::Ui,
    d: &Drawing,
    st: &mut State,
    saved: &TextStyle,
    current: &str,
    used: &[String],
    fonts: &[FontEntry],
    actions: &mut Vec<(&'static str, Value)>,
    open: &mut bool,
) {
    let t = Tokens::get();
    ui.label(RichText::new(crate::tf!("Current text style: {style}", style = current)).color(t.text_dim));
    ui.add_space(4.0);
    let mut v = st.edit.clone().unwrap_or_else(|| saved.clone());
    let before = st.selected.clone();
    let mut deleted = false;
    ui.horizontal_top(|ui| {
        // The styles and the preview.
        ui.vertical(|ui| {
            ui.set_width(200.0);
            ui.label(RichText::new(crate::tl!("Styles")).strong());
            style_list(ui, d, st, current, used, actions, open);
            egui::ComboBox::from_id_salt("textstyle_list_filter")
                .selected_text(if st.in_use_only { crate::tl!("Styles in use") } else { crate::tl!("All styles") })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.in_use_only, false, crate::tl!("All styles"));
                    ui.selectable_value(&mut st.in_use_only, true, crate::tl!("Styles in use"));
                });
            ui.add_space(6.0);
            ui.label(RichText::new(crate::tl!("Preview")).strong());
            preview(ui, d, &v, vec2(200.0, 70.0));
        });
        ui.add_space(12.0);
        // The font, size and effects.
        ui.vertical(|ui| {
            ui.set_width(330.0);
            font_group(ui, st, &mut v, fonts);
            ui.add_space(6.0);
            size_group(ui, d, &mut v);
            ui.add_space(6.0);
            effects_group(ui, &mut v);
        });
        // The buttons.
        ui.vertical(|ui| {
            let w = vec2(110.0, 22.0);
            if ui.add_enabled(!st.selected.eq_ignore_ascii_case(current), egui::Button::new(crate::tl!("Set Current")).min_size(w)).clicked() {
                request(d, st, After::SetCurrent, actions, open);
            }
            if ui.add(egui::Button::new(crate::tl!("New...")).min_size(w)).clicked() {
                request(d, st, After::New, actions, open);
            }
            if ui.add_enabled(deletable(&st.selected, current, used), egui::Button::new(crate::tl!("Delete")).min_size(w)).clicked() {
                actions.push(("style.delete", json!({ "name": st.selected })));
                deleted = true;
            }
        });
    });
    // An edit made this frame, or none when it is back to the saved style (kept as it was when
    // another style was chosen or a question is waiting).
    if deleted {
        st.edit = None;
    } else if st.selected == before && st.confirm.is_none() && st.new_name.is_none() {
        st.edit = (v != *saved).then_some(v);
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.add_enabled(st.edit.is_some(), egui::Button::new(crate::tl!("Apply"))).clicked()
            && let Some(e) = st.edit.take()
        {
            actions.push(("style", apply_params(&saved.name, &e)));
        }
        if ui.button(crate::tl!("Close")).clicked() {
            request(d, st, After::Close, actions, open);
        }
    });
}

fn style_list(
    ui: &mut egui::Ui,
    d: &Drawing,
    st: &mut State,
    current: &str,
    used: &[String],
    actions: &mut Vec<(&'static str, Value)>,
    open: &mut bool,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("textstyle_list").max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
            for s in &d.text_styles {
                let is_current = s.name.eq_ignore_ascii_case(current);
                if st.in_use_only && !is_current && !used.iter().any(|n| n.eq_ignore_ascii_case(&s.name)) {
                    continue;
                }
                ui.horizontal(|ui| {
                    // Annotative styles are marked, as AutoCAD marks them.
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                    if s.annotative {
                        icons::paint(ui.painter(), r, Icon::Annotative, false);
                    }
                    let mut label = RichText::new(&s.name);
                    if is_current {
                        label = label.strong();
                    }
                    let resp = ui.selectable_label(st.selected == s.name, label);
                    if resp.clicked() && st.selected != s.name {
                        request(d, st, After::Select(s.name.clone()), actions, open);
                    }
                    resp.context_menu(|ui| {
                        if ui.add_enabled(!is_current, egui::Button::new(crate::tl!("Set Current"))).clicked() {
                            actions.push(("style.current", json!({ "name": s.name })));
                            ui.close();
                        }
                        if ui.add_enabled(!s.name.eq_ignore_ascii_case("Standard"), egui::Button::new(crate::tl!("Rename"))).clicked() {
                            st.renaming = Some((s.name.clone(), s.name.clone()));
                            ui.close();
                        }
                        if ui.add_enabled(deletable(&s.name, current, used), egui::Button::new(crate::tl!("Delete"))).clicked() {
                            actions.push(("style.delete", json!({ "name": s.name })));
                            ui.close();
                        }
                    });
                });
            }
        });
    });
    if let Some((from, to)) = &mut st.renaming {
        let from = from.clone();
        let mut done = false;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(to).desired_width(120.0));
            let name = to.trim().to_string();
            // Only a name the rename takes: a refused one would leave the selection on no style
            // (and drop the edits waiting for Apply).
            let usable = !bad_name(&name) && d.text_style(&name).is_none_or(|t| t.name.eq_ignore_ascii_case(&from));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(usable, egui::Button::new(crate::tl!("OK"))).clicked() || enter && usable {
                if name != from {
                    actions.push(("style.rename", json!({ "from": from, "to": name })));
                    if st.selected == from {
                        st.selected = name;
                    }
                }
                done = true;
            }
        });
        if done {
            st.renaming = None;
        }
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

fn font_group(ui: &mut egui::Ui, st: &mut State, v: &mut TextStyle, fonts: &[FontEntry]) {
    heading(ui, crate::tl!("Font"));
    let face = face_of(&v.font, fonts).cloned();
    let mut families: Vec<&str> = Vec::new();
    for e in fonts {
        if families.last() != Some(&e.family.as_str()) {
            families.push(&e.family);
        }
    }
    egui::Grid::new("textstyle_font").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        ui.label(crate::tl!("Font Name"));
        let shown = match &face {
            Some(f) => f.family.clone(),
            None if is_builtin(&v.font) => BUILTIN_FONT.to_string(),
            None => crate::tf!("{font} (not installed)", font = v.font),
        };
        egui::ComboBox::from_id_salt("textstyle_font_name")
            .selected_text(shown)
            .width(220.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut st.font_filter).hint_text(crate::tl!("Search fonts")).desired_width(200.0));
                let filter = st.font_filter.trim().to_lowercase();
                egui::ScrollArea::vertical().id_salt("textstyle_font_list").max_height(280.0).show(ui, |ui| {
                    if (filter.is_empty() || BUILTIN_FONT.to_lowercase().contains(&filter))
                        && ui.selectable_label(is_builtin(&v.font), BUILTIN_FONT).clicked()
                    {
                        v.font = BUILTIN_FONT.to_string();
                        ui.close();
                    }
                    for fam in families.iter().filter(|f| filter.is_empty() || f.to_lowercase().contains(&filter)) {
                        if ui.selectable_label(face.as_ref().is_some_and(|f| f.family == *fam), *fam).clicked() {
                            // The same style in the new family when it has one, else Regular.
                            let want = face.as_ref().map_or("Regular", |f| f.style.as_str());
                            let in_family = || fonts.iter().filter(|e| e.family == *fam);
                            let pick = in_family()
                                .find(|e| e.style.eq_ignore_ascii_case(want))
                                .or_else(|| in_family().find(|e| e.style.eq_ignore_ascii_case("Regular")))
                                .or_else(|| in_family().next());
                            if let Some(p) = pick {
                                v.font = p.file.clone();
                            }
                            ui.close();
                        }
                    }
                });
            });
        ui.end_row();
        ui.label(crate::tl!("Font Style"));
        match &face {
            Some(f) => {
                egui::ComboBox::from_id_salt("textstyle_font_style").selected_text(&f.style).width(220.0).show_ui(ui, |ui| {
                    for e in fonts.iter().filter(|e| e.family == f.family) {
                        if ui.selectable_label(e.style == f.style, &e.style).clicked() {
                            v.font = e.file.clone();
                        }
                    }
                });
            }
            None => {
                ui.add_enabled_ui(false, |ui| {
                    egui::ComboBox::from_id_salt("textstyle_font_style_off").selected_text(crate::tl!("Regular")).width(220.0).show_ui(ui, |_| {});
                });
            }
        }
        ui.end_row();
    });
    // A big font extends an SHX font with Asian characters.
    let shx = is_shx(&v.font);
    let mut use_big = shx && (!v.big_font.is_empty() || st.big_font_on);
    if ui.add_enabled(shx, egui::Checkbox::new(&mut use_big, crate::tl!("Use Big Font"))).changed() {
        st.big_font_on = use_big;
        if !use_big {
            v.big_font.clear();
        }
    }
    if use_big {
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Big Font"));
            ui.add(egui::TextEdit::singleline(&mut v.big_font).desired_width(160.0));
        });
    }
}

fn size_group(ui: &mut egui::Ui, d: &Drawing, v: &mut TextStyle) {
    heading(ui, crate::tl!("Size"));
    ui.checkbox(&mut v.annotative, crate::tl!("Annotative"));
    ui.add_enabled(false, egui::Checkbox::new(&mut false, crate::tl!("Match text orientation to layout")))
        .on_disabled_hover_text(crate::tl!("Not available yet: CADCraft doesn't keep this setting."));
    let (lu, lp) = (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4));
    ui.horizontal(|ui| {
        ui.label(if v.annotative { crate::tl!("Paper Text Height") } else { crate::tl!("Height") });
        let shown = cadcraft_engine::units::format_distance(v.height, lu, lp);
        if let Some(h) = expr_field(ui, egui::Id::new(("textstyle_height", &v.name)), &shown)
            .and_then(|t| cadcraft_engine::units::parse_distance(&t))
            .filter(|h| h.is_finite() && *h >= 0.0)
        {
            v.height = h;
        }
    })
    .response
    .on_hover_text(crate::tl!("0 asks for the height each time text is placed; a fixed height is used for all text in the style."));
}

/// A number as a field shows it: up to four decimals, without trailing zeros.
fn trimmed(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn effects_group(ui: &mut egui::Ui, v: &mut TextStyle) {
    heading(ui, crate::tl!("Effects"));
    egui::Grid::new("textstyle_effects").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
        ui.checkbox(&mut v.upside_down, crate::tl!("Upside down"));
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Width Factor"));
            if let Some(w) = expr_field(ui, egui::Id::new(("textstyle_width", &v.name)), &trimmed(v.width_factor))
                .and_then(|t| t.trim().parse::<f64>().ok())
                .filter(|w| w.is_finite() && *w > 0.0 && *w <= 100.0)
            {
                v.width_factor = w;
            }
        });
        ui.end_row();
        ui.checkbox(&mut v.backwards, crate::tl!("Backwards"));
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Oblique Angle"));
            if let Some(a) = expr_field(ui, egui::Id::new(("textstyle_oblique", &v.name)), &trimmed(v.oblique.to_degrees()))
                .and_then(|t| t.trim().parse::<f64>().ok())
                .filter(|a| a.is_finite() && a.abs() <= 85.0)
            {
                v.oblique = a.to_radians();
            }
        });
        ui.end_row();
        let mut vertical = v.vertical;
        ui.add_enabled(false, egui::Checkbox::new(&mut vertical, crate::tl!("Vertical")))
            .on_disabled_hover_text(crate::tl!("Vertical text isn't available yet."));
        ui.end_row();
    });
}

/// "AaBb123" set in `style`.
fn preview(ui: &mut egui::Ui, d: &Drawing, style: &TextStyle, size: egui::Vec2) {
    let (w, h) = (size.x.max(16.0) as u32, size.y.max(16.0) as u32);
    let mut hasher = std::hash::DefaultHasher::new();
    serde_json::to_string(style).unwrap_or_default().hash(&mut hasher);
    (w, h, ui.visuals().dark_mode).hash(&mut hasher);
    let key = hasher.finish();
    let id = egui::Id::new(("textstyle_preview", w, h));
    let cached = ui.data(|m| m.get_temp::<(u64, egui::TextureHandle)>(id)).filter(|(k, _)| *k == key).map(|(_, t)| t);
    let tex = match cached {
        Some(t) => Some(t),
        None => sample(d, style, w, h, ui.visuals().dark_mode).map(|img| {
            let t = ui.ctx().load_texture("textstyle_preview", img, egui::TextureOptions::LINEAR);
            ui.data_mut(|m| m.insert_temp(id, (key, t.clone())));
            t
        }),
    };
    match tex {
        Some(t) => {
            ui.image((t.id(), size));
        }
        None => {
            ui.allocate_space(size);
        }
    }
}

fn sample(d: &Drawing, style: &TextStyle, w: u32, h: u32, dark: bool) -> Option<egui::ColorImage> {
    let mut s = cadcraft_engine::Session::new();
    {
        let doc = s.doc_mut().ok()?;
        doc.text_styles = vec![TextStyle { name: "Preview".into(), ..style.clone() }];
        doc.linetypes.clone_from(&d.linetypes);
        let text = Text {
            insert: Default::default(),
            align_pt: None,
            height: 1.0,
            value: "AaBb123".into(),
            rotation: 0.0,
            width_factor: style.width_factor,
            oblique: style.oblique,
            style: "Preview".into(),
            halign: Default::default(),
            valign: Default::default(),
        };
        doc.add(&Space::Model, Common::default(), EntityKind::Text(text)).ok()?;
    }
    let doc = s.doc().ok()?;
    let list = cadcraft_engine::render::build(doc, &Space::Model, &cadcraft_engine::render::Options::default());
    let view = cadcraft_engine::render::raster::View::fit(&list.bounds, w, h, 0.12);
    let background = if dark { cadcraft_engine::render::raster::RasterOptions::default().background } else { cadcraft_color::Rgb(255, 255, 255) };
    let png = cadcraft_engine::render::raster::render_png(
        &list,
        &view,
        &cadcraft_engine::render::raster::RasterOptions { background, ..Default::default() },
    )?;
    let img = image::load_from_memory(&png).ok()?.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}
