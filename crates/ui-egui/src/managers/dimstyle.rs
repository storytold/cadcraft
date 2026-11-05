//! Dimension Style Manager (DIMSTYLE, Format > Dimension Style...): the drawing's dimension
//! styles with New, Rename, Delete and Set Current, and the selected style's variables on four
//! tabs (Lines & Arrows, Text, Primary Units, Tolerances). Every edit runs `dimstyle` with the
//! changed variables, so it is undoable, and dimensions of that style are redrawn from it.

use cadcraft_doc::DimStyle;
use cadcraft_render::Arrowhead;
use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use super::{Action, no_drawing, num_field, set_temp, temp};
use crate::CadApp;
use crate::parametric::expr_field;
use crate::theme::Tokens;

/// The tabs of the dialog, in order.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Lines,
    Text,
    Units,
    Tolerances,
}

/// DIMZIN / DIMTZIN bits: suppress leading and trailing zeros of decimal values.
const LEADING: u8 = 4;
const TRAILING: u8 = 8;

/// The arrowheads offered, as DIMBLK names ("" = closed filled).
fn arrows() -> Vec<(Value, String)> {
    [
        ("", crate::tl!("Closed filled")),
        ("_ClosedBlank", crate::tl!("Closed blank")),
        ("_Closed", crate::tl!("Closed")),
        ("_Dot", crate::tl!("Dot")),
        ("_ArchTick", crate::tl!("Architectural tick")),
        ("_Oblique", crate::tl!("Oblique")),
        ("_Open", crate::tl!("Open")),
        ("_Origin", crate::tl!("Origin indicator")),
        ("_Open90", crate::tl!("Right angle")),
        ("_Open30", crate::tl!("Open 30")),
        ("_DotSmall", crate::tl!("Dot small")),
        ("_DotBlank", crate::tl!("Dot blank")),
        ("_Small", crate::tl!("Small")),
        ("_BoxFilled", crate::tl!("Box filled")),
        ("_BoxBlank", crate::tl!("Box")),
        ("_DatumFilled", crate::tl!("Datum triangle filled")),
        ("_DatumBlank", crate::tl!("Datum triangle")),
        ("_Integral", crate::tl!("Integral")),
        ("_None", crate::tl!("None")),
    ]
    .into_iter()
    .map(|(v, l)| (json!(v), l.to_string()))
    .collect()
}

fn numbered(labels: &[&str], first: u64) -> Vec<(Value, String)> {
    labels.iter().zip(first..).map(|(l, v)| (json!(v), (*l).to_string())).collect()
}

/// Precision choices 0 to 8 shown as AutoCAD shows them ("0", "0.0", "0.00", ...).
fn precisions() -> Vec<(Value, String)> {
    (0..=8u64).map(|n| (json!(n), if n == 0 { "0".to_string() } else { format!("0.{}", "0".repeat(n as usize)) })).collect()
}

/// Collects the variables a tab changed this frame.
struct Rows<'a> {
    salt: &'a str,
    out: Map<String, Value>,
}

impl Rows<'_> {
    fn label(ui: &mut egui::Ui, label: &str, var: &str) -> egui::Id {
        ui.label(label).on_hover_text(var).id
    }

    /// A number at least `min`.
    fn num(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, v: f64, min: f64) {
        let id = Self::label(ui, label, var);
        if let Some(x) = num_field(ui, (self.salt, key), v, Some(id)).filter(|x| *x >= min) {
            self.out.insert(key.into(), json!(x));
        }
        ui.end_row();
    }

    fn check(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, v: bool) {
        ui.label("");
        let mut b = v;
        if ui.checkbox(&mut b, label).on_hover_text(var).changed() {
            self.out.insert(key.into(), json!(b));
        }
        ui.end_row();
    }

    /// A zero-suppression bit of DIMZIN-like `key`.
    fn bit(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, bits: u8, bit: u8) {
        ui.label("");
        let mut b = bits & bit != 0;
        if ui.checkbox(&mut b, label).on_hover_text(var).changed() {
            self.out.insert(key.into(), json!(if b { bits | bit } else { bits & !bit }));
        }
        ui.end_row();
    }

    fn choice(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, selected: Option<usize>, options: &[(Value, String)]) {
        let id = Self::label(ui, label, var);
        let shown = selected.and_then(|i| options.get(i)).map_or("", |(_, l)| l.as_str());
        let mut picked = None;
        let r = craft_ui::ComboBox::from_id_salt((self.salt, key)).selected_text(shown).width(170.0).show_ui(ui, |ui| {
            for (i, (v, l)) in options.iter().enumerate() {
                if ui.selectable_label(selected == Some(i), l).clicked() && selected != Some(i) {
                    picked = Some(v.clone());
                }
            }
        });
        r.response.labelled_by(id);
        if let Some(v) = picked {
            self.out.insert(key.into(), v);
        }
        ui.end_row();
    }

    fn text(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, v: &str) {
        let id = Self::label(ui, label, var);
        if let Some(t) = expr_field(ui, ui.make_persistent_id((self.salt, key)), v, Some(id)) {
            self.out.insert(key.into(), json!(t));
        }
        ui.end_row();
    }

    fn heading(ui: &mut egui::Ui, text: &str) {
        ui.label(RichText::new(text).strong());
        ui.end_row();
    }
}

