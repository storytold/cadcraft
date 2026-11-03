//! Style and settings dialogs opened from the menus: Linetype Manager, Point Style, Text Style,
//! Table Style, Multileader Style, Page Setup Manager, Plot, Constraint Settings and Named
//! Viewports.
//!
//! Typed or chosen from a menu (no parameters) these commands open their dialog; JSON calls run
//! the command. The dialogs read the drawing and change it only through the same commands
//! (`app.run`), so every change is undoable and scriptable.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::{RichText, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::parametric::expr_field;
use crate::theme::Tokens;

/// The Block Attribute Manager (BATTMAN).
mod battman;
/// Named Viewports (View > Viewports > Named Viewports...).
mod named_vports;

/// Open the dialog for a typed or menu-invoked command. `None` if `id` has no dialog here or the
/// call carries parameters.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() {
        return None;
    }
    let dialog = match id {
        "battman" => "battman",
        "linetype" | "lt" | "ltype" => "linetype",
        "ddptype" | "ptype" => "ptype",
        "style" | "st" => "textstyle",
        "tablestyle" | "ts" => "tablestyle",
        "mleaderstyle" | "mls" => "mleaderstyle",
        "pagesetup" => "pagesetup",
        "constraintsettings" | "csettings" => "csettings",
        "vports.named" => "namedvports",
        "plot" | "print" => {
            PLOT_GEN.fetch_add(1, Ordering::Relaxed);
            "plot"
        }
        _ => return None,
    };
    app.ui.dialog = Some(dialog.into());
    Some(Ok(Value::Null))
}

/// Show the dialog named `name`; closes it (`open = false`) if it isn't one of these.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) {
    let action = match name {
        "battman" => battman::dialog(app, ctx, open),
        "linetype" => linetype(app, ctx, open),
        "ptype" => point_style(app, ctx, open),
        "textstyle" => text_style(app, ctx, open),
        "tablestyle" => table_style(app, ctx, open),
        "mleaderstyle" => mleader_style(app, ctx, open),
        "pagesetup" => page_setup(app, ctx, open),
        "csettings" => constraint_settings(app, ctx, open),
        "namedvports" => named_vports::dialog(app, ctx, open),
        "plot" => plot(app, ctx, open),
        _ => {
            *open = false;
            None
        }
    };
    if let Some((cmd, p)) = action {
        // Errors are echoed on the command line by `run`.
        let _ = app.run(cmd, p);
    }
}

/// A command to run after the dialog is drawn.
type Action = Option<(&'static str, Value)>;

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// A number field that commits on Enter or focus loss.
///
/// `label` is the visible label's widget id (see [`expr_field`]).
fn num_field(ui: &mut egui::Ui, id: impl egui::AsIdSalt, v: f64, label: Option<egui::Id>) -> Option<f64> {
    expr_field(ui, ui.make_persistent_id(id), &fmt_num(v), label).and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite())
}

fn temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(egui::Id::unique(id)))
}

fn set_temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(id), v));
}

fn no_drawing(ctx: &egui::Context, title: &str, open: &mut bool) {
    egui::Window::new(title).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label("Open a drawing first.");
    });
}

// ---------- Linetype Manager ----------

/// Linetype Manager: current linetype and global scale, loaded linetypes, and load/current actions; returns the command to run after drawing.
fn linetype(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Linetype Manager", open);
        return None;
    };
    let current = d.header.str("CELTYPE", "ByLayer");
    let ltscale = d.header.f64("LTSCALE", 1.0);
    let mut rows: Vec<(String, String, Vec<f64>)> =
        vec![("ByLayer".into(), String::new(), Vec::new()), ("ByBlock".into(), String::new(), Vec::new())];
    rows.extend(
        d.linetypes
            .iter()
            .filter(|l| !l.name.eq_ignore_ascii_case("ByLayer") && !l.name.eq_ignore_ascii_case("ByBlock"))
            .map(|l| (l.name.clone(), l.description.clone(), l.pattern.iter().map(|e| e.length).collect())),
    );
    let library: Vec<String> = cadcraft_doc::library::standard_linetypes().into_iter().map(|l| l.name).filter(|n| d.linetype(n).is_none()).collect();
    let mut sel = temp::<String>(ctx, "ltmgr_sel").filter(|s| rows.iter().any(|r| r.0.eq_ignore_ascii_case(s))).unwrap_or_else(|| current.clone());
    let mut action = None;
    egui::Window::new("Linetype Manager").open(open).default_size(vec2(520.0, 360.0)).resizable(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            craft_ui::ComboBox::from_id_salt("ltmgr_load").selected_text("Load...").width(110.0).show_ui(ui, |ui| {
                if library.is_empty() {
                    ui.label("Every library linetype is loaded.");
                } else if ui.selectable_label(false, "All").clicked() {
                    action = Some(("linetype", json!({ "load": "*" })));
                }
                for n in &library {
                    if ui.selectable_label(false, n).clicked() {
                        action = Some(("linetype", json!({ "load": n })));
                    }
                }
                // The user's own .lin file (where the host has a file picker).
                if let Some(pick) = app.services.pick_open.as_ref() {
                    ui.separator();
                    if ui.selectable_label(false, "From File...").clicked()
                        && let Some(path) = pick()
                    {
                        action = Some(("linetype", json!({ "load": "*", "file": path })));
                    }
                }
            });
            let is_current = sel.eq_ignore_ascii_case(&current);
            if ui.add_enabled(!is_current, egui::Button::new("Current")).on_hover_text("Draw new objects with the selected linetype").clicked() {
                action = Some(("linetype", json!({ "current": sel })));
            }
        });
        ui.label(RichText::new(format!("Current linetype: {current}")).color(t.text_dim));
        ui.separator();
        egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("ltmgr_grid").striped(true).num_columns(3).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                for h in ["Linetype", "Appearance", "Description"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (name, desc, pattern) in &rows {
                    let mut label = RichText::new(name);
                    if name.eq_ignore_ascii_case(&current) {
                        label = label.strong();
                    }
                    let r = ui.selectable_label(sel.eq_ignore_ascii_case(name), label);
                    if r.clicked() {
                        sel = name.clone();
                    }
                    if r.double_clicked() {
                        action = Some(("linetype", json!({ "current": name })));
                    }
                    let (rect, _) = ui.allocate_exact_size(vec2(120.0, 14.0), Sense::hover());
                    if !matches!(name.as_str(), "ByLayer" | "ByBlock") {
                        draw_pattern(ui.painter(), rect, pattern, Stroke::new(1.5, t.text));
                    }
                    ui.label(RichText::new(desc).color(t.text_dim));
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            let label = ui.label("Global scale factor");
            if let Some(v) = num_field(ui, "ltmgr_scale", ltscale, Some(label.id)).filter(|v| *v > 0.0) {
                action = Some(("ltscale", json!({ "scale": v })));
            }
        });
    });
    set_temp(ctx, "ltmgr_sel", sel);
    action
}

/// A linetype sample: dashes (positive lengths), gaps (negative) and dots (zero), two repeats.
fn draw_pattern(p: &egui::Painter, rect: egui::Rect, pattern: &[f64], stroke: Stroke) {
    let y = rect.center().y;
    let total: f64 = pattern.iter().map(|l| l.abs()).sum();
    if pattern.is_empty() || !total.is_finite() || total <= 0.0 {
        p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], stroke);
        return;
    }
    let k = f64::from(rect.width()) / (total * 2.0);
    let mut x = f64::from(rect.left());
    for l in pattern.iter().cycle().take(400) {
        if x >= f64::from(rect.right()) {
            break;
        }
        let len = l.abs() * k;
        if *l > 0.0 {
            let end = (x + len).min(f64::from(rect.right()));
            p.line_segment([egui::pos2(x as f32, y), egui::pos2(end as f32, y)], stroke);
        } else if *l == 0.0 {
            p.circle_filled(egui::pos2(x as f32, y), stroke.width, stroke.color);
        }
        x += len.max(1.0);
    }
}

// ---------- Point Style ----------

/// PDMODE values in dialog order: four rows of five.
const PDMODES: [i64; 20] = [0, 1, 2, 3, 4, 32, 33, 34, 35, 36, 64, 65, 66, 67, 68, 96, 97, 98, 99, 100];

