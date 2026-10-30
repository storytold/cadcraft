//! The Dimension Style Manager (DIMSTYLE, DDIM, Format > Dimension Style...): the drawing's
//! dimension styles with Set Current, New, Rename and Delete, and the selected style's variables
//! on the tabs AutoCAD's Modify dialog groups them in, beside a preview drawn by the dimension
//! renderer. Every change runs `dimstyle`, `dimstyle.current`, `dimstyle.rename` or
//! `dimstyle.delete`, so it is undoable and scriptable.

use cadcraft_doc::DimStyle;
use cadcraft_doc::color::Color;
use cadcraft_geom::{Vec2, Vec3};
use egui::{RichText, Stroke, vec2};
use serde_json::{Map, Value, json};

use super::{Action, no_drawing, num_field, set_temp, temp};
use crate::CadApp;
use crate::theme::Tokens;

const TABS: [&str; 7] = ["Lines", "Symbols and Arrows", "Text", "Fit", "Primary Units", "Alternate Units", "Tolerances"];

/// DIMBLK values (our own arrowheads, `cadcraft_render::Arrowhead`) and their names.
const ARROWHEADS: &[(&str, &str)] = &[
    ("", "Closed filled"),
    ("_CLOSEDBLANK", "Closed blank"),
    ("_CLOSED", "Closed"),
    ("_DOT", "Dot"),
    ("_ARCHTICK", "Architectural tick"),
    ("_OBLIQUE", "Oblique"),
    ("_OPEN", "Open"),
    ("_ORIGIN", "Origin indicator"),
    ("_ORIGIN2", "Origin indicator 2"),
    ("_OPEN90", "Right angle"),
    ("_OPEN30", "Open 30"),
    ("_DOTSMALL", "Dot small"),
    ("_DOTBLANK", "Dot blank"),
    ("_SMALL", "Small"),
    ("_BOXBLANK", "Box"),
    ("_BOXFILLED", "Box filled"),
    ("_DATUMBLANK", "Datum triangle"),
    ("_DATUMFILLED", "Datum triangle filled"),
    ("_INTEGRAL", "Integral"),
    ("_NONE", "None"),
];

/// What the dialog keeps between frames.
#[derive(Clone, Default)]
struct State {
    sel: String,
    tab: usize,
    new_name: String,
    based_on: String,
    rename: String,
}

/// What the tabs offer besides the style itself.
struct Lists {
    linetypes: Vec<String>,
    text_styles: Vec<String>,
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        no_drawing(ctx, "Dimension Style Manager", open);
        return None;
    };
    let mut styles: Vec<DimStyle> = d.dim_styles.clone();
    if styles.is_empty() {
        styles.push(DimStyle::default());
    }
    let current = d.header.str("DIMSTYLE", "Standard");
    let dimscale = d.header.f64("DIMSCALE", 1.0);
    let lists = Lists {
        linetypes: d
            .linetypes
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| !n.eq_ignore_ascii_case("ByLayer") && !n.eq_ignore_ascii_case("ByBlock"))
            .collect(),
        text_styles: d.text_styles.iter().map(|s| s.name.clone()).collect(),
    };
    let mut st = temp::<State>(ctx, "dimstyle_state").unwrap_or_default();
    if !styles.iter().any(|s| s.name == st.sel) {
        st.sel = styles.iter().find(|s| s.name.eq_ignore_ascii_case(&current)).map_or_else(|| current.clone(), |s| s.name.clone());
    }
    if !styles.iter().any(|s| s.name == st.based_on) {
        st.based_on = st.sel.clone();
    }
    let mut action = None;
    egui::Window::new("Dimension Style Manager").open(open).default_size(vec2(820.0, 520.0)).resizable(true).show(ctx, |ui| {
        ui.label(RichText::new(format!("Current dimension style: {current}")).color(t.text_dim));
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                if let Some(a) = style_list(ui, &styles, &current, &mut st) {
                    action = Some(a);
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let Some(style) = styles.iter().find(|s| s.name == st.sel) else { return };
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in TABS.iter().enumerate() {
                        if ui.selectable_label(st.tab == i, *name).clicked() {
                            st.tab = i;
                        }
                    }
                });
                ui.separator();
                ui.horizontal_top(|ui| {
                    let mut out = Map::new();
                    egui::ScrollArea::vertical().id_salt("dimstyle_props").max_height(380.0).min_scrolled_width(330.0).show(ui, |ui| {
                        egui::Grid::new(("dimstyle_grid", st.tab)).num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                            tab(ui, st.tab, style, &lists, &mut out);
                        });
                    });
                    if !out.is_empty() {
                        action = Some(("dimstyle", modify_params(&style.name, out)));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("Preview of {}", style.name)).strong());
                        preview(ui, style, dimscale, &t);
                    });
                });
            });
        });
    });
    set_temp(ctx, "dimstyle_state", st);
    action
}