fn index_of(options: &[(Value, String)], v: &Value) -> Option<usize> {
    options.iter().position(|(o, _)| o == v)
}

fn lines_tab(rows: &mut Rows<'_>, ui: &mut egui::Ui, s: &DimStyle) {
    Rows::heading(ui, crate::tl!("Dimension lines"));
    rows.num(ui, crate::tl!("Extend beyond ticks"), "DIMDLE", "dimLineExtend", s.dim_line_extend, 0.0);
    rows.num(ui, crate::tl!("Baseline spacing"), "DIMDLI", "baselineSpacing", s.baseline_spacing, 0.0);
    rows.check(ui, crate::tl!("Suppress dimension line 1"), "DIMSD1", "suppressDim1", s.suppress_dim1);
    rows.check(ui, crate::tl!("Suppress dimension line 2"), "DIMSD2", "suppressDim2", s.suppress_dim2);
    Rows::heading(ui, crate::tl!("Extension lines"));
    rows.num(ui, crate::tl!("Extend beyond dimension lines"), "DIMEXE", "extExtend", s.ext_extend, 0.0);
    rows.num(ui, crate::tl!("Offset from origin"), "DIMEXO", "extOffset", s.ext_offset, 0.0);
    rows.check(ui, crate::tl!("Suppress extension line 1"), "DIMSE1", "suppressExt1", s.suppress_ext1);
    rows.check(ui, crate::tl!("Suppress extension line 2"), "DIMSE2", "suppressExt2", s.suppress_ext2);
    Rows::heading(ui, crate::tl!("Arrowheads"));
    let arrows = arrows();
    let kind = Arrowhead::parse(&s.arrow_block);
    let sel = arrows.iter().position(|(v, _)| Arrowhead::parse(v.as_str().unwrap_or("")) == kind);
    rows.choice(ui, crate::tl!("Arrowhead"), "DIMBLK", "arrowBlock", sel, &arrows);
    rows.num(ui, crate::tl!("Arrow size"), "DIMASZ", "arrowSize", s.arrow_size, 0.0);
    rows.num(ui, crate::tl!("Tick size"), "DIMTSZ", "tickSize", s.tick_size, 0.0);
    rows.num(ui, crate::tl!("Center mark size"), "DIMCEN", "centerMark", s.center_mark, f64::MIN);
}