/// The point glyph for a PDMODE, as drawn by the point style tiles.
fn draw_point(p: &egui::Painter, c: egui::Pos2, r: f32, mode: i64, stroke: Stroke) {
    let line = |a: egui::Vec2, b: egui::Vec2| p.line_segment([c + a, c + b], stroke);
    match mode & 7 {
        0 => {
            p.circle_filled(c, stroke.width, stroke.color);
        }
        2 => {
            line(vec2(-r, 0.0), vec2(r, 0.0));
            line(vec2(0.0, -r), vec2(0.0, r));
        }
        3 => {
            line(vec2(-r, -r), vec2(r, r));
            line(vec2(-r, r), vec2(r, -r));
        }
        4 => {
            line(vec2(0.0, 0.0), vec2(0.0, -r));
        }
        _ => {}
    }
    if mode & 32 != 0 {
        p.circle_stroke(c, r * 0.7, stroke);
    }
    if mode & 64 != 0 {
        p.rect_stroke(egui::Rect::from_center_size(c, vec2(r * 1.4, r * 1.4)), 0.0, stroke, egui::StrokeKind::Middle);
    }
}

/// Point Style dialog for the PDMODE/PDSIZE presets; returns the `ddptype` action.
fn point_style(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Point Style", open);
        return None;
    };
    let (mode, pdsize) = (d.header.i64("PDMODE", 0), d.header.f64("PDSIZE", 0.0));
    // PDSIZE 0 is 5% of the screen, negative a percentage of the screen, positive drawing units.
    let rel = pdsize <= 0.0;
    let size = if pdsize == 0.0 { 5.0 } else { pdsize.abs() };
    let set = |m: i64, size: f64, rel: bool| Some(("ddptype", json!({ "pdmode": m, "pdsize": if rel { -size } else { size } })));
    let mut action = None;
    egui::Window::new("Point Style").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("ptype_grid").spacing(vec2(6.0, 6.0)).show(ui, |ui| {
            for (i, v) in PDMODES.iter().enumerate() {
                let (rect, r) = ui.allocate_exact_size(vec2(44.0, 36.0), Sense::click());
                let fill = if *v == mode {
                    t.accent
                } else if r.hovered() {
                    t.control_hover
                } else {
                    t.control
                };
                ui.painter().rect_filled(rect, 3.0, fill);
                draw_point(ui.painter(), rect.center(), 9.0, *v, Stroke::new(1.5, t.text));
                if r.on_hover_text(format!("PDMODE {v}")).clicked() && *v != mode {
                    action = set(*v, size, rel);
                }
                if i % 5 == 4 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let label = ui.label("Point Size");
            if let Some(v) = num_field(ui, "ptype_size", size, Some(label.id)).filter(|v| *v > 0.0) {
                action = set(mode, v, rel);
            }
            ui.label(if rel { "%" } else { "units" });
        });
        if ui.radio(rel, "Set Size Relative to Screen").clicked() && !rel {
            action = set(mode, size, true);
        }
        if ui.radio(!rel, "Set Size in Absolute Units").clicked() && rel {
            action = set(mode, size, false);
        }
    });
    action
}

// ---------- Text Style, Table Style and Multileader Style ----------

/// One editable style property.
enum Field {
    Num(&'static str, &'static str, f64),
    Bool(&'static str, &'static str, bool),
    Choice(&'static str, &'static str, String, Vec<String>),
}

struct Style {
    name: String,
    fields: Vec<Field>,
}

fn text_style_is_used(d: &cadcraft_doc::Drawing, name: &str) -> bool {
    use cadcraft_doc::EntityKind;
    if d.dim_styles.iter().any(|s| s.text_style.eq_ignore_ascii_case(name))
        || d.mleader_styles.iter().any(|s| s.text_style.eq_ignore_ascii_case(name))
    {
        return true;
    }
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for st in stores {
        for e in st.iter() {
            let used = match &e.kind {
                EntityKind::Text(t) => t.style.eq_ignore_ascii_case(name),
                EntityKind::MText(t) => t.style.eq_ignore_ascii_case(name),
                EntityKind::AttDef(a) => a.text.style.eq_ignore_ascii_case(name),
                EntityKind::Insert(i) => i.attribs.iter().any(|a| a.text.style.eq_ignore_ascii_case(name)),
                EntityKind::MLeader(m) => m.text.iter().any(|t| t.style.eq_ignore_ascii_case(name)),
                _ => false,
            };
            if used {
                return true;
            }
        }
    }
    false
}

