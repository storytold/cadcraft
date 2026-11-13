//! Style and settings dialogs opened from the menus: Linetype Manager, Point Style, Text Style,
//! Table Style, Multileader Style, Page Setup Manager, Plot, Constraint Settings and Named
//! Viewports.
//!
//! Typed or chosen from a menu (no parameters) these commands open their dialog; JSON calls run
//! the command. The dialogs read the drawing and change it only through the same commands
//! (`app.run`), so every change is undoable and scriptable.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::{Align2, Color32, FontId, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::parametric::expr_field;
use crate::theme::Tokens;
use crate::{CadApp, Pick};

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
    let mut pick_lin = false;
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
                if app.services.pick_open.is_some() {
                    ui.separator();
                    if ui.selectable_label(false, "From File...").clicked() {
                        pick_lin = true;
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
    // The user's .lin file, asked for once the window is drawn. Errors are echoed by `run`.
    if pick_lin {
        let _ = app.pick_then(Pick::Open, |app, path| app.run("linetype", json!({ "load": "*", "file": path })));
    }
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
    Choice(&'static str, &'static str, String, std::sync::Arc<[String]>),
}

struct Style {
    name: String,
    fields: Vec<Field>,
}

fn text_style(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Text Style", open);
        return None;
    };
    let mut styles: Vec<Style> = d
        .text_styles
        .iter()
        .map(|s| Style {
            name: s.name.clone(),
            fields: vec![
                Field::Num("height", "Height (0 = ask)", s.height),
                Field::Num("widthFactor", "Width factor", s.width_factor),
                Field::Bool("backwards", "Backwards", s.backwards),
                Field::Bool("upsideDown", "Upside down", s.upside_down),
            ],
        })
        .collect();
    if styles.is_empty() {
        let s = cadcraft_doc::TextStyle::default();
        styles.push(Style { name: s.name, fields: Vec::new() });
    }
    let current = d.header.str("TEXTSTYLE", "Standard");
    style_manager(ctx, open, "Text Style", "style", &current, &styles)
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
    // One list shared by every row: a copy per style multiplied the two counts.
    let text_styles: std::sync::Arc<[String]> = d.text_styles.iter().map(|s| s.name.clone()).collect();
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
                Field::Choice("textStyle", "Text style", s.text_style.clone(), std::sync::Arc::clone(&text_styles)),
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
                ui.add(egui::TextEdit::singleline(&mut new_name).hint_text("New style name").desired_width(120.0)).on_hover_text("New style name");
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
                            ui.label(*label);
                            let mut picked = None;
                            craft_ui::ComboBox::from_id_salt((cmd, *key)).selected_text(v.as_str()).show_ui(ui, |ui| {
                                for c in choices.iter() {
                                    if ui.selectable_label(c == v, c).clicked() && c != v {
                                        picked = Some(c.clone());
                                    }
                                }
                            });
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
    // The layouts a change applies to: `sel` (the one whose settings are shown) and the others
    // picked with Ctrl / Shift.
    let mut picked = temp::<Vec<String>>(ctx, "pagesetup_picked").unwrap_or_default();
    picked.retain(|n| layouts.iter().any(|l| l.1 == *n));
    if !picked.contains(&sel) {
        picked = vec![sel.clone()];
    }
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
                    let resp = ui.selectable_label(picked.contains(name), name);
                    if resp.clicked() {
                        let order: Vec<&str> = layouts.iter().map(|l| l.1.as_str()).collect();
                        click_layout(&mut picked, &mut sel, &order, name, ui.input(|i| i.modifiers));
                    }
                }
            });
            left.horizontal(|ui| {
                if ui.small_button("Select all").on_hover_text("Ctrl+click or Shift+click pick several layouts").clicked() {
                    picked = layouts.iter().map(|l| l.1.clone()).collect();
                }
            });
            let Some((_, name, page)) = layouts.iter().find(|l| l.1 == sel) else { return };
            right.label(RichText::new(format!("Page setup of {name}")).strong());
            if picked.len() > 1 {
                right.label(RichText::new(crate::tf!("{n} layouts selected: a change applies to all of them", n = picked.len())).color(t.text_dim));
            }
            // Layout plotting always prints the whole sheet, so only that area is offered.
            let rows = PageRows {
                device: true,
                paper: true,
                orientation: true,
                areas: &["layout"],
                fit: true,
                scale: true,
                center: true,
                lineweights: true,
                plot_transparency: true,
                plot_style_table: true,
                labels: PageRowLabels::PageSetup,
            };
            egui::Grid::new("pagesetup_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(right, |ui| {
                if let Some((k, v)) = page_rows(ui, egui::Id::unique(("pagesetup", name)), page, &rows, &choices, None, picked.len() > 1) {
                    action = Some(("pagesetup", pagesetup_change(&picked, name, k, v)));
                }
            });
            let (w, h) = if page.landscape { (page.height_mm, page.width_mm) } else { (page.width_mm, page.height_mm) };
            right.label(RichText::new(format!("{w:.1} × {h:.1} mm")).color(t.text_dim));
        });
    });
    set_temp(ctx, "pagesetup_sel", sel);
    set_temp(ctx, "pagesetup_picked", picked);
    action
}

/// A click on layout `name` in the Page Setup Manager's list: Ctrl (or Cmd) adds or removes it,
/// Shift picks the range from the layout shown to it, a plain click picks just that one. `sel` is
/// the layout whose settings are shown, `order` the layouts in tab order.
fn click_layout(picked: &mut Vec<String>, sel: &mut String, order: &[&str], name: &str, modifiers: egui::Modifiers) {
    if modifiers.shift {
        let (a, b) = (order.iter().position(|n| *n == sel.as_str()), order.iter().position(|n| *n == name));
        if let (Some(a), Some(b)) = (a, b) {
            let (lo, hi) = (a.min(b), a.max(b));
            *picked = order.get(lo..=hi).unwrap_or_default().iter().map(|n| n.to_string()).collect();
        }
    } else if modifiers.command || modifiers.ctrl {
        if let Some(i) = picked.iter().position(|n| n == name) {
            // The last one stays: something must be shown.
            if picked.len() > 1 {
                picked.remove(i);
                if sel == name {
                    *sel = picked.last().cloned().unwrap_or_default();
                }
            }
        } else {
            picked.push(name.to_string());
            *sel = name.to_string();
        }
    } else {
        *picked = vec![name.to_string()];
        *sel = name.to_string();
    }
}

/// The `pagesetup` parameters for one changed setting: for the layout shown, or, with several
/// picked, for all of them in one step.
fn pagesetup_change(picked: &[String], shown: &str, key: &str, value: Value) -> Value {
    if picked.len() > 1 { json!({ "layouts": picked, key: value }) } else { json!({ "layout": shown, key: value }) }
}

/// Which page-setup rows a dialog shows.
struct PageRows {
    device: bool,
    paper: bool,
    orientation: bool,
    /// Plot area choices (`plotArea` values).
    areas: &'static [&'static str],
    fit: bool,
    scale: bool,
    center: bool,
    lineweights: bool,
    plot_transparency: bool,
    plot_style_table: bool,
    labels: PageRowLabels,
}

#[derive(Clone, Copy)]
enum PageRowLabels {
    PageSetup,
    PlotDialog,
}

/// The devices and plot style tables the page-setup rows offer.
struct PageChoices {
    devices: std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>>,
    styles: Vec<String>,
}

impl PageChoices {
    /// The devices and plot style tables the page setup controls offer now.
    fn new(ctx: &egui::Context, d: &cadcraft_doc::Drawing) -> Self {
        PageChoices { devices: device_choices(ctx), styles: cadcraft_doc::plot_style_table_names(d) }
    }
}

