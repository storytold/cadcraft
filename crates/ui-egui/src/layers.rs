//! The Layer Properties Manager (LAYER): filter tree, layer table with inline editing, colour /
//! linetype / lineweight pickers, layer states and, inside a layout viewport, the VP Freeze and
//! VP Color columns. Every change runs an engine command (`layer.*`, `layerstate.*`, `vplayer`,
//! `linetype`).

use cadcraft_color::{Color, aci_rgb};
use cadcraft_doc::{Handle, Layer, Lineweight};
use egui::{Color32, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// Dialog-local UI state (kept in egui memory between frames).
#[derive(Clone, Debug, Default)]
struct LpmState {
    /// 0 = All, 1 = All Used Layers.
    filter: u8,
    search: String,
    selected: Option<String>,
    /// (layer, buffer) while renaming.
    renaming: Option<(String, String)>,
    /// (layer, buffer) while editing the description.
    describing: Option<(String, String)>,
    sort_desc: bool,
    state_name: String,
    state_sel: Option<String>,
}

fn rgb32(c: Color) -> Color32 {
    let r = c.resolve(Color::Index(7), Color::Index(7));
    Color32::from_rgb(r.0, r.1, r.2)
}

/// A colour swatch.
pub fn swatch(p: &egui::Painter, r: Rect, c: Color) {
    p.rect_filled(r, 1.0, rgb32(c));
    p.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_gray(25)), egui::StrokeKind::Inside);
}

/// A colour button (swatch + name) that opens the Select Color popup: the AutoCAD Color Index
/// palette and a true-colour editor. Returns the picked colour. `by` adds ByLayer/ByBlock.
pub fn color_button(ui: &mut egui::Ui, id: egui::Id, current: Color, by: bool, width: f32) -> Option<Color> {
    let t = Tokens::get();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 2.0, t.control_hover.gamma_multiply(0.5));
    }
    swatch(p, Rect::from_min_size(pos2(rect.left() + 3.0, rect.center().y - 6.0), vec2(12.0, 12.0)), current);
    p.text(pos2(rect.left() + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, current.name(), crate::theme::body(), t.text);
    let mut out = None;
    egui::Popup::from_toggle_button_response(&resp).id(id).width(300.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        out = color_palette(ui, id, current, by);
        if out.is_some() {
            ui.close();
        }
    });
    out
}

/// Layer-manager color control: localized label, unchanged DXF color selection.
fn color_button_localized(ui: &mut egui::Ui, id: egui::Id, current: Color, by: bool, width: f32, language: &str) -> Option<Color> {
    let t = Tokens::get();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() { p.rect_filled(rect, 2.0, t.control_hover.gamma_multiply(0.5)); }
    swatch(p, Rect::from_min_size(pos2(rect.left() + 3.0, rect.center().y - 6.0), vec2(12.0, 12.0)), current);
    let original = current.name();
    let label = crate::i18n::tr(language, &original);
    p.text(pos2(rect.left() + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, label, crate::theme::body(), t.text);
    let mut out = None;
    egui::Popup::from_toggle_button_response(&resp).id(id).width(300.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        out = color_palette(ui, id, current, by);
        if out.is_some() { ui.close(); }
    });
    out
}