fn draw_text_style_preview(p: &egui::Painter, rect: egui::Rect, style: &cadcraft_doc::TextStyle, t: &Tokens) {
    p.rect_filled(rect, 2.0, t.control);
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let font = cadcraft_fonts::TextFont::resolve(&style.font);
    let params = cadcraft_fonts::TextParams {
        height: 1.0,
        width_factor: style.width_factor.clamp(0.1, 10.0),
        oblique: style.oblique,
        backwards: style.backwards,
        upside_down: style.upside_down,
        ..cadcraft_fonts::TextParams::new(cadcraft_geom::Vec2::ZERO, 1.0)
    };
    let (sh, bb) = cadcraft_fonts::place(&font, "AaBb123", &params);
    if bb.is_empty() || bb.width() <= 0.0 || bb.height() <= 0.0 {
        return;
    }
    let inner = rect.shrink2(vec2(10.0, 10.0));
    if inner.width() <= 4.0 || inner.height() <= 4.0 {
        return;
    }
    let scale = (f64::from(inner.width()) / bb.width()).min(f64::from(inner.height()) / bb.height()).min(36.0);
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let center = bb.center();
    let to_screen = |pt: cadcraft_geom::Vec2| -> egui::Pos2 {
        egui::pos2(inner.center().x + ((pt.x - center.x) * scale) as f32, inner.center().y - ((pt.y - center.y) * scale) as f32)
    };
    let clipped = p.with_clip_rect(rect.shrink(2.0));
    let stroke = Stroke::new(1.4, t.text);
    for s in &sh.strokes {
        for pair in s.windows(2) {
            if let [a, b] = pair {
                clipped.line_segment([to_screen(*a), to_screen(*b)], stroke);
            }
        }
    }
    if !sh.glyphs.is_empty() {
        let mut mesh = egui::Mesh::default();
        for g in &sh.glyphs {
            let tris = cadcraft_render::triangulate_evenodd(g);
            for tri in tris.as_chunks::<3>().0 {
                let Ok(base) = u32::try_from(mesh.vertices.len()) else { break };
                for &pt in tri {
                    mesh.vertices.push(egui::epaint::Vertex { pos: to_screen(pt), uv: egui::epaint::WHITE_UV, color: t.text });
                }
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
        if !mesh.indices.is_empty() {
            clipped.add(egui::Shape::mesh(mesh));
        }
    }
}

fn text_style(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Text Style", open);
        return None;
    };
    let available = cadcraft_fonts::ttf::available_fonts();
    let mut all_styles: Vec<cadcraft_doc::TextStyle> = d.text_styles.clone();
    if all_styles.is_empty() {
        all_styles.push(cadcraft_doc::TextStyle::default());
    }
    let current = d.header.str("TEXTSTYLE", "Standard");
    let sel_key = "style_sel";
    let new_key = "style_new";
    let filter_key = "style_filter_in_use";
    let mut sel =
        temp::<String>(ctx, sel_key).filter(|s| all_styles.iter().any(|x| x.name.eq_ignore_ascii_case(s))).unwrap_or_else(|| current.clone());
    let mut new_name = temp::<String>(ctx, new_key).unwrap_or_default();
    let mut only_in_use = temp::<bool>(ctx, filter_key).unwrap_or(false);
    let mut action = None;
    let mut close = false;

    egui::Window::new("Text Style").open(open).default_size(vec2(610.0, 350.0)).resizable(true).show(ctx, |ui| {
        ui.label(RichText::new(format!("Current text style: {current}")).color(t.text_dim));
        ui.add_space(4.0);
        let selected = all_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&sel)).or_else(|| all_styles.first()).cloned().unwrap_or_default();
        sel = selected.name.clone();

        let trimmed = selected.font.trim();
        let clean_font = if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("txt")
            || trimmed.len() > 128
            || !trimmed.chars().all(|c| c.is_ascii_graphic() || c == ' ')
        {
            cadcraft_fonts::BUILTIN_FONT.to_string()
        } else {
            trimmed.to_string()
        };
        let described = cadcraft_fonts::ttf::describe_font(&clean_font);
        let is_ttf = described.is_some() || cadcraft_fonts::ttf::find(&clean_font).is_some();
        let (family_display, style_display, style_choices) = match described {
            Some((fam, st, choices)) => (fam, st, choices),
            None => (clean_font.clone(), "Regular".to_string(), vec!["Regular".to_string()]),
        };
        let mut fonts = available.clone();
        if !fonts.iter().any(|f| f.eq_ignore_ascii_case(&family_display)) {
            fonts.insert(0, family_display.clone());
        }

        ui.horizontal_top(|ui| {
            // Left column: Styles list, filter dropdown, and live Preview box.
            ui.vertical(|ui| {
                ui.set_width(155.0);
                ui.label("Styles:");
                egui::Frame::group(ui.style()).inner_margin(4.0).show(ui, |ui| {
                    ui.set_width(145.0);
                    egui::ScrollArea::vertical()
                        .id_salt("textstyle_list")
                        .max_height(130.0)
                        .min_scrolled_height(115.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for s in &all_styles {
                                if only_in_use
                                    && !s.name.eq_ignore_ascii_case(&current)
                                    && !s.name.eq_ignore_ascii_case(&sel)
                                    && !text_style_is_used(d, &s.name)
                                {
                                    continue;
                                }
                                let mut label = RichText::new(&s.name);
                                if s.name.eq_ignore_ascii_case(&current) {
                                    label = label.strong();
                                }
                                let r = ui.selectable_label(s.name.eq_ignore_ascii_case(&sel), label);
                                if r.clicked() {
                                    sel = s.name.clone();
                                }
                                if r.double_clicked() {
                                    action = Some(("style.current", json!({ "name": s.name })));
                                }
                            }
                        });
                });
                ui.add_space(4.0);
                let filter_text = if only_in_use { "Styles in use" } else { "All styles" };
                let filter_combo = craft_ui::ComboBox::from_id_salt("textstyle_filter").selected_text(filter_text).width(155.0).show_ui(ui, |ui| {
                    if ui.selectable_label(!only_in_use, "All styles").clicked() {
                        only_in_use = false;
                    }
                    if ui.selectable_label(only_in_use, "Styles in use").clicked() {
                        only_in_use = true;
                    }
                });
                filter_combo.response.accessible_name("Style filter");
                ui.add_space(4.0);
                ui.label(RichText::new("Preview").color(t.text_dim));
                let (rect, _) = ui.allocate_exact_size(vec2(155.0, 68.0), Sense::hover());
                draw_text_style_preview(ui.painter(), rect, &selected, &t);
            });

            ui.add_space(6.0);

            // Middle column: Font, Size, and Effects groups.
            ui.vertical(|ui| {
                ui.set_width(310.0);
                egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                    ui.set_width(296.0);
                    ui.label(RichText::new("Font").strong());
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            let fn_lbl = ui.label("Font Name:");
                            let mut picked_fam = None;
                            let combo = craft_ui::ComboBox::from_id_salt("textstyle_font_name")
                                .selected_text(family_display.as_str())
                                .width(175.0)
                                .show_ui(ui, |ui| {
                                    for c in &fonts {
                                        let same = c.eq_ignore_ascii_case(&family_display);
                                        if ui.selectable_label(same, c).clicked() && !same {
                                            picked_fam = Some(c.clone());
                                        }
                                    }
                                });
                            combo.response.labelled_by(fn_lbl.id);
                            if let Some(fam) = picked_fam {
                                let next_font = match cadcraft_fonts::ttf::describe_font(&fam) {
                                    Some((canon_fam, _, styles)) => {
                                        let keep = styles
                                            .iter()
                                            .find(|s| s.eq_ignore_ascii_case(&style_display))
                                            .cloned()
                                            .unwrap_or_else(|| "Regular".to_string());
                                        cadcraft_fonts::ttf::compose_font(&canon_fam, &keep)
                                    }
                                    None => fam,
                                };
                                action = Some(("style", json!({ "name": selected.name, "font": next_font, "current": false })));
                            }
                        });
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            let fs_lbl = ui.label("Font Style:");
                            let mut picked_style = None;
                            ui.add_enabled_ui(is_ttf && style_choices.len() > 1, |ui| {
                                let combo = craft_ui::ComboBox::from_id_salt("textstyle_font_style")
                                    .selected_text(style_display.as_str())
                                    .width(105.0)
                                    .show_ui(ui, |ui| {
                                        for st in &style_choices {
                                            let same = st.eq_ignore_ascii_case(&style_display);
                                            if ui.selectable_label(same, st).clicked() && !same {
                                                picked_style = Some(st.clone());
                                            }
                                        }
                                    });
                                combo.response.labelled_by(fs_lbl.id);
                            });
                            if let Some(st) = picked_style {
                                let next_font = cadcraft_fonts::ttf::compose_font(&family_display, &st);
                                action = Some(("style", json!({ "name": selected.name, "font": next_font, "current": false })));
                            }
                        });
                    });
                    let mut use_big = !selected.big_font.trim().is_empty();
                    if ui.add_enabled(!is_ttf, egui::Checkbox::new(&mut use_big, "Use Big Font")).changed() {
                        let big = if use_big { "bigfont.shx" } else { "" };
                        action = Some(("style", json!({ "name": selected.name, "bigFont": big, "current": false })));
                    }
                });

                ui.add_space(6.0);

                egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                    ui.set_width(296.0);
                    ui.label(RichText::new("Size").strong());
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(175.0);
                            let mut annotative = selected.annotative;
                            if ui.checkbox(&mut annotative, "Annotative").changed() {
                                action = Some(("style", json!({ "name": selected.name, "annotative": annotative, "current": false })));
                            }
                            let mut match_orient = false;
                            ui.add_enabled(false, egui::Checkbox::new(&mut match_orient, "Match text orientation to layout"));
                        });
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            let h_lbl = ui.label("Height");
                            if let Some(v) = num_field(ui, ("style", &selected.name, "height"), selected.height, Some(h_lbl.id)).filter(|v| *v >= 0.0)
                            {
                                action = Some(("style", json!({ "name": selected.name, "height": v, "current": false })));
                            }
                        });
                    });
                });

                ui.add_space(6.0);

                egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                    ui.set_width(296.0);
                    ui.label(RichText::new("Effects").strong());
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(175.0);
                            let mut upside_down = selected.upside_down;
                            if ui.checkbox(&mut upside_down, "Upside down").changed() {
                                action = Some(("style", json!({ "name": selected.name, "upsideDown": upside_down, "current": false })));
                            }
                            let mut backwards = selected.backwards;
                            if ui.checkbox(&mut backwards, "Backwards").changed() {
                                action = Some(("style", json!({ "name": selected.name, "backwards": backwards, "current": false })));
                            }
                            let mut vertical = selected.vertical;
                            if ui.add_enabled(!is_ttf, egui::Checkbox::new(&mut vertical, "Vertical")).changed() {
                                action = Some(("style", json!({ "name": selected.name, "vertical": vertical, "current": false })));
                            }
                        });
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            let wf_lbl = ui.label("Width Factor:");
                            if let Some(v) = num_field(ui, ("style", &selected.name, "widthFactor"), selected.width_factor, Some(wf_lbl.id))
                                .filter(|v| *v > 0.0 && *v <= 100.0)
                            {
                                action = Some(("style", json!({ "name": selected.name, "widthFactor": v, "current": false })));
                            }
                            let ob_lbl = ui.label("Oblique Angle:");
                            let ob_deg = selected.oblique.to_degrees();
                            if let Some(v) = num_field(ui, ("style", &selected.name, "oblique"), ob_deg, Some(ob_lbl.id)).filter(|v| v.abs() <= 85.0)
                            {
                                action = Some(("style", json!({ "name": selected.name, "oblique": v, "current": false })));
                            }
                        });
                    });
                });
            });

            ui.add_space(6.0);

            // Right column: Set Current, New, and Delete actions.
            ui.vertical(|ui| {
                ui.set_width(95.0);
                ui.add_space(18.0);
                let is_cur = selected.name.eq_ignore_ascii_case(&current);
                if ui.add_enabled(!is_cur, egui::Button::new("Set Current").min_size(vec2(90.0, 0.0))).clicked() {
                    action = Some(("style.current", json!({ "name": selected.name })));
                }
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut new_name).hint_text("New style name").desired_width(90.0))
                    .accessible_name("New style name")
                    .on_hover_text("New style name");
                let candidate = new_name.trim().to_string();
                let can_new = !candidate.is_empty() && !all_styles.iter().any(|s| s.name.eq_ignore_ascii_case(&candidate));
                if ui.add_enabled(can_new, egui::Button::new("New...").min_size(vec2(90.0, 0.0))).clicked() {
                    action = Some(("style", json!({ "name": candidate, "current": false })));
                    sel = candidate;
                    new_name.clear();
                }
                ui.add_space(6.0);
                let can_delete = !selected.name.eq_ignore_ascii_case("Standard")
                    && !selected.name.eq_ignore_ascii_case(&current)
                    && !text_style_is_used(d, &selected.name);
                if ui.add_enabled(can_delete, egui::Button::new("Delete").min_size(vec2(90.0, 0.0))).clicked() {
                    action = Some(("style.delete", json!({ "name": selected.name })));
                    sel = current.clone();
                }
            });
        });

        ui.add_space(6.0);
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });

    if close {
        *open = false;
    }
    set_temp(ctx, sel_key, sel);
    set_temp(ctx, new_key, new_name);
    set_temp(ctx, filter_key, only_in_use);
    action
}

