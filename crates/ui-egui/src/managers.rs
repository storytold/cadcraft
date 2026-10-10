//! Style and settings dialogs opened from the menus: Linetype Manager, Point Style, Table Style,
//! Multileader Style, Page Setup Manager, Plot and Constraint Settings.
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

/// Open the dialog for a typed or menu-invoked command. `None` if `id` has no dialog here or the
/// call carries parameters.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() {
        return None;
    }
    let dialog = match id {
        "linetype" | "lt" | "ltype" => "linetype",
        "ddptype" => "ptype",
        "tablestyle" | "ts" => "tablestyle",
        "mleaderstyle" | "mls" => "mleaderstyle",
        "pagesetup" => "pagesetup",
        "constraintsettings" | "csettings" => "csettings",
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
        "linetype" => linetype(app, ctx, open),
        "ptype" => point_style(app, ctx, open),
        "tablestyle" => table_style(app, ctx, open),
        "mleaderstyle" => mleader_style(app, ctx, open),
        "pagesetup" => page_setup(app, ctx, open),
        "csettings" => constraint_settings(app, ctx, open),
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
fn num_field(ui: &mut egui::Ui, id: impl egui::AsId, v: f64) -> Option<f64> {
    expr_field(ui, egui::Id::new(id), &fmt_num(v)).and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite())
}

fn temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(egui::Id::new(id)))
}

fn set_temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(id), v));
}

fn no_drawing(ctx: &egui::Context, title: &str, open: &mut bool) {
    egui::Window::new(title).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label("Open a drawing first.");
    });
}