/// The style list and the buttons beside it.
fn style_list(ui: &mut egui::Ui, styles: &[DimStyle], current: &str, st: &mut State) -> Option<(&'static str, Value)> {
    let mut action = None;
    ui.label(RichText::new("Styles").strong());
    egui::ScrollArea::vertical().id_salt("dimstyle_list").max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
        for s in styles {
            let mut label = RichText::new(&s.name);
            if s.name.eq_ignore_ascii_case(current) {
                label = label.strong();
            }
            if ui.selectable_label(s.name == st.sel, label).clicked() {
                st.sel = s.name.clone();
                st.rename.clear();
            }
        }
    });
    let is_current = st.sel.eq_ignore_ascii_case(current);
    let is_standard = st.sel.eq_ignore_ascii_case("Standard");
    ui.horizontal(|ui| {
        if ui.add_enabled(!is_current, egui::Button::new("Set Current")).clicked() {
            action = Some(("dimstyle.current", json!({ "name": st.sel })));
        }
        if ui.add_enabled(!is_current && !is_standard, egui::Button::new("Delete")).on_hover_text("Styles in use can't be deleted").clicked() {
            action = Some(("dimstyle.delete", json!({ "name": st.sel })));
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new("New style").strong());
    ui.add(egui::TextEdit::singleline(&mut st.new_name).hint_text("New style name").desired_width(180.0)).on_hover_text("New style name");
    ui.horizontal(|ui| {
        ui.label("Start with");
        craft_ui::ComboBox::from_id_salt("dimstyle_based_on").selected_text(st.based_on.as_str()).width(110.0).show_ui(ui, |ui| {
            for s in styles {
                ui.selectable_value(&mut st.based_on, s.name.clone(), &s.name);
            }
        });
    });
    let name = st.new_name.trim().to_string();
    let free = |n: &str| !n.is_empty() && !styles.iter().any(|s| s.name.eq_ignore_ascii_case(n));
    if ui.add_enabled(free(&name), egui::Button::new("New")).clicked()
        && let Some(base) = styles.iter().find(|s| s.name == st.based_on)
    {
        action = Some(("dimstyle", new_style_params(&name, base)));
        st.sel = name;
        st.new_name.clear();
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Rename").strong());
    ui.add_enabled_ui(!is_standard, |ui| {
        ui.add(egui::TextEdit::singleline(&mut st.rename).hint_text(st.sel.as_str()).desired_width(180.0))
            .on_hover_text("New name of the selected style");
        let to = st.rename.trim().to_string();
        if ui.add_enabled(free(&to), egui::Button::new("Rename")).clicked() {
            action = Some(("dimstyle.rename", json!({ "from": st.sel, "to": to })));
            st.sel = to;
            st.rename.clear();
        }
    });
    action
}

/// `dimstyle` parameters for a new style `name` with every variable of `base`; it doesn't become
/// current.
fn new_style_params(name: &str, base: &DimStyle) -> Value {
    let mut p = match serde_json::to_value(base) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    };
    p.insert("name".into(), json!(name));
    p.insert("current".into(), json!(false));
    Value::Object(p)
}

/// `dimstyle` parameters changing `changes` (DIM* variables) of the style `name`, leaving the
/// current style alone.
fn modify_params(name: &str, mut changes: Map<String, Value>) -> Value {
    changes.insert("name".into(), json!(name));
    changes.insert("current".into(), json!(false));
    Value::Object(changes)
}

fn tab(ui: &mut egui::Ui, tab: usize, s: &DimStyle, lists: &Lists, out: &mut Map<String, Value>) {
    let mut r = Rows { ui, id: s.name.as_str(), out };
    match tab {
        0 => {
            r.heading("Dimension lines");
            r.color("DIMCLRD", "Color", s.dim_line_color);
            r.linetype("DIMLTYPE", "Linetype", &s.dim_linetype, &lists.linetypes);
            r.lineweight("DIMLWD", "Lineweight", s.dim_lineweight);
            r.num("DIMDLE", "Extend beyond ticks", s.dim_line_extend, 0.0);
            r.num("DIMDLI", "Baseline spacing", s.baseline_spacing, 0.0);
            r.check("DIMSD1", "Suppress dim line 1", s.suppress_dim1);
            r.check("DIMSD2", "Suppress dim line 2", s.suppress_dim2);
            r.heading("Extension lines");
            r.color("DIMCLRE", "Color", s.ext_line_color);
            r.linetype("DIMLTEX1", "Linetype ext line 1", &s.ext_linetype1, &lists.linetypes);
            r.linetype("DIMLTEX2", "Linetype ext line 2", &s.ext_linetype2, &lists.linetypes);
            r.lineweight("DIMLWE", "Lineweight", s.ext_lineweight);
            r.check("DIMSE1", "Suppress ext line 1", s.suppress_ext1);
            r.check("DIMSE2", "Suppress ext line 2", s.suppress_ext2);
            r.num("DIMEXE", "Extend beyond dim lines", s.ext_extend, 0.0);
            r.num("DIMEXO", "Offset from origin", s.ext_offset, f64::NEG_INFINITY);
            r.check("DIMFXLON", "Fixed length extension lines", s.fixed_ext);
            r.num("DIMFXL", "Length", s.fixed_ext_length, 0.0);
        }
        1 => {
            r.heading("Arrowheads");
            r.arrowheads(s);
            r.text_choice("DIMLDRBLK", "Leader", &s.leader_arrow_block, ARROWHEADS);
            r.num("DIMASZ", "Arrow size", s.arrow_size, 0.0);
            r.num("DIMTSZ", "Tick size (0 = arrows)", s.tick_size, 0.0);
            r.heading("Center marks");
            r.num("DIMCEN", "Size (negative = lines)", s.center_mark, f64::NEG_INFINITY);
            r.heading("Arc length symbol");
            r.choice("DIMARCSYM", "Symbol", s.arc_symbol.into(), &[(0, "Preceding dimension text"), (1, "Above dimension text"), (2, "None")]);
            r.heading("Radius jog dimension");
            r.num("DIMJOGANG", "Jog angle (5–90°)", s.jog_angle, 5.0);
        }
        2 => {
            r.heading("Text appearance");
            let styles: Vec<(String, String)> = lists.text_styles.iter().map(|n| (n.clone(), n.clone())).collect();
            r.names("DIMTXSTY", "Text style", &s.text_style, &styles);
            r.color("DIMCLRT", "Text color", s.text_color);
            r.choice("DIMTFILL", "Fill color", s.text_fill.into(), &[(0, "None"), (1, "Background"), (2, "Color below")]);
            if s.text_fill == 2 {
                r.color("DIMTFILLCLR", "Fill color", s.text_fill_color);
            }
            r.num("DIMTXT", "Text height", s.text_height, f64::MIN_POSITIVE);
            r.heading("Text placement");
            r.choice("DIMTAD", "Vertical", s.text_above.into(), &[(0, "Centered"), (1, "Above"), (2, "Outside"), (3, "JIS"), (4, "Below")]);
            r.choice(
                "DIMJUST",
                "Horizontal",
                s.text_just.into(),
                &[(0, "Centered"), (1, "At ext line 1"), (2, "At ext line 2"), (3, "Over ext line 1"), (4, "Over ext line 2")],
            );
            r.num("DIMGAP", "Offset from dim line", s.text_gap, f64::NEG_INFINITY);
            r.num("DIMTVP", "Vertical position (× height)", s.text_vertical_pos, f64::NEG_INFINITY);
            r.heading("Text alignment");
            r.check("DIMTIH", "Horizontal inside ext lines", s.text_inside_horizontal);
            r.check("DIMTOH", "Horizontal outside ext lines", s.text_outside_horizontal);
        }
        3 => {
            r.heading("Fit options");
            r.choice(
                "DIMATFIT",
                "If it doesn't fit, move outside",
                s.fit.into(),
                &[(3, "Either text or arrows (best fit)"), (1, "Arrows"), (2, "Text"), (0, "Both text and arrows")],
            );
            r.check("DIMTIX", "Always keep text between ext lines", s.text_inside);
            r.check("DIMSOXD", "Suppress arrows that don't fit", s.suppress_outside_arrows);
            r.heading("Text placement");
            r.choice(
                "DIMTMOVE",
                "When text moves",
                s.text_move.into(),
                &[(0, "Beside the dimension line"), (1, "Over it, with leader"), (2, "Over it, no leader")],
            );
            r.heading("Scale for dimension features");
            r.check("annotative", "Annotative", s.annotative);
            r.num("DIMSCALE", "Overall scale", s.scale, 0.0);
            r.heading("Fine tuning");
            r.check("DIMTOFL", "Draw dim line between ext lines", s.force_line_inside);
        }
        4 => {
            r.heading("Linear dimensions");
            r.choice(
                "DIMLUNIT",
                "Unit format",
                s.linear_unit.into(),
                &[(1, "Scientific"), (2, "Decimal"), (3, "Engineering"), (4, "Architectural"), (5, "Fractional"), (6, "Windows desktop")],
            );
            r.int("DIMDEC", "Precision", s.decimals, 8);
            r.choice("DIMFRAC", "Fraction format", s.fraction_format.into(), &[(0, "Horizontal"), (1, "Diagonal"), (2, "Not stacked")]);
            r.text("DIMDSEP", "Decimal separator", &s.decimal_separator);
            r.num("DIMRND", "Round off", s.round, 0.0);
            r.text("DIMPOST", "Prefix/suffix (<> = value)", &s.post);
            r.num("DIMLFAC", "Measurement scale factor", s.linear_factor, f64::NEG_INFINITY);
            r.bit("DIMZIN", "Suppress leading zeros", s.zero_suppression, 4);
            r.bit("DIMZIN", "Suppress trailing zeros", s.zero_suppression, 8);
            r.heading("Angular dimensions");
            r.choice(
                "DIMAUNIT",
                "Units format",
                s.angular_unit.into(),
                &[(0, "Decimal degrees"), (1, "Deg/Min/Sec"), (2, "Gradians"), (3, "Radians")],
            );
            r.int("DIMADEC", "Precision", s.angular_decimals, 8);
            r.bit("DIMAZIN", "Suppress leading zeros", s.angular_zero_suppression, 1);
            r.bit("DIMAZIN", "Suppress trailing zeros", s.angular_zero_suppression, 2);
        }
        5 => {
            r.check("DIMALT", "Display alternate units", s.alt);
            r.heading("Alternate units");
            r.choice(
                "DIMALTU",
                "Unit format",
                s.alt_unit.into(),
                &[
                    (1, "Scientific"),
                    (2, "Decimal"),
                    (3, "Engineering"),
                    (4, "Architectural stacked"),
                    (5, "Fractional stacked"),
                    (6, "Architectural"),
                    (7, "Fractional"),
                    (8, "Windows desktop"),
                ],
            );
            r.int("DIMALTD", "Precision", s.alt_decimals, 8);
            r.num("DIMALTF", "Multiplier", s.alt_factor, f64::MIN_POSITIVE);
            r.num("DIMALTRND", "Round distances to", s.alt_round, 0.0);
            r.text("DIMAPOST", "Prefix/suffix ([] = value)", &s.alt_post);
            r.bit("DIMALTZ", "Suppress leading zeros", s.alt_zero_suppression, 4);
            r.bit("DIMALTZ", "Suppress trailing zeros", s.alt_zero_suppression, 8);
        }
        _ => {
            r.heading("Tolerance format");
            r.tolerance_method(s);
            r.int("DIMTDEC", "Precision", s.tol_decimals, 8);
            let symmetric = tolerance_method(s) == 1;
            if let Some(v) = r.num("DIMTP", if symmetric { "Value" } else { "Upper value" }, s.tol_plus, f64::NEG_INFINITY)
                && symmetric
            {
                r.out.insert("DIMTM".into(), json!(v));
            }
            if !symmetric {
                r.num("DIMTM", "Lower value", s.tol_minus, f64::NEG_INFINITY);
            }
            r.num("DIMTFAC", "Scaling for height", s.tol_scale, f64::MIN_POSITIVE);
            r.choice("DIMTOLJ", "Vertical position", s.tol_just.into(), &[(0, "Bottom"), (1, "Middle"), (2, "Top")]);
            r.bit("DIMTZIN", "Suppress leading zeros", s.tol_zero_suppression, 4);
            r.bit("DIMTZIN", "Suppress trailing zeros", s.tol_zero_suppression, 8);
        }
    }
}

/// The tolerance method of a style: 0 none, 1 symmetrical, 2 deviation, 3 limits.
fn tolerance_method(s: &DimStyle) -> i64 {
    if s.limits {
        3
    } else if !s.tolerance {
        0
    } else if (s.tol_plus - s.tol_minus).abs() < 1e-12 {
        1
    } else {
        2
    }
}

/// The DIMTOL / DIMLIM (and for symmetrical, DIMTM) changes for a tolerance method.
fn tolerance_changes(s: &DimStyle, method: i64) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("DIMTOL".into(), json!(matches!(method, 1 | 2)));
    m.insert("DIMLIM".into(), json!(method == 3));
    if method == 1 {
        m.insert("DIMTM".into(), json!(s.tol_plus));
    }
    m
}