/// The plotter devices (built-in, system printers, then the user's files), read again at most
/// every 2 s: the dialogs ask every frame and user devices are files. System printers are listed
/// in the background; while that runs the list is read again shortly, so they appear as soon as
/// it ends.
fn device_choices(ctx: &egui::Context) -> std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>> {
    type Cached = (f64, std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>>);
    let now = crate::now_ms();
    if let Some((at, list)) = temp::<Cached>(ctx, "plot_devices")
        && now - at < 2000.0
    {
        return list;
    }
    let list = std::sync::Arc::new(cadcraft_engine::cmd::plotters::devices());
    if cadcraft_engine::cmd::plotters::system_printers_pending() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    } else {
        set_temp::<Cached>(ctx, "plot_devices", (now, list.clone()));
    }
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

/// The translated name of a plot area (`layout`, `extents`, …).
fn area_label(a: &str) -> &str {
    match a {
        "layout" => crate::tl!("Layout"),
        "extents" => crate::tl!("Extents"),
        "display" => crate::tl!("Display"),
        "limits" => crate::tl!("Limits"),
        "window" => crate::tl!("Window"),
        other => other,
    }
}

/// A tab as the Plot dialog shows it: "Model" in the interface language, a layout by its name.
fn tab_label(name: &str) -> &str {
    if name == "Model" { crate::tl!("Model") } else { name }
}

/// A device or plot style table name as shown: "None" in the interface language.
fn shown_label(name: &str) -> &str {
    if name == "None" { crate::tl!("None") } else { name }
}

/// A device as the device lists show it: the operating system's default printer says so.
fn device_name(dev: &cadcraft_doc::PlotterConfig) -> String {
    if dev.system_default { crate::tf!("{name} (default)", name = dev.name) } else { dev.name.clone() }
}