// ---------- Linetype Manager ----------

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
            egui::ComboBox::from_id_salt("ltmgr_load").selected_text("Load...").width(110.0).show_ui(ui, |ui| {
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
            ui.label("Global scale factor");
            if let Some(v) = num_field(ui, "ltmgr_scale", ltscale).filter(|v| *v > 0.0) {
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
            ui.label("Point Size");
            if let Some(v) = num_field(ui, "ptype_size", size).filter(|v| *v > 0.0) {
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

// ---------- Table Style and Multileader Style ----------

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
                ui.add(egui::TextEdit::singleline(&mut new_name).hint_text("New style name").desired_width(120.0));
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
                            ui.label(*label);
                            num_field(ui, (cmd, &style.name, *key), *v).filter(|v| *v >= 0.0).map(|v| (*key, json!(v)))
                        }
                        Field::Bool(key, label, v) => {
                            ui.label("");
                            let mut b = *v;
                            ui.checkbox(&mut b, *label).changed().then(|| (*key, json!(b)))
                        }
                        Field::Choice(key, label, v, choices) => {
                            ui.label(*label);
                            let mut picked = None;
                            egui::ComboBox::from_id_salt((cmd, *key)).selected_text(v.as_str()).show_ui(ui, |ui| {
                                for c in choices {
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

fn page_setup(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Page Setup Manager", open);
        return None;
    };
    let mut layouts: Vec<(u32, String, cadcraft_doc::PageSetup)> = d.layouts.iter().map(|l| (l.tab_order, l.name.clone(), l.page.clone())).collect();
    layouts.sort_by_key(|l| l.0);
    let shown = match app.session.layout_space() {
        cadcraft_doc::Space::Paper(n) => Some(n),
        cadcraft_doc::Space::Model => None,
    };
    let mut sel = temp::<String>(ctx, "pagesetup_sel")
        .filter(|s| layouts.iter().any(|l| l.1 == *s))
        .or(shown)
        .or_else(|| layouts.first().map(|l| l.1.clone()))
        .unwrap_or_default();
    let mut action = None;
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
            let rows = PageRows { areas: &["layout", "extents", "display", "limits"], scale: true, center: true };
            egui::Grid::new("pagesetup_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(right, |ui| {
                if let Some((k, v)) = page_rows(ui, egui::Id::new(("pagesetup", name)), page, &rows) {
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

/// The page-setup rows (paper, orientation, plot area, fit, scale, center, lineweights) shared by
/// the Page Setup Manager and the Plot dialog, inside a two-column grid. Returns the setting the
/// user changed as a `pagesetup` parameter (`paper`, `landscape`, `plotArea`, `scaleToFit`,
/// `scale`, `center` or `lineweights`).
fn page_rows(ui: &mut egui::Ui, salt: egui::Id, page: &cadcraft_doc::PageSetup, rows: &PageRows) -> Option<(&'static str, Value)> {
    let mut change = None;
    ui.label("Paper size");
    egui::ComboBox::from_id_salt(salt.with("paper")).selected_text(page.paper.as_str()).width(190.0).show_ui(ui, |ui| {
        for p in cadcraft_render::PAPER_SIZES {
            if ui.selectable_label(p.name == page.paper, p.name).clicked() && p.name != page.paper {
                change = Some(("paper", json!(p.name)));
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
        egui::ComboBox::from_id_salt(salt.with("area")).selected_text(area_label(&page.plot_area)).show_ui(ui, |ui| {
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
    if rows.scale {
        ui.label("Scale");
        ui.add_enabled_ui(!page.scale_to_fit, |ui| {
            if let Some(v) = expr_field(ui, salt.with("scale"), &fmt_num(page.scale)).and_then(|t| parse_ratio(&t)) {
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
    change
}

/// Apply a [`page_rows`] change to a page setup held by a dialog.
fn apply_page_change(page: &mut cadcraft_doc::PageSetup, key: &str, v: &Value) {
    match key {
        "paper" => {
            if let Some(ps) = v.as_str().and_then(cadcraft_render::paper_size) {
                page.paper = ps.name.into();
                page.width_mm = ps.width_mm;
                page.height_mm = ps.height_mm;
            }
        }
        "plotArea" => page.plot_area = v.as_str().unwrap_or("extents").into(),
        "scale" => page.scale = v.as_f64().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(page.scale),
        _ => {
            let b = v.as_bool().unwrap_or_default();
            match key {
                "landscape" => page.landscape = b,
                "scaleToFit" => page.scale_to_fit = b,
                "center" => page.center = b,
                "lineweights" => page.lineweights = b,
                _ => {}
            }
        }
    }
}

// ---------- Plot ----------

/// Bumped each time PLOT opens its dialog, so it starts again from the page setup.
static PLOT_GEN: AtomicU32 = AtomicU32::new(0);

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

/// The `plot` parameters for the dialog's settings (without the output path).
fn plot_params(st: &PlotState) -> Value {
    let page = &st.page;
    let mut p = json!({
        "layout": st.target,
        "landscape": page.landscape,
        "fit": page.scale_to_fit,
        "lineweights": page.lineweights,
    });
    if let Some(o) = p.as_object_mut() {
        if cadcraft_render::paper_size(&page.paper).is_some() {
            o.insert("paper".into(), json!(page.paper));
        } else {
            o.insert("width".into(), json!(page.width_mm));
            o.insert("height".into(), json!(page.height_mm));
        }
        if st.target == "Model" {
            o.insert("plotArea".into(), json!(page.plot_area));
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
        let target = match app.session.layout_space() {
            cadcraft_doc::Space::Paper(n) => n,
            cadcraft_doc::Space::Model => "Model".into(),
        };
        let mut ext = d.extents(&cadcraft_doc::Space::Model);
        if ext.is_empty() {
            ext = cadcraft_geom::Bounds2::new(cadcraft_geom::Vec2::ZERO, cadcraft_geom::Vec2::new(12.0, 9.0));
        }
        PlotState { generation, page: plot_page(d, &target), target, window: [ext.min.x, ext.min.y, ext.max.x, ext.max.y] }
    });
    let model = st.target == "Model";
    let (mut ok, mut cancel, mut action) = (false, false, None);
    egui::Window::new("Plot").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("plot_target").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Printer/plotter");
            ui.label("PDF (vector)").on_hover_text("CADCraft plots to vector PDF files");
            ui.end_row();
            ui.label("Layout");
            egui::ComboBox::from_id_salt("plot_layout").selected_text(st.target.as_str()).width(190.0).show_ui(ui, |ui| {
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
                if let Some((k, v)) = page_rows(ui, egui::Id::new(("plot", st.target.as_str())), &st.page, &rows) {
                    apply_page_change(&mut st.page, k, &v);
                }
                if model && st.page.plot_area == "window" {
                    for (i, label) in [(0, "First corner"), (2, "Opposite corner")] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            for (j, axis) in ["X", "Y"].iter().enumerate() {
                                ui.label(*axis);
                                let k = i + j;
                                if let Some(slot) = st.window.get_mut(k)
                                    && let Some(v) = num_field(ui, ("plot_window", k), *slot)
                                {
                                    *slot = v;
                                }
                            }
                        });
                        ui.end_row();
                    }
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
                    json!({ "layout": st.target, "paper": page.paper, "landscape": page.landscape, "scaleToFit": page.scale_to_fit, "lineweights": page.lineweights }),
                ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button("Cancel").clicked();
                ok = ui.button("OK").clicked();
            });
        });
    });
    set_temp(ctx, "plot_state", st.clone());
    if cancel {
        *open = false;
    }
    if ok {
        let mut params = plot_params(&st);
        if let Some(pick) = app.services.pick_save.as_ref() {
            let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
            let suggested = format!("{}-{}.pdf", stem.unwrap_or_else(|| "Drawing".into()), st.target);
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
            ui.label("Constraint bar transparency (%)");
            if let Some(v) = num_field(ui, "csettings_transp", f64::from(s.bar_transparency)) {
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
            ui.label("Distance tolerance");
            if let Some(v) = num_field(ui, "csettings_dist", s.distance_tolerance) {
                action = set("distanceTolerance", json!(v));
            }
            ui.end_row();
            ui.label("Angle tolerance (degrees)");
            if let Some(v) = num_field(ui, "csettings_angle", s.angle_tolerance) {
                action = set("angleTolerance", json!(v));
            }
            ui.end_row();
        });
    });
    action
}
