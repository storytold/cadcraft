//! The Options dialog's Dimensions tab: the current dimension style's number format (units,
//! decimal places, decimal separator, zero suppression, rounding, prefix and suffix), angular
//! units, text and arrow sizes. A change goes into the `dimStyle` option, which sets the open
//! drawing's current dimension style and keeps the value for the drawings made from now on.

use egui::RichText;
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// DIMLUNIT values and their names.
fn linear_units() -> [(i64, &'static str); 6] {
    [
        (2, crate::tl!("Decimal")),
        (1, crate::tl!("Scientific")),
        (3, crate::tl!("Engineering")),
        (4, crate::tl!("Architectural")),
        (5, crate::tl!("Fractional")),
        (6, crate::tl!("Windows desktop")),
    ]
}

/// DIMAUNIT values and their names.
fn angular_units() -> [(i64, &'static str); 4] {
    [(0, crate::tl!("Decimal degrees")), (1, crate::tl!("Degrees, minutes, seconds")), (2, crate::tl!("Gradians")), (3, crate::tl!("Radians"))]
}

/// DIMTAD values and their names.
fn text_positions() -> [(i64, &'static str); 4] {
    [(0, crate::tl!("Centered")), (1, crate::tl!("Above")), (2, crate::tl!("Outside")), (3, crate::tl!("JIS"))]
}

/// DIMDSEP values and their names.
fn separators() -> [(&'static str, &'static str); 3] {
    [(".", crate::tl!("Period (.)")), (",", crate::tl!("Comma (,)")), (" ", crate::tl!("Space"))]
}

/// DIMZIN bit: suppress leading zeros / trailing zeros.
const LEADING: i64 = 4;
const TRAILING: i64 = 8;

/// A variable's value: the open drawing's current style, else the user's default for new
/// drawings, else the built-in style's.
fn value(app: &CadApp, style: &Map<String, Value>, var: &str) -> Value {
    if let Some(v) = style.get(var) {
        return v.clone();
    }
    if let Some(v) = app.session.dim_defaults.get(var) {
        return v.clone();
    }
    let mut built_in = serde_json::to_value(cadcraft_engine::doc::DimStyle::default()).unwrap_or(Value::Null);
    let field = cadcraft_engine::doc::DIMVARS.iter().find(|(v, _)| *v == var).map(|(_, f)| *f).unwrap_or("");
    built_in.get_mut(field).map(Value::take).unwrap_or(Value::Null)
}

/// A dimension variable as a whole number, a real one rounded (styles read from files may store
/// them as reals).
fn int(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)).unwrap_or(0)
}

/// A combo box over `(value, label)` pairs; a pick goes into `dim` under `var`.
fn choice(ui: &mut egui::Ui, label: egui::Id, salt: &str, current: i64, items: &[(i64, &str)], var: &str, dim: &mut Map<String, Value>) {
    let shown = items.iter().find(|(v, _)| *v == current).map_or_else(|| current.to_string(), |(_, l)| (*l).to_string());
    let combo = craft_ui::ComboBox::from_id_salt(salt).selected_text(shown).width(220.0).show_ui(ui, |ui| {
        for (v, label) in items {
            if ui.selectable_label(*v == current, *label).clicked() && *v != current {
                dim.insert(var.into(), json!(v));
            }
        }
    });
    combo.response.labelled_by(label);
}

/// What a drag field commits: while it is dragged its value lives in egui's temp data under `id`
/// (the field shows it) and nothing is sent; the value goes out once, when the drag ends or a
/// typed value is entered, so one edit is one `dimstyle` run and one undo step.
fn commit<N: Copy + PartialEq + Send + Sync + 'static>(
    ui: &egui::Ui,
    id: egui::Id,
    r: &egui::Response,
    held: Option<N>,
    x: N,
    current: N,
) -> Option<N> {
    if r.dragged() {
        ui.data_mut(|d| d.insert_temp(id, x));
        return None;
    }
    if held.is_some() {
        ui.data_mut(|d| d.remove::<N>(id));
    }
    ((held.is_some() || r.changed()) && x != current).then_some(x)
}