/// A plot scale typed as a number (paper units per drawing unit) or a ratio such as `1:50`.
fn parse_ratio(t: &str) -> Option<f64> {
    let v = match t.trim().split_once([':', '/']) {
        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
        None => t.trim().trim_end_matches(['x', 'X']).parse().ok()?,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

/// The selected page-setup rows shared by the Page Setup Manager and the Plot dialog. Returns the
/// setting the user changed as a `pagesetup` parameter.
/// With `force`, picking the value already shown is a change too: with several layouts picked,
/// that is how the others are made to match the one shown.
fn page_rows(
    ui: &mut egui::Ui,
    salt: egui::Id,
    page: &cadcraft_doc::PageSetup,
    rows: &PageRows,
    choices: &PageChoices,
    placement: Option<(&mut bool, &mut [f64; 2], bool)>,
    force: bool,
) -> Option<(&'static str, Value)> {
    let mut change = None;
    if rows.device {
        let caption = ui.label(match rows.labels {
            PageRowLabels::PageSetup => crate::tl!("Printer/plotter"),
            PageRowLabels::PlotDialog => crate::tl!("Name"),
        });
        ui.horizontal(|ui| {
            let shown = choices
                .devices
                .iter()
                .find(|dv| dv.name.eq_ignore_ascii_case(&page.device))
                .map_or_else(|| shown_label(device_label(page)).to_string(), device_name);
            let combo = craft_ui::ComboBox::from_id_salt(salt.with("device")).selected_text(shown).width(230.0).show_ui(ui, |ui| {
                if ui
                    .selectable_label(device_label(page) == "None", crate::tl!("None"))
                    .on_hover_text(crate::tl!("PDF on the paper chosen below"))
                    .clicked()
                    && (force || device_label(page) != "None")
                {
                    change = Some(("device", json!("None")));
                }
                for dev in choices.devices.iter() {
                    let on = dev.name.eq_ignore_ascii_case(&page.device);
                    if ui
                        .selectable_label(on, device_name(dev))
                        .on_hover_text(format!("{} ({})", dev.description, dev.output.as_str().to_uppercase()))
                        .clicked()
                        && (force || !on)
                    {
                        change = Some(("device", json!(dev.name)));
                    }
                }
            });
            combo.response.labelled_by(caption.id);
        });
        ui.end_row();
    }
    // The device's papers, else the standard sizes.
    if rows.paper {
        let device = choices.devices.iter().find(|dv| dv.name.eq_ignore_ascii_case(&page.device));
        let caption = ui.label(crate::tl!("Paper size"));
        ui.horizontal(|ui| {
            let combo =
                craft_ui::ComboBox::from_id_salt(salt.with("paper")).selected_text(page.paper.as_str()).width(190.0).show_ui(ui, |ui| match device {
                    Some(dev) => {
                        for p in &dev.papers {
                            if ui.selectable_label(p.name == page.paper, &p.name).clicked() && (force || p.name != page.paper) {
                                change = Some(("paper", json!(p.name)));
                            }
                        }
                    }
                    None => {
                        for p in cadcraft_render::PAPER_SIZES {
                            if ui.selectable_label(p.name == page.paper, p.name).clicked() && (force || p.name != page.paper) {
                                change = Some(("paper", json!(p.name)));
                            }
                        }
                    }
                });
            combo.response.labelled_by(caption.id);
        });
        ui.end_row();
    }
    if rows.orientation {
        ui.label(match rows.labels {
            PageRowLabels::PageSetup => crate::tl!("Orientation"),
            PageRowLabels::PlotDialog => "",
        });
        let mut orientation = |ui: &mut egui::Ui| {
            if ui.radio(!page.landscape, crate::tl!("Portrait")).clicked() && (force || page.landscape) {
                change = Some(("landscape", json!(false)));
            }
            if ui.radio(page.landscape, crate::tl!("Landscape")).clicked() && (force || !page.landscape) {
                change = Some(("landscape", json!(true)));
            }
        };
        if matches!(rows.labels, PageRowLabels::PlotDialog) {
            ui.vertical(&mut orientation);
        } else {
            ui.horizontal(&mut orientation);
        }
        ui.end_row();
    }
    if !rows.areas.is_empty() {
        let caption = ui.label(match rows.labels {
            PageRowLabels::PageSetup => crate::tl!("Plot area"),
            PageRowLabels::PlotDialog => crate::tl!("What to plot"),
        });
        ui.add_enabled_ui(rows.areas.len() > 1, |ui| {
            let combo = craft_ui::ComboBox::from_id_salt(salt.with("area")).selected_text(area_label(&page.plot_area)).show_ui(ui, |ui| {
                for a in rows.areas {
                    if ui.selectable_label(page.plot_area == *a, area_label(a)).clicked() && (force || page.plot_area != *a) {
                        change = Some(("plotArea", json!(a)));
                    }
                }
            });
            combo.response.labelled_by(caption.id);
        });
        ui.end_row();
    }
    if rows.fit {
        ui.label("");
        let mut fit = page.scale_to_fit;
        if ui.checkbox(&mut fit, crate::tl!("Fit to paper")).changed() {
            change = Some(("scaleToFit", json!(fit)));
        }
        ui.end_row();
    }
    if let Some((center, offset_mm, supported)) = placement {
        ui.label("");
        ui.add_enabled_ui(supported, |ui| ui.checkbox(center, crate::tl!("Center the plot"))).response.on_hover_text(if supported {
            crate::tl!("Center the plotted model bounds on the printable area")
        } else {
            crate::tl!("Model-space offset placement is not supported by SVG")
        });
        ui.end_row();
        for (i, label) in [(0, crate::tl!("X offset (mm)")), (1, crate::tl!("Y offset (mm)"))] {
            let field_label = ui.label(label);
            ui.add_enabled_ui(supported && !*center, |ui| {
                if let Some(v) = num_field(ui, ("plot_offset", i), offset_mm[i], Some(field_label.id)) {
                    offset_mm[i] = v;
                }
            })
            .response
            .on_hover_text(crate::tl!("Distance from the printable area's lower-left corner; values range from −100000 to 100000 mm"));
            ui.end_row();
        }
        if supported && !*center && offset_mm.iter().any(|v| !v.is_finite() || v.abs() > cadcraft_render::paper::MAX_PLOT_OFFSET_MM) {
            ui.label("");
            ui.colored_label(Tokens::get().warn, crate::tl!("Offsets must be within ±100000 mm"));
            ui.end_row();
        }
    }
    if rows.scale {
        let label = ui.label(crate::tl!("Scale"));
        ui.add_enabled_ui(!page.scale_to_fit, |ui| {
            if let Some(v) = expr_field(ui, salt.with("scale"), &fmt_num(page.scale), Some(label.id)).and_then(|t| parse_ratio(&t)) {
                change = Some(("scale", json!(v)));
            }
        })
        .response
        .on_hover_text(crate::tl!("Paper units per drawing unit, or a ratio such as 1:50"));
        ui.end_row();
    }
    if rows.center {
        ui.label("");
        let mut center = page.center;
        if ui.checkbox(&mut center, crate::tl!("Center the plot")).changed() {
            change = Some(("center", json!(center)));
        }
        ui.end_row();
    }
    if rows.lineweights {
        ui.label("");
        let mut lw = page.lineweights;
        if ui.checkbox(&mut lw, crate::tl!("Plot object lineweights")).changed() {
            change = Some(("lineweights", json!(lw)));
        }
        ui.end_row();
    }
    if rows.plot_transparency {
        ui.label("");
        let mut transparency = page.plot_transparency;
        if ui
            .checkbox(&mut transparency, crate::tl!("Plot transparency"))
            .on_hover_text(crate::tl!("Plot objects with their transparency (PLOTTRANSPARENCYOVERRIDE can override it)"))
            .changed()
        {
            change = Some(("plotTransparency", json!(transparency)));
        }
        ui.end_row();
    }
    if rows.plot_style_table {
        let caption = ui.label(match rows.labels {
            PageRowLabels::PageSetup => crate::tl!("Plot style table"),
            PageRowLabels::PlotDialog => crate::tl!("Table"),
        });
        ui.horizontal(|ui| {
            let combo =
                craft_ui::ComboBox::from_id_salt(salt.with("styles")).selected_text(shown_label(style_label(page))).width(190.0).show_ui(ui, |ui| {
                    if ui.selectable_label(style_label(page) == "None", crate::tl!("None")).clicked() && (force || style_label(page) != "None") {
                        change = Some(("plotStyleTable", json!("None")));
                    }
                    for name in &choices.styles {
                        let on = name.eq_ignore_ascii_case(&page.plot_style_table);
                        if ui.selectable_label(on, name).clicked() && (force || !on) {
                            change = Some(("plotStyleTable", json!(name)));
                        }
                    }
                });
            combo.response.labelled_by(caption.id);
        });
        ui.end_row();
    }
    change
}

/// Apply a [`page_rows`] change to a page setup held by a dialog.
/// Device and paper changes follow PAGESETUP's rules (`plotters::page_device`): a device brings
/// its papers, orientation and printable areas.
fn apply_page_change(page: &mut cadcraft_doc::PageSetup, key: &str, v: &Value) {
    use cadcraft_engine::cmd::plotters::page_device;
    match key {
        "device" => {
            let had_device = device_label(page) != "None";
            let _ = page_device("plot", &json!({ "device": v }), page);
            // No device: the previous device's printable area no longer applies, so the page
            // takes the no-device margins and, for a standard paper, its size.
            if had_device && device_label(page) == "None" {
                page.margins_mm = cadcraft_doc::PageSetup::default().margins_mm;
                if let Some(ps) = cadcraft_render::paper_size(&page.paper) {
                    page.paper = ps.name.into();
                    page.width_mm = ps.width_mm;
                    page.height_mm = ps.height_mm;
                }
            }
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
    /// The sheets OK plots, in tab order (`target` alone, until others are ticked). With several,
    /// they go into one PDF, each with its own page setup.
    sheets: Vec<String>,
    page: cadcraft_doc::PageSetup,
    /// Plot area "window" corners in drawing units: [x1, y1, x2, y2].
    window: [f64; 4],
    /// Copies a system printer prints.
    copies: u32,
    /// With a system printer: write the plot's PDF to a file too ("Plot to file").
    to_file: bool,
    /// Plot with the plot style table (off: as if there were none, the table kept).
    with_styles: bool,
    /// Plot a stamp (drawing, sheet, date, device, paper, scale) at the sheet's corner.
    stamp: bool,
    /// OK also saves these settings in the layout's page setup.
    save_to_layout: bool,
    /// Model-space placement; stays outside PageSetup so layouts keep their existing behavior.
    center: bool,
    offset_mm: [f64; 2],
}

impl PlotState {
    /// The dialog's settings for `target` on `page`: one sheet, centred, one copy, plot styles on.
    fn new(generation: u32, target: String, page: cadcraft_doc::PageSetup, window: [f64; 4]) -> PlotState {
        PlotState {
            generation,
            sheets: vec![target.clone()],
            target,
            page,
            window,
            copies: 1,
            to_file: false,
            with_styles: true,
            stamp: false,
            save_to_layout: false,
            center: true,
            offset_mm: [0.0, 0.0],
        }
    }
}

/// The plot stamp the dialog asks for: the drawing, the sheet, the date and time (UTC), the
/// device, the paper and the scale. `page` names the device and paper (the preview plots the same
/// sheet to an image); without one, the engine and the plotter fill them in for each sheet, and
/// always the drawing, the sheet and the scale.
fn stamp_template(page: Option<&cadcraft_doc::PageSetup>) -> String {
    let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (device, paper) = match page {
        Some(p) => (if device_label(p) == "None" { "CADCraft PDF" } else { p.device.as_str() }, p.paper.as_str()),
        None => ("{device}", "{paper}"),
    };
    format!("{{drawing}}, {{layout}}, {} UTC, {device}, {paper}, {{scale}}", crate::autosave::utc(secs))
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

/// Tick or untick `name` among the sheets to plot. `targets` lists every sheet in tab order, which
/// is the order of the result. At least one sheet always stays.
fn toggle_sheet(sheets: &mut Vec<String>, targets: &[String], name: &str, on: bool) {
    let next: Vec<String> = targets.iter().filter(|t| if t.as_str() == name { on } else { sheets.contains(t) }).cloned().collect();
    if !next.is_empty() {
        *sheets = next;
    }
}

/// Drop the ticked sheets that no longer exist (`targets` lists those that do). With none left,
/// `target` is the one; with exactly one left, that one is the target, which the dialog then edits
/// and OK plots. Returns whether the target changed.
fn sync_sheets(sheets: &mut Vec<String>, target: &mut String, targets: &[String]) -> bool {
    sheets.retain(|n| targets.contains(n));
    if sheets.is_empty() {
        sheets.push(target.clone());
    }
    match sheets.as_slice() {
        [only] if only != target => {
            *target = only.clone();
            true
        }
        _ => false,
    }
}

/// The `plot` parameters for several sheets in one PDF: no device or page overrides, so each
/// sheet plots with its own page setup.
fn sheets_params(sheets: &[String]) -> Value {
    json!({ "layouts": sheets, "layout": sheets.first(), "device": "None" })
}

/// One line about a sheet's page setup for the summary shown with several sheets ticked.
fn sheet_summary(d: &cadcraft_doc::Drawing, name: &str) -> String {
    let page = plot_page(d, name);
    let orientation = if page.landscape { crate::tl!("landscape") } else { crate::tl!("portrait") };
    format!("{}: {} · {orientation} · {}", tab_label(name), page.paper, shown_label(style_label(&page)))
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
        // Always sent, "None" included: left out, the layout's own table would apply to a plot
        // the dialog shows without one.
        o.insert(
            "plotStyleTable".into(),
            json!(if !st.with_styles || style_label(page) == "None" { "None" } else { page.plot_style_table.as_str() }),
        );
        if st.stamp {
            o.insert("stamp".into(), json!(stamp_template(Some(page))));
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

/// The page setup values exposed by Plot for the selected layout.
fn apply_to_layout_params(st: &PlotState) -> Value {
    let page = &st.page;
    let mut params = json!({
        "layout": st.target,
        "landscape": page.landscape,
        "scaleToFit": page.scale_to_fit,
        "lineweights": page.lineweights,
        "plotTransparency": page.plot_transparency,
        "plotStyleTable": style_label(page),
        "margins": page.margins_mm,
    });
    let Some(object) = params.as_object_mut() else { return params };
    if device_label(page) != "None" {
        object.insert("device".into(), json!(page.device));
        object.insert("paper".into(), json!(page.paper));
    } else if cadcraft_render::paper_size(&page.paper).is_some() {
        object.insert("device".into(), json!("None"));
        object.insert("paper".into(), json!(page.paper));
    } else {
        object.insert("device".into(), json!("None"));
        object.insert("width".into(), json!(page.width_mm));
        object.insert("height".into(), json!(page.height_mm));
    }
    params
}

/// A titled, framed section of the Plot dialog.
fn plot_section(ui: &mut egui::Ui, heading: &str, content: impl FnOnce(&mut egui::Ui)) {
    plot_section_filled(ui, heading, 0.0, content);
}

/// A plot section whose frame is at least `min_height` tall, so sections side by side share a bottom edge.
fn plot_section_filled(ui: &mut egui::Ui, heading: &str, min_height: f32, content: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::group(ui.style());
    let padding = frame.inner_margin.sum().y;
    frame.show(ui, |ui| {
        ui.take_available_width();
        ui.set_min_height((min_height - padding).max(0.0));
        ui.label(RichText::new(heading).strong());
        ui.add_space(4.0);
        content(ui);
    });
}

/// No page-setup rows; a section turns on the ones it shows.
const NO_ROWS: PageRows = PageRows {
    device: false,
    paper: false,
    orientation: false,
    areas: &[],
    fit: false,
    scale: false,
    center: false,
    lineweights: false,
    plot_transparency: false,
    plot_style_table: false,
    labels: PageRowLabels::PlotDialog,
};

/// Where a plot on `device` goes, as the Plot dialog says it.
fn device_where(device: Option<&cadcraft_doc::PlotterConfig>) -> String {
    match device {
        Some(dv) if dv.system => crate::tl!("System printer").to_string(),
        Some(dv) => crate::tf!("File ({kind})", kind = dv.output.as_str().to_uppercase()),
        None => crate::tf!("File ({kind})", kind = "PDF"),
    }
}

/// The Plot dialog: what to plot (a layout, model space, or several sheets), on which device and
/// paper, at what scale and with which options. OK prints on a system printer (and, with "Plot to
/// file", writes the PDF too) or asks where to save the file (where the host has a file dialog),
/// then runs `plot` with those settings.
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
        let page = plot_page(d, &target);
        PlotState::new(generation, target, page, [ext.min.x, ext.min.y, ext.max.x, ext.max.y])
    });
    // Layouts deleted since the dialog was last open drop out of the ticked sheets.
    if sync_sheets(&mut st.sheets, &mut st.target, &targets) {
        st.page = plot_page(d, &st.target);
    }
    let model = st.target == "Model";
    let choices = PageChoices::new(ctx, d);
    let device = choices.devices.iter().find(|dv| dv.name.eq_ignore_ascii_case(&st.page.device)).cloned();
    let offset_supported = device.as_ref().is_none_or(|dev| dev.output.as_str() != "svg");
    // A system printer prints the plot; other devices write a file.
    let printer = device.as_ref().is_some_and(|dv| dv.system);
    let searching = cadcraft_engine::cmd::plotters::system_printers_pending();
    // Print... in a preview opened from here plots with these settings.
    let mut ok = temp::<bool>(ctx, PLOT_NOW) == Some(true);
    set_temp(ctx, PLOT_NOW, false);
    let (mut cancel, mut action, mut pick, mut preview, mut refresh, mut edit_styles) = (false, None, false, false, false, false);
    let viewport = ctx.content_rect();
    let default_size = vec2(900.0, 640.0).min(viewport.size());
    let min_size = vec2(760.0, 520.0).min(default_size);
    let id = egui::Id::unique(("plot", st.target.clone()));
    egui::Window::new(crate::tf!("Plot - {layout}", layout = tab_label(&st.target)))
        .id(egui::Id::unique("cc_plot_window"))
        .open(open)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(viewport.center())
        .default_size(default_size)
        .min_size(min_size)
        .max_size(viewport.size())
        .constrain_to(viewport)
        .resizable(true)
        .show(ctx, |ui| {
            let content_height = ui.available_height().min((viewport.height() - 128.0).max(220.0));
            egui::ScrollArea::vertical().id_salt("plot_content").auto_shrink([false, true]).max_height(content_height).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let available = ui.available_size_before_wrap();
                    let spacing = ui.spacing().item_spacing.x;
                    let left_width = (available.x - spacing) * 0.62;
                    let right_width = available.x - spacing - left_width;
                    ui.allocate_ui_with_layout(vec2(left_width, available.y), egui::Layout::top_down(egui::Align::Min), |left| {
                        left.set_width(left_width);
                        plot_section(left, crate::tl!("Page setup"), |ui| {
                            egui::Grid::new("plot_layout_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                let caption = ui.label(crate::tl!("Layout"));
                                let combo = craft_ui::ComboBox::from_id_salt("plot_layout")
                                    .selected_text(tab_label(&st.target))
                                    .width(190.0)
                                    .show_ui(ui, |ui| {
                                        for n in &targets {
                                            if ui.selectable_label(*n == st.target, tab_label(n)).clicked()
                                                && (*n != st.target || st.sheets.len() > 1)
                                            {
                                                st.page = plot_page(d, n);
                                                st.target = n.clone();
                                                st.sheets = vec![n.clone()];
                                            }
                                        }
                                    });
                                combo.response.labelled_by(caption.id);
                                ui.end_row();
                            });
                            // Several sheets in one PDF: tick them here. Each plots with its own page setup.
                            egui::CollapsingHeader::new(crate::tl!("Plot several sheets")).default_open(st.sheets.len() > 1).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button(crate::tl!("All layouts"))
                                        .on_hover_text(crate::tl!("Every layout, in tab order (not the Model tab)"))
                                        .clicked()
                                    {
                                        st.sheets = targets.iter().filter(|n| *n != "Model").cloned().collect();
                                        if let Some(first) = st.sheets.first().cloned() {
                                            st.page = plot_page(d, &first);
                                            st.target = first;
                                        }
                                    }
                                    if ui.small_button(crate::tl!("Only this one")).clicked() {
                                        st.sheets = vec![st.target.clone()];
                                    }
                                });
                                egui::ScrollArea::vertical().id_salt("plot_sheets").max_height(120.0).auto_shrink([true, true]).show(ui, |ui| {
                                    for n in &targets {
                                        let mut on = st.sheets.contains(n);
                                        if ui.checkbox(&mut on, tab_label(n)).changed() {
                                            toggle_sheet(&mut st.sheets, &targets, n, on);
                                            // One sheet left: it is the one whose settings the dialog edits.
                                            if let [only] = st.sheets.as_slice()
                                                && *only != st.target
                                            {
                                                st.page = plot_page(d, only);
                                                st.target = only.clone();
                                            }
                                        }
                                    }
                                });
                            });
                        });
                        if st.sheets.len() > 1 {
                            plot_section(left, crate::tl!("Sheets"), |ui| {
                                ui.label(
                                    RichText::new(crate::tf!(
                                        "{n} sheets in one PDF, each with its own page setup (change them in the Page Setup Manager):",
                                        n = st.sheets.len()
                                    ))
                                    .color(t.text_dim),
                                );
                                for n in &st.sheets {
                                    ui.label(sheet_summary(d, n));
                                }
                            });
                            return;
                        }
                        plot_section(left, crate::tl!("Printer/plotter"), |ui| {
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    egui::Grid::new("plot_device_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                        let rows = PageRows { device: true, paper: true, ..NO_ROWS };
                                        if let Some((k, v)) = page_rows(ui, id, &st.page, &rows, &choices, None, false) {
                                            apply_page_change(&mut st.page, k, &v);
                                        }
                                        if printer {
                                            let label = ui.label(crate::tl!("Number of copies"));
                                            ui.add(egui::DragValue::new(&mut st.copies).range(1..=999)).labelled_by(label.id);
                                            ui.end_row();
                                        }
                                        ui.label(crate::tl!("Where"));
                                        ui.label(device_where(device.as_ref()));
                                        ui.end_row();
                                        ui.label(crate::tl!("Description"));
                                        ui.add(
                                            egui::Label::new(
                                                device.as_ref().map_or(crate::tl!("PDF on the paper chosen below"), |dv| dv.description.as_str()),
                                            )
                                            .wrap(),
                                        );
                                        ui.end_row();
                                    });
                                    ui.horizontal(|ui| {
                                        if printer {
                                            ui.checkbox(&mut st.to_file, crate::tl!("Plot to file"))
                                                .on_hover_text(crate::tl!("Also save the printed PDF in a file you choose after OK"));
                                        } else {
                                            let mut to_file = true;
                                            ui.add_enabled(false, egui::Checkbox::new(&mut to_file, crate::tl!("Plot to file")))
                                                .on_disabled_hover_text(crate::tl!("This device writes a file: OK asks where"));
                                        }
                                        if searching {
                                            ui.add(egui::Spinner::new());
                                            ui.label(RichText::new(crate::tl!("Looking for printers…")).color(t.text_dim));
                                        } else {
                                            refresh = ui
                                                .small_button(crate::tl!("Refresh printers"))
                                                .on_hover_text(crate::tl!(
                                                    "Ask the system for its printers again (a printer just added or switched on)"
                                                ))
                                                .clicked();
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                                sheet_sketch(ui, &st.page, &t);
                            });
                        });

                        plot_section(left, crate::tl!("Plot area"), |ui| {
                            egui::Grid::new("plot_area_grid").num_columns(2).min_col_width(90.0).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                let rows = PageRows { areas: if model { MODEL_AREAS } else { &["layout"] }, ..NO_ROWS };
                                if let Some((k, v)) = page_rows(ui, id, &st.page, &rows, &choices, None, false) {
                                    apply_page_change(&mut st.page, k, &v);
                                }
                            });
                            // The corners only apply to Window. Outside it their space stays with a note instead of values that
                            // would not be plotted, so the section keeps its height.
                            let window_rows = model && st.page.plot_area == "window";
                            let corners = ui.scope(|ui| {
                                if !window_rows {
                                    ui.set_invisible();
                                }
                                egui::Grid::new("plot_corner_grid").num_columns(2).min_col_width(90.0).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                    for (i, label) in [(0, crate::tl!("First corner")), (2, crate::tl!("Opposite corner"))] {
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
                                    pick = ui
                                        .button(crate::tl!("Window <"))
                                        .on_hover_text(crate::tl!("Pick the window's corners in the drawing"))
                                        .clicked();
                                    ui.end_row();
                                });
                            });
                            if !window_rows {
                                let note =
                                    if model { crate::tl!("Choose Window to set the corners") } else { crate::tl!("Layouts plot the whole sheet") };
                                ui.painter().text(
                                    corners.response.rect.left_top(),
                                    egui::Align2::LEFT_TOP,
                                    note,
                                    egui::FontId::proportional(13.0),
                                    t.text_dim,
                                );
                            }
                        });

                        // The right column is laid out after this row, so the row reads its bottom from the previous pass.
                        let row_top = left.cursor().top();
                        let row_h = temp::<f32>(ctx, "plot_right_bottom").map_or(0.0, |bottom| (bottom - row_top).max(0.0));
                        left.columns(if model { 2 } else { 1 }, |cols| {
                            let scale_col = cols.len() - 1;
                            if model {
                                plot_section_filled(&mut cols[0], crate::tl!("Plot offset"), row_h, |ui| {
                                    ui.label(RichText::new(crate::tl!("Origin: printable area")).color(t.text_dim));
                                    egui::Grid::new("plot_offset_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                        let placement = Some((&mut st.center, &mut st.offset_mm, offset_supported));
                                        if let Some((k, v)) = page_rows(ui, id, &st.page, &NO_ROWS, &choices, placement, false) {
                                            apply_page_change(&mut st.page, k, &v);
                                        }
                                    });
                                });
                            }
                            plot_section_filled(&mut cols[scale_col], crate::tl!("Plot scale"), row_h, |ui| {
                                egui::Grid::new("plot_scale_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                    let rows = PageRows { fit: true, scale: model, ..NO_ROWS };
                                    if let Some((k, v)) = page_rows(ui, id, &st.page, &rows, &choices, None, false) {
                                        apply_page_change(&mut st.page, k, &v);
                                    }
                                });
                            });
                        });
                    });

                    ui.allocate_ui_with_layout(vec2(right_width, available.y), egui::Layout::top_down(egui::Align::Min), |right| {
                        right.set_width(right_width);
                        let several = st.sheets.len() > 1;
                        if !several {
                            plot_section(right, crate::tl!("Plot style table"), |ui| {
                                egui::Grid::new("plot_style_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                    let rows = PageRows { plot_style_table: true, ..NO_ROWS };
                                    if let Some((k, v)) = page_rows(ui, id, &st.page, &rows, &choices, None, false) {
                                        apply_page_change(&mut st.page, k, &v);
                                    }
                                });
                                edit_styles = ui
                                    .button(crate::tl!("Edit…"))
                                    .on_hover_text(crate::tl!("Open the Plot Style Manager; Plot comes back when it closes"))
                                    .clicked();
                            });
                        }
                        plot_section(right, crate::tl!("Plot options"), |ui| {
                            egui::Grid::new("plot_options_grid").num_columns(2).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                                if !several {
                                    let rows = PageRows { lineweights: true, plot_transparency: true, ..NO_ROWS };
                                    if let Some((k, v)) = page_rows(ui, id, &st.page, &rows, &choices, None, false) {
                                        apply_page_change(&mut st.page, k, &v);
                                    }
                                    ui.label("");
                                    ui.add_enabled_ui(style_label(&st.page) != "None", |ui| {
                                        ui.checkbox(&mut st.with_styles, crate::tl!("Plot with plot styles")).on_hover_text(crate::tl!(
                                            "Off: plot the objects' own colours and lineweights, keeping the table chosen above"
                                        ));
                                    });
                                    ui.end_row();
                                }
                                ui.label("");
                                ui.checkbox(&mut st.stamp, crate::tl!("Plot stamp on")).on_hover_text(crate::tl!(
                                    "A line at the sheet's lower-left corner: drawing, layout, date and time (UTC), device, paper and scale"
                                ));
                                ui.end_row();
                                if !several && !model {
                                    ui.label("");
                                    ui.checkbox(&mut st.save_to_layout, crate::tl!("Save changes to layout"))
                                        .on_hover_text(crate::tl!("OK also saves these settings in the layout's page setup"));
                                    ui.end_row();
                                }
                            });
                        });
                        if !several {
                            plot_section(right, crate::tl!("Drawing orientation"), |ui| {
                                egui::Grid::new("plot_orientation_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                                    let rows = PageRows { orientation: true, ..NO_ROWS };
                                    if let Some((k, v)) = page_rows(ui, id, &st.page, &rows, &choices, None, false) {
                                        apply_page_change(&mut st.page, k, &v);
                                    }
                                });
                            });
                        }
                        let bottom = right.min_rect().bottom();
                        if temp::<f32>(ctx, "plot_right_bottom") != Some(bottom) {
                            set_temp(ctx, "plot_right_bottom", bottom);
                            ctx.request_repaint();
                        }
                    });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                preview = ui.button(crate::tl!("Preview")).on_hover_text(crate::tl!("The sheet as these settings plot it")).clicked();
                let apply = ui
                    .add_enabled(!model && st.sheets.len() < 2, egui::Button::new(crate::tl!("Apply to Layout")))
                    .on_hover_text(crate::tl!("Save these settings in the layout's page setup"));
                if apply.clicked() {
                    action = Some(("pagesetup", apply_to_layout_params(&st)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    cancel = ui.button(crate::tl!("Cancel")).clicked();
                    ok |= ui.button(crate::tl!("OK")).clicked();
                });
            });
        });
    // The output file type follows the device; a printer's file is its PDF.
    let ext = device.as_ref().filter(|dv| !dv.system).map_or("pdf", |dv| dv.output.as_str());
    set_temp(ctx, "plot_state", st.clone());
    if cancel {
        *open = false;
    }
    if refresh {
        // Errors are echoed by `run`; the device list is read again on the next frame.
        let _ = app.run("plottermanager", json!({ "action": "refresh" }));
        ctx.data_mut(|m| m.remove::<(f64, std::sync::Arc<Vec<cadcraft_doc::PlotterConfig>>)>(egui::Id::unique("plot_devices")));
        ctx.request_repaint();
        return action;
    }
    if edit_styles {
        // The Plot Style Manager opens on this table and gives the dialog back when it closes.
        if style_label(&st.page) != "None" {
            ctx.data_mut(|m| m.insert_temp(egui::Id::unique("pstyle_sel"), st.page.plot_style_table.clone()));
        }
        set_temp(ctx, RETURN_TO_PLOT, true);
        *open = false;
        app.ui.dialog = Some("stylesmanager".into());
        return action;
    }
    if pick {
        // The dialog comes back with these settings and the picked window (`plot_window_resume`).
        set_temp(ctx, PICKING_WINDOW, true);
        *open = false;
        app.start("plotwindow");
        return action;
    }
    if preview {
        crate::exchange::preview_plot(app, ctx, plot_params(&st, Some(preview_dpi(&st.page))), plot_caption(&st));
        return action;
    }
    if ok {
        let several = st.sheets.len() > 1;
        let mut params = if several { sheets_params(&st.sheets) } else { plot_params(&st, None) };
        if several
            && st.stamp
            && let Some(o) = params.as_object_mut()
        {
            o.insert("stamp".into(), json!(stamp_template(None)));
        }
        // Several sheets always make a PDF; one sheet's file type follows its device.
        let ext = if several { "pdf" } else { ext };
        let what = if several { crate::tf!("{n} sheets", n = st.sheets.len()) } else { st.target.clone() };
        let printer = printer && !several;
        if printer && let Some(o) = params.as_object_mut() {
            o.insert("copies".into(), json!(st.copies.max(1)));
        }
        let job = PlotRun {
            what,
            printer: printer.then(|| st.page.device.clone()),
            // "Save changes to layout": the settings go into the page setup first (an undoable
            // step of its own); a refusal is echoed and the plot still runs with them.
            layout_setup: (st.save_to_layout && !model && !several).then(|| apply_to_layout_params(&st)),
        };
        // A file to write: a file device's plot, or a printer's PDF with "Plot to file".
        if (!printer || st.to_file) && app.services.pick_save.is_some() {
            let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
            let suggested = format!("{}-{}.{ext}", stem.unwrap_or_else(|| "Drawing".into()), if several { "sheets" } else { st.target.as_str() });
            // Cancelling the file dialog keeps the Plot dialog open with its settings.
            let _ = app.pick_then(Pick::Save(suggested), move |app, path| {
                let mut params = params;
                if let Some(o) = params.as_object_mut() {
                    o.insert("path".into(), json!(path));
                }
                run_plot(app, params, job);
                Ok(Value::Null)
            });
        } else {
            run_plot(app, params, job);
        }
    }
    action
}