/// The DIMBLK / DIMBLK1 / DIMBLK2 changes that give the first and second arrowheads `first` and
/// `second`: the same one at both ends is DIMBLK alone.
fn arrowhead_changes(first: &str, second: &str) -> Map<String, Value> {
    let mut m = Map::new();
    if same_arrowhead(first, second) {
        m.insert("DIMBLK".into(), json!(first));
        m.insert("DIMBLK1".into(), json!(""));
        m.insert("DIMBLK2".into(), json!(""));
    } else {
        m.insert("DIMBLK1".into(), json!(first));
        m.insert("DIMBLK2".into(), json!(second));
    }
    m
}

fn same_arrowhead(a: &str, b: &str) -> bool {
    cadcraft_render::Arrowhead::parse(a) == cadcraft_render::Arrowhead::parse(b)
}

/// The arrowheads a style draws at each end.
fn arrowheads_of(s: &DimStyle) -> (&str, &str) {
    fn pick<'a>(s: &'a DimStyle, b: &'a str) -> &'a str {
        if b.is_empty() { &s.arrow_block } else { b }
    }
    (pick(s, &s.arrow_block1), pick(s, &s.arrow_block2))
}

/// The name an arrowhead list shows for a DIMBLK value.
fn arrowhead_label(v: &str) -> String {
    ARROWHEADS.iter().find(|(k, _)| same_arrowhead(k, v)).map_or_else(|| v.to_string(), |(_, n)| (*n).to_string())
}

