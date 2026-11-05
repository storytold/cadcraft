//! Dimension Style Manager (DIMSTYLE, Format > Dimension Style...): the drawing's dimension
//! styles with New (a copy of the selected style), Rename, Delete and Set Current, and the
//! selected style's variables on four tabs (Lines & Arrows, Text, Primary Units, Tolerances).
//! Every edit runs `dimstyle` with the changed variables, so it is undoable, and dimensions of
//! that style are redrawn from it.

use cadcraft_doc::{DimStyle, EntityKind};
use cadcraft_render::Arrowhead;
use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use super::{Action, no_drawing, num_field, set_temp, temp};
use crate::CadApp;
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
    /// Why a typed value was refused this frame (it is out of range).
    rejected: Option<String>,
}

impl<'a> Rows<'a> {
    fn new(salt: &'a str) -> Self {
        Rows { salt, out: Map::new(), rejected: None }
    }

    fn label(ui: &mut egui::Ui, label: &str, var: &str) -> egui::Id {
        ui.label(label).on_hover_text(var).id
    }

    /// A number at least `min`; a smaller one is refused with a message.
    fn num(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, v: f64, min: f64) {
        let id = Self::label(ui, label, var);
        if let Some(x) = num_field(ui, (self.salt, key), v, Some(id)) {
            if x >= min {
                self.out.insert(key.into(), json!(x));
            } else if min > f64::MIN {
                // Only a real minimum (0 or more, above 0) has a message.
                self.rejected = Some(range_message(label, min));
            }
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

    /// Text kept exactly as typed (DIMPOST's leading spaces matter) and allowed to be empty.
    fn text(&mut self, ui: &mut egui::Ui, label: &str, var: &str, key: &str, v: &str) {
        let id = Self::label(ui, label, var);
        if let Some(t) = text_field(ui, ui.make_persistent_id((self.salt, key)), v, id) {
            self.out.insert(key.into(), json!(t));
        }
        ui.end_row();
    }

    fn heading(ui: &mut egui::Ui, text: &str) {
        ui.label(RichText::new(text).strong());
        ui.end_row();
    }
}

/// The message for a value below `min` in the field labelled `label`.
fn range_message(label: &str, min: f64) -> String {
    if min > 0.0 { crate::tf!("{field} must be greater than 0", field = label) } else { crate::tf!("{field} must be 0 or more", field = label) }
}

/// Whether a dimension, leader or tolerance in the drawing (model, layouts or blocks) uses
/// dimension style `name`, so that `dimstyle.delete` would refuse it.
fn style_in_use(d: &cadcraft_doc::Drawing, name: &str) -> bool {
    let uses = |e: &cadcraft_doc::Entity| match &e.kind {
        EntityKind::Dimension(dm) => dm.style.eq_ignore_ascii_case(name),
        EntityKind::Leader(l) => l.style.eq_ignore_ascii_case(name),
        EntityKind::Tolerance(t) => t.style.eq_ignore_ascii_case(name),
        _ => false,
    };
    d.model.iter().any(|e| uses(e))
        || d.layouts.iter().filter_map(|l| d.space(&cadcraft_doc::Space::Paper(l.name.clone()))).any(|st| st.iter().any(|e| uses(e)))
        || d.blocks.values().any(|b| b.entities.iter().any(|e| uses(e)))
}

/// The text a field commits: on focus loss, when it differs from the stored value. Never trimmed,
/// and empty is a value (it clears DIMPOST).
fn committed(lost_focus: bool, buf: &str, value: &str) -> Option<String> {
    (lost_focus && buf != value).then(|| buf.to_string())
}

/// A single-line field committing its exact text on Enter / focus loss.
fn text_field(ui: &mut egui::Ui, id: egui::Id, value: &str, label: egui::Id) -> Option<String> {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).id(id.with("te")).desired_width(150.0)).labelled_by(label);
    // Focus from memory, as `expr_field` does: `has_focus` is false while the OS window is unfocused.
    if ui.memory(|m| m.has_focus(id.with("te"))) {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    committed(resp.lost_focus(), &buf, value)
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

/// The method shown, given the one last picked in the dialog: the style's, except that a picked
/// Deviation stays while the upper and lower values are equal (the style alone reads Symmetrical).
fn shown_method(s: &DimStyle, picked: Option<usize>) -> usize {
    match tolerance_method(s) {
        1 if picked == Some(2) => 2,
        m => m,
    }
}

/// Where the dialog remembers the tolerance method shown for style `name`.
fn tol_method_id(name: &str) -> egui::Id {
    egui::Id::unique(("dimstyle_tol_method", name))
}

fn tolerances_tab(rows: &mut Rows<'_>, ui: &mut egui::Ui, s: &DimStyle) {
    Rows::heading(ui, crate::tl!("Tolerance format"));
    let methods = numbered(&[crate::tl!("None"), crate::tl!("Symmetrical"), crate::tl!("Deviation"), crate::tl!("Limits")], 0);
    // The method shown for this style, kept across frames (DIMTOL alone can't tell Deviation
    // with equal values from Symmetrical).
    let kept_id = tol_method_id(&s.name);
    let mut method = shown_method(s, ui.data(|d| d.get_temp::<usize>(kept_id)));
    let mut picked = Rows::new(rows.salt);
    picked.choice(ui, crate::tl!("Method"), "DIMTOL / DIMLIM", "method", Some(method), &methods);
    if let Some(m) = picked.out.get("method").and_then(Value::as_u64) {
        // The rows below follow the new method this frame.
        method = usize::try_from(m).unwrap_or(0);
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
    // Remember Symmetrical or Deviation whenever it is shown, so Deviation stays when its values
    // become equal; None and Limits need no memory.
    ui.data_mut(|d| {
        if matches!(method, 1 | 2) {
            d.insert_temp(kept_id, method);
        } else {
            d.remove::<usize>(kept_id);
        }
    });
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
    // The Rename text belongs to the style it was typed for: another selection starts it empty.
    let mut rename = temp::<(String, String)>(ctx, "dimstyle_rename").filter(|(of, _)| *of == sel).map(|(_, r)| r).unwrap_or_default();
    let mut tab = temp::<Tab>(ctx, "dimstyle_tab").unwrap_or_default();
    // Why the last typed value was refused (style name, message).
    let mut warning = temp::<Option<(String, String)>>(ctx, "dimstyle_warning").flatten();
    // A New style copies the selected one; the drawing has no styles when it shows the default.
    let base_exists = d.dim_styles.iter().any(|s| s.name == sel);
    let in_use = !sel.eq_ignore_ascii_case("Standard") && !sel.eq_ignore_ascii_case(&current) && {
        // Scanned once per drawing revision and selection, not every frame.
        let key = app.session.state().ok().map(|st| (st.uid, st.revision, sel.clone()));
        match temp::<(Option<(u64, u64, String)>, bool)>(ctx, "dimstyle_in_use") {
            Some((k, used)) if k.is_some() && k == key => used,
            _ => {
                let used = style_in_use(d, &sel);
                set_temp(ctx, "dimstyle_in_use", (key, used));
                used
            }
        }
    };
    // A remembered tolerance method is dropped once the style has no tolerances (turned off
    // elsewhere, or undone), whichever tab is shown.
    if styles.iter().find(|s| s.name == sel).is_some_and(|s| !s.tolerance || s.limits) {
        ctx.data_mut(|m| m.remove::<usize>(tol_method_id(&sel)));
    }
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
                    let reason = if standard {
                        crate::tl!("The Standard style cannot be deleted")
                    } else if is_current {
                        crate::tl!("The current style cannot be deleted")
                    } else {
                        crate::tl!("Dimensions or leaders use this style")
                    };
                    if ui
                        .add_enabled(!standard && !is_current && !in_use, egui::Button::new(crate::tl!("Delete")))
                        .on_disabled_hover_text(reason)
                        .clicked()
                    {
                        action = Some(("dimstyle.delete", json!({ "name": sel })));
                        // A later style of the same name starts without this one's memory.
                        ui.data_mut(|m| m.remove::<usize>(tol_method_id(&sel)));
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
                        // A new style starts as a copy of the selected one, as in AutoCAD.
                        let mut p = json!({ "name": name, "current": false });
                        if base_exists && let Some(o) = p.as_object_mut() {
                            o.insert("base".into(), json!(sel));
                        }
                        action = Some(("dimstyle", p));
                        ui.data_mut(|m| m.remove::<usize>(tol_method_id(&name)));
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
                        // The remembered tolerance method goes with the style.
                        ui.data_mut(|m| {
                            let kept = m.get_temp::<usize>(tol_method_id(&sel));
                            m.remove::<usize>(tol_method_id(&sel));
                            match kept {
                                Some(k) => {
                                    m.insert_temp(tol_method_id(&to), k);
                                }
                                None => {
                                    m.remove::<usize>(tol_method_id(&to));
                                }
                            }
                        });
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
                let mut rows = Rows::new(&salt);
                if let Some(msg) = warning.as_ref().filter(|(of, _)| *of == style.name).map(|(_, m)| m) {
                    ui.colored_label(t.warn, msg);
                }
                egui::ScrollArea::vertical().id_salt("dimstyle_props").auto_shrink([false, true]).show(ui, |ui| {
                    egui::Grid::new(("dimstyle_grid", tab as u8)).num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| match tab {
                        Tab::Lines => lines_tab(&mut rows, ui, style),
                        Tab::Text => text_tab(&mut rows, ui, style, &text_styles),
                        Tab::Units => units_tab(&mut rows, ui, style),
                        Tab::Tolerances => tolerances_tab(&mut rows, ui, style),
                    });
                });
                // A refused value shows why until the next accepted edit.
                if let Some(msg) = rows.rejected.take() {
                    warning = Some((style.name.clone(), msg));
                } else if !rows.out.is_empty() {
                    warning = None;
                }
                if !rows.out.is_empty() {
                    let mut p = rows.out;
                    p.insert("name".into(), json!(style.name));
                    p.insert("current".into(), json!(false));
                    action = Some(("dimstyle", Value::Object(p)));
                }
            });
        });
    });
    set_temp(ctx, "dimstyle_rename", (sel.clone(), rename));
    set_temp(ctx, "dimstyle_sel", sel);
    set_temp(ctx, "dimstyle_new", new_name);
    set_temp(ctx, "dimstyle_tab", tab);
    set_temp(ctx, "dimstyle_warning", warning);
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
    fn a_picked_deviation_stays_with_equal_values() {
        let mut s = DimStyle { tolerance: true, ..DimStyle::default() };
        assert_eq!(shown_method(&s, None), 1);
        assert_eq!(shown_method(&s, Some(2)), 2);
        assert_eq!(shown_method(&s, Some(1)), 1);
        // The style decides when it no longer agrees with the pick.
        s.tolerance = false;
        assert_eq!(shown_method(&s, Some(2)), 0);
        s.limits = true;
        assert_eq!(shown_method(&s, Some(2)), 3);
    }

    #[test]
    fn a_refused_value_says_why() {
        assert_eq!(range_message("Arrow size", 0.0), "Arrow size must be 0 or more");
        assert_eq!(range_message("Text height", f64::MIN_POSITIVE), "Text height must be greater than 0");
    }

    #[test]
    fn prefix_suffix_text_is_committed_exactly_and_may_be_empty() {
        assert_eq!(committed(true, "  <> mm", "<> mm"), Some("  <> mm".to_string()));
        assert_eq!(committed(true, "", "<> mm"), Some(String::new()));
        assert_eq!(committed(true, "<> mm ", "<> mm"), Some("<> mm ".to_string()));
        assert_eq!(committed(true, "<> mm", "<> mm"), None);
        assert_eq!(committed(false, "x", "<> mm"), None);
    }

    #[test]
    fn rename_text_is_cleared_when_another_style_is_selected() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        for n in ["A", "B"] {
            app.session.execute("dimstyle", &json!({ "name": n, "current": false })).unwrap();
        }
        let ctx = egui::Context::default();
        let frame = |app: &mut CadApp| {
            let mut open = true;
            ctx.run_ui(egui::RawInput::default(), |ui| {
                dialog(app, ui.ctx(), &mut open);
            })
            .drop_without_applying_deltas();
        };
        set_temp(&ctx, "dimstyle_sel", "A".to_string());
        set_temp(&ctx, "dimstyle_rename", ("A".to_string(), "Typed".to_string()));
        frame(&mut app);
        // Same selection: the typed text stays.
        assert_eq!(temp::<(String, String)>(&ctx, "dimstyle_rename"), Some(("A".to_string(), "Typed".to_string())));
        set_temp(&ctx, "dimstyle_sel", "B".to_string());
        frame(&mut app);
        assert_eq!(temp::<(String, String)>(&ctx, "dimstyle_rename"), Some(("B".to_string(), String::new())));
    }

    #[test]
    fn every_arrowhead_offered_is_a_distinct_one() {
        let kinds: Vec<Arrowhead> = arrows().iter().map(|(v, _)| Arrowhead::parse(v.as_str().unwrap_or("?"))).collect();
        for (i, k) in kinds.iter().enumerate() {
            assert!(!kinds[..i].contains(k), "{k:?} offered twice");
        }
    }
}