fn text_tab(rows: &mut Rows<'_>, ui: &mut egui::Ui, s: &DimStyle, text_styles: &[String]) {
    Rows::heading(ui, crate::tl!("Text appearance"));
    let styles: Vec<(Value, String)> = text_styles.iter().map(|n| (json!(n), n.clone())).collect();
    let sel = styles.iter().position(|(_, n)| n.eq_ignore_ascii_case(&s.text_style));
    rows.choice(ui, crate::tl!("Text style"), "DIMTXSTY", "textStyle", sel, &styles);
    rows.num(ui, crate::tl!("Text height"), "DIMTXT", "textHeight", s.text_height, f64::MIN_POSITIVE);
    Rows::heading(ui, crate::tl!("Text placement"));
    let vertical = numbered(&[crate::tl!("Centered"), crate::tl!("Above"), crate::tl!("Outside"), "JIS", crate::tl!("Below")], 0);
    rows.choice(ui, crate::tl!("Vertical"), "DIMTAD", "textAbove", index_of(&vertical, &json!(s.text_above)), &vertical);
    let horizontal = numbered(
        &[
            crate::tl!("Centered"),
            crate::tl!("At extension line 1"),
            crate::tl!("At extension line 2"),
            crate::tl!("Over extension line 1"),
            crate::tl!("Over extension line 2"),
        ],
        0,
    );
    rows.choice(ui, crate::tl!("Horizontal"), "DIMJUST", "textJust", index_of(&horizontal, &json!(s.text_just)), &horizontal);
    rows.num(ui, crate::tl!("Offset from dimension line"), "DIMGAP", "textGap", s.text_gap, f64::MIN);
    Rows::heading(ui, crate::tl!("Text alignment"));
    rows.check(ui, crate::tl!("Horizontal inside extension lines"), "DIMTIH", "textInsideHorizontal", s.text_inside_horizontal);
    rows.check(ui, crate::tl!("Horizontal outside extension lines"), "DIMTOH", "textOutsideHorizontal", s.text_outside_horizontal);
    Rows::heading(ui, crate::tl!("Scale for dimension features"));
    rows.num(ui, crate::tl!("Overall scale"), "DIMSCALE", "scale", s.scale, 0.0);
}

fn units_tab(rows: &mut Rows<'_>, ui: &mut egui::Ui, s: &DimStyle) {
    Rows::heading(ui, crate::tl!("Linear dimensions"));
    let units = numbered(
        &[
            crate::tl!("Scientific"),
            crate::tl!("Decimal"),
            crate::tl!("Engineering"),
            crate::tl!("Architectural"),
            crate::tl!("Fractional"),
            crate::tl!("Windows Desktop"),
        ],
        1,
    );
    rows.choice(ui, crate::tl!("Unit format"), "DIMLUNIT", "linearUnit", index_of(&units, &json!(s.linear_unit)), &units);
    let precisions = precisions();
    rows.choice(ui, crate::tl!("Precision"), "DIMDEC", "decimals", index_of(&precisions, &json!(s.decimals)), &precisions);
    let separators: Vec<(Value, String)> = [(".", crate::tl!("'.' (Period)")), (",", crate::tl!("',' (Comma)")), (" ", crate::tl!("' ' (Space)"))]
        .into_iter()
        .map(|(v, l)| (json!(v), l.to_string()))
        .collect();
    rows.choice(ui, crate::tl!("Decimal separator"), "DIMDSEP", "decimalSeparator", index_of(&separators, &json!(s.decimal_separator)), &separators);
    rows.num(ui, crate::tl!("Round off"), "DIMRND", "round", s.round, 0.0);
    rows.text(ui, crate::tl!("Prefix and suffix (<> = measurement)"), "DIMPOST", "post", &s.post);
    rows.num(ui, crate::tl!("Measurement scale factor"), "DIMLFAC", "linearFactor", s.linear_factor, f64::MIN);
    rows.bit(ui, crate::tl!("Suppress leading zeros"), "DIMZIN", "zeroSuppression", s.zero_suppression, LEADING);
    rows.bit(ui, crate::tl!("Suppress trailing zeros"), "DIMZIN", "zeroSuppression", s.zero_suppression, TRAILING);
    Rows::heading(ui, crate::tl!("Angular dimensions"));
    let angular = numbered(&[crate::tl!("Decimal Degrees"), crate::tl!("Degrees Minutes Seconds"), crate::tl!("Gradians"), crate::tl!("Radians")], 0);
    rows.choice(ui, crate::tl!("Units format"), "DIMAUNIT", "angularUnit", index_of(&angular, &json!(s.angular_unit)), &angular);
    rows.choice(ui, crate::tl!("Angular precision"), "DIMADEC", "angularDecimals", index_of(&precisions, &json!(s.angular_decimals)), &precisions);
}

/// The tolerance method shown: 0 none, 1 symmetrical, 2 deviation, 3 limits.
fn tolerance_method(s: &DimStyle) -> usize {
    match (s.limits, s.tolerance) {
        (true, _) => 3,
        (false, true) if s.tol_plus == s.tol_minus => 1,
        (false, true) => 2,
        (false, false) => 0,
    }
}