/// A decimal number field; an edit goes into `dim` under `var` (see [`commit`]).
fn number(
    ui: &mut egui::Ui,
    label: egui::Id,
    current: f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    var: &str,
    dim: &mut Map<String, Value>,
) {
    let id = egui::Id::unique(("cc_options_dim_number", var));
    let held = ui.data(|d| d.get_temp::<f64>(id));
    let mut x = held.unwrap_or(current);
    let r = ui.add(egui::DragValue::new(&mut x).range(range).speed(speed).max_decimals(6).update_while_editing(false)).labelled_by(label);
    if let Some(x) = commit(ui, id, &r, held, x, current) {
        dim.insert(var.into(), json!(x));
    }
}

/// A whole-number field; an edit goes into `dim` under `var` (see [`commit`]).
fn whole(ui: &mut egui::Ui, label: egui::Id, current: i64, range: std::ops::RangeInclusive<i64>, var: &str, dim: &mut Map<String, Value>) {
    let id = egui::Id::unique(("cc_options_dim_whole", var));
    let held = ui.data(|d| d.get_temp::<i64>(id));
    let mut x = held.unwrap_or(current);
    let r = ui.add(egui::DragValue::new(&mut x).range(range).update_while_editing(false)).labelled_by(label);
    if let Some(x) = commit(ui, id, &r, held, x, current) {
        dim.insert(var.into(), json!(x));
    }
}

/// A text field that commits when it loses focus; `Some(text)` on a change.
fn text_field(ui: &mut egui::Ui, label: egui::Id, id: &str, current: &str, width: f32) -> Option<String> {
    let id = egui::Id::unique(("cc_options_dim_text", id));
    let mut buf = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(width)).labelled_by(label);
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if buf != current {
            return Some(buf);
        }
    }
    None
}

/// DIMPOST split into its prefix and suffix around the `<>` that stands for the measurement; a
/// template without one is a suffix.
pub(super) fn split_post(post: &str) -> (String, String) {
    match post.split_once("<>") {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (String::new(), post.to_string()),
    }
}

/// The DIMPOST template of a prefix and suffix (empty when both are).
pub(super) fn join_post(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() && suffix.is_empty() { String::new() } else { format!("{prefix}<>{suffix}") }
}