/// Grid rows of property editors; each change goes into `out` as `DIM*: value`.
struct Rows<'a, 'u> {
    ui: &'u mut egui::Ui,
    /// The style name, part of each field's id so another style's fields start fresh.
    id: &'a str,
    out: &'a mut Map<String, Value>,
}

impl Rows<'_, '_> {
    fn heading(&mut self, text: &str) {
        self.ui.label(RichText::new(text).strong());
        self.ui.end_row();
    }

    /// A number at least `min`; returns it when changed.
    fn num(&mut self, var: &str, label: &str, v: f64, min: f64) -> Option<f64> {
        let l = self.ui.label(label);
        let r = num_field(self.ui, ("dimstyle", self.id, var), v, Some(l.id)).filter(|x| *x >= min);
        if let Some(x) = r {
            self.out.insert(var.into(), json!(x));
        }
        self.ui.end_row();
        r
    }

    /// A whole number from 0 to `max`.
    fn int(&mut self, var: &str, label: &str, v: u8, max: u8) {
        let l = self.ui.label(label);
        if let Some(x) = num_field(self.ui, ("dimstyle", self.id, var), f64::from(v), Some(l.id)).filter(|x| (0.0..=f64::from(max)).contains(x)) {
            self.out.insert(var.into(), json!(x.round() as i64));
        }
        self.ui.end_row();
    }

