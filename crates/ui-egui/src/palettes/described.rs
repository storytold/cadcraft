//! The Properties palette rows the engine describes (`properties.describe`): text, mtext and
//! dimensions, for one object or many of the same type, with *VARIES*, values in the drawing's
//! units, and dimension variables the dimension overrides. Quick Properties draws the rows the
//! description flags `quick` the same way.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cadcraft_engine::doc::{Drawing, Handle};
use cadcraft_engine::properties::{Description, Kind, Row, VARIES};
use cadcraft_engine::units::AngleSettings;
use egui::{Sense, pos2, vec2};
use serde_json::{Value, json};

use super::{edit_field, number, value_box};
use crate::CadApp;
use crate::theme::Tokens;

/// What the engine says the palette shows for `sel`, worked out again only when the drawing,
/// its revision or the selection changes (dimension rows draw each dimension to measure it).
pub(crate) fn described(ui: &egui::Ui, app: &CadApp, sel: &[Handle]) -> Option<Arc<Description>> {
    let st = app.session.state().ok()?;
    let mut hasher = std::hash::DefaultHasher::new();
    (st.uid, st.revision, sel).hash(&mut hasher);
    let key = hasher.finish();
    let id = egui::Id::new("properties-describe");
    if let Some((k, d)) = ui.data(|m| m.get_temp::<(u64, Arc<Description>)>(id))
        && k == key
    {
        return Some(d);
    }
    let d = Arc::new(cadcraft_engine::properties::describe(&st.doc, sel, None));
    ui.data_mut(|m| m.insert_temp(id, (key, d.clone())));
    Some(d)
}

/// A group header, as the palette draws General and Geometry.
fn group(ui: &mut egui::Ui, title: &str) {
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().rect_filled(r.shrink2(vec2(6.0, 1.0)), 2.0, t.chrome_dark);
    ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, crate::i18n::t(title), crate::theme::small(), t.text_dim);
}

/// An edit the palette makes: the `properties.set` call and the row it came from.
pub(crate) struct Edit {
    pub params: Value,
    pub label: &'static str,
}

/// Make `edit`; a value the engine couldn't take (out of range…) says so on the command line.
pub(crate) fn apply(app: &mut CadApp, edit: Option<Edit>) {
    if let Some(e) = edit
        && let Ok(r) = app.run("properties.set", e.params)
        && r["ignored"].as_array().is_some_and(|a| !a.is_empty())
    {
        app.session.echo(crate::tf!("{property}: that value can't be used here.", property = crate::i18n::t(e.label)));
    }
}

/// Quick Properties: the rows the description flags `quick` (AutoCAD's defaults: Layer,
/// Contents, Style, Annotative, Justification, Height and Rotation for text; Layer, Dim style,
/// Annotative, Measurement and Text override for dimensions). False when the selection isn't of
/// a described type, so Quick Properties draws its own rows.
pub(crate) fn quick_rows(
    ui: &mut egui::Ui,
    d: &Drawing,
    desc: &Description,
    ids: &[String],
    edit: &mut Option<Edit>,
    bad: &mut Option<String>,
) -> bool {
    if !desc.groups.iter().any(|g| g.name != "General") {
        return false;
    }
    let angles = AngleSettings::from_header(&d.header);
    let text_style = desc.groups.iter().flat_map(|g| &g.rows).find(|r| r.key == "textStyle" && !r.varies).map(|r| r.display.clone());
    for row in desc.groups.iter().flat_map(|g| &g.rows).filter(|r| r.quick) {
        described_row(ui, d, row, ids, &angles, text_style.as_deref(), edit, bad);
    }
    true
}

/// The described groups after General (the palette draws General itself). Returns false when
/// the description has none, so the palette draws its own rows for the object.
pub(super) fn groups(ui: &mut egui::Ui, d: &Drawing, desc: &Description, ids: &[String], edit: &mut Option<Edit>, bad: &mut Option<String>) -> bool {
    let typed: Vec<_> = desc.groups.iter().filter(|g| g.name != "General").collect();
    if typed.is_empty() {
        return false;
    }
    let angles = AngleSettings::from_header(&d.header);
    // A fixed-height text style names itself in the dimension's Text style row.
    let text_style = typed.iter().flat_map(|g| &g.rows).find(|r| r.key == "textStyle" && !r.varies).map(|r| r.display.clone());
    for g in typed {
        group(ui, g.name);
        for row in &g.rows {
            described_row(ui, d, row, ids, &angles, text_style.as_deref(), edit, bad);
        }
    }
    true
}

/// The value as the row shows it: *VARIES* and choice labels in the interface language, the
/// drawing's own names and the objects' text as they are.
fn shown(row: &Row) -> String {
    if row.varies {
        return crate::i18n::t(VARIES).to_string();
    }
    match row.kind {
        Kind::Bool | Kind::Choice | Kind::Color => crate::i18n::t(&row.display).to_string(),
        Kind::Linetype if row.display == "ByBlock" => crate::i18n::t("ByBlock").to_string(),
        _ => row.display.clone(),
    }
}

/// Why a row can't be edited (or what decides its value), in the interface language.
fn note(row: &Row, text_style: Option<&str>) -> Option<String> {
    row.note.as_ref()?;
    Some(match (row.key, row.editable, text_style) {
        ("textHeight", false, Some(style)) => crate::tf!(
            "Set by text style \"{style}\", which has a fixed height. Give the style a height of 0 to use the dimension text height.",
            style = style
        )
        .to_string(),
        ("textHeight", _, _) => crate::tl!("Some of these dimensions take their text height from a fixed-height text style.").to_string(),
        _ => crate::i18n::t(row.note.as_deref().unwrap_or_default()).to_string(),
    })
}