/// Dimensions tab: the current dimension style's number format, text and arrow sizes; edits go into
/// `changes` as one `dimStyle` change.
pub(super) fn dimensions(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    let t = Tokens::get();
    let style = super::current_dim_style(app);
    let mut dim = Map::new();
    match super::current_dim_style_name(app) {
        Some(name) => {
            ui.label(RichText::new(format!("{}: {name}", crate::tl!("Current dimension style"))).strong());
        }
        None => {
            ui.label(RichText::new(crate::tl!("No drawing is open: these values are kept for new drawings.")).color(t.text_dim));
        }
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("options_dimensions").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
        super::dialog::grid(ui, "options_dimensions_grid", |ui| {
            let v = |var: &str| value(app, &style, var);

            let label = ui.label(crate::tl!("Linear units"));
            choice(ui, label.id, "dim_lunit", int(&v("DIMLUNIT")), &linear_units(), "DIMLUNIT", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Precision (decimal places)"));
            whole(ui, label.id, int(&v("DIMDEC")), 0..=8, "DIMDEC", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Decimal separator"));
            let sep = v("DIMDSEP").as_str().unwrap_or(".").to_string();
            let sep = if sep.is_empty() { ".".to_string() } else { sep };
            let shown = separators().iter().find(|(c, _)| *c == sep).map_or_else(|| sep.clone(), |(_, l)| (*l).to_string());
            let combo = craft_ui::ComboBox::from_id_salt("dim_dsep").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                for (c, label) in separators() {
                    if ui.selectable_label(c == sep, label).clicked() && c != sep {
                        dim.insert("DIMDSEP".into(), json!(c));
                    }
                }
            });
            combo.response.labelled_by(label.id);
            ui.end_row();

            ui.label(crate::tl!("Zero suppression"));
            ui.vertical(|ui| {
                let zin = int(&v("DIMZIN"));
                for (bit, label) in [(LEADING, crate::tl!("Suppress leading zeros")), (TRAILING, crate::tl!("Suppress trailing zeros"))] {
                    let mut on = zin & bit != 0;
                    if ui.checkbox(&mut on, label).changed() {
                        dim.insert("DIMZIN".into(), json!(if on { zin | bit } else { zin & !bit }));
                    }
                }
            });
            ui.end_row();

            let label = ui.label(crate::tl!("Round off to"));
            number(ui, label.id, v("DIMRND").as_f64().unwrap_or(0.0), 0.0..=1.0e6, 0.01, "DIMRND", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Linear scale factor"));
            number(ui, label.id, v("DIMLFAC").as_f64().unwrap_or(1.0), 1.0e-6..=1.0e6, 0.01, "DIMLFAC", &mut dim);
            ui.end_row();

            let (prefix, suffix) = split_post(v("DIMPOST").as_str().unwrap_or(""));
            let label = ui.label(crate::tl!("Prefix"));
            if let Some(new) = text_field(ui, label.id, "prefix", &prefix, 120.0) {
                dim.insert("DIMPOST".into(), json!(join_post(&new, &suffix)));
            }
            ui.end_row();
            let label = ui.label(crate::tl!("Suffix"));
            if let Some(new) = text_field(ui, label.id, "suffix", &suffix, 120.0) {
                dim.insert("DIMPOST".into(), json!(join_post(&prefix, &new)));
            }
            ui.end_row();

            let label = ui.label(crate::tl!("Angular units"));
            choice(ui, label.id, "dim_aunit", int(&v("DIMAUNIT")), &angular_units(), "DIMAUNIT", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Angular precision"));
            whole(ui, label.id, int(&v("DIMADEC")), 0..=8, "DIMADEC", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Text height"));
            number(ui, label.id, v("DIMTXT").as_f64().unwrap_or(0.18), 1.0e-6..=1.0e6, 0.01, "DIMTXT", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Text position"));
            choice(ui, label.id, "dim_tad", int(&v("DIMTAD")), &text_positions(), "DIMTAD", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Offset from dimension line"));
            number(ui, label.id, v("DIMGAP").as_f64().unwrap_or(0.09), 0.0..=1.0e6, 0.01, "DIMGAP", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Arrow size"));
            number(ui, label.id, v("DIMASZ").as_f64().unwrap_or(0.18), 0.0..=1.0e6, 0.01, "DIMASZ", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Extension line offset"));
            number(ui, label.id, v("DIMEXO").as_f64().unwrap_or(0.0625), 0.0..=1.0e6, 0.01, "DIMEXO", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Extension beyond dimension line"));
            number(ui, label.id, v("DIMEXE").as_f64().unwrap_or(0.18), 0.0..=1.0e6, 0.01, "DIMEXE", &mut dim);
            ui.end_row();

            let label = ui.label(crate::tl!("Overall scale"));
            number(ui, label.id, v("DIMSCALE").as_f64().unwrap_or(1.0), 0.0..=1.0e6, 0.1, "DIMSCALE", &mut dim);
            ui.end_row();
        });
    });
    if !dim.is_empty() {
        changes.insert("dimStyle".into(), Value::Object(dim));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let has_defaults = !app.session.dim_defaults.is_empty();
        if ui.add_enabled(has_defaults, egui::Button::new(crate::tl!("Forget defaults for new drawings"))).clicked() {
            changes.insert("dimDefaults".into(), json!({}));
        }
    });
    super::dialog::hint(
        ui,
        crate::tl!("Changes apply to the current dimension style at once and are kept for new drawings. Other drawings keep their own styles."),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prefix and suffix are DIMPOST's text around `<>`; splitting and joining it gives the same
    /// template back.
    #[test]
    fn prefix_and_suffix_round_trip_through_the_post_template() {
        assert_eq!(split_post("<> mm"), (String::new(), " mm".to_string()));
        assert_eq!(split_post("Ø<>"), ("Ø".to_string(), String::new()));
        assert_eq!(split_post("~<> cm"), ("~".to_string(), " cm".to_string()));
        assert_eq!(split_post(" mm"), (String::new(), " mm".to_string()), "a template without <> is a suffix");
        assert_eq!(split_post(""), (String::new(), String::new()));
        assert_eq!(join_post("", ""), "");
        assert_eq!(join_post("Ø", " mm"), "Ø<> mm");
        let (a, b) = split_post("~<> cm");
        assert_eq!(join_post(&a, &b), "~<> cm");
    }
}