/// What the Plot dialog's OK plots besides its parameters: what the message calls it, the system
/// printer it goes to, and the page setup to save in the layout first.
struct PlotRun {
    what: String,
    printer: Option<String>,
    layout_setup: Option<Value>,
}

/// Plot with the Plot dialog's settings: it closes, and opens again when the plot fails.
fn run_plot(app: &mut CadApp, params: Value, job: PlotRun) {
    if app.ui.dialog.as_deref() == Some("plot") {
        app.ui.dialog = None;
    }
    if let Some(setup) = job.layout_setup {
        let _ = app.run("pagesetup", setup);
    }
    let what = job.what;
    match app.run("plot", params) {
        Ok(r) => {
            let job_id = r.get("job").and_then(Value::as_str).unwrap_or("?");
            let msg = match (r.get("path").and_then(Value::as_str), job.printer.as_deref()) {
                (Some(path), Some(printer)) => crate::tf!(
                    "Sent {what} to {printer} (job {job}) and saved it to {path}",
                    what = what,
                    printer = printer,
                    job = job_id,
                    path = path
                ),
                (None, Some(printer)) => crate::tf!("Sent {what} to {printer} (job {job})", what = what, printer = printer, job = job_id),
                (Some(path), None) => crate::tf!("Plotted {what} to {path}", what = what, path = path),
                (None, None) => crate::tf!("Plotted {what} ({bytes} bytes; there is no file dialog here)", what = what, bytes = r["bytes"]),
            };
            app.session.echo(msg);
        }
        // `run` already echoed the error; keep the dialog open to fix the settings.
        Err(_) => app.ui.dialog = Some("plot".into()),
    }
}