fn table_style(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Table Style", open);
        return None;
    };
    let mut styles: Vec<Style> = d
        .table_styles
        .iter()
        .map(|s| Style {
            name: s.name.clone(),
            fields: vec![
                Field::Num("textHeight", "Text height", s.text_height),
                Field::Num("margin", "Cell margin", s.margin),
                Field::Bool("title", "Title row", s.title),
                Field::Bool("header", "Header row", s.header),
            ],
        })
        .collect();
    if styles.is_empty() {
        let s = cadcraft_doc::TableStyle::default();
        styles.push(Style { name: s.name, fields: Vec::new() });
    }
    let current = d.header.str("CTABLESTYLE", "Standard");
    style_manager(ctx, open, "Table Style", "tablestyle", &current, &styles)
}

fn mleader_style(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Multileader Style", open);
        return None;
    };
    let text_styles: Vec<String> = d.text_styles.iter().map(|s| s.name.clone()).collect();
    let mut styles: Vec<Style> = d
        .mleader_styles
        .iter()
        .map(|s| Style {
            name: s.name.clone(),
            fields: vec![
                Field::Num("arrowSize", "Arrowhead size", s.arrow_size),
                Field::Num("textHeight", "Text height", s.text_height),
                Field::Num("landingGap", "Landing gap", s.landing_gap),
                Field::Num("dogleg", "Landing length", s.dogleg),
                Field::Choice("textStyle", "Text style", s.text_style.clone(), text_styles.clone()),
            ],
        })
        .collect();
    if styles.is_empty() {
        let s = cadcraft_doc::MLeaderStyle::default();
        styles.push(Style { name: s.name, fields: Vec::new() });
    }
    let current = d.header.str("CMLEADERSTYLE", "Standard");
    style_manager(ctx, open, "Multileader Style", "mleaderstyle", &current, &styles)
}

/// A style list (set current, new) beside the selected style's properties. `cmd` takes
/// `{name, <field>: value, current?}`.
fn style_manager(ctx: &egui::Context, open: &mut bool, title: &str, cmd: &'static str, current: &str, styles: &[Style]) -> Action {
    let t = Tokens::get();
    let sel_key = format!("{cmd}_sel");
    let new_key = format!("{cmd}_new");
    let mut sel = temp::<String>(ctx, &sel_key).filter(|s| styles.iter().any(|x| x.name == *s)).unwrap_or_else(|| current.to_string());
    let mut new_name = temp::<String>(ctx, &new_key).unwrap_or_default();
    let mut action = None;
    egui::Window::new(title).open(open).default_size(vec2(520.0, 280.0)).resizable(true).show(ctx, |ui| {
        ui.label(RichText::new(format!("Current style: {current}")).color(t.text_dim));
        ui.separator();
        ui.columns(2, |cols| {
            let [left, right] = cols else { return };
            left.label(RichText::new("Styles").strong());
            egui::ScrollArea::vertical().id_salt((cmd, "list")).max_height(160.0).auto_shrink([false, true]).show(left, |ui| {
                for s in styles {
                    let mut label = RichText::new(&s.name);
                    if s.name.eq_ignore_ascii_case(current) {
                        label = label.strong();
                    }
                    if ui.selectable_label(s.name == sel, label).clicked() {
                        sel = s.name.clone();
                    }
                }
            });
            if left.add_enabled(!sel.eq_ignore_ascii_case(current), egui::Button::new("Set Current")).clicked() {
                action = Some((cmd, json!({ "name": sel })));
            }
            left.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut new_name).hint_text("New style name").desired_width(120.0))
                    .accessible_name("New style name")
                    .on_hover_text("New style name");
                let name = new_name.trim().to_string();
                let ok = !name.is_empty() && !styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name));
                if ui.add_enabled(ok, egui::Button::new("New")).clicked() {
                    action = Some((cmd, json!({ "name": name, "current": false })));
                    sel = name;
                    new_name.clear();
                }
            });
            right.label(RichText::new(format!("Properties of {sel}")).strong());
            let Some(style) = styles.iter().find(|s| s.name == sel) else { return };
            egui::Grid::new((cmd, "props")).num_columns(2).spacing(vec2(10.0, 6.0)).show(right, |ui| {
                for f in &style.fields {
                    let change = match f {
                        Field::Num(key, label, v) => {
                            let field_label = ui.label(*label);
                            num_field(ui, (cmd, &style.name, *key), *v, Some(field_label.id)).filter(|v| *v >= 0.0).map(|v| (*key, json!(v)))
                        }
                        Field::Bool(key, label, v) => {
                            ui.label("");
                            let mut b = *v;
                            ui.checkbox(&mut b, *label).changed().then(|| (*key, json!(b)))
                        }
                        Field::Choice(key, label, v, choices) => {
                            let field_label = ui.label(*label);
                            let mut picked = None;
                            let combo = craft_ui::ComboBox::from_id_salt((cmd, *key)).selected_text(v.as_str()).show_ui(ui, |ui| {
                                for c in choices {
                                    let same = c.eq_ignore_ascii_case(v);
                                    if ui.selectable_label(same, c).clicked() && !same {
                                        picked = Some(c.clone());
                                    }
                                }
                            });
                            combo.response.labelled_by(field_label.id);
                            picked.map(|c| (*key, json!(c)))
                        }
                    };
                    if let Some((key, v)) = change {
                        action = Some((cmd, json!({ "name": style.name, key: v, "current": false })));
                    }
                    ui.end_row();
                }
            });
        });
    });
    set_temp(ctx, &sel_key, sel);
    set_temp(ctx, &new_key, new_name);
    action
}

// ---------- Page Setup Manager ----------