/// The palette body (index colours + true colour).
pub fn color_palette(ui: &mut egui::Ui, id: egui::Id, current: Color, by: bool) -> Option<Color> {
    let t = Tokens::get();
    let tab_id = id.with("tab");
    let mut tab = ui.data_mut(|d| d.get_temp::<u8>(tab_id)).unwrap_or(if matches!(current, Color::True(_)) { 1 } else { 0 });
    let mut out = None;
    ui.horizontal(|ui| {
        for (i, l) in ["Index Color", "True Color"].iter().enumerate() {
            if ui.selectable_label(tab == i as u8, *l).clicked() {
                tab = i as u8;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    ui.separator();
    let cell = |ui: &mut egui::Ui, i: u8, size: f32, out: &mut Option<Color>| {
        let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
        let c = aci_rgb(i);
        ui.painter().rect_filled(r.shrink(0.5), 0.0, Color32::from_rgb(c.0, c.1, c.2));
        let sel = current == Color::Index(i);
        if sel || resp.hovered() {
            ui.painter().rect_stroke(r, 0.0, Stroke::new(if sel { 2.0 } else { 1.0 }, Color32::WHITE), egui::StrokeKind::Inside);
        }
        if resp.on_hover_text(format!("Index color: {i}")).clicked() {
            *out = Some(Color::Index(i));
        }
    };
    if tab == 0 {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // The 240 graded colours: 24 hues × 10 shades.
        for shade in [0u8, 2, 4, 6, 8, 1, 3, 5, 7, 9] {
            ui.horizontal(|ui| {
                for hue in 0..24u8 {
                    cell(ui, 10 + hue * 10 + shade, 11.0, &mut out);
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for i in 1..=9u8 {
                cell(ui, i, 18.0, &mut out);
                ui.add_space(2.0);
            }
            ui.add_space(10.0);
            for i in 250..=255u8 {
                cell(ui, i, 18.0, &mut out);
                ui.add_space(2.0);
            }
        });
        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
        ui.add_space(6.0);
        if by {
            ui.horizontal(|ui| {
                if ui.button("ByLayer").clicked() {
                    out = Some(Color::ByLayer);
                }
                if ui.button("ByBlock").clicked() {
                    out = Some(Color::ByBlock);
                }
            });
        }
        ui.label(RichText::new(format!("Current: {}", current.name())).small().color(t.text_dim));
    } else {
        let rid = id.with("rgb");
        let start = current.resolve(Color::Index(7), Color::Index(7));
        let mut c = ui.data_mut(|d| d.get_temp::<[u8; 3]>(rid)).unwrap_or([start.0, start.1, start.2]);
        let mut c32 = Color32::from_rgb(c[0], c[1], c[2]);
        egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::Opaque);
        c = [c32.r(), c32.g(), c32.b()];
        ui.horizontal(|ui| {
            for (i, l) in ["R", "G", "B"].iter().enumerate() {
                ui.label(*l);
                if let Some(v) = c.get_mut(i) {
                    ui.add(egui::DragValue::new(v).range(0..=255));
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(rid, c));
        if ui.button("Apply true color").clicked() {
            out = Some(Color::True(cadcraft_color::Rgb(c[0], c[1], c[2])));
            ui.data_mut(|d| d.remove::<[u8; 3]>(rid));
        }
    }
    out
}

/// A small horizontal bar showing a lineweight.
fn lw_sample(p: &egui::Painter, r: Rect, lw: Lineweight, c: Color32) {
    let w = match lw {
        Lineweight::Mm100(v) => (f32::from(v) / 100.0 * 3.0).clamp(1.0, 7.0),
        _ => 1.0,
    };
    p.line_segment([pos2(r.left(), r.center().y), pos2(r.right(), r.center().y)], Stroke::new(w, c));
}

/// A linetype preview drawn from its own dash pattern.
fn lt_sample(p: &egui::Painter, r: Rect, lt: Option<&cadcraft_doc::Linetype>, c: Color32) {
    let y = r.center().y;
    let Some(lt) = lt.filter(|l| !l.pattern.is_empty() && l.pattern.iter().any(|e| e.length.abs() > 1e-9)) else {
        p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, c));
        return;
    };
    let total: f64 = lt.pattern.iter().map(|e| e.length.abs()).sum::<f64>().max(1e-9);
    let k = f64::from(r.width()) / 2.0 / total;
    let mut x = f64::from(r.left());
    let mut guard = 0;
    while x < f64::from(r.right()) && guard < 200 {
        for e in &lt.pattern {
            let l = e.length.abs() * k;
            let x1 = (x + l).min(f64::from(r.right()));
            if e.length > 0.0 {
                p.line_segment([pos2(x as f32, y), pos2(x1 as f32, y)], Stroke::new(1.0, c));
            } else if e.length == 0.0 {
                p.circle_filled(pos2(x as f32, y), 0.8, c);
            }
            x = x1.max(x + 1.0);
            guard += 1;
        }
    }
}

/// Status column: a green check for the current layer, a sheet for the others.
fn status_icon(p: &egui::Painter, r: Rect, current: bool, used: bool) {
    if current {
        let g = Color32::from_rgb(0x4c, 0xc2, 0x5a);
        p.line_segment([pos2(r.left() + 3.0, r.center().y), pos2(r.left() + 7.0, r.bottom() - 4.0)], Stroke::new(2.0, g));
        p.line_segment([pos2(r.left() + 7.0, r.bottom() - 4.0), pos2(r.right() - 3.0, r.top() + 3.0)], Stroke::new(2.0, g));
    } else {
        let t = Tokens::get();
        let c = if used { t.icon } else { t.text_faint };
        let pts = vec![
            pos2(r.left() + 2.0, r.center().y + 1.0),
            pos2(r.center().x, r.top() + 4.0),
            pos2(r.right() - 2.0, r.center().y - 1.0),
            pos2(r.center().x, r.bottom() - 3.0),
        ];
        p.add(egui::Shape::closed_line(pts, Stroke::new(1.2, c)));
    }
}

fn natural_lower(s: &str) -> String {
    s.to_lowercase()
}

/// The Layer Properties Manager window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let language = app.ui.language.clone();
    let tr = |english: &str| crate::i18n::tr(&language, english).to_owned();
    let t = Tokens::get();
    let sid = egui::Id::new("lpm_state");
    let mut st: LpmState = ctx.data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    let mut actions: Vec<(&str, Value)> = Vec::new();
    let Ok(dst) = app.session.state() else {
        *open = false;
        return;
    };
    let d = &dst.doc;
    let cur = d.header.str("CLAYER", "0");
    let used = used_layers(d);
    let vp: Option<(Handle, cadcraft_doc::Viewport)> = dst.active_viewport();
    let linetypes: Vec<cadcraft_doc::Linetype> = d.linetypes.clone();
    let library: Vec<String> = cadcraft_doc::library::standard_linetypes().into_iter().map(|l| l.name).filter(|n| d.linetype(n).is_none()).collect();
    let states: Vec<String> = d.layer_states.iter().map(|s| s.name.clone()).collect();
    let mut layers: Vec<Layer> = d.layers.clone();
    layers.sort_by_key(|l| natural_lower(&l.name));
    if st.sort_desc {
        layers.reverse();
    }
    let total = layers.len();
    let q = st.search.to_lowercase();
    let shown: Vec<Layer> = layers
        .into_iter()
        .filter(|l| st.filter == 0 || used.contains(&l.name.to_ascii_lowercase()))
        .filter(|l| q.is_empty() || crate_wild(&q, &l.name.to_lowercase()))
        .collect();
    let title = match &vp {
        Some(_) => "Layer Properties Manager — current viewport",
        None => "Layer Properties Manager",
    };
    egui::Window::new(tr(title)).id(egui::Id::new("lpm_window")).open(open).default_size(vec2(1180.0, 440.0)).min_width(640.0).resizable(true).show(
        ctx,
        |ui| {
            // ----- tool row -----
            ui.horizontal(|ui| {
                if icons::button(ui, Icon::Plus, 24.0, &tr("New Layer"), false).clicked() {
                    let mut n = 1;
                    while d.layer(&format!("Layer{n}")).is_some() {
                        n += 1;
                    }
                    let name = format!("Layer{n}");
                    actions.push(("layer.new", json!({ "name": name })));
                    st.renaming = Some((name.clone(), name.clone()));
                    st.selected = Some(name);
                }
                if icons::button(ui, Icon::Close, 24.0, &tr("Delete Layer"), false).clicked()
                    && let Some(s) = &st.selected
                {
                    actions.push(("layer.delete", json!({ "name": s })));
                }
                if icons::button(ui, Icon::MakeCurrent, 24.0, &tr("Set Current"), false).clicked()
                    && let Some(s) = &st.selected
                {
                    actions.push(("layer.current", json!({ "name": s })));
                }
                ui.separator();
                // Layer states.
                ui.label(tr("Layer State"));
                let label = st.state_sel.clone().unwrap_or_else(|| tr("Unsaved Layer State"));
                egui::ComboBox::from_id_salt("lpm_states").selected_text(label).width(170.0).show_ui(ui, |ui| {
                    for s in &states {
                        if ui.selectable_label(st.state_sel.as_deref() == Some(s), s).clicked() {
                            st.state_sel = Some(s.clone());
                        }
                    }
                    if states.is_empty() {
                        ui.label(RichText::new(tr("No saved layer states")).color(t.text_dim));
                    }
                });
                let has = st.state_sel.is_some();
                if ui.add_enabled(has, egui::Button::new(tr("Restore"))).clicked()
                    && let Some(s) = &st.state_sel
                {
                    actions.push(("layerstate.restore", json!({ "name": s })));
                }
                if ui.add_enabled(has, egui::Button::new(tr("Delete"))).clicked()
                    && let Some(s) = st.state_sel.take()
                {
                    actions.push(("layerstate.delete", json!({ "name": s })));
                }
                ui.add(egui::TextEdit::singleline(&mut st.state_name).hint_text(tr("New state name")).desired_width(185.0));
                if ui.add_enabled(!st.state_name.trim().is_empty(), egui::Button::new(tr("Save State"))).clicked() {
                    let n = st.state_name.trim().to_string();
                    actions.push(("layerstate.save", json!({ "name": n })));
                    st.state_sel = Some(n);
                    st.state_name.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.search).hint_text(tr("Search for layer")).desired_width(160.0));
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(ui.cursor().right_center() - vec2(8.0, 0.0), vec2(14.0, 14.0)),
                        Icon::Search,
                        false,
                    );
                    ui.add_space(18.0);
                });
            });
            ui.separator();
            // A fixed-height table: the window keeps its size instead of growing to the screen.
            let avail = 330.0;
            ui.horizontal_top(|ui| {
                // ----- filter tree -----
                ui.allocate_ui_with_layout(vec2(150.0, avail), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_width(150.0);
                    ui.label(RichText::new(tr("Filters")).small().color(t.text_dim));
                    for (i, l) in ["All", "All Used Layers"].iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_space(if i == 0 { 0.0 } else { 14.0 });
                            if ui.selectable_label(st.filter == i as u8, tr(l)).clicked() {
                                st.filter = i as u8;
                            }
                        });
                    }
                    ui.add_space(8.0);
                    if let Some((h, _)) = &vp {
                        ui.label(RichText::new(format!("Viewport {}", h.hex())).small().color(t.text_dim));
                    }
                });
                ui.separator();
                // ----- the table -----
                ui.vertical(|ui| {
                    egui::ScrollArea::both().id_salt("lpm_table").auto_shrink([false, false]).max_height(avail).show(ui, |ui| {
                        let mut cols: Vec<&str> =
                            vec!["Status", "Name", "On", "Freeze", "Lock", "Plot", "Color", "Linetype", "Lineweight", "Transparency"];
                        if vp.is_some() {
                            cols.extend(["VP Freeze", "VP Color"]);
                        }
                        cols.extend(["New VP Freeze", "Description"]);
                        egui::Grid::new("lpm_grid").striped(true).num_columns(cols.len()).spacing(vec2(10.0, 3.0)).min_row_height(22.0).show(
                            ui,
                            |ui| {
                                for h in &cols {
                                    if *h == "Name" {
                                        let arrow = if st.sort_desc { " ▼" } else { " ▲" };
                                        if ui.add(egui::Label::new(RichText::new(format!("{}{arrow}", tr("Name"))).strong()).sense(Sense::click())).clicked() {
                                            st.sort_desc = !st.sort_desc;
                                        }
                                    } else {
                                        ui.label(RichText::new(tr(h)).strong());
                                    }
                                }
                                ui.end_row();
                                for l in &shown {
                                    row(ui, l, &cur, &used, vp.as_ref(), &linetypes, &library, &language, &mut st, &mut actions);
                                    ui.end_row();
                                }
                            },
                        );
                    });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}: {} / {}.   {}: {}", tr("Displayed layers"), shown.len(), total, tr("Current layer"), cur))
                        .small()
                        .color(t.text_dim),
                );
            });
        },
    );
    ctx.data_mut(|dd| dd.insert_temp(sid, st));
    for (c, p) in actions {
        let _ = app.run(c, p);
    }
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    l: &Layer,
    cur: &str,
    used: &std::collections::HashSet<String>,
    vp: Option<&(Handle, cadcraft_doc::Viewport)>,
    linetypes: &[cadcraft_doc::Linetype],
    library: &[String],
    language: &str,
    st: &mut LpmState,
    actions: &mut Vec<(&'static str, Value)>,
) {
    let t = Tokens::get();
    macro_rules! tr {
        ($english:expr) => { crate::i18n::tr(language, $english) };
    }
    let name = l.name.clone();
    let current = name.eq_ignore_ascii_case(cur);
    let is_used = used.contains(&name.to_ascii_lowercase());
    let selected = st.selected.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&name));
    // Status.
    let (sr, sresp) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
    if selected {
        ui.painter().rect_filled(sr, 2.0, t.accent.gamma_multiply(0.35));
    }
    status_icon(ui.painter(), sr.shrink(2.0), current, is_used);
    let tip = if current {
        "Current layer"
    } else if is_used {
        "Layer contains objects (double-click to make current)"
    } else {
        "Layer is empty (double-click to make current)"
    };
    if sresp.on_hover_text(tr!(tip)).double_clicked() {
        actions.push(("layer.current", json!({ "name": name })));
    }
    // Name (double-click or F2-style rename).
    match st.renaming.clone() {
        Some((n, mut buf)) if n.eq_ignore_ascii_case(&name) => {
            let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(150.0).id(egui::Id::new(("lpm_rename", &name))));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if !cancel && buf.trim() != name && !buf.trim().is_empty() {
                    actions.push(("layer.set", json!({ "name": name, "newName": buf.trim() })));
                    st.selected = Some(buf.trim().to_string());
                }
                st.renaming = None;
            } else {
                st.renaming = Some((n, buf));
            }
        }
        _ => {
            let (nr, r) = ui.allocate_exact_size(vec2(150.0, 20.0), Sense::click());
            if selected {
                ui.painter().rect_filled(nr, 2.0, t.accent.gamma_multiply(0.45));
            } else if r.hovered() {
                ui.painter().rect_filled(nr, 2.0, t.control_hover.gamma_multiply(0.5));
            }
            ui.painter().text(pos2(nr.left() + 4.0, nr.center().y), egui::Align2::LEFT_CENTER, &name, crate::theme::body(), t.text);
            if r.clicked() {
                st.selected = Some(name.clone());
            }
            if r.double_clicked() && name != "0" && !name.eq_ignore_ascii_case("Defpoints") {
                st.renaming = Some((name.clone(), name.clone()));
            }
            r.context_menu(|ui| {
                if ui.button(tr!("Set Current")).clicked() {
                    actions.push(("layer.current", json!({ "name": name })));
                    ui.close();
                }
                if ui.button(tr!("Rename Layer")).clicked() {
                    st.renaming = Some((name.clone(), name.clone()));
                    ui.close();
                }
                if ui.button(tr!("Delete Layer")).clicked() {
                    actions.push(("layer.delete", json!({ "name": name })));
                    ui.close();
                }
            });
        }
    }
    // On / Freeze / Lock / Plot.
    if icons::button(ui, if l.on { Icon::Bulb } else { Icon::BulbOff }, 20.0, tr!("On/Off"), false).clicked() {
        actions.push(("layer.set", json!({ "name": name, "on": !l.on })));
    }
    if icons::button(ui, if l.frozen { Icon::Snowflake } else { Icon::Sun }, 20.0, tr!("Freeze/Thaw in all viewports"), false).clicked() {
        actions.push(("layer.set", json!({ "name": name, "frozen": !l.frozen })));
    }
    if icons::button(ui, if l.locked { Icon::Lock } else { Icon::Unlock }, 20.0, tr!("Lock/Unlock"), false).clicked() {
        actions.push(("layer.set", json!({ "name": name, "locked": !l.locked })));
    }
    let pr = icons::button(ui, Icon::Plot, 20.0, if l.plot { tr!("Plot (click: do not plot)") } else { tr!("Do not plot (click: plot)") }, false);
    if !l.plot {
        let r = pr.rect.shrink(3.0);
        ui.painter().line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.6, Color32::from_rgb(0xe0, 0x50, 0x50)));
    }
    if pr.clicked() {
        actions.push(("layer.set", json!({ "name": name, "plot": !l.plot })));
    }
    // Color.
    if let Some(c) = color_button_localized(ui, egui::Id::new(("lpm_color", &name)), l.color, false, 96.0, language) {
        actions.push(("layer.set", json!({ "name": name, "color": c.name() })));
    }
    // Linetype.
    let lt_resp = ui
        .allocate_ui(vec2(130.0, 20.0), |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(130.0, 20.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 2.0, t.control_hover.gamma_multiply(0.5));
            }
            lt_sample(
                ui.painter(),
                Rect::from_min_size(pos2(r.left() + 3.0, r.top()), vec2(30.0, r.height())),
                linetypes.iter().find(|x| x.name.eq_ignore_ascii_case(&l.linetype)),
                t.text,
            );
            ui.painter().text(pos2(r.left() + 38.0, r.center().y), egui::Align2::LEFT_CENTER, tr!(&l.linetype), crate::theme::body(), t.text);
            resp
        })
        .inner;
    egui::Popup::from_toggle_button_response(&lt_resp)
        .id(egui::Id::new(("lpm_lt", &name)))
        .width(240.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.label(RichText::new(tr!("Loaded linetypes")).small().color(t.text_dim));
            for lt in linetypes.iter().filter(|x| !["bylayer", "byblock"].contains(&x.name.to_ascii_lowercase().as_str())) {
                let (r, resp) = ui.allocate_exact_size(vec2(230.0, 20.0), Sense::click());
                if resp.hovered() || lt.name.eq_ignore_ascii_case(&l.linetype) {
                    ui.painter().rect_filled(r, 2.0, t.accent.gamma_multiply(if resp.hovered() { 0.6 } else { 0.3 }));
                }
                lt_sample(ui.painter(), Rect::from_min_size(pos2(r.left() + 4.0, r.top()), vec2(50.0, r.height())), Some(lt), t.text);
                ui.painter().text(pos2(r.left() + 62.0, r.center().y), egui::Align2::LEFT_CENTER, tr!(&lt.name), crate::theme::body(), t.text);
                if resp.on_hover_text(&lt.description).clicked() {
                    actions.push(("layer.set", json!({ "name": name, "linetype": lt.name })));
                    ui.close();
                }
            }
            if !library.is_empty() {
                ui.separator();
                ui.menu_button(tr!("Load..."), |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for n in library {
                            if ui.button(n).clicked() {
                                actions.push(("linetype", json!({ "load": n })));
                                actions.push(("layer.set", json!({ "name": name, "linetype": n })));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button(tr!("Load all")).clicked() {
                        actions.push(("linetype", json!({ "load": "*" })));
                        ui.close();
                    }
                });
            }
        });
    // Lineweight.
    let lw_resp = ui
        .allocate_ui(vec2(100.0, 20.0), |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(100.0, 20.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 2.0, t.control_hover.gamma_multiply(0.5));
            }
            lw_sample(ui.painter(), Rect::from_min_size(pos2(r.left() + 3.0, r.top()), vec2(22.0, r.height())), l.lineweight, t.text);
            ui.painter().text(pos2(r.left() + 30.0, r.center().y), egui::Align2::LEFT_CENTER, tr!(&l.lineweight.name()), crate::theme::body(), t.text);
            resp
        })
        .inner;
    egui::Popup::from_toggle_button_response(&lw_resp)
        .id(egui::Id::new(("lpm_lw", &name)))
        .width(170.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                let opts = std::iter::once(Lineweight::Default).chain(Lineweight::STANDARD.iter().map(|v| Lineweight::Mm100(*v)));
                for lw in opts {
                    let (r, resp) = ui.allocate_exact_size(vec2(160.0, 20.0), Sense::click());
                    if resp.hovered() || lw == l.lineweight {
                        ui.painter().rect_filled(r, 2.0, t.accent.gamma_multiply(if resp.hovered() { 0.6 } else { 0.3 }));
                    }
                    lw_sample(ui.painter(), Rect::from_min_size(pos2(r.left() + 4.0, r.top()), vec2(40.0, r.height())), lw, t.text);
                    ui.painter().text(pos2(r.left() + 52.0, r.center().y), egui::Align2::LEFT_CENTER, tr!(&lw.name()), crate::theme::body(), t.text);
                    if resp.clicked() {
                        let v = match lw {
                            Lineweight::Mm100(v) => json!(f64::from(v) / 100.0),
                            _ => json!("Default"),
                        };
                        actions.push(("layer.set", json!({ "name": name, "lineweight": v })));
                        ui.close();
                    }
                }
            });
        });
    // Transparency (commits when the drag or edit ends).
    let tid = egui::Id::new(("lpm_tr", &name));
    let mut tr = ui.data_mut(|d| d.get_temp::<u8>(tid)).unwrap_or(l.transparency);
    let r = ui.add(egui::DragValue::new(&mut tr).range(0..=90).speed(0.5));
    if r.dragged() || r.has_focus() {
        ui.data_mut(|d| d.insert_temp(tid, tr));
    }
    if (r.drag_stopped() || r.lost_focus()) && tr != l.transparency {
        actions.push(("layer.set", json!({ "name": name, "transparency": tr })));
        ui.data_mut(|d| d.remove::<u8>(tid));
    } else if !r.dragged() && !r.has_focus() {
        ui.data_mut(|d| d.remove::<u8>(tid));
    }
    // VP Freeze / VP Color (inside a layout viewport).
    if let Some((h, v)) = vp {
        let vf = v.frozen_layers.iter().any(|f| f.eq_ignore_ascii_case(&name));
        if icons::button(ui, if vf { Icon::Snowflake } else { Icon::Sun }, 20.0, "Freeze/Thaw in the current viewport", false).clicked() {
            let key = if vf { "thaw" } else { "freeze" };
            actions.push(("vplayer", json!({ "handle": h.hex(), key: [name] })));
        }
        let vc = v.layer_colors.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)).map(|(_, c)| *c);
        ui.horizontal(|ui| {
            if let Some(c) = color_button(ui, egui::Id::new(("lpm_vpcolor", &name)), vc.unwrap_or(l.color), false, 96.0) {
                actions.push(("vplayer", json!({ "handle": h.hex(), "colors": { name.clone(): c.name() } })));
            }
            if vc.is_some() && ui.small_button("×").on_hover_text("Remove the viewport override").clicked() {
                actions.push(("vplayer", json!({ "handle": h.hex(), "colors": { name.clone(): Value::Null } })));
            }
        });
    }
    // New VP Freeze.
    let nvf = icons::button(ui, if l.vp_freeze_new { Icon::Snowflake } else { Icon::Sun }, 20.0, "Freeze in new viewports", false);
    {
        // A small "new" mark distinguishes it from the Freeze column.
        let r = nvf.rect;
        ui.painter().rect_filled(Rect::from_min_size(r.right_bottom() - vec2(7.0, 7.0), vec2(6.0, 6.0)), 1.0, t.icon_accent);
    }
    if nvf.clicked() {
        actions.push(("layer.set", json!({ "name": name, "newVpFreeze": !l.vp_freeze_new })));
    }
    // Description (double-click to edit).
    match st.describing.clone() {
        Some((n, mut buf)) if n.eq_ignore_ascii_case(&name) => {
            let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(200.0).id(egui::Id::new(("lpm_desc", &name))));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && buf != l.description {
                    actions.push(("layer.set", json!({ "name": name, "description": buf })));
                }
                st.describing = None;
            } else {
                st.describing = Some((n, buf));
            }
        }
        _ => {
            let text = if l.description.is_empty() { RichText::new("—").color(t.text_faint) } else { RichText::new(&l.description) };
            let r = ui.add_sized([200.0, 20.0], egui::Label::new(text).sense(Sense::click()).truncate());
            if r.on_hover_text("Double-click to edit the description").double_clicked() {
                st.describing = Some((name.clone(), l.description.clone()));
            }
        }
    }
}

/// Lower-case names of layers with objects on them.
fn used_layers(d: &cadcraft_doc::Drawing) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let ents = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    for e in ents.take(5_000_000) {
        if !set.contains(&e.common.layer.to_ascii_lowercase()) {
            set.insert(e.common.layer.to_ascii_lowercase());
        }
    }
    set
}

/// Layer search: plain text matches anywhere; `*` / `?` are wildcards.
fn crate_wild(q: &str, name: &str) -> bool {
    if !q.contains('*') && !q.contains('?') {
        return name.contains(q);
    }
    wild(q.as_bytes(), name.as_bytes())
}

fn wild(p: &[u8], t: &[u8]) -> bool {
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        match p.get(pi) {
            Some(b'*') => {
                star = Some(pi);
                pi += 1;
                mark = ti;
            }
            Some(&c) if c == b'?' || Some(&c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some(s) => {
                    pi = s + 1;
                    mark += 1;
                    ti = mark;
                }
                None => return false,
            },
        }
    }
    p.iter().skip(pi).all(|c| *c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_search() {
        assert!(crate_wild("wal", "walls"));
        assert!(crate_wild("w*s", "walls"));
        assert!(crate_wild("?alls", "walls"));
        assert!(!crate_wild("x*", "walls"));
    }
}