/// Set while the Plot Style Manager opened from Plot is open: closing it opens Plot again.
pub(crate) const RETURN_TO_PLOT: &str = "plot_return_from_styles";

/// After the Plot Style Manager closes: Plot again, when Plot opened it ([`RETURN_TO_PLOT`]).
pub(crate) fn return_to_plot(app: &mut CadApp, ctx: &egui::Context) {
    if temp::<bool>(ctx, RETURN_TO_PLOT) == Some(true) {
        set_temp(ctx, RETURN_TO_PLOT, false);
        app.ui.dialog = Some("plot".into());
    }
}

/// The resolution (dpi) that draws a sheet about 1600 pixels on its long side: the preview's
/// starting image of the whole sheet.
fn preview_dpi(page: &cadcraft_doc::PageSetup) -> f64 {
    let long_in = page.width_mm.max(page.height_mm).max(1.0) / 25.4;
    (1600.0 / long_in).clamp(50.0, 300.0)
}

/// The caption of a preview of these settings: the sheet and its paper.
fn plot_caption(st: &PlotState) -> String {
    let mut info = format!("{} · {}", st.target, st.page.paper);
    if st.page.landscape {
        info.push_str(" · ");
        info.push_str(crate::tl!("landscape"));
    }
    info
}

/// The Plot Preview of the tab being shown: the `plot` parameters of a PNG of its sheet as its page
/// setup plots it (the Plot dialog's starting settings, so the two previews agree), and a caption.
pub(crate) fn current_preview(app: &CadApp) -> Result<(Value, String), String> {
    let d = app.session.doc().map_err(|e| e.to_string())?;
    let target = app.session.layout_space().tab_name().to_string();
    let page = plot_page(d, &target);
    let st = PlotState::new(0, target, page, [0.0; 4]);
    Ok((plot_params(&st, Some(preview_dpi(&st.page))), plot_caption(&st)))
}