/// Page Setup Manager over layouts sorted by tab order; returns the selected setup command.
fn page_setup(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Page Setup Manager", open);
        return None;
    };
    let mut layouts: Vec<(u32, String, cadcraft_doc::PageSetup)> = d.layouts.iter().map(|l| (l.tab_order, l.name.clone(), l.page.clone())).collect();
    layouts.sort_by_key(|l| l.0);
    let shown = app.session.layout_space().layout().map(str::to_string);
    let mut sel = temp::<String>(ctx, "pagesetup_sel")
        .filter(|s| layouts.iter().any(|l| l.1 == *s))
        .or(shown)
        .or_else(|| layouts.first().map(|l| l.1.clone()))
        .unwrap_or_default();
    let mut action = None;
    let choices = PageChoices::new(ctx, d);
    egui::Window::new("Page Setup Manager").open(open).default_size(vec2(560.0, 300.0)).resizable(true).show(ctx, |ui| {
        if layouts.is_empty() {
            ui.label("This drawing has no layouts.");
            return;
        }
        ui.columns(2, |cols| {
            let [left, right] = cols else { return };
            left.label(RichText::new("Layouts").strong());
            egui::ScrollArea::vertical().id_salt("pagesetup_list").max_height(200.0).auto_shrink([false, true]).show(left, |ui| {
                for (_, name, _) in &layouts {
                    if ui.selectable_label(*name == sel, name).clicked() {
                        sel = name.clone();
                    }
                }
            });
            let Some((_, name, page)) = layouts.iter().find(|l| l.1 == sel) else { return };
            right.label(RichText::new(format!("Page setup of {name}")).strong());
            // Layout plotting always prints the whole sheet, so only that area is offered.
            let rows = PageRows { areas: &["layout"], scale: true, center: true };
            egui::Grid::new("pagesetup_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(right, |ui| {
                if let Some((k, v)) = page_rows(ui, egui::Id::unique(("pagesetup", name)), page, &rows, &choices, None) {
                    action = Some(("pagesetup", json!({ "layout": name, k: v })));
                }
            });
            let (w, h) = if page.landscape { (page.height_mm, page.width_mm) } else { (page.width_mm, page.height_mm) };
            right.label(RichText::new(format!("{w:.1} × {h:.1} mm")).color(t.text_dim));
        });
    });
    set_temp(ctx, "pagesetup_sel", sel);
    action
}

/// Which page-setup rows a dialog shows.
struct PageRows {
    /// Plot area choices (`plotArea` values).
    areas: &'static [&'static str],
    scale: bool,
    center: bool,
}

/// The devices and plot style tables the page-setup rows offer.
struct PageChoices {
    devices: std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>>,
    styles: Vec<String>,
}

impl PageChoices {
    fn new(ctx: &egui::Context, d: &cadcraft_doc::Drawing) -> Self {
        PageChoices { devices: device_choices(ctx), styles: cadcraft_doc::plot_style_table_names(d) }
    }
}

/// The plotter devices (built-in, then the user's files), read again at most every 2 s: the
/// dialogs ask every frame and user devices are files.
fn device_choices(ctx: &egui::Context) -> std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>> {
    type Cached = (f64, std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>>);
    let now = crate::now_ms();
    if let Some((at, list)) = temp::<Cached>(ctx, "plot_devices")
        && now - at < 2000.0
    {
        return list;
    }
    let list = std::sync::Arc::new(cadcraft_engine::cmd::plotters::devices());
    set_temp::<Cached>(ctx, "plot_devices", (now, list.clone()));
    list
}

/// A page setup's device, or `"None"` (plot to PDF on the page's paper).
fn device_label(page: &cadcraft_doc::PageSetup) -> &str {
    if page.device.trim().is_empty() { "None" } else { page.device.as_str() }
}

/// A page setup's plot style table, or `"None"`.
fn style_label(page: &cadcraft_doc::PageSetup) -> &str {
    if cadcraft_doc::no_plot_style_table(&page.plot_style_table) { "None" } else { page.plot_style_table.as_str() }
}

fn area_label(a: &str) -> &str {
    match a {
        "layout" => "Layout",
        "extents" => "Extents",
        "display" => "Display",
        "limits" => "Limits",
        "window" => "Window",
        other => other,
    }
}