fn tolerances_tab(rows: &mut Rows<'_>, ui: &mut egui::Ui, s: &DimStyle) {
    Rows::heading(ui, crate::tl!("Tolerance format"));
    let methods = numbered(&[crate::tl!("None"), crate::tl!("Symmetrical"), crate::tl!("Deviation"), crate::tl!("Limits")], 0);
    let method = tolerance_method(s);
    let mut picked = Rows { salt: rows.salt, out: Map::new() };
    picked.choice(ui, crate::tl!("Method"), "DIMTOL / DIMLIM", "method", Some(method), &methods);
    if let Some(m) = picked.out.get("method").and_then(Value::as_u64) {
        let (tol, lim) = match m {
            1 | 2 => (true, false),
            3 => (false, true),
            _ => (false, false),
        };
        rows.out.insert("tolerance".into(), json!(tol));
        rows.out.insert("limits".into(), json!(lim));
        if m == 1 {
            rows.out.insert("tolMinus".into(), json!(s.tol_plus));
        }
    }
    let precisions = precisions();
    rows.choice(ui, crate::tl!("Precision"), "DIMTDEC", "tolDecimals", index_of(&precisions, &json!(s.tol_decimals)), &precisions);
    rows.num(ui, crate::tl!("Upper value"), "DIMTP", "tolPlus", s.tol_plus, f64::MIN);
    // Symmetrical: one value, plus and minus.
    if method == 1 {
        if let Some(v) = rows.out.get("tolPlus").cloned() {
            rows.out.insert("tolMinus".into(), v);
        }
    } else {
        rows.num(ui, crate::tl!("Lower value"), "DIMTM", "tolMinus", s.tol_minus, f64::MIN);
    }
    rows.num(ui, crate::tl!("Scaling for height"), "DIMTFAC", "tolScale", s.tol_scale, f64::MIN_POSITIVE);
    let just = numbered(&[crate::tl!("Bottom"), crate::tl!("Middle"), crate::tl!("Top")], 0);
    rows.choice(ui, crate::tl!("Vertical position"), "DIMTOLJ", "tolJust", index_of(&just, &json!(s.tol_just)), &just);
    rows.bit(ui, crate::tl!("Suppress leading zeros"), "DIMTZIN", "tolZeroSuppression", s.tol_zero_suppression, LEADING);
    rows.bit(ui, crate::tl!("Suppress trailing zeros"), "DIMTZIN", "tolZeroSuppression", s.tol_zero_suppression, TRAILING);
}