    fn check(&mut self, var: &str, label: &str, v: bool) {
        self.ui.label("");
        let mut b = v;
        if self.ui.checkbox(&mut b, label).changed() {
            self.out.insert(var.into(), json!(b));
        }
        self.ui.end_row();
    }

    /// One bit of a zero-suppression variable.
    fn bit(&mut self, var: &str, label: &str, v: u8, bit: u8) {
        self.ui.label("");
        let mut b = v & bit != 0;
        if self.ui.checkbox(&mut b, label).changed() {
            // Both bits of a variable can change in the same frame only one at a time.
            let base = self.out.get(var).and_then(Value::as_u64).and_then(|x| u8::try_from(x).ok()).unwrap_or(v);
            self.out.insert(var.into(), json!(if b { base | bit } else { base & !bit }));
        }
        self.ui.end_row();
    }

    /// A pick list; returns the picked value when it changes.
    fn pick<T: Clone + PartialEq>(&mut self, var: &str, label: &str, v: &T, shown: &str, opts: &[(T, String)]) -> Option<T> {
        self.ui.label(label);
        let mut picked = None;
        craft_ui::ComboBox::from_id_salt(("dimstyle", self.id, var, label)).selected_text(shown).width(190.0).show_ui(self.ui, |ui| {
            for (k, name) in opts {
                if ui.selectable_label(k == v, name).clicked() && k != v {
                    picked = Some(k.clone());
                }
            }
        });
        self.ui.end_row();
        picked
    }