/// The sheet in the chosen orientation: a paper with its printable area, its width and height
/// dimensioned on the bottom and right edges.
fn sheet_sketch(ui: &mut egui::Ui, page: &cadcraft_doc::PageSetup, t: &Tokens) {
    const PAPER: Color32 = Color32::from_rgb(250, 250, 247);
    let (w, h) = if page.landscape { (page.height_mm, page.width_mm) } else { (page.width_mm, page.height_mm) };
    ui.vertical(|ui| {
        // The dimension lines and their labels take the space the sheet leaves on its bottom and right.
        let (rect, _) = ui.allocate_exact_size(vec2(140.0, 124.0), Sense::hover());
        let area = egui::Rect::from_min_size(rect.min, rect.size() - vec2(30.0, 30.0));
        let k = (area.width() / w.max(1.0) as f32).min(area.height() / h.max(1.0) as f32);
        let sheet = egui::Rect::from_center_size(area.center(), vec2(w as f32 * k, h as f32 * k));
        let painter = ui.painter();
        painter.rect_filled(sheet.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(70));
        // The folded top-right corner is what makes it read as paper.
        let fold = sheet.width().min(sheet.height()) * 0.18;
        let (right, top) = (sheet.max.x, sheet.min.y);
        painter.add(egui::Shape::convex_polygon(
            vec![sheet.min, pos2(right - fold, top), pos2(right, top + fold), sheet.max, pos2(sheet.min.x, sheet.max.y)],
            PAPER,
            Stroke::new(1.0, t.border),
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(right - fold, top), pos2(right - fold, top + fold), pos2(right, top + fold)],
            Color32::from_gray(222),
            Stroke::new(1.0, t.border),
        ));
        let [l, b, r, top_margin] = page.margins_mm.map(|m| m as f32 * k);
        let printable = egui::Rect::from_min_max(sheet.min + vec2(l, top_margin), sheet.max - vec2(r, b));
        if printable.is_positive() {
            painter.rect_stroke(printable, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        }
        // Width along the bottom edge; height along the right edge, reading upwards.
        let dim = Stroke::new(1.0, t.text_dim);
        let font = FontId::proportional(11.0);
        let y = sheet.max.y + 10.0;
        painter.line_segment([pos2(sheet.min.x, y), pos2(sheet.max.x, y)], dim);
        for x in [sheet.min.x, sheet.max.x] {
            painter.line_segment([pos2(x, sheet.max.y + 2.0), pos2(x, y + 3.0)], dim);
        }
        dim_arrow(painter, pos2(sheet.min.x, y), vec2(1.0, 0.0), t.text_dim);
        dim_arrow(painter, pos2(sheet.max.x, y), vec2(-1.0, 0.0), t.text_dim);
        painter.text(pos2(sheet.center().x, y + 2.0), Align2::CENTER_TOP, format!("{w:.1} mm"), font.clone(), t.text_dim);
        let x = sheet.max.x + 10.0;
        painter.line_segment([pos2(x, sheet.min.y), pos2(x, sheet.max.y)], dim);
        for y in [sheet.min.y, sheet.max.y] {
            painter.line_segment([pos2(sheet.max.x + 2.0, y), pos2(x + 3.0, y)], dim);
        }
        dim_arrow(painter, pos2(x, sheet.min.y), vec2(0.0, 1.0), t.text_dim);
        dim_arrow(painter, pos2(x, sheet.max.y), vec2(0.0, -1.0), t.text_dim);
        // Rotated a quarter turn counter-clockwise about its top-left corner, so its centre lands beside the line.
        let galley = painter.layout_no_wrap(format!("{h:.1} mm"), font, t.text_dim);
        let (gw, gh) = (galley.size().x, galley.size().y);
        let mut label = egui::epaint::TextShape::new(pos2(x + 9.0 - gh / 2.0, sheet.center().y + gw / 2.0), galley, t.text_dim);
        label.angle = -std::f32::consts::FRAC_PI_2;
        painter.add(label);
    });
}