/// Show the Dimension Style Manager; returns the command to run.
pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let title = crate::tl!("Dimension Style Manager");
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, title, open);
        return None;
    };
    let t = Tokens::get();
    let mut styles: Vec<DimStyle> = d.dim_styles.clone();
    if styles.is_empty() {
        styles.push(DimStyle::default());
    }
    let text_styles: Vec<String> = d.text_styles.iter().map(|s| s.name.clone()).collect();
    let current = d.header.str("DIMSTYLE", "Standard");
    let mut sel = temp::<String>(ctx, "dimstyle_sel")
        .filter(|s| styles.iter().any(|x| x.name == *s))
        .unwrap_or_else(|| styles.iter().find(|s| s.name.eq_ignore_ascii_case(&current)).map_or_else(|| current.clone(), |s| s.name.clone()));
    let mut new_name = temp::<String>(ctx, "dimstyle_new").unwrap_or_default();
    let mut rename = temp::<String>(ctx, "dimstyle_rename").unwrap_or_default();
    let mut tab = temp::<Tab>(ctx, "dimstyle_tab").unwrap_or_default();
    let mut action: Action = None;
    let taken = |n: &str| styles.iter().any(|s| s.name.eq_ignore_ascii_case(n));
    egui::Window::new(title).id(egui::Id::unique("dimstyle_manager")).open(open).default_size(vec2(720.0, 460.0)).resizable(true).show(ctx, |ui| {
        ui.label(RichText::new(crate::tf!("Current dimension style: {name}", name = current)).color(t.text_dim));
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.label(RichText::new(crate::tl!("Styles")).strong());
                egui::ScrollArea::vertical().id_salt("dimstyle_list").max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                    for s in &styles {
                        let mut label = RichText::new(&s.name);
                        if s.name.eq_ignore_ascii_case(&current) {
                            label = label.strong();
                        }
                        if ui.selectable_label(s.name == sel, label).clicked() {
                            sel = s.name.clone();
                        }
                    }
                });
                let standard = sel.eq_ignore_ascii_case("Standard");
                let is_current = sel.eq_ignore_ascii_case(&current);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!is_current, egui::Button::new(crate::tl!("Set Current"))).clicked() {
                        action = Some(("dimstyle.current", json!({ "name": sel })));
                    }
                    if ui.add_enabled(!standard && !is_current, egui::Button::new(crate::tl!("Delete"))).clicked() {
                        action = Some(("dimstyle.delete", json!({ "name": sel })));
                    }
                });
                ui.horizontal(|ui| {
                    let hint = crate::tl!("New style name");
                    let field = ui
                        .add(egui::TextEdit::singleline(&mut new_name).hint_text(hint).desired_width(120.0))
                        .accessible_name(hint)
                        .on_hover_text(hint);
                    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let name = new_name.trim().to_string();
                    let ok = !name.is_empty() && !taken(&name);
                    if (ui.add_enabled(ok, egui::Button::new(crate::tl!("New"))).clicked() || entered) && ok {
                        // A new style starts as a copy of the current one (DIMSTYLE's base).
                        action = Some(("dimstyle", json!({ "name": name, "current": false })));
                        sel = name;
                        new_name.clear();
                    }
                });
                ui.horizontal(|ui| {
                    let hint = crate::tl!("Rename to");
                    let field = ui
                        .add_enabled(!standard, egui::TextEdit::singleline(&mut rename).hint_text(hint).desired_width(120.0))
                        .accessible_name(hint)
                        .on_hover_text(hint);
                    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let to = rename.trim().to_string();
                    let ok = !standard && !to.is_empty() && !taken(&to);
                    if (ui.add_enabled(ok, egui::Button::new(crate::tl!("Rename"))).clicked() || entered) && ok {
                        action = Some(("dimstyle.rename", json!({ "from": sel, "to": to })));
                        sel = to;
                        rename.clear();
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut tab, Tab::Lines, crate::tl!("Lines & Arrows"));
                    ui.selectable_value(&mut tab, Tab::Text, crate::tl!("Text"));
                    ui.selectable_value(&mut tab, Tab::Units, crate::tl!("Primary Units"));
                    ui.selectable_value(&mut tab, Tab::Tolerances, crate::tl!("Tolerances"));
                });
                ui.separator();
                let Some(style) = styles.iter().find(|s| s.name == sel) else { return };
                ui.label(RichText::new(crate::tf!("Properties of {name}", name = style.name)).strong());
                let salt = format!("dimstyle_{}", style.name);
                let mut rows = Rows { salt: &salt, out: Map::new() };
                egui::ScrollArea::vertical().id_salt("dimstyle_props").auto_shrink([false, true]).show(ui, |ui| {
                    egui::Grid::new(("dimstyle_grid", tab as u8)).num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| match tab {
                        Tab::Lines => lines_tab(&mut rows, ui, style),
                        Tab::Text => text_tab(&mut rows, ui, style, &text_styles),
                        Tab::Units => units_tab(&mut rows, ui, style),
                        Tab::Tolerances => tolerances_tab(&mut rows, ui, style),
                    });
                });
                if !rows.out.is_empty() {
                    let mut p = rows.out;
                    p.insert("name".into(), json!(style.name));
                    p.insert("current".into(), json!(false));
                    action = Some(("dimstyle", Value::Object(p)));
                }
            });
        });
    });
    set_temp(ctx, "dimstyle_sel", sel);
    set_temp(ctx, "dimstyle_new", new_name);
    set_temp(ctx, "dimstyle_rename", rename);
    set_temp(ctx, "dimstyle_tab", tab);
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_method_reads_the_style() {
        let mut s = DimStyle::default();
        assert_eq!(tolerance_method(&s), 0);
        s.tolerance = true;
        s.tol_plus = 0.1;
        s.tol_minus = 0.1;
        assert_eq!(tolerance_method(&s), 1);
        s.tol_minus = 0.2;
        assert_eq!(tolerance_method(&s), 2);
        s.limits = true;
        assert_eq!(tolerance_method(&s), 3);
    }

    #[test]
    fn every_arrowhead_offered_is_a_distinct_one() {
        let kinds: Vec<Arrowhead> = arrows().iter().map(|(v, _)| Arrowhead::parse(v.as_str().unwrap_or("?"))).collect();
        for (i, k) in kinds.iter().enumerate() {
            assert!(!kinds[..i].contains(k), "{k:?} offered twice");
        }
    }
}