    fn choice(&mut self, var: &str, label: &str, v: i64, opts: &[(i64, &str)]) {
        let opts: Vec<(i64, String)> = opts.iter().map(|(k, n)| (*k, (*n).to_string())).collect();
        let shown = opts.iter().find(|o| o.0 == v).map_or_else(|| v.to_string(), |o| o.1.clone());
        if let Some(k) = self.pick(var, label, &v, &shown, &opts) {
            self.out.insert(var.into(), json!(k));
        }
    }

    fn names(&mut self, var: &str, label: &str, v: &str, opts: &[(String, String)]) {
        let shown = opts.iter().find(|o| o.0.eq_ignore_ascii_case(v)).map_or_else(|| v.to_string(), |o| o.1.clone());
        if let Some(k) = self.pick(var, label, &v.to_string(), &shown, opts) {
            self.out.insert(var.into(), json!(k));
        }
    }

    fn text_choice(&mut self, var: &str, label: &str, v: &str, opts: &[(&str, &str)]) {
        let opts: Vec<(String, String)> = opts.iter().map(|(k, n)| ((*k).to_string(), (*n).to_string())).collect();
        let cur = opts.iter().find(|o| same_arrowhead(&o.0, v)).map_or_else(|| v.to_string(), |o| o.0.clone());
        if let Some(k) = self.pick(var, label, &cur, &arrowhead_label(v), &opts) {
            self.out.insert(var.into(), json!(k));
        }
    }

    fn arrowheads(&mut self, s: &DimStyle) {
        let (first, second) = arrowheads_of(s);
        let opts: Vec<(String, String)> = ARROWHEADS.iter().map(|(k, n)| ((*k).to_string(), (*n).to_string())).collect();
        let canon = |v: &str| opts.iter().find(|o| same_arrowhead(&o.0, v)).map_or_else(|| v.to_string(), |o| o.0.clone());
        let (first, second) = (canon(first), canon(second));
        if let Some(k) = self.pick("DIMBLK1", "First", &first, &arrowhead_label(&first), &opts) {
            // A new first arrowhead brings the second along when both were the same.
            let second = if same_arrowhead(&first, &second) { k.clone() } else { second.clone() };
            self.out.extend(arrowhead_changes(&k, &second));
        }
        if let Some(k) = self.pick("DIMBLK2", "Second", &second, &arrowhead_label(&second), &opts) {
            self.out.extend(arrowhead_changes(&first, &k));
        }
    }

    fn tolerance_method(&mut self, s: &DimStyle) {
        let v = tolerance_method(s);
        let opts: Vec<(i64, String)> =
            ["None", "Symmetrical", "Deviation", "Limits"].iter().enumerate().map(|(i, n)| (i as i64, (*n).to_string())).collect();
        let shown = opts.iter().find(|o| o.0 == v).map(|o| o.1.clone()).unwrap_or_default();
        if let Some(k) = self.pick("DIMTOL", "Method", &v, &shown, &opts) {
            self.out.extend(tolerance_changes(s, k));
        }
    }

    fn color(&mut self, var: &str, label: &str, v: Color) {
        let mut opts: Vec<(i64, String)> = vec![(0, "ByBlock".into()), (256, "ByLayer".into())];
        opts.extend((1..=7).map(|i| (i, Color::Index(i as u8).name())));
        let aci = match v {
            Color::True(_) => -1,
            c => i64::from(c.to_aci()),
        };
        if !opts.iter().any(|o| o.0 == aci) {
            opts.push((aci, v.name()));
        }
        if let Some(k) = self.pick(var, label, &aci, &v.name(), &opts) {
            self.out.insert(var.into(), json!(k));
        }
    }