/// A filled arrowhead with its tip at `tip`, opening along `into` (the way from the tip into the dimension line).
fn dim_arrow(painter: &egui::Painter, tip: egui::Pos2, into: egui::Vec2, color: Color32) {
    let base = tip + into * 6.0;
    let side = vec2(-into.y, into.x) * 2.5;
    painter.add(egui::Shape::convex_polygon(vec![tip, base + side, base - side], color, Stroke::NONE));
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

    /// The Plot dialog's state for `target` in a new metric drawing.
    fn state(target: &str) -> PlotState {
        let d = cadcraft_doc::Drawing::new_metric();
        PlotState::new(0, target.into(), plot_page(&d, target), [0.0, 0.0, 10.0, 10.0])
    }

    /// Choosing a device gives the page one of its papers and printable areas, and the plot parameters name them.
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
        // "None" is sent as such: left out, a layout's own table would plot and preview in a dialog
        // that shows none.
        apply_page_change(&mut st.page, "plotStyleTable", &json!("None"));
        assert_eq!(plot_params(&st, None)["plotStyleTable"], json!("None"));
    }

    /// "Plot with plot styles" off plots without the table (kept in the dialog); "Plot stamp
    /// on" sends a stamp naming the drawing, sheet, date, device, paper and scale.
    #[test]
    fn plot_styles_switch_and_stamp_reach_the_plot() {
        let mut st = state("Model");
        apply_page_change(&mut st.page, "plotStyleTable", &json!("monochrome.ctb"));
        st.with_styles = false;
        assert_eq!(plot_params(&st, None)["plotStyleTable"], json!("None"));
        assert_eq!(plot_params(&st, Some(150.0))["plotStyleTable"], json!("None"), "the preview agrees");
        assert_eq!(st.page.plot_style_table, "monochrome.ctb", "the table is kept");
        st.with_styles = true;
        assert_eq!(plot_params(&st, None)["plotStyleTable"], json!("monochrome.ctb"));
        assert!(plot_params(&st, None).get("stamp").is_none());
        st.stamp = true;
        let stamp = plot_params(&st, None)["stamp"].as_str().unwrap().to_string();
        let paper = st.page.paper.clone();
        assert!(stamp.starts_with("{drawing}, {layout}, ") && stamp.ends_with(&format!(" UTC, CADCraft PDF, {paper}, {{scale}}")), "{stamp}");
        assert!(plot_params(&st, Some(150.0)).get("stamp").is_some(), "the preview shows the stamp too");
    }

    /// Where a plot goes and how a device is listed: a system printer prints, the system's
    /// default printer says so.
    #[test]
    fn printers_are_listed_and_described() {
        let mut dev = cadcraft_doc::PlotterConfig { name: "Office".into(), system: true, ..Default::default() };
        assert_eq!(device_name(&dev), "Office");
        assert_eq!(device_where(Some(&dev)), "System printer");
        dev.system_default = true;
        assert_eq!(device_name(&dev), "Office (default)");
        let png = cadcraft_engine::cmd::plotters::find_device("CADCraft PNG").unwrap();
        assert_eq!(device_where(Some(&png)), "File (PNG)");
        assert_eq!(device_where(None), "File (PDF)");
    }

    /// "Edit…" next to the plot style table opens the Plot Style Manager; closing it brings Plot
    /// back with its settings.
    #[test]
    fn the_plot_style_manager_gives_plot_back() {
        let ctx = egui::Context::default();
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        app.ui.dialog = Some("stylesmanager".into());
        return_to_plot(&mut app, &ctx);
        assert_eq!(app.ui.dialog.as_deref(), Some("stylesmanager"), "not opened from Plot: nothing changes");
        set_temp(&ctx, RETURN_TO_PLOT, true);
        return_to_plot(&mut app, &ctx);
        assert_eq!(app.ui.dialog.as_deref(), Some("plot"));
        assert_eq!(temp::<bool>(&ctx, RETURN_TO_PLOT), Some(false), "once");
    }

    /// Apply to Layout sends the chosen device, plot style table and paper to the layout's page setup.
    #[test]
    fn apply_to_layout_saves_selected_device_and_plot_style() {
        let mut st = state("Layout1");
        apply_page_change(&mut st.page, "device", &json!("CADCraft PNG"));
        apply_page_change(&mut st.page, "plotStyleTable", &json!("monochrome.ctb"));

        let params = apply_to_layout_params(&st);
        assert_eq!(params["layout"], json!("Layout1"));
        assert_eq!(params["device"], json!("CADCraft PNG"));
        assert_eq!(params["paper"], json!(st.page.paper));
        assert_eq!(params["plotStyleTable"], json!("monochrome.ctb"));
        assert_eq!(params["margins"], json!(st.page.margins_mm));

        // Back to no device: the device's printable area is not saved with the layout.
        st.page.margins_mm = [1.0, 2.0, 3.0, 4.0];
        apply_page_change(&mut st.page, "device", &json!("None"));
        let params = apply_to_layout_params(&st);
        assert_eq!(params["device"], json!("None"));
        assert_eq!(params["margins"], json!(cadcraft_doc::PageSetup::default().margins_mm));
        let mut s = Session::new();
        s.execute("pagesetup", &params).unwrap();
        assert_eq!(plot_page(s.doc().unwrap(), "Layout1").margins_mm, cadcraft_doc::PageSetup::default().margins_mm);
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

    /// After picking a plot window on screen, the Plot dialog comes back with that window.
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

#[cfg(test)]
mod plot_sheets_tests {
    use super::*;

    /// Model and three layouts, in tab order.
    fn targets() -> Vec<String> {
        ["Model", "Layout1", "Layout2", "Sheet 3"].map(String::from).to_vec()
    }

    /// Sheets of deleted layouts drop out; one left becomes the target.
    #[test]
    fn deleted_sheets_drop_out_and_a_single_sheet_left_is_the_target() {
        let t = targets();
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Nothing deleted: nothing changes, whatever the target (it may have been unticked).
        let (mut sheets, mut target) = (names(&["Layout1", "Layout2"]), "Sheet 3".to_string());
        assert!(!sync_sheets(&mut sheets, &mut target, &t));
        assert_eq!((sheets.as_slice(), target.as_str()), (names(&["Layout1", "Layout2"]).as_slice(), "Sheet 3"));
        // One of three ticked layouts is deleted while the target was not among them: two are left.
        let (mut sheets, mut target) = (names(&["Layout1", "Gone", "Layout2"]), "Sheet 3".to_string());
        assert!(!sync_sheets(&mut sheets, &mut target, &t));
        assert_eq!(sheets, ["Layout1", "Layout2"]);
        // One left: it becomes the target, so the dialog's single sheet is what OK plots.
        let (mut sheets, mut target) = (names(&["Gone", "Layout2"]), "Layout1".to_string());
        assert!(sync_sheets(&mut sheets, &mut target, &t));
        assert_eq!((sheets, target.as_str()), (names(&["Layout2"]), "Layout2"));
        // None left: the target alone.
        let (mut sheets, mut target) = (names(&["Gone", "Also gone"]), "Layout1".to_string());
        assert!(!sync_sheets(&mut sheets, &mut target, &t));
        assert_eq!((sheets, target.as_str()), (names(&["Layout1"]), "Layout1"));
    }
    /// Ticking keeps tab order and leaves at least one sheet.
    #[test]
    fn ticking_sheets_keeps_tab_order_and_at_least_one() {
        let t = targets();
        let mut sheets = vec!["Layout2".to_string()];
        toggle_sheet(&mut sheets, &t, "Model", true);
        toggle_sheet(&mut sheets, &t, "Sheet 3", true);
        assert_eq!(sheets, ["Model", "Layout2", "Sheet 3"], "tab order, not the order they were ticked");
        toggle_sheet(&mut sheets, &t, "Layout2", false);
        assert_eq!(sheets, ["Model", "Sheet 3"]);
        toggle_sheet(&mut sheets, &t, "Model", false);
        toggle_sheet(&mut sheets, &t, "Sheet 3", false);
        assert_eq!(sheets, ["Sheet 3"], "the last sheet can't be unticked");
        toggle_sheet(&mut sheets, &t, "Nope", true);
        assert_eq!(sheets, ["Sheet 3"], "an unknown name changes nothing");
    }

    /// Several sheets plot with no page overrides, so each uses its own page setup.
    #[test]
    fn several_sheets_plot_with_their_own_page_setups_into_one_pdf() {
        let p = sheets_params(&["Layout1".to_string(), "Layout2".to_string()]);
        assert_eq!(p, json!({ "layouts": ["Layout1", "Layout2"], "layout": "Layout1", "device": "None" }));
        // No page overrides: each sheet keeps its paper, orientation, style table and lineweights.
        for k in ["paper", "width", "height", "landscape", "plotStyleTable", "lineweights", "plotTransparency", "fit"] {
            assert!(p.get(k).is_none(), "{k} would apply to every sheet");
        }
    }

    /// The summary names each sheet's paper and style table.
    #[test]
    fn the_summary_names_each_sheets_paper_and_style() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        app.run("pagesetup", json!({ "layout": "Layout1", "paper": "A3", "plotStyleTable": "monochrome.ctb" })).unwrap();
        let d = app.session.doc().unwrap();
        let line = sheet_summary(d, "Layout1");
        assert!(line.starts_with("Layout1: ") && line.contains("A3") && line.ends_with("monochrome.ctb"), "{line}");
        assert!(sheet_summary(d, "Model").starts_with("Model: "));
    }
}