/// A plot scale typed as a number (paper units per drawing unit) or a ratio such as `1:50`.
fn parse_ratio(t: &str) -> Option<f64> {
    let v = match t.trim().split_once([':', '/']) {
        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
        None => t.trim().trim_end_matches(['x', 'X']).parse().ok()?,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

/// The page-setup rows (device, paper, orientation, plot area, fit, scale, center, lineweights,
/// transparency, plot style table) shared by the Page Setup Manager and the Plot dialog, inside a
/// two-column grid. Returns the setting the user changed as a `pagesetup` parameter (`device`,
/// `paper`, `landscape`, `plotArea`, `scaleToFit`, `scale`, `center`, `lineweights`,
/// `plotTransparency` or `plotStyleTable`).
fn page_rows(
    ui: &mut egui::Ui,
    salt: egui::Id,
    page: &cadcraft_doc::PageSetup,
    rows: &PageRows,
    choices: &PageChoices,
    placement: Option<(&mut bool, &mut [f64; 2], bool)>,
) -> Option<(&'static str, Value)> {
    let mut change = None;
    ui.label("Printer/plotter");
    craft_ui::ComboBox::from_id_salt(salt.with("device")).selected_text(device_label(page)).width(190.0).show_ui(ui, |ui| {
        if ui.selectable_label(device_label(page) == "None", "None").on_hover_text("PDF on the paper chosen below").clicked()
            && device_label(page) != "None"
        {
            change = Some(("device", json!("None")));
        }
        for dev in choices.devices.iter() {
            let on = dev.name.eq_ignore_ascii_case(&page.device);
            if ui.selectable_label(on, &dev.name).on_hover_text(format!("{} ({})", dev.description, dev.output.as_str().to_uppercase())).clicked()
                && !on
            {
                change = Some(("device", json!(dev.name)));
            }
        }
    });
    ui.end_row();
    // The device's papers, else the standard sizes.
    let device = choices.devices.iter().find(|dv| dv.name.eq_ignore_ascii_case(&page.device));
    ui.label("Paper size");
    craft_ui::ComboBox::from_id_salt(salt.with("paper")).selected_text(page.paper.as_str()).width(190.0).show_ui(ui, |ui| match device {
        Some(dev) => {
            for p in &dev.papers {
                if ui.selectable_label(p.name == page.paper, &p.name).clicked() && p.name != page.paper {
                    change = Some(("paper", json!(p.name)));
                }
            }
        }
        None => {
            for p in cadcraft_render::PAPER_SIZES {
                if ui.selectable_label(p.name == page.paper, p.name).clicked() && p.name != page.paper {
                    change = Some(("paper", json!(p.name)));
                }
            }
        }
    });
    ui.end_row();
    ui.label("Orientation");
    ui.horizontal(|ui| {
        if ui.radio(!page.landscape, "Portrait").clicked() && page.landscape {
            change = Some(("landscape", json!(false)));
        }
        if ui.radio(page.landscape, "Landscape").clicked() && !page.landscape {
            change = Some(("landscape", json!(true)));
        }
    });
    ui.end_row();
    ui.label("Plot area");
    ui.add_enabled_ui(rows.areas.len() > 1, |ui| {
        craft_ui::ComboBox::from_id_salt(salt.with("area")).selected_text(area_label(&page.plot_area)).show_ui(ui, |ui| {
            for a in rows.areas {
                if ui.selectable_label(page.plot_area == *a, area_label(a)).clicked() && page.plot_area != *a {
                    change = Some(("plotArea", json!(a)));
                }
            }
        });
    });
    ui.end_row();
    ui.label("");
    let mut fit = page.scale_to_fit;
    if ui.checkbox(&mut fit, "Fit to paper").changed() {
        change = Some(("scaleToFit", json!(fit)));
    }
    ui.end_row();
    if let Some((center, offset_mm, supported)) = placement {
        ui.label("Plot offset");
        ui.add_enabled_ui(supported, |ui| ui.checkbox(center, "Center the plot")).response.on_hover_text(if supported {
            "Center the plotted model bounds on the printable area"
        } else {
            "Model-space offset placement is not supported by SVG"
        });
        ui.end_row();
        for (i, label) in [(0, "X offset (mm)"), (1, "Y offset (mm)")] {
            let field_label = ui.label(label);
            ui.add_enabled_ui(supported && !*center, |ui| {
                if let Some(v) = num_field(ui, ("plot_offset", i), offset_mm[i], Some(field_label.id)) {
                    offset_mm[i] = v;
                }
            })
            .response
            .on_hover_text("Distance from the printable area's lower-left corner; values range from −100000 to 100000 mm");
            ui.end_row();
        }
        if supported && !*center && offset_mm.iter().any(|v| !v.is_finite() || v.abs() > cadcraft_render::paper::MAX_PLOT_OFFSET_MM) {
            ui.label("");
            ui.colored_label(Tokens::get().warn, "Offsets must be within ±100000 mm");
            ui.end_row();
        }
    }
    if rows.scale {
        let label = ui.label("Scale");
        ui.add_enabled_ui(!page.scale_to_fit, |ui| {
            if let Some(v) = expr_field(ui, salt.with("scale"), &fmt_num(page.scale), Some(label.id)).and_then(|t| parse_ratio(&t)) {
                change = Some(("scale", json!(v)));
            }
        })
        .response
        .on_hover_text("Paper units per drawing unit, or a ratio such as 1:50");
        ui.end_row();
    }
    if rows.center {
        ui.label("");
        let mut center = page.center;
        if ui.checkbox(&mut center, "Center the plot").changed() {
            change = Some(("center", json!(center)));
        }
        ui.end_row();
    }
    ui.label("");
    let mut lw = page.lineweights;
    if ui.checkbox(&mut lw, "Plot object lineweights").changed() {
        change = Some(("lineweights", json!(lw)));
    }
    ui.end_row();
    ui.label("");
    let mut transparency = page.plot_transparency;
    if ui
        .checkbox(&mut transparency, "Plot transparency")
        .on_hover_text("Plot objects with their transparency (PLOTTRANSPARENCYOVERRIDE can override it)")
        .changed()
    {
        change = Some(("plotTransparency", json!(transparency)));
    }
    ui.end_row();
    ui.label("Plot style table");
    craft_ui::ComboBox::from_id_salt(salt.with("styles")).selected_text(style_label(page)).width(190.0).show_ui(ui, |ui| {
        if ui.selectable_label(style_label(page) == "None", "None").clicked() && style_label(page) != "None" {
            change = Some(("plotStyleTable", json!("None")));
        }
        for name in &choices.styles {
            let on = name.eq_ignore_ascii_case(&page.plot_style_table);
            if ui.selectable_label(on, name).clicked() && !on {
                change = Some(("plotStyleTable", json!(name)));
            }
        }
    });
    ui.end_row();
    change
}

/// Apply a [`page_rows`] change to a page setup held by a dialog.
/// Device and paper changes follow PAGESETUP's rules (`plotters::page_device`): a device brings
/// its papers, orientation and printable areas.
fn apply_page_change(page: &mut cadcraft_doc::PageSetup, key: &str, v: &Value) {
    use cadcraft_engine::cmd::plotters::page_device;
    match key {
        "device" => {
            let _ = page_device("plot", &json!({ "device": v }), page);
        }
        "paper" => {
            // A device's paper, else a standard size (no device).
            if !page_device("plot", &json!({ "paper": v }), page).unwrap_or(false)
                && let Some(ps) = v.as_str().and_then(cadcraft_render::paper_size)
            {
                page.paper = ps.name.into();
                page.width_mm = ps.width_mm;
                page.height_mm = ps.height_mm;
            }
        }
        "plotStyleTable" => page.plot_style_table = v.as_str().unwrap_or_default().chars().take(260).collect(),
        "plotArea" => page.plot_area = v.as_str().unwrap_or("extents").into(),
        "scale" => page.scale = v.as_f64().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(page.scale),
        _ => {
            let b = v.as_bool().unwrap_or_default();
            match key {
                "landscape" => page.landscape = b,
                "scaleToFit" => page.scale_to_fit = b,
                "center" => page.center = b,
                "lineweights" => page.lineweights = b,
                "plotTransparency" => page.plot_transparency = b,
                _ => {}
            }
        }
    }
}

// ---------- Plot ----------

/// Bumped each time PLOT opens its dialog, so it starts again from the page setup.
static PLOT_GEN: AtomicU32 = AtomicU32::new(0);

/// Set while PLOTWINDOW picks the Plot dialog's window ("Window <").
const PICKING_WINDOW: &str = "plot_picking_window";
/// Set by Print... in a preview opened from the Plot dialog: plot as soon as the dialog is back.
pub(crate) const PLOT_NOW: &str = "plot_now";

/// Every frame: once PLOTWINDOW (started by the Plot dialog's "Window <") ends, bring the dialog
/// back with its settings, plotting the picked window (unchanged if the pick was cancelled).
pub fn plot_window_resume(app: &mut CadApp, ctx: &egui::Context) {
    if temp::<bool>(ctx, PICKING_WINDOW) != Some(true) || app.session.running.is_some() {
        return;
    }
    set_temp(ctx, PICKING_WINDOW, false);
    if let (Some(w), Some(mut st)) = (app.session.plot_window.take(), temp::<PlotState>(ctx, "plot_state")) {
        st.window = [w.min.x, w.min.y, w.max.x, w.max.y];
        st.page.plot_area = "window".into();
        set_temp(ctx, "plot_state", st);
    }
    app.ui.dialog = Some("plot".into());
}

/// Set a flag the Plot dialog reads (see [`PLOT_NOW`]).
pub(crate) fn set_flag(ctx: &egui::Context, id: &str, on: bool) {
    set_temp(ctx, id, on);
}

/// Plot areas for model space (a layout always plots its sheet).
const MODEL_AREAS: &[&str] = &["extents", "display", "limits", "window"];

/// The Plot dialog's settings until OK.
#[derive(Clone)]
struct PlotState {
    generation: u32,
    /// "Model" or a layout name.
    target: String,
    page: cadcraft_doc::PageSetup,
    /// Plot area "window" corners in drawing units: [x1, y1, x2, y2].
    window: [f64; 4],
    /// Model-space placement; stays outside PageSetup so layouts keep their existing behavior.
    center: bool,
    offset_mm: [f64; 2],
}

/// The page setup a plot of `target` starts from: the layout's, or model-space defaults (fitted
/// extents on A4 in metric drawings, ANSI A otherwise).
fn plot_page(d: &cadcraft_doc::Drawing, target: &str) -> cadcraft_doc::PageSetup {
    if let Some(l) = d.layouts.iter().find(|l| l.name == target) {
        let mut page = l.page.clone();
        page.plot_area = "layout".into();
        return page;
    }
    let mut page = cadcraft_doc::PageSetup { plot_area: "extents".into(), scale_to_fit: true, ..Default::default() };
    if cadcraft_render::paper::paper_unit_mm(d) == 1.0
        && let Some(a4) = cadcraft_render::paper_size("A4")
    {
        page.paper = a4.name.into();
        page.width_mm = a4.width_mm;
        page.height_mm = a4.height_mm;
    }
    page
}

/// The `plot` parameters for the dialog's settings (without the output path). `preview_dpi`
/// asks for a PNG at that resolution instead of the device's output, on the same sheet.
fn plot_params(st: &PlotState, preview_dpi: Option<f64>) -> Value {
    let page = &st.page;
    let mut p = json!({
        "layout": st.target,
        "landscape": page.landscape,
        "fit": page.scale_to_fit,
        "lineweights": page.lineweights,
        "plotTransparency": page.plot_transparency,
    });
    if let Some(o) = p.as_object_mut() {
        let device = device_label(page) != "None";
        let output = cadcraft_engine::cmd::plotters::find_device(&page.device).map_or("pdf", |d| d.output.as_str());
        match preview_dpi {
            // The page's own sheet, so a device's paper and printable area show as they plot.
            Some(dpi) => {
                o.insert("device".into(), json!("None"));
                o.insert("output".into(), json!("png"));
                o.insert("resolution".into(), json!(dpi));
                o.insert("width".into(), json!(page.width_mm));
                o.insert("height".into(), json!(page.height_mm));
                o.insert("margins".into(), json!(page.margins_mm));
            }
            None if device => {
                o.insert("device".into(), json!(page.device));
                o.insert("paper".into(), json!(page.paper));
            }
            None if cadcraft_render::paper_size(&page.paper).is_some() => {
                o.insert("device".into(), json!("None"));
                o.insert("paper".into(), json!(page.paper));
            }
            None => {
                o.insert("device".into(), json!("None"));
                o.insert("width".into(), json!(page.width_mm));
                o.insert("height".into(), json!(page.height_mm));
            }
        }
        if style_label(page) != "None" {
            o.insert("plotStyleTable".into(), json!(page.plot_style_table));
        }
        if st.target == "Model" {
            o.insert("plotArea".into(), json!(page.plot_area));
            if output != "svg" {
                o.insert("center".into(), json!(st.center));
                if !st.center {
                    o.insert("offset".into(), json!(st.offset_mm));
                }
            }
            if !page.scale_to_fit {
                o.insert("scale".into(), json!(page.scale));
            }
            if page.plot_area == "window" {
                let [x1, y1, x2, y2] = st.window;
                o.insert("window".into(), json!([[x1, y1], [x2, y2]]));
            }
        }
    }
    p
}

/// The Plot dialog: what to plot, on which paper and at what scale; OK asks where to save the PDF
/// (where the host has a file dialog) and runs `plot` with those settings.
fn plot(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Plot", open);
        return None;
    };
    let mut targets = vec!["Model".to_string()];
    let mut layouts: Vec<(u32, String)> = d.layouts.iter().map(|l| (l.tab_order, l.name.clone())).collect();
    layouts.sort_by_key(|l| l.0);
    targets.extend(layouts.into_iter().map(|l| l.1));
    let generation = PLOT_GEN.load(Ordering::Relaxed);
    let mut st = temp::<PlotState>(ctx, "plot_state").filter(|s| s.generation == generation && targets.contains(&s.target)).unwrap_or_else(|| {
        let target = app.session.layout_space().tab_name().to_string();
        let mut ext = d.extents(&cadcraft_doc::Space::Model);
        if ext.is_empty() {
            ext = cadcraft_geom::Bounds2::new(cadcraft_geom::Vec2::ZERO, cadcraft_geom::Vec2::new(12.0, 9.0));
        }
        PlotState {
            generation,
            page: plot_page(d, &target),
            target,
            window: [ext.min.x, ext.min.y, ext.max.x, ext.max.y],
            center: true,
            offset_mm: [0.0, 0.0],
        }
    });
    let model = st.target == "Model";
    let choices = PageChoices::new(ctx, d);
    let offset_supported =
        choices.devices.iter().find(|dev| dev.name.eq_ignore_ascii_case(&st.page.device)).is_none_or(|dev| dev.output.as_str() != "svg");
    // Print... in a preview opened from here plots with these settings.
    let mut ok = temp::<bool>(ctx, PLOT_NOW) == Some(true);
    set_temp(ctx, PLOT_NOW, false);
    let (mut cancel, mut action, mut pick, mut preview) = (false, None, false, false);
    let center = ctx.content_rect().center();
    egui::Window::new("Plot").open(open).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(center).show(ctx, |ui| {
        egui::Grid::new("plot_target").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Layout");
            craft_ui::ComboBox::from_id_salt("plot_layout").selected_text(st.target.as_str()).width(190.0).show_ui(ui, |ui| {
                for n in &targets {
                    if ui.selectable_label(*n == st.target, n).clicked() && *n != st.target {
                        st.page = plot_page(d, n);
                        st.target = n.clone();
                    }
                }
            });
            ui.end_row();
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            egui::Grid::new("plot_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                let rows = if model {
                    PageRows { areas: MODEL_AREAS, scale: true, center: false }
                } else {
                    PageRows { areas: &["layout"], scale: false, center: false }
                };
                let placement = model.then_some((&mut st.center, &mut st.offset_mm, offset_supported));
                if let Some((k, v)) = page_rows(ui, egui::Id::unique(("plot", st.target.as_str())), &st.page, &rows, &choices, placement) {
                    apply_page_change(&mut st.page, k, &v);
                }
                if model && st.page.plot_area == "window" {
                    for (i, label) in [(0, "First corner"), (2, "Opposite corner")] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            for (j, axis) in ["X", "Y"].iter().enumerate() {
                                let axis_label = ui.label(*axis);
                                let k = i + j;
                                if let Some(slot) = st.window.get_mut(k)
                                    && let Some(v) = num_field(ui, ("plot_window", k), *slot, Some(axis_label.id))
                                {
                                    *slot = v;
                                }
                            }
                        });
                        ui.end_row();
                    }
                    ui.label("");
                    pick = ui.button("Window <").on_hover_text("Pick the window's corners in the drawing").clicked();
                    ui.end_row();
                }
            });
            sheet_sketch(ui, &st.page, &t);
        });
        ui.separator();
        ui.horizontal(|ui| {
            if !model && ui.button("Apply to Layout").on_hover_text("Save these settings in the layout's page setup").clicked() {
                let page = &st.page;
                action = Some((
                    "pagesetup",
                    json!({
                        "layout": st.target,
                        "paper": page.paper,
                        "landscape": page.landscape,
                        "scaleToFit": page.scale_to_fit,
                        "lineweights": page.lineweights,
                        "plotTransparency": page.plot_transparency,
                    }),
                ));
            }
            preview = ui.button("Preview").on_hover_text("The sheet as these settings plot it").clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button("Cancel").clicked();
                ok |= ui.button("OK").clicked();
            });
        });
    });
    // The output file type follows the device.
    let ext = choices.devices.iter().find(|dv| dv.name.eq_ignore_ascii_case(&st.page.device)).map_or("pdf", |dv| dv.output.as_str());
    set_temp(ctx, "plot_state", st.clone());
    if cancel {
        *open = false;
    }
    if pick {
        // The dialog comes back with these settings and the picked window (`plot_window_resume`).
        set_temp(ctx, PICKING_WINDOW, true);
        *open = false;
        app.start("plotwindow");
        return action;
    }
    if preview {
        // A sheet about 1600 pixels on its long side.
        let long_in = st.page.width_mm.max(st.page.height_mm).max(1.0) / 25.4;
        let mut info = format!("{} · {}", st.target, st.page.paper);
        if st.page.landscape {
            info.push_str(" · landscape");
        }
        crate::exchange::preview_plot(app, ctx, plot_params(&st, Some((1600.0 / long_in).clamp(50.0, 300.0))), info);
        return action;
    }
    if ok {
        let mut params = plot_params(&st, None);
        if let Some(pick) = app.services.pick_save.as_ref() {
            let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
            let suggested = format!("{}-{}.{ext}", stem.unwrap_or_else(|| "Drawing".into()), st.target);
            // Cancelling the file dialog keeps the Plot dialog open with its settings.
            let Some(path) = pick(&suggested) else { return action };
            if let Some(o) = params.as_object_mut() {
                o.insert("path".into(), json!(path));
            }
        }
        *open = false;
        match app.run("plot", params) {
            Ok(r) => {
                let msg = match r.get("path").and_then(Value::as_str) {
                    Some(path) => format!("Plotted {} to {path}", st.target),
                    None => format!("Plotted {} ({} bytes of PDF; there is no file dialog here)", st.target, r["bytes"]),
                };
                app.session.echo(msg);
            }
            // `run` already echoed the error; keep the dialog open to fix the settings.
            Err(_) => *open = true,
        }
    }
    action
}