    fn lineweight(&mut self, var: &str, label: &str, v: i16) {
        let mut opts: Vec<(i64, String)> = vec![(-2, "ByBlock".into()), (-1, "ByLayer".into()), (-3, "Default".into())];
        opts.extend(cadcraft_doc::Lineweight::STANDARD.iter().map(|w| (i64::from(*w), format!("{:.2} mm", f64::from(*w) / 100.0))));
        let shown = cadcraft_doc::Lineweight::from_dxf(v).name();
        if let Some(k) = self.pick(var, label, &i64::from(v), &shown, &opts) {
            self.out.insert(var.into(), json!(k));
        }
    }

    fn linetype(&mut self, var: &str, label: &str, v: &str, linetypes: &[String]) {
        let mut opts: Vec<(String, String)> = vec![(String::new(), "ByBlock".into())];
        opts.extend(linetypes.iter().map(|n| (n.clone(), n.clone())));
        self.names(var, label, v, &opts);
    }

    /// Free text that may be emptied; commits on Enter or focus loss.
    fn text(&mut self, var: &str, label: &str, v: &str) {
        let l = self.ui.label(label);
        let id = self.ui.make_persistent_id(("dimstyle", self.id, var));
        let mut buf = self.ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| v.to_string());
        let resp = self.ui.add(egui::TextEdit::singleline(&mut buf).id(id.with("te")).desired_width(150.0)).labelled_by(l.id);
        if self.ui.memory(|m| m.has_focus(id.with("te"))) {
            self.ui.data_mut(|d| d.insert_temp(id, buf.clone()));
        } else {
            self.ui.data_mut(|d| d.remove::<String>(id));
        }
        if resp.lost_focus() && buf != v {
            self.out.insert(var.into(), json!(buf));
        }
        self.ui.end_row();
    }
}

/// A horizontal linear dimension of the style, drawn by the dimension renderer.
fn preview(ui: &mut egui::Ui, s: &DimStyle, dimscale: f64, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(300.0, 220.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, t.canvas);
    let g = cadcraft_render::dimension_geometry(&preview_dimension(s, dimscale), s, dimscale);
    let mut pts = g.lines.iter().chain(&g.fills).chain(&g.text).flatten().chain(g.text_glyphs.iter().flatten().flatten()).filter(|p| p.is_finite());
    let Some(first) = pts.next() else { return };
    let (mut lo, mut hi) = (*first, *first);
    for p in pts {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let size = hi - lo;
    let inner = rect.shrink(16.0);
    let k = (f64::from(inner.width()) / size.x.max(1e-9)).min(f64::from(inner.height()) / size.y.max(1e-9));
    if !k.is_finite() || k <= 0.0 {
        return;
    }
    let mid = (lo + hi) * 0.5;
    let c = inner.center();
    let to_screen = |p: &Vec2| egui::pos2(c.x + ((p.x - mid.x) * k) as f32, c.y - ((p.y - mid.y) * k) as f32);
    let color_of = |c: Color| match c {
        Color::ByBlock | Color::ByLayer => t.canvas_ink,
        c => {
            let rgb = c.resolve(Color::Index(7), Color::Index(7));
            egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2)
        }
    };
    let (dim_color, ext_color, text_color) = (color_of(s.dim_line_color), color_of(s.ext_line_color), color_of(s.text_color));
    for (i, line) in g.lines.iter().enumerate() {
        let color = match g.line_roles.get(i) {
            Some(cadcraft_render::LineRole::Ext) => ext_color,
            _ => dim_color,
        };
        painter.line(line.iter().map(to_screen).collect(), Stroke::new(1.2, color));
    }
    for tris in &g.fills {
        for tri in tris.as_chunks::<3>().0 {
            painter.add(egui::Shape::convex_polygon(tri.iter().map(to_screen).collect(), dim_color, Stroke::NONE));
        }
    }
    for stroke in &g.text {
        painter.line(stroke.iter().map(to_screen).collect(), Stroke::new(1.0, text_color));
    }
    for glyph in &g.text_glyphs {
        for outline in glyph {
            painter.add(egui::Shape::closed_line(outline.iter().map(to_screen).collect(), Stroke::new(1.0, text_color)));
        }
    }
}

