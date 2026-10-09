//! Quick Select (QSELECT dialog → the engine `qselect` command) and the Quick Properties
//! palette shown near the cursor while objects are selected and QPMODE is on.

use cadcraft_doc::{EntityKind, Handle};
use egui::{Pos2, RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

// ---------------- QSELECT ----------------

#[derive(Clone, Debug)]
struct QsState {
    apply_selection: bool,
    ty: String,
    property: String,
    operator: String,
    value: String,
    exclude: bool,
    append: bool,
    /// (doc key, applyTo, type) → info reply.
    info_key: Option<(usize, bool, String)>,
    info: Value,
}

impl Default for QsState {
    fn default() -> Self {
        QsState {
            apply_selection: false,
            ty: "Multiple".into(),
            property: "color".into(),
            operator: "=".into(),
            value: String::new(),
            exclude: false,
            append: false,
            info_key: None,
            info: Value::Null,
        }
    }
}

const OPERATORS: &[(&str, &str)] =
    &[("=", "= Equals"), ("!=", "<> Not Equal"), (">", "> Greater than"), ("<", "< Less than"), ("*", "* Wildcard Match")];

/// Friendly label for a property key (`centerX` → `Center X`).
fn pretty(k: &str) -> String {
    let mut out = String::new();
    for (i, c) in k.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            out.push(' ');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

pub fn qselect_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let sid = egui::Id::new("qselect_state");
    let mut st: QsState = ctx.data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    let key = app.session.state().map(|s| std::sync::Arc::as_ptr(&s.doc) as usize).unwrap_or(0);
    let want = (key, st.apply_selection, st.ty.clone());
    if st.info_key.as_ref() != Some(&want) {
        let ty = if st.ty == "Multiple" { Value::Null } else { json!(st.ty) };
        st.info = app
            .session
            .execute("qselect.info", &json!({ "applyTo": if st.apply_selection { "selection" } else { "drawing" }, "type": ty }))
            .unwrap_or(Value::Null);
        st.info_key = Some(want);
    }
    let has_sel = !app.session.selection().is_empty();
    let mut run: Option<Value> = None;
    let mut close = false;
    egui::Window::new(crate::tl!("Quick Select"))
        .id(egui::Id::new("Quick Select"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .show(ctx, |ui| {
            egui::Grid::new("qs_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(crate::tl!("Apply to:"));
                egui::ComboBox::from_id_salt("qs_apply")
                    .width(220.0)
                    .selected_text(crate::i18n::t(if st.apply_selection { "Current selection" } else { "Entire drawing" }))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.apply_selection, false, crate::tl!("Entire drawing"));
                        ui.add_enabled_ui(has_sel, |ui| {
                            ui.selectable_value(&mut st.apply_selection, true, crate::tl!("Current selection"));
                        });
                    });
                ui.end_row();
                ui.label(crate::tl!("Object type:"));
                let types: Vec<(String, u64)> = st
                    .info
                    .get("types")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| Some((v.get("type")?.as_str()?.to_string(), v.get("count").and_then(Value::as_u64).unwrap_or(0))))
                            .collect()
                    })
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("qs_type").width(220.0).selected_text(crate::i18n::t(&st.ty)).show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.ty, "Multiple".to_string(), crate::tl!("Multiple"));
                    for (n, c) in &types {
                        ui.selectable_value(&mut st.ty, n.clone(), format!("{} ({c})", crate::i18n::t(n)));
                    }
                });
                ui.end_row();
                ui.label(crate::tl!("Properties:"));
                let props: Vec<String> = st
                    .info
                    .get("properties")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                egui::Frame::NONE.fill(t.chrome_dark).corner_radius(3).inner_margin(4).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(150.0).min_scrolled_height(150.0).show(ui, |ui| {
                        ui.set_width(212.0);
                        ui.vertical(|ui| {
                            for p in &props {
                                if ui.selectable_label(st.property == *p, crate::i18n::t(&pretty(p))).clicked() {
                                    st.property = p.clone();
                                }
                            }
                        });
                    });
                });
                ui.end_row();
                ui.label(crate::tl!("Operator:"));
                let op_label = OPERATORS.iter().find(|o| o.0 == st.operator).map(|o| o.1).unwrap_or("= Equals");
                egui::ComboBox::from_id_salt("qs_op").width(220.0).selected_text(crate::i18n::t(op_label)).show_ui(ui, |ui| {
                    for (o, l) in OPERATORS {
                        ui.selectable_value(&mut st.operator, o.to_string(), crate::i18n::t(l));
                    }
                });
                ui.end_row();
                ui.label(crate::tl!("Value:"));
                let choices: Vec<String> = match st.property.as_str() {
                    "color" => {
                        ["ByLayer", "ByBlock", "Red", "Yellow", "Green", "Cyan", "Blue", "Magenta", "White"].iter().map(|s| s.to_string()).collect()
                    }
                    "layer" => app.session.doc().map(|d| d.layers.iter().map(|l| l.name.clone()).collect()).unwrap_or_default(),
                    "linetype" => app.session.doc().map(|d| d.linetypes.iter().map(|l| l.name.clone()).collect()).unwrap_or_default(),
                    "type" => types.iter().map(|(n, _)| n.clone()).collect(),
                    _ => Vec::new(),
                };
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.value).desired_width(if choices.is_empty() { 220.0 } else { 190.0 }));
                    if !choices.is_empty() {
                        ui.menu_button("▼", |ui| {
                            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                                for c in &choices {
                                    if ui
                                        .button(if ["color", "type"].contains(&st.property.as_str()) { crate::i18n::t(c) } else { c.as_str() })
                                        .clicked()
                                    {
                                        st.value = c.clone();
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                });
                ui.end_row();
            });
            ui.separator();
            ui.label(crate::tl!("How to apply:"));
            ui.radio_value(&mut st.exclude, false, crate::tl!("Include in new selection set"));
            ui.radio_value(&mut st.exclude, true, crate::tl!("Exclude from new selection set"));
            ui.checkbox(&mut st.append, crate::tl!("Append to current selection set"));
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("  OK  ")).clicked() {
                        let mode = if st.exclude {
                            "exclude"
                        } else if st.append {
                            "append"
                        } else {
                            "new"
                        };
                        let mut p = json!({
                            "applyTo": if st.apply_selection { "selection" } else { "drawing" },
                            "mode": mode,
                        });
                        if let Some(o) = p.as_object_mut() {
                            if st.ty != "Multiple" {
                                o.insert("type".into(), json!(st.ty));
                            }
                            if !st.property.is_empty() && (!st.value.trim().is_empty() || st.operator == "*") {
                                o.insert("property".into(), json!(st.property));
                                o.insert("operator".into(), json!(st.operator));
                                let v = st.value.trim();
                                o.insert("value".into(), v.parse::<f64>().map(|f| json!(f)).unwrap_or_else(|_| json!(v)));
                            }
                        }
                        run = Some(p);
                        close = true;
                    }
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        close = true;
                    }
                });
            });
        });
    ctx.data_mut(|d| d.insert_temp(sid, st));
    if let Some(p) = run {
        let _ = app.run("qselect", p);
    }
    if close {
        *open = false;
    }
}