fn described_row(
    ui: &mut egui::Ui,
    d: &Drawing,
    row: &Row,
    ids: &[String],
    angles: &AngleSettings,
    text_style: Option<&str>,
    edit: &mut Option<Edit>,
    bad: &mut Option<String>,
) {
    let t = Tokens::get();
    let key = row.key;
    let mut picked: Option<Value> = None;
    ui.horizontal(|ui| {
        // The label: in the accent color where a dimension overrides its style, with a menu
        // that goes back to the style's value.
        let color = if row.overridden { t.accent } else { t.text_dim };
        let label = crate::i18n::t(row.label).to_string();
        let mut job = egui::text::LayoutJob::simple(label.clone(), crate::theme::body(), color, 106.0);
        job.halign = egui::Align::RIGHT;
        let galley = ui.painter().layout_job(job);
        let sense = if row.overridden { Sense::click() } else { Sense::hover() };
        let (lr, resp) = ui.allocate_exact_size(vec2(118.0, galley.size().y.max(22.0)), sense);
        // Painted, so screen readers are given the label too.
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
        ui.painter().galley(pos2(lr.right() - 6.0, lr.center().y - galley.size().y / 2.0), galley, color);
        if row.overridden {
            resp.on_hover_text(crate::tl!("Overrides the dimension style. Right-click to use the style's value.")).context_menu(|ui| {
                if ui.button(crate::tl!("Use the style's value")).clicked() {
                    *edit = Some(Edit { params: json!({ "handles": ids, "clear": [key] }), label: row.label });
                    ui.close();
                }
            });
        }
        let shown = shown(row);
        if !row.editable {
            let tip = note(row, text_style).unwrap_or_else(|| crate::tl!("Read-only").to_string());
            value_box(ui, &shown, false, false).on_hover_text(tip);
            return;
        }
        let id = ui.id().with(("described", key, ids));
        match row.kind {
            Kind::Text | Kind::Contents => picked = edit_field(ui, id, shown).map(Value::String),
            Kind::Number | Kind::Distance => picked = number(edit_field(ui, id, shown), bad).map(|v| json!(v)),
            Kind::Angle | Kind::Direction => {
                if let Some(typed) = edit_field(ui, id, shown) {
                    // A direction is typed as the drawing measures directions (from ANGBASE in the
                    // ANGDIR sense; bearings and `<<` overrides absolute); properties.set takes it
                    // from +X counterclockwise.
                    let parsed = if row.kind == Kind::Direction {
                        angles.direction(&typed).map(|a| a.rem_euclid(std::f64::consts::TAU))
                    } else {
                        angles.amount(&typed)
                    };
                    match parsed {
                        Some(a) => picked = Some(json!(a.to_degrees())),
                        None => *bad = Some(typed),
                    }
                }
            }
            Kind::Bool => {
                ui.menu_button(shown, |ui| {
                    for (v, label) in [(true, "Yes"), (false, "No")] {
                        if ui.button(crate::i18n::t(label)).clicked() {
                            picked = Some(json!(v));
                            ui.close();
                        }
                    }
                });
            }
            Kind::Choice | Kind::TextStyle | Kind::DimStyle => {
                // Choice labels are ours to translate; style names are the drawing's.
                let translate = row.kind == Kind::Choice;
                ui.menu_button(shown, |ui| {
                    for o in &row.options {
                        let label = if translate { crate::i18n::t(&o.label).to_string() } else { o.label.clone() };
                        if ui.button(label).clicked() {
                            picked = Some(o.value.clone());
                            ui.close();
                        }
                    }
                });
            }
            Kind::Color => {
                ui.menu_button(format!("■ {shown}"), |ui| {
                    for (n, c) in [
                        ("ByBlock", 0),
                        ("ByLayer", 256),
                        ("Red", 1),
                        ("Yellow", 2),
                        ("Green", 3),
                        ("Cyan", 4),
                        ("Blue", 5),
                        ("Magenta", 6),
                        ("White", 7),
                    ] {
                        if ui.button(crate::i18n::t(n)).clicked() {
                            // The row shows a color by name: picking the one it has is no edit.
                            if row.varies || row.value != json!(n) {
                                picked = Some(json!(c));
                            }
                            ui.close();
                        }
                    }
                });
            }
            Kind::Linetype => {
                let names = std::iter::once("ByBlock".to_string()).chain(
                    d.linetypes.iter().map(|l| l.name.clone()).filter(|n| !n.eq_ignore_ascii_case("ByLayer") && !n.eq_ignore_ascii_case("ByBlock")),
                );
                ui.menu_button(shown, |ui| {
                    for n in names {
                        let label = if n == "ByBlock" { crate::i18n::t("ByBlock").to_string() } else { n.clone() };
                        if ui.button(label).clicked() {
                            picked = Some(json!(n));
                            ui.close();
                        }
                    }
                });
            }
            // Quick Properties shows General's Layer.
            Kind::Layer => {
                ui.menu_button(shown, |ui| {
                    for l in &d.layers {
                        if ui.button(&l.name).clicked() {
                            picked = Some(json!(l.name));
                            ui.close();
                        }
                    }
                });
            }
            // General's other kinds (lineweight, transparency) aren't in the typed groups.
            _ => {
                value_box(ui, &shown, false, false);
            }
        }
    });
    if let Some(v) = picked.filter(|v| row.varies || *v != row.value) {
        *edit = Some(Edit { params: json!({ "handles": ids, key: v }), label: row.label });
    }
}