#[cfg(test)]
mod page_setup_selection_tests {
    use super::*;

    const ORDER: [&str; 5] = ["Layout1", "Layout2", "Sheet 3", "Sheet 4", "Sheet 5"];

    /// Modifier keys for a click.
    fn mods(shift: bool, command: bool) -> egui::Modifiers {
        egui::Modifiers { shift, command, ..Default::default() }
    }

    /// Ctrl alone counts like Cmd, as some key events set only `ctrl`.
    #[test]
    fn ctrl_alone_counts_like_command() {
        // Synthetic and some platforms' key events set `ctrl` without `command`.
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        let (mut picked, mut sel) = (vec!["Layout1".to_string()], "Layout1".to_string());
        click_layout(&mut picked, &mut sel, &ORDER, "Sheet 3", ctrl);
        assert_eq!(picked, ["Layout1", "Sheet 3"]);
    }

    /// A click on layout `name` with modifiers `m`.
    fn click(picked: &mut Vec<String>, sel: &mut String, name: &str, m: egui::Modifiers) {
        click_layout(picked, sel, &ORDER, name, m);
    }

    /// Plain, Ctrl and Shift clicks pick one, toggle, and take a range.
    #[test]
    fn click_picks_one_ctrl_toggles_shift_takes_a_range() {
        let (mut picked, mut sel) = (vec!["Layout1".to_string()], "Layout1".to_string());
        click(&mut picked, &mut sel, "Sheet 3", mods(false, false));
        assert_eq!((picked.as_slice(), sel.as_str()), (&["Sheet 3".to_string()][..], "Sheet 3"));
        // Ctrl adds and shows the one just added.
        click(&mut picked, &mut sel, "Layout1", mods(false, true));
        click(&mut picked, &mut sel, "Sheet 5", mods(false, true));
        assert_eq!(picked, ["Sheet 3", "Layout1", "Sheet 5"]);
        assert_eq!(sel, "Sheet 5");
        // Ctrl on a picked layout removes it; the shown one moves to another picked.
        click(&mut picked, &mut sel, "Sheet 5", mods(false, true));
        assert_eq!((picked.as_slice(), sel.as_str()), (&["Sheet 3".to_string(), "Layout1".to_string()][..], "Layout1"));
        // The last picked layout can't be removed.
        click(&mut picked, &mut sel, "Layout1", mods(false, true));
        click(&mut picked, &mut sel, "Sheet 3", mods(false, true));
        assert_eq!(picked, ["Sheet 3"], "{picked:?}");
        // Shift takes the range from the shown layout, in tab order, whichever way it goes.
        click(&mut picked, &mut sel, "Sheet 5", mods(true, false));
        assert_eq!(picked, ["Sheet 3", "Sheet 4", "Sheet 5"]);
        click(&mut picked, &mut sel, "Layout2", mods(true, false));
        assert_eq!(picked, ["Layout2", "Sheet 3"], "from the shown layout (Sheet 3) back to Layout2");
    }

    /// A change for several picked layouts is one `pagesetup` with `layouts`.
    #[test]
    fn a_change_goes_to_all_picked_layouts_in_one_command() {
        let one = pagesetup_change(&["Layout1".to_string()], "Layout1", "paper", json!("A3"));
        assert_eq!(one, json!({ "layout": "Layout1", "paper": "A3" }));
        let many = pagesetup_change(&["Layout1".to_string(), "Sheet 3".to_string()], "Sheet 3", "plotStyleTable", json!("monochrome.ctb"));
        assert_eq!(many, json!({ "layouts": ["Layout1", "Sheet 3"], "plotStyleTable": "monochrome.ctb" }));
        // And the engine takes exactly that.
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        app.run("layout.new", json!({ "name": "Sheet 3" })).unwrap();
        app.run("pagesetup", many).unwrap();
        let d = app.session.doc().unwrap();
        assert_eq!(d.layout("Layout1").map(|l| l.page.plot_style_table.as_str()), Some("monochrome.ctb"));
        assert_eq!(d.layout("Sheet 3").map(|l| l.page.plot_style_table.as_str()), Some("monochrome.ctb"));
        assert_ne!(d.layout("Layout2").map(|l| l.page.plot_style_table.as_str()), Some("monochrome.ctb"), "an unpicked layout is untouched");
    }
}