/// The sheet in the chosen orientation with its printable area, and its size.
fn sheet_sketch(ui: &mut egui::Ui, page: &cadcraft_doc::PageSetup, t: &Tokens) {
    let (w, h) = if page.landscape { (page.height_mm, page.width_mm) } else { (page.width_mm, page.height_mm) };
    ui.vertical(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(120.0, 120.0), Sense::hover());
        let k = (110.0 / w.max(h).max(1.0)) as f32;
        let sheet = egui::Rect::from_center_size(rect.center(), vec2(w as f32 * k, h as f32 * k));
        ui.painter().rect_filled(sheet, 0.0, t.field);
        ui.painter().rect_stroke(sheet, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        let [l, b, r, top] = page.margins_mm.map(|m| m as f32 * k);
        let printable = egui::Rect::from_min_max(sheet.min + vec2(l, top), sheet.max - vec2(r, b));
        if printable.is_positive() {
            ui.painter().rect_stroke(printable, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        }
        ui.label(RichText::new(format!("{w:.1} × {h:.1} mm")).color(t.text_dim));
    });
}

// ---------- Constraint Settings ----------

/// Constraint Settings dialog editing the drawing's parametric settings; returns the `constraintsettings` action.
fn constraint_settings(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Constraint Settings", open);
        return None;
    };
    let s = d.parametric.settings.clone();
    let types = cadcraft_doc::ParametricSettings::default().auto_types;
    let mut action = None;
    let set = |k: &str, v: Value| Some(("constraintsettings", json!({ k: v })));
    egui::Window::new("Constraint Settings").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.heading("Geometric");
        let mut infer = s.infer;
        if ui.checkbox(&mut infer, "Infer geometric constraints").changed() {
            action = set("infer", json!(infer));
        }
        let mut bars = s.bars_visible;
        if ui.checkbox(&mut bars, "Show constraint bars").changed() {
            action = set("barsVisible", json!(bars));
        }
        ui.horizontal(|ui| {
            let label = ui.label("Constraint bar transparency (%)");
            if let Some(v) = num_field(ui, "csettings_transp", f64::from(s.bar_transparency), Some(label.id)) {
                action = set("barTransparency", json!(v));
            }
        });
        ui.separator();
        ui.heading("Dimensional");
        let mut dims = s.dims_visible;
        if ui.checkbox(&mut dims, "Show dynamic constraints").changed() {
            action = set("dimsVisible", json!(dims));
        }
        ui.separator();
        ui.heading("AutoConstrain");
        ui.label(RichText::new("Constraint types AUTOCONSTRAIN applies").color(t.text_dim));
        egui::Grid::new("csettings_types").num_columns(3).show(ui, |ui| {
            for (i, ty) in types.iter().enumerate() {
                let mut on = s.auto_types.iter().any(|a| a.eq_ignore_ascii_case(ty));
                if ui.checkbox(&mut on, ty.as_str()).changed() {
                    let list: Vec<&String> =
                        types.iter().filter(|x| if *x == ty { on } else { s.auto_types.iter().any(|a| a.eq_ignore_ascii_case(x)) }).collect();
                    action = set("autoTypes", json!(list));
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        egui::Grid::new("csettings_tol").num_columns(2).show(ui, |ui| {
            let label = ui.label("Distance tolerance");
            if let Some(v) = num_field(ui, "csettings_dist", s.distance_tolerance, Some(label.id)) {
                action = set("distanceTolerance", json!(v));
            }
            ui.end_row();
            let label = ui.label("Angle tolerance (degrees)");
            if let Some(v) = num_field(ui, "csettings_angle", s.angle_tolerance, Some(label.id)) {
                action = set("angleTolerance", json!(v));
            }
            ui.end_row();
        });
    });
    action
}

#[cfg(test)]
mod plot_dialog_tests {
    use super::*;
    use cadcraft_engine::Session;

    fn state(target: &str) -> PlotState {
        let d = cadcraft_doc::Drawing::new_metric();
        PlotState {
            generation: 0,
            page: plot_page(&d, target),
            target: target.into(),
            window: [0.0, 0.0, 10.0, 10.0],
            center: true,
            offset_mm: [0.0, 0.0],
        }
    }

    #[test]
    fn a_device_brings_its_papers_and_the_plot_names_them() {
        let mut st = state("Model");
        apply_page_change(&mut st.page, "device", &json!("CADCraft PNG"));
        assert_eq!(st.page.device, "CADCraft PNG");
        let dev = cadcraft_engine::cmd::plotters::find_device("CADCraft PNG").unwrap();
        assert!(dev.papers.iter().any(|p| p.name == st.page.paper), "a paper of the device: {}", st.page.paper);
        assert!(st.page.margins_mm.iter().any(|m| *m > 0.0), "the paper's printable area");
        apply_page_change(&mut st.page, "plotStyleTable", &json!("monochrome.ctb"));
        let p = plot_params(&st, None);
        assert_eq!((p["device"].as_str(), p["paper"].as_str()), (Some("CADCraft PNG"), Some(st.page.paper.as_str())));
        assert_eq!(p["plotStyleTable"], json!("monochrome.ctb"));
        // The preview plots the same sheet to PNG, whatever the device.
        let q = plot_params(&st, Some(150.0));
        assert_eq!((q["output"].as_str(), q["device"].as_str()), (Some("png"), Some("None")));
        assert_eq!(q["width"].as_f64(), Some(st.page.width_mm));
        assert_eq!(q["margins"], json!(st.page.margins_mm));
        // Back to no device: a standard paper, no device sent.
        apply_page_change(&mut st.page, "device", &json!("None"));
        apply_page_change(&mut st.page, "paper", &json!("A3"));
        let p = plot_params(&st, None);
        let a3 = cadcraft_render::paper_size("A3").unwrap().name;
        assert_eq!((p["device"].as_str(), p["paper"].as_str()), (Some("None"), Some(a3)));
        apply_page_change(&mut st.page, "plotStyleTable", &json!("None"));
        assert!(plot_params(&st, None).get("plotStyleTable").is_none());
    }

    #[test]
    fn plot_transparency_checkbox_reaches_the_plot() {
        let mut st = state("Model");
        assert!(!st.page.plot_transparency, "off by default");
        assert_eq!(plot_params(&st, None)["plotTransparency"], json!(false));
        apply_page_change(&mut st.page, "plotTransparency", &json!(true));
        assert!(st.page.plot_transparency);
        assert_eq!(plot_params(&st, None)["plotTransparency"], json!(true));
        // The preview plots it the same way.
        assert_eq!(plot_params(&st, Some(150.0))["plotTransparency"], json!(true));
        // Into a layout's page setup and back out to its Plot dialog.
        let mut s = Session::new();
        s.execute("pagesetup", &json!({ "layout": "Layout1", "plotTransparency": true })).unwrap();
        let page = plot_page(s.doc().unwrap(), "Layout1");
        assert!(page.plot_transparency);
        apply_page_change(&mut st.page, "plotTransparency", &json!(false));
        assert_eq!(plot_params(&st, None)["plotTransparency"], json!(false));
    }

    #[test]
    fn plot_center_and_offsets_are_model_only_and_omitted_for_svg() {
        let mut st = state("Model");
        assert_eq!(plot_params(&st, None)["center"], json!(true));
        st.center = false;
        st.offset_mm = [12.5, -7.0];
        let params = plot_params(&st, None);
        assert_eq!(params["center"], json!(false));
        assert_eq!(params["offset"], json!([12.5, -7.0]));
        let preview = plot_params(&st, Some(150.0));
        assert_eq!(preview["output"], json!("png"));
        assert_eq!(preview["offset"], json!([12.5, -7.0]));

        let svg = cadcraft_engine::cmd::plotters::devices().into_iter().find(|dev| dev.output.as_str() == "svg").expect("built-in SVG device");
        apply_page_change(&mut st.page, "device", &json!(svg.name));
        let params = plot_params(&st, None);
        assert!(params.get("center").is_none() && params.get("offset").is_none(), "SVG does not receive unsupported placement options: {params}");
        let preview = plot_params(&st, Some(150.0));
        assert!(
            preview.get("center").is_none() && preview.get("offset").is_none(),
            "disabled SVG placement controls cannot affect preview output either"
        );

        st.target = "Layout1".into();
        st.page = plot_page(&cadcraft_doc::Drawing::new_metric(), "Layout1");
        let params = plot_params(&st, None);
        assert!(params.get("center").is_none() && params.get("offset").is_none(), "layout plots retain their page setup mapping");
    }

    #[test]
    fn window_pick_brings_the_dialog_back_with_the_picked_window() {
        let ctx = egui::Context::default();
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        set_temp(&ctx, "plot_state", state("Model"));
        set_temp(&ctx, PICKING_WINDOW, true);
        app.start("plotwindow");
        plot_window_resume(&mut app, &ctx);
        assert_eq!(app.ui.dialog, None, "still picking");
        app.session.cmdline("2,3").unwrap();
        app.session.cmdline("12,9").unwrap();
        plot_window_resume(&mut app, &ctx);
        assert_eq!(app.ui.dialog.as_deref(), Some("plot"));
        let st = temp::<PlotState>(&ctx, "plot_state").unwrap();
        assert_eq!((st.page.plot_area.as_str(), st.window), ("window", [2.0, 3.0, 12.0, 9.0]));
        // A cancelled pick brings the dialog back unchanged.
        app.ui.dialog = None;
        set_temp(&ctx, PICKING_WINDOW, true);
        app.start("plotwindow");
        app.session.input(cadcraft_engine::Input::Cancel).ok();
        plot_window_resume(&mut app, &ctx);
        assert_eq!(app.ui.dialog.as_deref(), Some("plot"));
        assert_eq!(temp::<PlotState>(&ctx, "plot_state").unwrap().window, [2.0, 3.0, 12.0, 9.0]);
    }
}