/// The dimension the preview draws: about ten text heights long, its line four text heights
/// above the measured points.
fn preview_dimension(s: &DimStyle, dimscale: f64) -> cadcraft_doc::Dimension {
    let unit = s.text_height.max(s.arrow_size).max(1e-6) * s.effective_scale(dimscale);
    let len = unit * 10.0;
    let y = unit * 4.0;
    cadcraft_doc::Dimension {
        kind: cadcraft_doc::DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(len, y, 0.0),
        text_mid: Vec3::new(len / 2.0, y, 0.0),
        p13: Vec3::ZERO,
        p14: Vec3::new(len, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: s.name.clone(),
        measurement: len,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Map::new(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    #[test]
    fn a_new_style_copies_every_variable_of_the_style_it_starts_with() {
        let mut s = Session::new();
        s.execute("dimstyle", &json!({ "name": "Base", "DIMTXT": 3.5, "DIMBLK": "_ARCHTICK", "DIMCLRD": 1, "DIMLUNIT": 4, "current": false }))
            .unwrap();
        let base = s.doc().unwrap().dim_style("Base").cloned().unwrap();
        s.execute("dimstyle", &new_style_params("Copy", &base)).unwrap();
        let d = s.doc().unwrap();
        let copy = d.dim_style("Copy").unwrap();
        assert_eq!(*copy, DimStyle { name: "Copy".into(), ..base });
        assert!(!d.header.str("DIMSTYLE", "Standard").eq_ignore_ascii_case("Copy"), "a new style doesn't become current");
    }

    #[test]
    fn edits_change_the_selected_style_only() {
        let mut s = Session::new();
        s.execute("dimstyle", &json!({ "name": "Other", "current": false })).unwrap();
        let mut m = Map::new();
        m.insert("DIMASZ".into(), json!(4.0));
        s.execute("dimstyle", &modify_params("Other", m)).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.dim_style("Other").unwrap().arrow_size, 4.0);
        assert_ne!(d.dim_style("Standard").unwrap().arrow_size, 4.0);
        assert!(d.header.str("DIMSTYLE", "Standard").eq_ignore_ascii_case("Standard"));
    }

    #[test]
    fn arrowheads_and_tolerance_methods_map_to_their_variables() {
        let mut s = Session::new();
        s.execute("dimstyle", &json!({ "name": "A", "current": false })).unwrap();
        s.execute("dimstyle", &modify_params("A", arrowhead_changes("_DOT", "_OBLIQUE"))).unwrap();
        let st = s.doc().unwrap().dim_style("A").cloned().unwrap();
        assert_eq!(arrowheads_of(&st), ("_DOT", "_OBLIQUE"));
        s.execute("dimstyle", &modify_params("A", arrowhead_changes("_OBLIQUE", "_OBLIQUE"))).unwrap();
        let st = s.doc().unwrap().dim_style("A").cloned().unwrap();
        assert_eq!((st.arrow_block.as_str(), st.arrow_block1.as_str(), st.arrow_block2.as_str()), ("_OBLIQUE", "", ""));

        s.execute("dimstyle", &json!({ "name": "A", "DIMTP": 0.2, "DIMTM": 0.1, "current": false })).unwrap();
        // Symmetrical last: it makes the lower value equal the upper one.
        for method in [0, 2, 3, 1] {
            let st = s.doc().unwrap().dim_style("A").cloned().unwrap();
            s.execute("dimstyle", &modify_params("A", tolerance_changes(&st, method))).unwrap();
            assert_eq!(tolerance_method(s.doc().unwrap().dim_style("A").unwrap()), method);
        }
    }

    #[test]
    fn the_preview_draws_the_dimension() {
        for st in [DimStyle::default(), DimStyle::iso25(), DimStyle { tick_size: 0.1, scale: 0.0, ..Default::default() }] {
            let g = cadcraft_render::dimension_geometry(&preview_dimension(&st, 1.0), &st, 1.0);
            assert!(!g.lines.is_empty() && !(g.text.is_empty() && g.text_glyphs.is_empty()), "{}", st.name);
        }
    }

    #[test]
    fn menu_and_typed_dimstyle_open_the_manager_and_it_draws() {
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        for id in ["dimstyle", "dimstyle.dimension", "ddim", "d", "dst"] {
            app.ui.dialog = None;
            assert!(super::super::route(&mut app, id, &Value::Null).is_some(), "{id}");
            assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{id}");
        }
        assert!(super::super::route(&mut app, "dimstyle", &json!({ "name": "X" })).is_none(), "JSON calls run the command");
        let ctx = egui::Context::default();
        for tab in 0..TABS.len() {
            set_temp(&ctx, "dimstyle_state", State { tab, ..Default::default() });
            let mut open = true;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(dialog(&mut app, ui.ctx(), &mut open).is_none());
            });
            out.textures_delta.clear();
        }
    }
}