// ---------------- Quick Properties ----------------

#[derive(Clone, Debug, Default)]
struct QpState {
    sel: Vec<Handle>,
    at: Option<Pos2>,
}

fn num_row(ui: &mut egui::Ui, label: &str, id: egui::Id, v: f64, set: &mut Option<(String, Value)>, key: &str) {
    ui.label(RichText::new(crate::i18n::t(label)).color(Tokens::get().text_dim));
    let s = format!("{v:.4}");
    if let Some(n) = crate::parametric::expr_field(ui, id, &s)
        && let Some(f) = cadcraft_engine::units::parse_distance(&n).or_else(|| n.parse::<f64>().ok()).filter(|f| f.is_finite())
    {
        *set = Some((key.to_string(), json!(f)));
    }
    ui.end_row();
}

fn ro_row(ui: &mut egui::Ui, label: &str, v: String) {
    ui.label(RichText::new(crate::i18n::t(label)).color(Tokens::get().text_dim));
    ui.label(v);
    ui.end_row();
}

/// The Quick Properties palette, near where the selection was made.
pub fn quick_properties(app: &mut CadApp, ctx: &egui::Context) {
    let sid = egui::Id::new("qp_state");
    let sel = app.session.selection();
    let active = app.session.settings.qpmode
        && !sel.is_empty()
        && app.session.running.is_none()
        && app.canvas.hot_grip.is_none()
        && !app.ui.start_tab
        && app.ui.dialog.as_deref() != Some("qselect");
    let mut st: QpState = ctx.data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    if !active {
        if !st.sel.is_empty() {
            ctx.data_mut(|d| d.insert_temp(sid, QpState::default()));
        }
        return;
    }
    if st.sel != sel || st.at.is_none() {
        let hp = ctx.input(|i| i.pointer.hover_pos());
        let r = app.canvas.rect.unwrap_or(egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        let p = hp.filter(|p| r.contains(*p)).unwrap_or(r.center()) + vec2(28.0, 28.0);
        // Keep it on the canvas.
        let p = Pos2::new(p.x.min(r.right() - 270.0).max(r.left()), p.y.min(r.bottom() - 200.0).max(r.top()));
        st.sel = sel.clone();
        st.at = Some(p);
    }
    let Some(at) = st.at else { return };
    let Ok(d) = app.session.doc() else { return };
    let ents: Vec<_> = sel.iter().filter_map(|h| d.entity(*h)).collect();
    let Some(first) = ents.first() else { return };
    let ty = first.kind.type_name();
    let same_type = ents.iter().all(|e| e.kind.type_name() == ty);
    let title = if ents.len() == 1 {
        ty.to_string()
    } else if same_type {
        format!("{} ({})", crate::i18n::t(ty), ents.len())
    } else {
        format!("{} ({})", crate::tl!("All"), ents.len())
    };
    let same = |f: &dyn Fn(&cadcraft_doc::Entity) -> String| -> String {
        let v0 = ents.first().map(|e| f(e)).unwrap_or_default();
        if ents.iter().all(|e| f(e) == v0) { v0 } else { "*VARIES*".into() }
    };
    let color = ents.first().map(|e| e.common.color).filter(|c| ents.iter().all(|e| e.common.color == *c));
    let layer = same(&|e| e.common.layer.clone());
    let linetype = same(&|e| e.common.linetype.clone());
    let layers: Vec<String> = d.layers.iter().map(|l| l.name.clone()).collect();
    let linetypes: Vec<String> = d.linetypes.iter().map(|l| l.name.clone()).collect();
    let kind = if ents.len() == 1 { Some(first.kind.clone()) } else { None };
    let ids: Vec<String> = sel.iter().map(|h| h.hex()).collect();
    let mut set: Option<(String, Value)> = None;
    let mut close = false;
    egui::Window::new(title)
        .id(egui::Id::new(("quick_properties", &st.sel)))
        .default_pos(at)
        .collapsible(false)
        .resizable(false)
        .title_bar(true)
        .default_width(250.0)
        .show(ctx, |ui| {
            egui::Grid::new("qp_grid").num_columns(2).spacing(vec2(10.0, 4.0)).min_col_width(80.0).show(ui, |ui| {
                ui.label(RichText::new(crate::tl!("Color")).color(Tokens::get().text_dim));
                let cur = color.unwrap_or(cadcraft_color::Color::ByLayer);
                if let Some(c) = crate::layers::color_button(ui, egui::Id::new(("qp_color", &ids)), cur, true, 150.0) {
                    set = Some(("color".into(), json!(c.name())));
                }
                ui.end_row();
                ui.label(RichText::new(crate::tl!("Layer")).color(Tokens::get().text_dim));
                egui::ComboBox::from_id_salt("qp_layer").width(150.0).selected_text(layer.clone()).show_ui(ui, |ui| {
                    for l in &layers {
                        if ui.selectable_label(*l == layer, l).clicked() {
                            set = Some(("layer".into(), json!(l)));
                        }
                    }
                });
                ui.end_row();
                ui.label(RichText::new(crate::tl!("Linetype")).color(Tokens::get().text_dim));
                egui::ComboBox::from_id_salt("qp_lt").width(150.0).selected_text(linetype.clone()).show_ui(ui, |ui| {
                    for l in std::iter::once("ByLayer".to_string())
                        .chain(std::iter::once("ByBlock".to_string()))
                        .chain(linetypes.iter().filter(|l| !["ByLayer", "ByBlock"].contains(&l.as_str())).cloned())
                    {
                        if ui.selectable_label(l == linetype, &l).clicked() {
                            set = Some(("linetype".into(), json!(l)));
                        }
                    }
                });
                ui.end_row();
                let id = |k: &str| egui::Id::new(("qp", k, &ids));
                match &kind {
                    Some(EntityKind::Line(l)) => {
                        let dl = l.b.xy() - l.a.xy();
                        num_row(ui, "Length", id("len"), dl.len(), &mut set, "length");
                        num_row(ui, "Angle", id("ang"), dl.angle().to_degrees(), &mut set, "angle");
                    }
                    Some(EntityKind::Circle(c)) => {
                        num_row(ui, "Center X", id("cx"), c.center.x, &mut set, "cx");
                        num_row(ui, "Center Y", id("cy"), c.center.y, &mut set, "cy");
                        num_row(ui, "Radius", id("r"), c.radius, &mut set, "radius");
                        num_row(ui, "Diameter", id("d"), c.radius * 2.0, &mut set, "diameter");
                        ro_row(ui, "Area", format!("{:.4}", c.radius * c.radius * std::f64::consts::PI));
                        if let Some((k, v)) = &set {
                            if k == "cx" {
                                set = Some(("center".into(), json!([v, c.center.y])));
                            } else if k == "cy" {
                                set = Some(("center".into(), json!([c.center.x, v])));
                            }
                        }
                    }
                    Some(EntityKind::Arc(a)) => {
                        num_row(ui, "Radius", id("r"), a.radius, &mut set, "radius");
                        num_row(ui, "Start angle", id("sa"), a.start.to_degrees(), &mut set, "startAngle");
                        num_row(ui, "End angle", id("ea"), a.end.to_degrees(), &mut set, "endAngle");
                    }
                    Some(EntityKind::LwPolyline(p)) => {
                        let g = cadcraft_geom::Polyline { vertices: p.vertices.clone(), closed: p.closed };
                        ro_row(ui, "Length", format!("{:.4}", g.len()));
                        if p.closed {
                            ro_row(ui, "Area", format!("{:.4}", g.area().abs()));
                        }
                        num_row(ui, "Global width", id("w"), p.const_width, &mut set, "width");
                    }
                    Some(EntityKind::Text(tx)) => {
                        ui.label(RichText::new(crate::tl!("Contents")).color(Tokens::get().text_dim));
                        if let Some(v) = crate::parametric::expr_field(ui, id("txt"), &tx.value) {
                            set = Some(("text".into(), json!(v)));
                        }
                        ui.end_row();
                        num_row(ui, "Height", id("h"), tx.height, &mut set, "height");
                        num_row(ui, "Rotation", id("rot"), tx.rotation.to_degrees(), &mut set, "rotation");
                    }
                    Some(EntityKind::MText(tx)) => {
                        ui.label(RichText::new(crate::tl!("Contents")).color(Tokens::get().text_dim));
                        if let Some(v) = crate::parametric::expr_field(ui, id("mtxt"), &tx.contents) {
                            set = Some(("text".into(), json!(v)));
                        }
                        ui.end_row();
                        num_row(ui, "Text height", id("h"), tx.height, &mut set, "height");
                    }
                    Some(EntityKind::Insert(i)) => {
                        ro_row(ui, "Name", i.block.clone());
                        num_row(ui, "Rotation", id("rot"), i.rotation.to_degrees(), &mut set, "rotation");
                        num_row(ui, "Scale", id("sc"), i.scale.x, &mut set, "scale");
                    }
                    Some(EntityKind::Dimension(dm)) => {
                        ui.label(RichText::new(crate::tl!("Text override")).color(Tokens::get().text_dim));
                        if let Some(v) = crate::parametric::expr_field(ui, id("dt"), &dm.text) {
                            set = Some(("textOverride".into(), json!(v)));
                        }
                        ui.end_row();
                    }
                    _ => {}
                }
            });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::tl!("Close")).on_hover_text(crate::tl!("Turn Quick Properties off (QPMODE)")).clicked() {
                        close = true;
                    }
                });
            });
        });
    ctx.data_mut(|dd| dd.insert_temp(sid, st));
    if let Some((k, v)) = set {
        let _ = app.run("properties.set", json!({ "handles": ids, k: v }));
    }
    if close {
        let _ = app.run("qpmode", json!({ "on": false }));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pretty_names() {
        assert_eq!(super::pretty("centerX"), "Center X");
        assert_eq!(super::pretty("layer"), "Layer");
    }
}
